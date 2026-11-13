use anyhow::Result;
use aws_config::SdkConfig;

/// A probe's call: it owns its client and its future, so the struct stays a plain table.
type ProbeRun =
    fn(&SdkConfig) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>>;

/// A read-only SDK call against the emulator, used to build the capability matrix.
pub struct Probe {
    pub service: &'static str,
    pub call: &'static str,
    run: ProbeRun,
}

/// Uniform summary so probes do not depend on per-service accessor names.
fn describe<T: std::fmt::Debug>(resp: &T) -> String {
    let text = format!("{resp:?}");
    let head: String = text.chars().take(60).collect();
    format!("{head}... ({} chars)", text.len())
}

pub fn probes() -> Vec<Probe> {
    vec![
        Probe {
            service: "ec2",
            call: "DescribeInstances",
            run: |config| {
                let c = aws_sdk_ec2::Client::new(config);
                Box::pin(async move {
                    Ok(describe(
                        &c.describe_instances().max_results(5).send().await?,
                    ))
                })
            },
        },
        Probe {
            service: "iam",
            call: "ListUsers",
            run: |config| {
                let c = aws_sdk_iam::Client::new(config);
                Box::pin(async move { Ok(describe(&c.list_users().max_items(5).send().await?)) })
            },
        },
        Probe {
            service: "sns",
            call: "ListTopics",
            run: |config| {
                let c = aws_sdk_sns::Client::new(config);
                Box::pin(async move { Ok(describe(&c.list_topics().send().await?)) })
            },
        },
        Probe {
            service: "cloudwatch-logs",
            call: "DescribeLogGroups",
            run: |config| {
                let c = aws_sdk_cloudwatchlogs::Client::new(config);
                Box::pin(
                    async move { Ok(describe(&c.describe_log_groups().limit(5).send().await?)) },
                )
            },
        },
        Probe {
            service: "cloudwatch",
            call: "DescribeAlarms",
            run: |config| {
                let c = aws_sdk_cloudwatch::Client::new(config);
                Box::pin(
                    async move { Ok(describe(&c.describe_alarms().max_records(5).send().await?)) },
                )
            },
        },
        Probe {
            service: "rds",
            call: "DescribeDBInstances",
            run: |config| {
                let c = aws_sdk_rds::Client::new(config);
                Box::pin(async move {
                    Ok(describe(
                        &c.describe_db_instances().max_records(20).send().await?,
                    ))
                })
            },
        },
        Probe {
            service: "secretsmanager",
            call: "ListSecrets",
            run: |config| {
                let c = aws_sdk_secretsmanager::Client::new(config);
                Box::pin(
                    async move { Ok(describe(&c.list_secrets().max_results(5).send().await?)) },
                )
            },
        },
        Probe {
            service: "kinesis",
            call: "ListStreams",
            run: |config| {
                let c = aws_sdk_kinesis::Client::new(config);
                Box::pin(async move { Ok(describe(&c.list_streams().limit(5).send().await?)) })
            },
        },
    ]
}

pub async fn run_all(config: &SdkConfig) -> Vec<(String, String, String)> {
    let mut rows = Vec::new();
    for p in probes() {
        let outcome = (p.run)(config)
            .await
            .unwrap_or_else(|e| format!("FAIL {e:#}"));
        rows.push((p.service.to_string(), p.call.to_string(), outcome));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; prints the SDK capability matrix"]
    async fn live_sdk_capability_matrix() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("build aws config");
        for (service, call, outcome) in run_all(&config).await {
            println!("{service:16} {call:22} {outcome}");
        }
    }
}
