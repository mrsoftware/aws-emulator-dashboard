use crate::app_state::Resource;
use anyhow::{Context, Result, bail};
use aws_config::SdkConfig;
use aws_sdk_ec2::Client;

pub const LABEL: &str = "EC2";

#[derive(Clone, Default)]
pub struct InstanceInfo {
    pub id: String,
    pub name: String,
    pub state: String,
    pub instance_type: String,
    pub az: String,
    /// The IPv4 DescribeInstances answered, empty when it did not. A stopped instance has no
    /// public address, and the emulator may not assign one at all — the page shows a dash.
    pub ip: String,
    /// Launch time as the SDK prints it (RFC 3339), empty when absent.
    pub launched: String,
}

fn tag(instance: &aws_sdk_ec2::types::Instance, key: &str) -> String {
    instance
        .tags()
        .iter()
        .find(|t| t.key().unwrap_or_default() == key)
        .and_then(|t| t.value())
        .unwrap_or_default()
        .to_string()
}

fn info(instance: &aws_sdk_ec2::types::Instance) -> InstanceInfo {
    InstanceInfo {
        id: instance.instance_id().unwrap_or_default().to_string(),
        name: tag(instance, "Name"),
        state: instance
            .state()
            .and_then(|s| s.name())
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        instance_type: instance
            .instance_type()
            .map(|t| t.as_str().to_string())
            .unwrap_or_default(),
        az: instance
            .placement()
            .and_then(|p| p.availability_zone())
            .unwrap_or_default()
            .to_string(),
        ip: instance
            .public_ip_address()
            .or_else(|| instance.private_ip_address())
            .unwrap_or_default()
            .to_string(),
        launched: instance
            .launch_time()
            .map(|t| t.to_string())
            .unwrap_or_default(),
    }
}

pub async fn list_instances(config: &SdkConfig) -> Result<Vec<InstanceInfo>> {
    let client = Client::new(config);
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.describe_instances().max_results(1000);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeInstances")?;
        for r in resp.reservations() {
            for i in r.instances() {
                out.push(info(i));
            }
        }
        token = resp.next_token.clone();
        if token.is_none() {
            break;
        }
    }
    Ok(out)
}

pub fn resources(instances: &[InstanceInfo]) -> Vec<Resource> {
    instances
        .iter()
        .map(|i| Resource {
            name: i.id.clone(),
            detail: format!("{} · {} · {}", i.state, i.instance_type, i.az),
        })
        .collect()
}

pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    Ok(resources(&list_instances(config).await?))
}

async fn transition(config: &SdkConfig, id: &str, verb: &str) -> Result<()> {
    let client = Client::new(config);
    match verb {
        "start" => {
            client
                .start_instances()
                .instance_ids(id)
                .send()
                .await
                .context("StartInstances")?;
        }
        "stop" => {
            client
                .stop_instances()
                .instance_ids(id)
                .send()
                .await
                .context("StopInstances")?;
        }
        _ => bail!("unsupported transition {verb}"),
    }
    Ok(())
}

pub async fn start_instance(config: &SdkConfig, id: &str) -> Result<()> {
    transition(config, id, "start").await
}

pub async fn stop_instance(config: &SdkConfig, id: &str) -> Result<()> {
    transition(config, id, "stop").await
}

pub async fn terminate_instance(config: &SdkConfig, id: &str) -> Result<()> {
    let client = Client::new(config);
    client
        .terminate_instances()
        .instance_ids(id)
        .send()
        .await
        .context("TerminateInstances")?;
    Ok(())
}

/// Current instance state name ("pending" | "running" | "stopping" | "stopped" | ...).
pub async fn instance_state(config: &SdkConfig, id: &str) -> Result<String> {
    let client = Client::new(config);
    let filter = aws_sdk_ec2::types::Filter::builder()
        .name("instance-id")
        .values(id)
        .build();
    let resp = client
        .describe_instances()
        .set_filters(Some(vec![filter]))
        .send()
        .await
        .context("DescribeInstances")?;
    for r in resp.reservations() {
        for i in r.instances() {
            if i.instance_id() == Some(id) {
                return Ok(i
                    .state()
                    .and_then(|s| s.name())
                    .map(|n| n.as_str().to_string())
                    .unwrap_or_else(|| "unknown".to_string()));
            }
        }
    }
    bail!("instance {id} not found")
}

/// The emulator advances instance states on a clock, so callers must wait for the
/// transition to land before issuing the next one. Live tests only: the UI reloads
/// the inventory instead of blocking on a transition.
#[cfg(test)]
pub async fn wait_state(config: &SdkConfig, id: &str, want: &str, secs: u64) -> Result<()> {
    for _ in 0..(secs * 2) {
        let state = instance_state(config, id).await?;
        if state == want {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    bail!("instance {id} did not reach {want} within {secs}s")
}

pub async fn create_instance(config: &SdkConfig, instance_type: &str) -> Result<String> {
    let client = Client::new(config);
    let resp = client
        .run_instances()
        .instance_type(aws_sdk_ec2::types::InstanceType::from(instance_type))
        .image_id("ami-0123456789abcdef0")
        .min_count(1)
        .max_count(1)
        .send()
        .await
        .context("RunInstances")?;
    resp.instances()
        .first()
        .and_then(|i| i.instance_id())
        .map(str::to_string)
        .context("RunInstances returned no instance")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; launches and terminates an instance"]
    async fn live_ec2_lifecycle() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("aws config");

        let id = create_instance(&config, "t3.micro")
            .await
            .expect("run instances");
        let listed = fetch(&config).await.expect("describe instances");
        assert!(
            listed
                .iter()
                .any(|r| r.name == id || r.detail.contains("t3.micro"))
        );

        let outcome = async {
            stop_instance(&config, &id).await.expect("stop");
            wait_state(&config, &id, "stopped", 20)
                .await
                .expect("wait stopped");
            start_instance(&config, &id).await.expect("start");
            wait_state(&config, &id, "running", 20)
                .await
                .expect("wait running");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        terminate_instance(&config, &id).await.expect("terminate");
        outcome.expect("lifecycle");

        let after = fetch(&config).await.expect("describe after terminate");
        assert!(
            !after
                .iter()
                .any(|r| r.name == id && r.detail.starts_with("running"))
        );
    }
}
