use crate::app_state::Resource;
use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_cloudwatch::Client;
use aws_sdk_cloudwatch::types::{
    AlarmType, ComparisonOperator, Dimension, Metric, MetricDataQuery, MetricDatum, MetricStat,
    ScanBy, StateValue, Statistic,
};

pub const LABEL: &str = "CloudWatch";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_secs()
}

fn stamp(secs: u64) -> aws_smithy_types::DateTime {
    aws_smithy_types::DateTime::from_secs(secs as i64)
}

/// Short one-line rendering of a possibly long history payload.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max).collect();
    clipped.push('…');
    clipped
}

fn dimensions_text(dimensions: &[Dimension]) -> String {
    if dimensions.is_empty() {
        return "no dimensions".to_string();
    }
    dimensions
        .iter()
        .map(|d| {
            format!(
                "{}={}",
                d.name().unwrap_or_default(),
                d.value().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Every alarm the emulator holds: metric alarms render state · metric · threshold,
/// composite alarms render state · rule.
pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut resources = Vec::new();
    let mut token: Option<String> = None;
    loop {
        // DescribeAlarms answers with metric alarms only unless AlarmTypes names the
        // kinds it should include, so composites are asked for explicitly.
        let mut req = client
            .describe_alarms()
            .max_records(100)
            .alarm_types(AlarmType::MetricAlarm)
            .alarm_types(AlarmType::CompositeAlarm);
        if let Some(t) = token.take() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeAlarms")?;
        for a in resp.metric_alarms() {
            let state = a.state_value().map(|s| s.as_str()).unwrap_or("unknown");
            let metric = a.metric_name().unwrap_or_default();
            let comparison = a.comparison_operator().map(|c| c.as_str()).unwrap_or("");
            let threshold = a
                .threshold()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".to_string());
            resources.push(Resource {
                name: a.alarm_name().unwrap_or_default().to_string(),
                detail: format!("{state} · {metric} {comparison} {threshold}"),
            });
        }
        for a in resp.composite_alarms() {
            let state = a.state_value().map(|s| s.as_str()).unwrap_or("unknown");
            let rule = a.alarm_rule().unwrap_or_default();
            resources.push(Resource {
                name: a.alarm_name().unwrap_or_default().to_string(),
                detail: format!("{state} · rule: {}", clip(rule, 80)),
            });
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(resources)
}

/// Create (or replace) a single-metric alarm watching `metric_name` in `namespace`,
/// alarming when the 5-minute `Average` is >= `threshold`.
pub async fn put_metric_alarm(
    config: &SdkConfig,
    alarm_name: &str,
    namespace: &str,
    metric_name: &str,
    threshold: f64,
) -> Result<()> {
    let client = Client::new(config);
    client
        .put_metric_alarm()
        .alarm_name(alarm_name)
        .namespace(namespace)
        .metric_name(metric_name)
        .statistic(Statistic::Average)
        .comparison_operator(ComparisonOperator::GreaterThanOrEqualToThreshold)
        .threshold(threshold)
        .period(300)
        .evaluation_periods(1)
        .send()
        .await
        .context("PutMetricAlarm")?;
    Ok(())
}

/// Create (or replace) a composite alarm whose state is computed from `alarm_rule`
/// (e.g. `ALARM("cpu-high") OR ALARM("mem-high")`).
pub async fn put_composite_alarm(
    config: &SdkConfig,
    alarm_name: &str,
    alarm_rule: &str,
) -> Result<()> {
    let client = Client::new(config);
    client
        .put_composite_alarm()
        .alarm_name(alarm_name)
        .alarm_rule(alarm_rule)
        .send()
        .await
        .context("PutCompositeAlarm")?;
    Ok(())
}

pub async fn delete_alarms(config: &SdkConfig, names: &[String]) -> Result<()> {
    let client = Client::new(config);
    let mut req = client.delete_alarms();
    for name in names {
        req = req.alarm_names(name.clone());
    }
    req.send().await.context("DeleteAlarms")?;
    Ok(())
}

/// Force an alarm's state: `state` is "OK", "ALARM" or "INSUFFICIENT_DATA".
pub async fn set_alarm_state(
    config: &SdkConfig,
    alarm_name: &str,
    state: &str,
    reason: &str,
) -> Result<()> {
    let client = Client::new(config);
    client
        .set_alarm_state()
        .alarm_name(alarm_name)
        .state_value(StateValue::from(state))
        .state_reason(reason)
        .send()
        .await
        .context("SetAlarmState")?;
    Ok(())
}

/// Every time series the metric store holds; one resource per metric.
pub async fn list_metrics(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut resources = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_metrics();
        if let Some(t) = token.take() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListMetrics")?;
        for m in resp.metrics() {
            let namespace = m.namespace().unwrap_or_default();
            let metric_name = m.metric_name().unwrap_or_default();
            resources.push(Resource {
                name: format!("{namespace}/{metric_name}"),
                detail: dimensions_text(m.dimensions()),
            });
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(resources)
}

/// The distinct namespaces the metric store holds, sorted.
pub async fn list_namespaces(config: &SdkConfig) -> Result<Vec<String>> {
    let metrics = list_metrics(config).await?;
    let mut namespaces: Vec<String> = metrics
        .iter()
        .filter_map(|r| r.name.split('/').next())
        .map(str::to_string)
        .collect();
    namespaces.sort();
    namespaces.dedup();
    Ok(namespaces)
}

/// The alarm state-change and configuration history, newest first.
pub async fn describe_alarm_history(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut resources = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.describe_alarm_history().max_records(100);
        if let Some(t) = token.take() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeAlarmHistory")?;
        for item in resp.alarm_history_items() {
            let kind = item
                .history_item_type()
                .map(|t| t.as_str())
                .unwrap_or("unknown");
            let data = item.history_data().unwrap_or_default();
            let when = item
                .timestamp()
                .map(|ts| ts.secs().to_string())
                .unwrap_or_else(|| "-".to_string());
            resources.push(Resource {
                name: item.alarm_name().unwrap_or_default().to_string(),
                detail: format!("{kind} · {when} · {}", clip(data, 80)),
            });
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(resources)
}

/// Publish one datapoint into a custom namespace.
pub async fn put_metric_data(
    config: &SdkConfig,
    namespace: &str,
    metric_name: &str,
    value: f64,
) -> Result<()> {
    let client = Client::new(config);
    let datum = MetricDatum::builder()
        .metric_name(metric_name)
        .value(value)
        .timestamp(stamp(now_secs()))
        .build();
    client
        .put_metric_data()
        .namespace(namespace)
        .metric_data(datum)
        .send()
        .await
        .context("PutMetricData")?;
    Ok(())
}

/// The datapoints of one series over the last `hours` (folded by `stat` at a
/// 5-minute period), newest last.
pub async fn get_metric_data(
    config: &SdkConfig,
    namespace: &str,
    metric_name: &str,
    stat: &str,
    hours: u64,
) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let query = MetricDataQuery::builder()
        .id("m1")
        .metric_stat(
            MetricStat::builder()
                .metric(
                    Metric::builder()
                        .namespace(namespace)
                        .metric_name(metric_name)
                        .build(),
                )
                .period(300)
                .stat(stat)
                .build(),
        )
        .build();
    let now = now_secs();
    let resp = client
        .get_metric_data()
        .metric_data_queries(query)
        .start_time(stamp(now.saturating_sub(hours * 3600)))
        .end_time(stamp(now + 3600))
        .scan_by(ScanBy::TimestampAscending)
        .send()
        .await
        .context("GetMetricData")?;
    let mut resources = Vec::new();
    for result in resp.metric_data_results() {
        let id = result.id().unwrap_or_default();
        for (ts, value) in result.timestamps().iter().zip(result.values()) {
            resources.push(Resource {
                name: format!("{namespace}/{metric_name} ({id})"),
                detail: format!("{stat} {value} at {}", ts.secs()),
            });
        }
    }
    Ok(resources)
}

/// All dashboards (a dashboard is global, not regional).
pub async fn list_dashboards(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut resources = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_dashboards();
        if let Some(t) = token.take() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListDashboards")?;
        for d in resp.dashboard_entries() {
            let modified = d
                .last_modified()
                .map(|ts| ts.secs().to_string())
                .unwrap_or_else(|| "-".to_string());
            resources.push(Resource {
                name: d.dashboard_name().unwrap_or_default().to_string(),
                detail: format!("last modified {modified}"),
            });
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(resources)
}

/// The stored JSON body of one dashboard.
pub async fn get_dashboard(config: &SdkConfig, name: &str) -> Result<String> {
    let client = Client::new(config);
    let resp = client
        .get_dashboard()
        .dashboard_name(name)
        .send()
        .await
        .context("GetDashboard")?;
    Ok(resp.dashboard_body().unwrap_or_default().to_string())
}

/// Create or fully replace a dashboard. `body` must be a valid dashboard JSON object.
pub async fn put_dashboard(config: &SdkConfig, name: &str, body: &str) -> Result<()> {
    let client = Client::new(config);
    client
        .put_dashboard()
        .dashboard_name(name)
        .dashboard_body(body)
        .send()
        .await
        .context("PutDashboard")?;
    Ok(())
}

pub async fn delete_dashboards(config: &SdkConfig, names: &[String]) -> Result<()> {
    let client = Client::new(config);
    let mut req = client.delete_dashboards();
    for name in names {
        req = req.dashboard_names(name.clone());
    }
    req.send().await.context("DeleteDashboards")?;
    Ok(())
}

/// Poll `fetch` until the alarm `name` reports `want` as its state prefix, or give up.
/// The live tests use this to see an alarm settle; the UI never waits, it reloads.
#[cfg(test)]
pub async fn wait_alarm_state(config: &SdkConfig, name: &str, want: &str, secs: u64) -> Result<()> {
    for _ in 0..(secs * 2) {
        let alarms = fetch(config).await?;
        if let Some(a) = alarms.iter().find(|a| a.name == name) {
            if a.detail.starts_with(want) {
                return Ok(());
            }
        } else {
            anyhow::bail!("alarm {name} not found");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    anyhow::bail!("alarm {name} did not reach {want} within {secs}s")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;
    use anyhow::ensure;

    fn alarm_names(items: &[Resource]) -> String {
        items
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes alarms, metrics and a dashboard"]
    async fn live_cloudwatch_lifecycle() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("aws config");

        let suffix = now_secs();
        let alarm = format!("qoder-cw-alarm-{suffix}");
        let composite = format!("qoder-cw-composite-{suffix}");
        let dashboard = format!("qoder-cw-dash-{suffix}");
        let namespace = format!("QoderDashboardTest{suffix}");
        let metric = "LiveLatency";

        let outcome = async {
            // Data-plane round trip: publish a datapoint and read it back.
            put_metric_data(&config, &namespace, metric, 42.0).await?;
            let points = get_metric_data(&config, &namespace, metric, "Average", 1).await?;
            ensure!(
                points.iter().any(|p| p.detail.contains("42")),
                "round-tripped datapoint missing ({} points)",
                points.len()
            );
            let namespaces = list_namespaces(&config).await?;
            ensure!(
                namespaces.iter().any(|n| n == &namespace),
                "namespace {namespace} missing from {namespaces:?}"
            );

            // Alarm create → state force → composite over it.
            put_metric_alarm(&config, &alarm, &namespace, metric, 100.0).await?;
            let alarms = fetch(&config).await?;
            ensure!(
                alarms.iter().any(|a| a.name == alarm),
                "alarm {alarm} missing from list"
            );
            set_alarm_state(&config, &alarm, "ALARM", "dashboard lifecycle test").await?;
            wait_alarm_state(&config, &alarm, "ALARM", 15).await?;
            let rule = format!("ALARM(\"{alarm}\")");
            put_composite_alarm(&config, &composite, &rule).await?;
            let alarms = fetch(&config).await?;
            ensure!(
                alarms.iter().any(|a| a.name == composite),
                "composite alarm missing: {}",
                alarm_names(&alarms)
            );

            // Alarm history should record the writes above.
            let history = describe_alarm_history(&config).await?;
            ensure!(
                history.iter().any(|h| h.name == alarm),
                "no history for {alarm}: {}",
                alarm_names(&history)
            );

            // Dashboard put/get/list round trip.
            let body =
                r#"{"widgets":[{"type":"text","properties":{"markdown":"qoder live test"}}]}"#;
            put_dashboard(&config, &dashboard, body).await?;
            let dashboards = list_dashboards(&config).await?;
            ensure!(
                dashboards.iter().any(|d| d.name == dashboard),
                "dashboard {dashboard} missing from list"
            );
            let fetched = get_dashboard(&config, &dashboard).await?;
            ensure!(fetched.contains("qoder live test"), "body: {fetched}");

            Ok::<_, anyhow::Error>(())
        }
        .await;

        delete_alarms(&config, &[alarm.clone(), composite.clone()])
            .await
            .expect("cleanup alarms");
        delete_dashboards(&config, std::slice::from_ref(&dashboard))
            .await
            .expect("cleanup dashboards");
        outcome.expect("lifecycle");

        let after = fetch(&config).await.expect("describe after cleanup");
        assert!(
            !after.iter().any(|a| a.name == alarm || a.name == composite),
            "alarms survived delete"
        );
        let after_dashboards = list_dashboards(&config).await.expect("list after cleanup");
        assert!(
            !after_dashboards.iter().any(|d| d.name == dashboard),
            "dashboard survived delete"
        );
    }
}
