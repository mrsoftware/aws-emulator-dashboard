use crate::aws::client::EmulatorConnection;
use crate::settings::Settings;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Docker,
    Native,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Backend::Docker => "Docker",
            Backend::Native => "Native",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StartupInfo {
    pub backend: Option<Backend>,
    pub endpoint_url: String,
    pub health_check_url: String,
    pub config_file: PathBuf,
    pub ca_file: PathBuf,
    pub pid: Option<u32>,
    pub container: Option<String>,
}

#[derive(Default)]
pub struct EmulatorManager {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    child: Option<Child>,
    container: Option<String>,
    startup: Option<StartupInfo>,
    started_at: Option<Instant>,
}

impl EmulatorManager {
    pub async fn start(&self, settings: &Settings) -> Result<StartupInfo> {
        let mut inner = self.inner.lock().await;
        if inner.child.is_some() || inner.container.is_some() {
            bail!("an emulator is already managed by the dashboard; stop it first");
        }
        let backend = detect_backend(settings);
        let mut info = match backend {
            Backend::Docker => start_docker(settings).await?,
            Backend::Native => start_native(settings, &mut inner).await?,
        };
        info.backend = Some(backend);
        wait_healthy(&info, Duration::from_secs(60)).await?;
        inner.container = info.container.clone();
        inner.startup = Some(info.clone());
        inner.started_at = Some(Instant::now());
        Ok(info)
    }

    pub async fn stop(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if let Some(name) = inner.container.take() {
            let out = Command::new("docker")
                .args(["stop", &name])
                .output()
                .await
                .context("failed to run docker stop")?;
            if !out.status.success() {
                bail!(
                    "docker stop failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
        if let Some(mut child) = inner.child.take() {
            if let Some(pid) = child.id() {
                let _ = Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .status()
                    .await;
            }
            let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;
        }
        inner.startup = None;
        inner.started_at = None;
        Ok(())
    }

    pub async fn restart(&self, settings: &Settings) -> Result<StartupInfo> {
        self.stop().await?;
        self.start(settings).await
    }

    pub async fn adopt_if_running(&self, settings: &Settings) -> Option<StartupInfo> {
        let mut inner = self.inner.lock().await;
        if inner.child.is_some() || inner.container.is_some() {
            return inner.startup.clone();
        }
        let port = settings.default_port;
        let name = container_name(port);
        let out = Command::new("docker")
            .args([
                "ps",
                "--filter",
                &format!("name={name}"),
                "--format",
                "{{.Names}}",
            ])
            .output()
            .await
            .ok()?;
        let names = String::from_utf8_lossy(&out.stdout);
        if !names.lines().any(|l| l.trim() == name) {
            return None;
        }
        let dir = dirs::home_dir()?
            .join(".aws/emulator")
            .join(port.to_string());
        let info = StartupInfo {
            backend: Some(Backend::Docker),
            endpoint_url: format!("https://127.0.0.1:{port}"),
            health_check_url: format!("https://127.0.0.1:{port}/__admin/health"),
            config_file: dir.join("config"),
            ca_file: dir.join("ca.pem"),
            pid: None,
            container: Some(name),
        };
        inner.container = info.container.clone();
        inner.startup = Some(info.clone());
        Some(info)
    }
}

fn detect_backend(settings: &Settings) -> Backend {
    if program_exists(&settings.emulator_binary) {
        Backend::Native
    } else {
        Backend::Docker
    }
}

fn program_exists(path: &Path) -> bool {
    if path.components().count() > 1 {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(path).is_file()))
        .unwrap_or(false)
}

fn container_name(port: u16) -> String {
    format!("aws-emulator-{port}")
}

fn emulator_args(settings: &Settings) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        settings.default_port.to_string(),
        "-a".to_string(),
        if settings.auth_enabled { "on" } else { "off" }.to_string(),
        "-t".to_string(),
        if settings.telemetry_enabled {
            "on"
        } else {
            "off"
        }
        .to_string(),
    ];
    if settings.idle_timeout_secs > 0 {
        args.push("--idle-timeout".to_string());
        args.push(settings.idle_timeout_secs.to_string());
    }
    args
}

