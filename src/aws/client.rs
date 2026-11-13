use anyhow::{Context, Result, bail};
use aws_config::SdkConfig;
use aws_credential_types::Credentials;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The region every page and every SDK call is made in. The emulator serves one.
pub const REGION: &str = "us-east-1";

// GPUI's executor is not Tokio; reqwest and the AWS SDK need a Tokio reactor.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
    })
}

#[derive(Debug, Clone)]
pub struct EmulatorConnection {
    pub endpoint: String,
    pub credentials_path: PathBuf,
    pub ca_path: PathBuf,
    pub auth_enabled: bool,
}

impl EmulatorConnection {
    pub fn from_port(port: u16) -> Self {
        Self {
            endpoint: format!("https://127.0.0.1:{}", port),
            credentials_path: dirs::home_dir()
                .map(|h| h.join(format!(".aws/emulator/{}/emulator_credentials", port)))
                .unwrap_or_default(),
            ca_path: dirs::home_dir()
                .map(|h| h.join(format!(".aws/emulator/{}/ca.pem", port)))
                .unwrap_or_default(),
            auth_enabled: true,
        }
    }

    pub fn from_startup(endpoint_url: String, config_file: PathBuf, ca_file: PathBuf) -> Self {
        // Infer port from endpoint URL like https://127.0.0.1:9297
        let _port = endpoint_url
            .rsplit_once(':')
            .and_then(|(_, p)| p.parse::<u16>().ok())
            .unwrap_or(9297);
        Self {
            endpoint: endpoint_url,
            credentials_path: config_file
                .parent()
                .map(|p| p.join("emulator_credentials"))
                .unwrap_or_default(),
            ca_path: ca_file,
            auth_enabled: true,
        }
    }

    pub async fn health(&self) -> Result<String> {
        let client = self.http_client()?;
        let url = format!("{}/__admin/health", self.endpoint);
        let response = client.get(&url).send().await?;
        let status = response.status();
        let body = response.text().await?;
        if status.is_success() {
            Ok(body)
        } else {
            bail!("health check failed: {} {}", status, body)
        }
    }

    pub async fn build_aws_config(&self) -> Result<SdkConfig> {
        // Trust the emulator's self-signed certificate for the AWS SDK.
        // rustls-native-certs honors SSL_CERT_FILE.
        if self.ca_path.exists() {
            unsafe { std::env::set_var("SSL_CERT_FILE", &self.ca_path) };
        }

        let loader = aws_config::from_env().region(aws_config::Region::new(REGION));
        let loader = if self.auth_enabled && self.credentials_path.exists() {
            let (key, secret) = parse_credentials_file(&self.credentials_path)
                .with_context(|| format!("reading {}", self.credentials_path.display()))?;
            loader.credentials_provider(Credentials::new(key, secret, None, None, "emulator"))
        } else {
            loader.credentials_provider(Credentials::new(
                "emulator", "emulator", None, None, "emulator",
            ))
        };
        let loader = loader.endpoint_url(&self.endpoint);
        Ok(loader.load().await)
    }

    pub fn http_client(&self) -> Result<reqwest::Client> {
        let mut builder = reqwest::ClientBuilder::new();
        if self.ca_path.exists() {
            let cert = std::fs::read(&self.ca_path)?;
            let cert = reqwest::Certificate::from_pem(&cert)?;
            builder = builder.add_root_certificate(cert);
        }
        Ok(builder.build()?)
    }
}

fn parse_credentials_file(path: &Path) -> Result<(String, String)> {
    let contents = std::fs::read_to_string(path)?;
    let mut key = None;
    let mut secret = None;
    for line in contents.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let v = v.trim().to_string();
            if k == "aws_access_key_id" {
                key = Some(v);
            } else if k == "aws_secret_access_key" {
                secret = Some(v);
            }
        }
    }
    match (key, secret) {
        (Some(k), Some(s)) => Ok((k, s)),
        _ => bail!("credentials file missing key or secret"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297"]
    async fn live_s3_roundtrip() {
        let conn = EmulatorConnection::from_port(9297);
        conn.health().await.expect("emulator health check");
        let config = conn.build_aws_config().await.expect("build aws config");
        let s3 = aws_sdk_s3::Client::new(&config);
        s3.create_bucket()
            .bucket("dashboard-live-test")
            .send()
            .await
            .expect("create bucket");
        let buckets = crate::aws::services::fetch_s3(&config)
            .await
            .expect("list buckets");
        assert!(buckets.iter().any(|r| r.name == "dashboard-live-test"));
        s3.delete_bucket()
            .bucket("dashboard-live-test")
            .send()
            .await
            .expect("delete bucket");
    }

    #[tokio::test]
    #[ignore = "requires a running emulator; creates demo resources and leaves them"]
    async fn live_seed_demo_resources() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("build aws config");

        let s3 = aws_sdk_s3::Client::new(&config);
        let _ = s3.create_bucket().bucket("demo-bucket").send().await;

        let ddb = aws_sdk_dynamodb::Client::new(&config);
        let _ = ddb
            .create_table()
            .table_name("demo-table")
            .attribute_definitions(
                aws_sdk_dynamodb::types::AttributeDefinition::builder()
                    .attribute_name("id")
                    .attribute_type(aws_sdk_dynamodb::types::ScalarAttributeType::S)
                    .build()
                    .unwrap(),
            )
            .key_schema(
                aws_sdk_dynamodb::types::KeySchemaElement::builder()
                    .attribute_name("id")
                    .key_type(aws_sdk_dynamodb::types::KeyType::Hash)
                    .build()
                    .unwrap(),
            )
            .billing_mode(aws_sdk_dynamodb::types::BillingMode::PayPerRequest)
            .send()
            .await;

        let sqs = aws_sdk_sqs::Client::new(&config);
        let _ = sqs.create_queue().queue_name("demo-queue").send().await;
    }
}