async fn start_docker(settings: &Settings) -> Result<StartupInfo> {
    let port = settings.default_port;
    let name = container_name(port);

    let inspect = Command::new("docker")
        .args(["image", "inspect", "aws-emulator"])
        .output()
        .await
        .context("failed to run docker; is Docker installed and running?")?;
    if !inspect.status.success() {
        build_image().await?;
    }

    let home = dirs::home_dir().context("no home directory")?;
    let shared = home.join(".aws/emulator");
    tokio::fs::create_dir_all(&shared).await?;

    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        name.clone(),
        "--memory=2g".to_string(),
        "--memory-swap=2g".to_string(),
        "-p".to_string(),
        format!("127.0.0.1:{port}:{port}"),
        "-e".to_string(),
        format!("HOME={}", home.display()),
        "-v".to_string(),
        format!("{}:{}", shared.display(), shared.display()),
        "aws-emulator".to_string(),
    ];
    args.extend(emulator_args(settings));

    let out = Command::new("docker")
        .args(&args)
        .output()
        .await
        .context("failed to run docker run")?;
    if !out.status.success() {
        bail!(
            "docker run failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    let dir = shared.join(port.to_string());
    Ok(StartupInfo {
        backend: Some(Backend::Docker),
        endpoint_url: format!("https://127.0.0.1:{port}"),
        health_check_url: format!("https://127.0.0.1:{port}/__admin/health"),
        config_file: dir.join("config"),
        ca_file: dir.join("ca.pem"),
        pid: None,
        container: Some(name),
    })
}

async fn build_image() -> Result<()> {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let dockerfile = format!(
        "FROM public.ecr.aws/amazonlinux/amazonlinux:2023-minimal\n\
         ADD https://downloads.aws.com/aws-emulator/packages/aws-emulator/LTS/latest/aws-emulator-{arch}.rpm /tmp/aws-emulator.rpm\n\
         RUN rpm -i /tmp/aws-emulator.rpm && rm /tmp/aws-emulator.rpm \\\n\
         \x20   && mkdir -p /home/emulator && chown 1000 /home/emulator\n\
         USER 1000\n\
         ENV HOME=/home/emulator\n\
         EXPOSE 9297\n\
         ENTRYPOINT [\"aws-emulator\", \"-b\", \"0.0.0.0\"]\n"
    );
    let dir = std::env::temp_dir().join("aws-emulator-dashboard-image");
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join("Dockerfile"), dockerfile).await?;
    let out = Command::new("docker")
        .args(["build", "-t", "aws-emulator"])
        .arg(&dir)
        .output()
        .await
        .context("failed to run docker build")?;
    if !out.status.success() {
        bail!(
            "docker build failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

async fn start_native(settings: &Settings, inner: &mut Inner) -> Result<StartupInfo> {
    let mut child = Command::new(&settings.emulator_binary)
        .args(emulator_args(settings))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn {}", settings.emulator_binary.display()))?;

    let stdout = child.stdout.take().context("no stdout from emulator")?;
    let mut lines = BufReader::new(stdout).lines();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut info = None;
    while Instant::now() < deadline {
        let next = tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await;
        let Ok(line) = next else { continue };
        let Some(line) = line.context("reading emulator stdout")? else {
            break;
        };
        tracing::debug!(line, "emulator stdout");
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line)
            && v.get("message").and_then(|m| m.as_str()) == Some("emulator server information")
        {
            let str_field = |k: &str| {
                v.get(k)
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            info = Some(StartupInfo {
                backend: Some(Backend::Native),
                endpoint_url: str_field("endpoint_url"),
                health_check_url: str_field("health_check_url"),
                config_file: PathBuf::from(str_field("config_file")),
                ca_file: PathBuf::from(str_field("ca_file")),
                pid: child.id(),
                container: None,
            });
            break;
        }
    }
    let info = info.context("emulator did not emit its startup line within 30s")?;

    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(line, "emulator stdout");
        }
    });
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(line, "emulator stderr");
            }
        });
    }

    inner.child = Some(child);
    Ok(info)
}

async fn wait_healthy(info: &StartupInfo, timeout: Duration) -> Result<()> {
    let conn = EmulatorConnection::from_startup(
        info.endpoint_url.clone(),
        info.config_file.clone(),
        info.ca_file.clone(),
    );
    let deadline = Instant::now() + timeout;
    loop {
        if conn.health().await.is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "emulator did not become healthy within {}s",
                timeout.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires Docker; manages a real emulator container, leaves it running"]
    async fn live_manager_start_stop() {
        let manager = EmulatorManager::default();
        let settings = Settings::default();
        if manager.adopt_if_running(&settings).await.is_some() {
            eprintln!("skipping: an emulator already runs on this port (dashboard owns it)");
            return;
        }
        let info = manager.start(&settings).await.expect("start");
        assert_eq!(info.endpoint_url, "https://127.0.0.1:9297");
        assert!(info.ca_file.exists());
        manager.stop().await.expect("stop");
        assert!(
            !info.ca_file.exists(),
            "emulator should delete its files on clean stop"
        );
        manager.start(&settings).await.expect("start again");
    }
}
