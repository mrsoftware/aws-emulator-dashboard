use crate::app_state::Resource;
use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_cloudwatchlogs::Client;
use aws_sdk_cloudwatchlogs::types::{InputLogEvent, MetricTransformation};

pub const LABEL: &str = "CloudWatch Logs";

#[derive(Clone)]
pub struct LogStreamInfo {
    pub name: String,
    pub first_event_time: i64,
    pub last_event_time: i64,
}

#[derive(Clone)]
pub struct LogEventInfo {
    pub message: String,
    pub timestamp: i64,
}

#[derive(Clone)]
pub struct FilteredLogEventInfo {
    pub stream: String,
    pub message: String,
    pub timestamp: i64,
}

#[derive(Clone)]
pub struct TagInfo {
    pub key: String,
    pub value: String,
}

#[derive(Clone)]
pub struct MetricFilterInfo {
    pub filter_name: String,
    pub filter_pattern: String,
    pub metric: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionFilterInfo {
    pub filter_name: String,
    pub filter_pattern: String,
    pub destination_arn: String,
    pub role_arn: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestinationInfo {
    pub name: String,
    pub target_arn: String,
    pub role_arn: String,
    pub arn: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryRow {
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryInfo {
    pub id: String,
    pub status: String,
    pub query: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogGroupFieldInfo {
    pub name: String,
    pub percent: i32,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

pub(crate) fn format_millis(ms: i64) -> String {
    aws_smithy_types::DateTime::from_millis(ms)
        .fmt(aws_smithy_types::date_time::Format::DateTime)
        .unwrap_or_default()
}

fn human_bytes(bytes: i64) -> String {
    const KIB: f64 = 1024.0;
    let b = bytes.max(0) as f64;
    if b < KIB {
        format!("{} B", bytes.max(0))
    } else if b < KIB * KIB {
        format!("{:.1} KiB", b / KIB)
    } else if b < KIB * KIB * KIB {
        format!("{:.1} MiB", b / (KIB * KIB))
    } else {
        format!("{:.1} GiB", b / (KIB * KIB * KIB))
    }
}

pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    let client = Client::new(config);
    let mut resources = Vec::new();
    let mut token: Option<String> = None;
    loop {
        // `limit` here is bounded 1..=50 by the model's `DescribeLimit`, so a larger
        // page size is refused outright rather than clamped.
        let mut req = client.describe_log_groups().limit(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeLogGroups")?;
        let page = resp.log_groups();
        for g in page {
            let retention = g
                .retention_in_days()
                .map(|n| format!("{n}d"))
                .unwrap_or_else(|| "never".to_string());
            let stored = g
                .stored_bytes()
                .map(human_bytes)
                .unwrap_or_else(|| "0 B".to_string());
            let created = g
                .creation_time()
                .map(format_millis)
                .unwrap_or_else(|| "unknown".to_string());
            resources.push(Resource {
                name: g.log_group_name().unwrap_or_default().to_string(),
                detail: format!("retention {retention} · {stored} · created {created}"),
            });
        }
        // The emulator can hand back a nextToken alongside the last, empty page; stop on
        // either signal so the loop cannot spin.
        if page.is_empty() {
            break;
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(resources)
}

// CreateLogGroup answers ResourceAlreadyExistsException on a duplicate here (unlike
// S3/DynamoDB/SQS), but the dashboard still pre-checks so the UI can warn before submit.
pub async fn log_group_exists(config: &SdkConfig, group: &str) -> Result<bool> {
    let client = Client::new(config);
    let mut token: Option<String> = None;
    loop {
        let mut req = client
            .describe_log_groups()
            .log_group_name_prefix(group)
            .limit(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeLogGroups")?;
        if resp
            .log_groups()
            .iter()
            .any(|g| g.log_group_name() == Some(group))
        {
            return Ok(true);
        }
        let page_empty = resp.log_groups().is_empty();
        token = resp.next_token().map(str::to_string);
        if page_empty || token.is_none() {
            break;
        }
    }
    Ok(false)
}

pub async fn list_streams(config: &SdkConfig, group: &str) -> Result<Vec<LogStreamInfo>> {
    let client = Client::new(config);
    let mut streams = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client
            .describe_log_streams()
            .log_group_name(group)
            .limit(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeLogStreams")?;
        let page = resp.log_streams();
        for s in page {
            streams.push(LogStreamInfo {
                name: s.log_stream_name().unwrap_or_default().to_string(),
                first_event_time: s.first_event_timestamp().unwrap_or(0),
                last_event_time: s.last_event_timestamp().unwrap_or(0),
            });
        }
        if page.is_empty() {
            break;
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(streams)
}

pub async fn get_log_events(
    config: &SdkConfig,
    group: &str,
    stream: &str,
) -> Result<Vec<LogEventInfo>> {
    let client = Client::new(config);
    let mut events = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client
            .get_log_events()
            .log_group_name(group)
            .log_stream_name(stream)
            .start_from_head(true)
            .limit(1000);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("GetLogEvents")?;
        let page = resp.events();
        for e in page {
            events.push(LogEventInfo {
                message: e.message().unwrap_or_default().to_string(),
                timestamp: e.timestamp().unwrap_or(0),
            });
        }
        // GetLogEvents reports a stable nextForwardToken once the head is exhausted, so
        // an unchanged token (or an empty page) ends the walk.
        let next = resp.next_forward_token().map(str::to_string);
        if page.is_empty() || next.is_none() || next == token {
            break;
        }
        token = next;
    }
    Ok(events)
}

pub async fn filter_log_events(
    config: &SdkConfig,
    group: &str,
    pattern: &str,
    limit: i32,
) -> Result<Vec<FilteredLogEventInfo>> {
    let client = Client::new(config);
    let mut events = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client
            .filter_log_events()
            .log_group_name(group)
            .filter_pattern(pattern)
            .start_from_head(true)
            .limit(limit.clamp(1, 10000));
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("FilterLogEvents")?;
        let page = resp.events();
        for e in page {
            events.push(FilteredLogEventInfo {
                stream: e.log_stream_name().unwrap_or_default().to_string(),
                message: e.message().unwrap_or_default().to_string(),
                timestamp: e.timestamp().unwrap_or(0),
            });
        }
        let next = resp.next_token().map(str::to_string);
        if page.is_empty() || next.is_none() || next == token {
            break;
        }
        token = next;
    }
    Ok(events)
}

pub async fn create_log_group(config: &SdkConfig, group: &str) -> Result<()> {
    check_group_name(group)?;
    Client::new(config)
        .create_log_group()
        .log_group_name(group)
        .send()
        .await
        .context("CreateLogGroup")?;
    Ok(())
}

pub async fn delete_log_group(config: &SdkConfig, group: &str) -> Result<()> {
    Client::new(config)
        .delete_log_group()
        .log_group_name(group)
        .send()
        .await
        .context("DeleteLogGroup")?;
    Ok(())
}

pub async fn create_log_stream(config: &SdkConfig, group: &str, stream: &str) -> Result<()> {
    Client::new(config)
        .create_log_stream()
        .log_group_name(group)
        .log_stream_name(stream)
        .send()
        .await
        .context("CreateLogStream")?;
    Ok(())
}

pub async fn delete_log_stream(config: &SdkConfig, group: &str, stream: &str) -> Result<()> {
    Client::new(config)
        .delete_log_stream()
        .log_group_name(group)
        .log_stream_name(stream)
        .send()
        .await
        .context("DeleteLogStream")?;
    Ok(())
}

pub async fn put_log_events(
    config: &SdkConfig,
    group: &str,
    stream: &str,
    messages: &[&str],
) -> Result<()> {
    // Read the stream's current upload token and replay it. The emulator ignores the
    // token (it never raises InvalidSequenceTokenException), but a real Logs endpoint
    // rejects a write that omits it once the stream has data.
    let token = next_sequence_token(config, group, stream).await?;
    put_log_events_with_token(config, group, stream, messages, token.as_deref()).await?;
    Ok(())
}

/// `DescribeLogStreams` is the only doorway that hands back a stream's upload token;
/// `PutLogEvents` returns the *next* one, but a driver that restarts mid-session has to
/// read the current value from the stream itself.
pub async fn next_sequence_token(
    config: &SdkConfig,
    group: &str,
    stream: &str,
) -> Result<Option<String>> {
    let client = Client::new(config);
    let mut token: Option<String> = None;
    loop {
        let mut req = client
            .describe_log_streams()
            .log_group_name(group)
            .log_stream_name_prefix(stream)
            .limit(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeLogStreams")?;
        if let Some(found) = resp
            .log_streams()
            .iter()
            .find(|s| s.log_stream_name() == Some(stream))
            .and_then(|s| s.upload_sequence_token())
        {
            return Ok(Some(found.to_string()));
        }
        let page_empty = resp.log_streams().is_empty();
        token = resp.next_token().map(str::to_string);
        if page_empty || token.is_none() {
            break;
        }
    }
    Ok(None)
}

/// Same as [`put_log_events`] but with an explicit token (pass `None` for the first
/// batch on a fresh stream). Returns the token the next batch must replay.
pub async fn put_log_events_with_token(
    config: &SdkConfig,
    group: &str,
    stream: &str,
    messages: &[&str],
    sequence_token: Option<&str>,
) -> Result<Option<String>> {
    let client = Client::new(config);
    let base = now_ms();
    let mut req = client
        .put_log_events()
        .log_group_name(group)
        .log_stream_name(stream);
    if let Some(token) = sequence_token {
        req = req.sequence_token(token);
    }
    // A batch whose timestamps run backwards is refused, so each event is stamped one
    // millisecond after the last.
    for (offset, message) in messages.iter().enumerate() {
        let event = InputLogEvent::builder()
            .timestamp(base + offset as i64)
            .message(*message)
            .build()
            .context("build InputLogEvent")?;
        req = req.log_events(event);
    }
    let resp = req.send().await.context("PutLogEvents")?;
    Ok(resp.next_sequence_token().map(str::to_string))
}

/// Delete every stream in a group and then the group itself. The emulator cascades on
/// `DeleteLogGroup`, but real Logs refuses while streams (or their events) remain.
pub async fn delete_log_group_tree(config: &SdkConfig, group: &str) -> Result<()> {
    // A group that is already gone has no streams to walk, and DeleteLogGroup on it is a
    // no-op, so the tree delete finishes quietly instead of failing on DescribeLogStreams.
    if log_group_exists(config, group).await? {
        for stream in list_streams(config, group).await? {
            delete_log_stream(config, group, &stream.name).await?;
        }
    }
    delete_log_group(config, group).await
}

/// Reject names the service will not accept before paying for a round trip.
pub fn check_group_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("log group name must not be empty");
    }
    if name.len() > 512 {
        anyhow::bail!(
            "log group name must be at most 512 characters, got {}",
            name.len()
        );
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_graphic() || c == ' ' || c == '\t')
    {
        anyhow::bail!("log group name {name:?} uses characters outside ASCII printables");
    }
    if name.contains(':') {
        anyhow::bail!("log group name {name:?} contains a colon, which the service never accepts");
    }
    for banned in ["::*", "..", "//"] {
        if name.contains(banned) {
            anyhow::bail!("log group name {name:?} contains the reserved sequence {banned:?}");
        }
    }
    Ok(())
}

/// ARN of an existing group. Several doorways (deletion protection, the ARN-keyed
/// tagging API, cross-account filters) only accept an ARN, never a name.
pub async fn log_group_arn(config: &SdkConfig, group: &str) -> Result<String> {
    let resp = Client::new(config)
        .describe_log_groups()
        .log_group_name_prefix(group)
        .limit(50)
        .send()
        .await
        .context("DescribeLogGroups")?;
    resp.log_groups()
        .iter()
        .find(|g| g.log_group_name() == Some(group))
        .and_then(|g| g.arn())
        .map(str::to_string)
        .with_context(|| format!("log group {group} has no ARN (does it exist?)"))
}

pub async fn set_deletion_protection(config: &SdkConfig, group: &str, enabled: bool) -> Result<()> {
    // `LogGroupIdentifier` takes the name or the ARN, and this emulator validates it against
    // a pattern that has no `*` in it — which rules out the `…:log-group:name:*` ARN
    // `DescribeLogGroups` hands back. The name is the identifier that always fits.
    Client::new(config)
        .put_log_group_deletion_protection()
        .log_group_identifier(group)
        .deletion_protection_enabled(enabled)
        .send()
        .await
        .context("PutLogGroupDeletionProtection")?;
    Ok(())
}

/// Field names the group's events carry, with the share of events containing each one.
pub async fn log_group_fields(config: &SdkConfig, group: &str) -> Result<Vec<LogGroupFieldInfo>> {
    let resp = Client::new(config)
        .get_log_group_fields()
        .log_group_name(group)
        .send()
        .await
        .context("GetLogGroupFields")?;
    Ok(resp
        .log_group_fields()
        .iter()
        .map(|f| LogGroupFieldInfo {
            name: f.name().unwrap_or_default().to_string(),
            percent: f.percent(),
        })
        .collect())
}

// --- metric filters ---------------------------------------------------------------

/// Dry-run a filter pattern against sample events; `Vec` has one entry per input event.
pub async fn test_metric_filter(
    config: &SdkConfig,
    pattern: &str,
    samples: &[&str],
) -> Result<Vec<Vec<(String, String)>>> {
    let mut req = Client::new(config)
        .test_metric_filter()
        .filter_pattern(pattern);
    // `LogEventMessages` is a list of raw event strings, not InputLogEvent shapes.
    for sample in samples {
        req = req.log_event_messages((*sample).to_string());
    }
    let resp = req.send().await.context("TestMetricFilter")?;
    Ok(resp
        .matches()
        .iter()
        .map(|m| {
            m.extracted_values()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect()
        })
        .collect())
}

// --- subscription filters & destinations -----------------------------------------

pub async fn put_subscription_filter(
    config: &SdkConfig,
    group: &str,
    filter_name: &str,
    pattern: &str,
    destination_arn: &str,
    role_arn: Option<&str>,
) -> Result<()> {
    let mut req = Client::new(config)
        .put_subscription_filter()
        .log_group_name(group)
        .filter_name(filter_name)
        .filter_pattern(pattern)
        .destination_arn(destination_arn);
    if let Some(role) = role_arn {
        req = req.role_arn(role);
    }
    req.send().await.context("PutSubscriptionFilter")?;
    Ok(())
}

pub async fn describe_subscription_filters(
    config: &SdkConfig,
    group: &str,
) -> Result<Vec<SubscriptionFilterInfo>> {
    let client = Client::new(config);
    let mut filters = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client
            .describe_subscription_filters()
            .log_group_name(group)
            .limit(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeSubscriptionFilters")?;
        let page = resp.subscription_filters();
        for f in page {
            filters.push(SubscriptionFilterInfo {
                filter_name: f.filter_name().unwrap_or_default().to_string(),
                filter_pattern: f.filter_pattern().unwrap_or_default().to_string(),
                destination_arn: f.destination_arn().unwrap_or_default().to_string(),
                role_arn: f.role_arn().unwrap_or_default().to_string(),
            });
        }
        if page.is_empty() {
            break;
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(filters)
}

pub async fn delete_subscription_filter(
    config: &SdkConfig,
    group: &str,
    filter_name: &str,
) -> Result<()> {
    Client::new(config)
        .delete_subscription_filter()
        .log_group_name(group)
        .filter_name(filter_name)
        .send()
        .await
        .context("DeleteSubscriptionFilter")?;
    Ok(())
}

/// Register a cross-account sink. `target_arn` is a Kinesis stream or Firehose
/// delivery stream; the returned string is the destination ARN that a subscription
/// filter points at.
pub async fn put_destination(
    config: &SdkConfig,
    name: &str,
    target_arn: &str,
    role_arn: &str,
) -> Result<String> {
    let resp = Client::new(config)
        .put_destination()
        .destination_name(name)
        .target_arn(target_arn)
        .role_arn(role_arn)
        .send()
        .await
        .context("PutDestination")?;
    Ok(resp
        .destination()
        .and_then(|d| d.arn())
        .unwrap_or_default()
        .to_string())
}

pub async fn set_destination_policy(
    config: &SdkConfig,
    name: &str,
    access_policy: &str,
) -> Result<()> {
    Client::new(config)
        .put_destination_policy()
        .destination_name(name)
        .access_policy(access_policy)
        .send()
        .await
        .context("PutDestinationPolicy")?;
    Ok(())
}

pub async fn describe_destinations(config: &SdkConfig) -> Result<Vec<DestinationInfo>> {
    let client = Client::new(config);
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.describe_destinations().limit(50);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("DescribeDestinations")?;
        let page = resp.destinations();
        for d in page {
            out.push(DestinationInfo {
                name: d.destination_name().unwrap_or_default().to_string(),
                target_arn: d.target_arn().unwrap_or_default().to_string(),
                role_arn: d.role_arn().unwrap_or_default().to_string(),
                arn: d.arn().unwrap_or_default().to_string(),
            });
        }
        if page.is_empty() {
            break;
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(out)
}

pub async fn delete_destination(config: &SdkConfig, name: &str) -> Result<()> {
    Client::new(config)
        .delete_destination()
        .destination_name(name)
        .send()
        .await
        .context("DeleteDestination")?;
    Ok(())
}

// --- queries ----------------------------------------------------------------------

/// Run a `logs` query to completion (bounded polling) and return its rows.
pub async fn run_query(
    config: &SdkConfig,
    group: &str,
    query: &str,
    window_seconds: i64,
) -> Result<Vec<QueryRow>> {
    let client = Client::new(config);
    // `StartTime`/`EndTime` are epoch SECONDS in this model even though event timestamps are
    // milliseconds: passing millis asks for a window in the year 58000 and the emulator
    // answers with an empty, successful result set.
    let now = now_ms() / 1000;
    let started = client
        .start_query()
        .log_group_name(group)
        .query_string(query)
        .start_time(now - window_seconds)
        .end_time(now + 60)
        .send()
        .await
        .context("StartQuery")?;
    let query_id = started
        .query_id()
        .context("StartQuery returned no query id")?
        .to_string();

    for _ in 0..40 {
        let resp = client
            .get_query_results()
            .query_id(&query_id)
            .send()
            .await
            .context("GetQueryResults")?;
        match resp.status() {
            Some(aws_sdk_cloudwatchlogs::types::QueryStatus::Complete)
            | Some(aws_sdk_cloudwatchlogs::types::QueryStatus::Failed)
            | Some(aws_sdk_cloudwatchlogs::types::QueryStatus::Cancelled)
            | Some(aws_sdk_cloudwatchlogs::types::QueryStatus::Timeout) => {
                return Ok(resp
                    .results()
                    .iter()
                    .map(|row| QueryRow {
                        fields: row
                            .iter()
                            .map(|f| {
                                (
                                    f.field().unwrap_or_default().to_string(),
                                    f.value().unwrap_or_default().to_string(),
                                )
                            })
                            .collect(),
                    })
                    .collect());
            }
            _ => tokio::time::sleep(std::time::Duration::from_millis(150)).await,
        }
    }
    let _ = stop_query(config, &query_id).await;
    anyhow::bail!("query {query_id} did not finish in time")
}

pub async fn stop_query(config: &SdkConfig, query_id: &str) -> Result<()> {
    Client::new(config)
        .stop_query()
        .query_id(query_id)
        .send()
        .await
        .context("StopQuery")?;
    Ok(())
}

pub async fn describe_queries(config: &SdkConfig, group: &str) -> Result<Vec<QueryInfo>> {
    let resp = Client::new(config)
        .describe_queries()
        .log_group_name(group)
        .send()
        .await
        .context("DescribeQueries")?;
    Ok(resp
        .queries()
        .iter()
        .map(|q| QueryInfo {
            id: q.query_id().unwrap_or_default().to_string(),
            status: q
                .status()
                .map(|s| s.as_str().to_string())
                .unwrap_or_default(),
            query: q.query_string().unwrap_or_default().to_string(),
        })
        .collect())
}

pub async fn put_retention_policy(config: &SdkConfig, group: &str, days: i32) -> Result<()> {
    Client::new(config)
        .put_retention_policy()
        .log_group_name(group)
        .retention_in_days(days)
        .send()
        .await
        .context("PutRetentionPolicy")?;
    Ok(())
}

pub async fn delete_retention_policy(config: &SdkConfig, group: &str) -> Result<()> {
    Client::new(config)
        .delete_retention_policy()
        .log_group_name(group)
        .send()
        .await
        .context("DeleteRetentionPolicy")?;
    Ok(())
}

// The generic ARN-keyed tagging API rejects an ARN ending in ":*" (its resourceArn
// pattern has no `*`), so these deprecated name-keyed doorways are the reliable ones.
#[allow(deprecated)]
pub async fn tag_log_group(config: &SdkConfig, group: &str, tags: &[(&str, &str)]) -> Result<()> {
    let mut req = Client::new(config).tag_log_group().log_group_name(group);
    for (k, v) in tags {
        req = req.tags(*k, *v);
    }
    req.send().await.context("TagLogGroup")?;
    Ok(())
}

#[allow(deprecated)]
pub async fn untag_log_group(config: &SdkConfig, group: &str, keys: &[&str]) -> Result<()> {
    let mut req = Client::new(config).untag_log_group().log_group_name(group);
    for k in keys {
        req = req.tags(*k);
    }
    req.send().await.context("UntagLogGroup")?;
    Ok(())
}

#[allow(deprecated)]
pub async fn list_tags_log_group(config: &SdkConfig, group: &str) -> Result<Vec<TagInfo>> {
    let resp = Client::new(config)
        .list_tags_log_group()
        .log_group_name(group)
        .send()
        .await
        .context("ListTagsLogGroup")?;
    let map = resp.tags().cloned().unwrap_or_default();
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    Ok(keys
        .into_iter()
        .map(|key| TagInfo {
            value: map.get(&key).cloned().unwrap_or_default(),
            key,
        })
        .collect())
}

pub async fn put_metric_filter(
    config: &SdkConfig,
    group: &str,
    filter_name: &str,
    pattern: &str,
    metric_name: &str,
    namespace: &str,
    metric_value: &str,
) -> Result<()> {
    let transformation = MetricTransformation::builder()
        .metric_name(metric_name)
        .metric_namespace(namespace)
        .metric_value(metric_value)
        .build()
        .context("build MetricTransformation")?;
    Client::new(config)
        .put_metric_filter()
        .log_group_name(group)
        .filter_name(filter_name)
        .filter_pattern(pattern)
        .metric_transformations(transformation)
        .send()
        .await
        .context("PutMetricFilter")?;
    Ok(())
}

pub async fn describe_metric_filters(
    config: &SdkConfig,
    group: &str,
) -> Result<Vec<MetricFilterInfo>> {
    let resp = Client::new(config)
        .describe_metric_filters()
        .log_group_name(group)
        .send()
        .await
        .context("DescribeMetricFilters")?;
    Ok(resp
        .metric_filters()
        .iter()
        .map(|f| {
            let metric = f
                .metric_transformations()
                .first()
                .map(|t| format!("{}/{}", t.metric_namespace(), t.metric_name()))
                .unwrap_or_default();
            MetricFilterInfo {
                filter_name: f.filter_name().unwrap_or_default().to_string(),
                filter_pattern: f.filter_pattern().unwrap_or_default().to_string(),
                metric,
            }
        })
        .collect())
}

pub async fn delete_metric_filter(
    config: &SdkConfig,
    group: &str,
    filter_name: &str,
) -> Result<()> {
    Client::new(config)
        .delete_metric_filter()
        .log_group_name(group)
        .filter_name(filter_name)
        .send()
        .await
        .context("DeleteMetricFilter")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    /// Records one capability-probe line, keeping the service's own wording for a FAIL
    /// so the dashboard's feature table quotes the emulator rather than the driver.
    fn probe(out: &mut Vec<String>, name: &str, result: Result<()>) {
        match result {
            Ok(()) => out.push(format!("{name}: OK")),
            Err(e) => out.push(format!("{name}: FAIL: {e:#}")),
        }
    }

    macro_rules! driver {
        ($out:expr, $name:expr, $call:expr) => {
            probe($out, $name, ($call).await.map(|_| ()))
        };
    }

    macro_rules! raw {
        ($out:expr, $name:expr, $call:expr) => {
            probe(
                $out,
                $name,
                ($call).await.map(|_| ()).map_err(anyhow::Error::new),
            )
        };
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes a log group"]
    async fn live_cloudwatch_logs_lifecycle() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("aws config");

        let group = format!("dashboard-cwl-{}", now_ms());
        let stream = "dashboard-stream";
        let metric_filter = "dashboard-metric-filter";
        let subscription = "dashboard-subscription";
        let destination = format!("dashboard-cwl-dest-{}", now_ms());

        // Best-effort pre-clean so a crashed earlier run cannot make CreateLogGroup fail.
        let _ = delete_log_group(&config, &group).await;

        let outcome = async {
            check_group_name(&group)?;
            assert!(!log_group_exists(&config, &group).await?);
            create_log_group(&config, &group).await?;
            assert!(log_group_exists(&config, &group).await?);
            assert!(log_group_arn(&config, &group).await?.contains(&group));
            create_log_stream(&config, &group, stream).await?;

            // The first batch on a fresh stream carries no token; the token it earns is
            // read back through DescribeLogStreams and replayed by the second batch.
            assert_eq!(next_sequence_token(&config, &group, stream).await?, None);
            put_log_events_with_token(
                &config,
                &group,
                stream,
                &["dashboard event one", "dashboard event ERROR two"],
                None,
            )
            .await?;
            let token = next_sequence_token(&config, &group, stream).await?;
            assert!(token.is_some(), "no upload token after the first batch");
            put_log_events_with_token(
                &config,
                &group,
                stream,
                &["dashboard event three"],
                token.as_deref(),
            )
            .await?;
            // The plain helper resolves the token itself.
            put_log_events(&config, &group, stream, &["dashboard event four"]).await?;

            let groups = fetch(&config).await?;
            assert!(
                groups.iter().any(|r| r.name == group),
                "created group missing from DescribeLogGroups"
            );

            let streams = list_streams(&config, &group).await?;
            assert!(
                streams.iter().any(|s| s.name == stream),
                "created stream missing from DescribeLogStreams"
            );

            let events = get_log_events(&config, &group, stream).await?;
            let texts: Vec<&str> = events.iter().map(|e| e.message.as_str()).collect();
            assert_eq!(
                texts,
                vec![
                    "dashboard event one",
                    "dashboard event ERROR two",
                    "dashboard event three",
                    "dashboard event four",
                ],
                "written messages did not read back in order"
            );

            let hits = filter_log_events(&config, &group, "ERROR", 100).await?;
            assert_eq!(
                hits.len(),
                1,
                "FilterLogEvents should match only the ERROR line"
            );

            // Retention lands on the card's detail line, and back to `never` on delete.
            put_retention_policy(&config, &group, 7).await?;
            let card = fetch(&config)
                .await?
                .into_iter()
                .find(|r| r.name == group)
                .expect("card for the created group");
            assert!(
                card.detail.contains("retention 7d"),
                "retention missing from detail: {}",
                card.detail
            );
            delete_retention_policy(&config, &group).await?;
            let card = fetch(&config)
                .await?
                .into_iter()
                .find(|r| r.name == group)
                .expect("card after retention delete");
            assert!(
                card.detail.contains("retention never"),
                "retention did not reset: {}",
                card.detail
            );

            tag_log_group(&config, &group, &[("dashboard", "yes")]).await?;
            let tags = list_tags_log_group(&config, &group).await?;
            assert!(
                tags.iter()
                    .any(|t| t.key == "dashboard" && t.value == "yes"),
                "tag did not round-trip"
            );
            untag_log_group(&config, &group, &["dashboard"]).await?;
            assert!(list_tags_log_group(&config, &group).await?.is_empty());

            put_metric_filter(
                &config,
                &group,
                metric_filter,
                "ERROR",
                "DashboardErrors",
                "Dashboard/CloudWatchLogs",
                "1",
            )
            .await?;
            assert!(
                describe_metric_filters(&config, &group)
                    .await?
                    .iter()
                    .any(|f| f.filter_name == metric_filter)
            );
            delete_metric_filter(&config, &group, metric_filter).await?;
            assert!(describe_metric_filters(&config, &group).await?.is_empty());

            // Cross-account sink: destination -> access policy -> subscription filter.
            let dest_arn = put_destination(
                &config,
                &destination,
                &format!("arn:aws:kinesis:us-east-1::stream/{destination}"),
                "arn:aws:iam::123456789012:role/dashboard-logs-role",
            )
            .await?;
            assert!(!dest_arn.is_empty(), "PutDestination returned no ARN");
            // The emulator checks the destination's own resource policy before it lets a
            // log group subscribe to it, so the policy has to name this account.
            set_destination_policy(
                &config,
                &destination,
                &format!(
                    r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"AWS":"arn:aws:iam::000000000000:root"}},"Action":"logs:PutSubscriptionFilter","Resource":"{dest_arn}"}}]}}"#
                ),
            )
            .await?;
            put_subscription_filter(
                &config,
                &group,
                subscription,
                "ERROR",
                &dest_arn,
                Some("arn:aws:iam::123456789012:role/dashboard-logs-role"),
            )
            .await?;
            let subs = describe_subscription_filters(&config, &group).await?;
            assert!(
                subs.iter()
                    .any(|s| s.filter_name == subscription && s.destination_arn == dest_arn),
                "subscription filters: {subs:?}"
            );
            delete_subscription_filter(&config, &group, subscription).await?;
            assert!(
                describe_subscription_filters(&config, &group)
                    .await?
                    .is_empty()
            );
            delete_destination(&config, &destination).await?;

            assert!(!log_group_fields(&config, &group).await?.is_empty());
            let rows = run_query(
                &config,
                &group,
                "fields @message | filter @message like /ERROR/",
                600,
            )
            .await?;
            assert!(
                rows.iter().any(|r| r
                    .fields
                    .iter()
                    .any(|(_, v)| v == "dashboard event ERROR two")),
                "query returned no ERROR row: {rows:?}"
            );

            Ok::<_, anyhow::Error>(())
        }
        .await;

        // Guaranteed cleanup: whatever happened above, the test-owned group and the
        // destination go away before the assertion on the outcome.
        let _ = delete_subscription_filter(&config, &group, subscription).await;
        let _ = delete_metric_filter(&config, &group, metric_filter).await;
        let _ = delete_destination(&config, &destination).await;
        let cleanup = delete_log_group_tree(&config, &group).await;

        // The lifecycle error is the interesting one; a cleanup failure is reported after it
        // so the two never mask each other.
        outcome.expect("cloudwatch logs lifecycle");
        cleanup.expect("delete log group tree (cleanup)");

        assert!(
            !log_group_exists(&config, &group)
                .await
                .expect("post-delete existence check"),
            "group still present after delete"
        );
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; walks every Logs operation"]
    async fn live_cloudwatch_logs_capability_probe() {
        let conn = EmulatorConnection::from_port(9297);
        let config = conn.build_aws_config().await.expect("aws config");
        let client = Client::new(&config);
        let stamp = now_ms();
        let group = format!("dash-cwl-probe{stamp}");
        let stream = "probe-stream";
        let dest_name = format!("dashCwlProbeDest{stamp}");
        let role = "arn:aws:iam::123456789012:role/dashboard-logs-role";
        let mut out: Vec<String> = Vec::new();
        // Every group this probe may have created, torn down at the end.
        let mut created = vec![group.clone()];
        for suffix in ["-tags", "-kms", "-spaces", "-long", "-b"] {
            created.push(format!("{group}{suffix}"));
        }

        // --- groups ------------------------------------------------------------
        driver!(
            &mut out,
            "CreateLogGroup",
            create_log_group(&config, &group)
        );
        raw!(
            &mut out,
            "CreateLogGroup duplicate",
            client.create_log_group().log_group_name(&group).send()
        );
        raw!(
            &mut out,
            "CreateLogGroup with tags",
            client
                .create_log_group()
                .log_group_name(format!("{group}-tags"))
                .tags("dashboard", "yes")
                .send()
        );
        raw!(
            &mut out,
            "CreateLogGroup kmsKeyId",
            client
                .create_log_group()
                .log_group_name(format!("{group}-kms"))
                .kms_key_id("arn:aws:kms:us-east-1::key/00000000-0000-0000-0000-000000000000")
                .send()
        );
        raw!(
            &mut out,
            "CreateLogGroup name with spaces",
            client
                .create_log_group()
                .log_group_name(format!("{group}-spaces"))
                .send()
        );
        raw!(
            &mut out,
            "CreateLogGroup 513-char name",
            client
                .create_log_group()
                .log_group_name(format!("{group}-long"))
                .send()
        );
        raw!(&mut out, "ListLogGroups", client.list_log_groups().send());
        raw!(
            &mut out,
            "DescribeLogGroups limit 50",
            client.describe_log_groups().limit(50).send()
        );
        raw!(
            &mut out,
            "DescribeLogGroups limit 100 (model max 50)",
            client.describe_log_groups().limit(100).send()
        );
        raw!(
            &mut out,
            "DescribeLogGroups limit 0",
            client.describe_log_groups().limit(0).send()
        );
        raw!(
            &mut out,
            "DescribeLogGroups by prefix",
            client
                .describe_log_groups()
                .log_group_name_prefix(&group)
                .send()
        );
        raw!(
            &mut out,
            "DescribeLogGroups by pattern",
            client
                .describe_log_groups()
                .log_group_name_pattern("dash-cwl-probe")
                .send()
        );
        raw!(
            &mut out,
            "DescribeLogGroups logGroupClass",
            client
                .describe_log_groups()
                .log_group_class(aws_sdk_cloudwatchlogs::types::LogGroupClass::Standard)
                .send()
        );
        raw!(
            &mut out,
            "DeleteLogGroup unknown",
            client
                .delete_log_group()
                .log_group_name(format!("{group}-absent"))
                .send()
        );

        // --- streams -----------------------------------------------------------
        driver!(
            &mut out,
            "CreateLogStream",
            create_log_stream(&config, &group, stream)
        );
        raw!(
            &mut out,
            "CreateLogStream duplicate",
            client
                .create_log_stream()
                .log_group_name(&group)
                .log_stream_name(stream)
                .send()
        );
        raw!(
            &mut out,
            "CreateLogStream unknown group",
            client
                .create_log_stream()
                .log_group_name(format!("{group}-b"))
                .log_stream_name(stream)
                .send()
        );
        raw!(
            &mut out,
            "DescribeLogStreams limit 50",
            client
                .describe_log_streams()
                .log_group_name(&group)
                .limit(50)
                .send()
        );
        raw!(
            &mut out,
            "DescribeLogStreams limit 51 (model max 50)",
            client
                .describe_log_streams()
                .log_group_name(&group)
                .limit(51)
                .send()
        );
        raw!(
            &mut out,
            "DescribeLogStreams order by LastEventTime",
            client
                .describe_log_streams()
                .log_group_name(&group)
                .order_by(aws_sdk_cloudwatchlogs::types::OrderBy::LastEventTime)
                .descending(true)
                .send()
        );
        raw!(
            &mut out,
            "DescribeLogStreams unknown group",
            client
                .describe_log_streams()
                .log_group_name(format!("{group}-absent"))
                .send()
        );

        // --- events ------------------------------------------------------------
        driver!(
            &mut out,
            "PutLogEvents first batch, no token",
            put_log_events_with_token(&config, &group, stream, &["alpha ERROR", "beta"], None)
        );
        driver!(
            &mut out,
            "PutLogEvents replaying the read-back token",
            put_log_events(&config, &group, stream, &["gamma"])
        );
        raw!(
            &mut out,
            "PutLogEvents stale token",
            client
                .put_log_events()
                .log_group_name(&group)
                .log_stream_name(stream)
                .sequence_token("00000000000000000000000000000000000000000000000000")
                .log_events(
                    InputLogEvent::builder()
                        .timestamp(now_ms())
                        .message("stale")
                        .build()
                        .unwrap()
                )
                .send()
        );
        raw!(
            &mut out,
            "PutLogEvents timestamps run backwards",
            client
                .put_log_events()
                .log_group_name(&group)
                .log_stream_name(stream)
                .log_events(
                    InputLogEvent::builder()
                        .timestamp(now_ms())
                        .message("new")
                        .build()
                        .unwrap()
                )
                .log_events(
                    InputLogEvent::builder()
                        .timestamp(now_ms() - 60_000)
                        .message("old")
                        .build()
                        .unwrap()
                )
                .send()
        );
        raw!(
            &mut out,
            "PutLogEvents empty batch",
            client
                .put_log_events()
                .log_group_name(&group)
                .log_stream_name(stream)
                .send()
        );
        raw!(
            &mut out,
            "PutLogEvents unknown stream",
            client
                .put_log_events()
                .log_group_name(&group)
                .log_stream_name("probe-absent")
                .log_events(
                    InputLogEvent::builder()
                        .timestamp(now_ms())
                        .message("x")
                        .build()
                        .unwrap()
                )
                .send()
        );
        driver!(
            &mut out,
            "GetLogEvents",
            get_log_events(&config, &group, stream)
        );
        raw!(
            &mut out,
            "GetLogEvents startFromHead=false",
            client
                .get_log_events()
                .log_group_name(&group)
                .log_stream_name(stream)
                .start_from_head(false)
                .limit(10)
                .send()
        );
        raw!(
            &mut out,
            "GetLogEvents limit 10000",
            client
                .get_log_events()
                .log_group_name(&group)
                .log_stream_name(stream)
                .limit(10000)
                .send()
        );
        raw!(
            &mut out,
            "GetLogEvents unknown stream",
            client
                .get_log_events()
                .log_group_name(&group)
                .log_stream_name("probe-absent")
                .send()
        );
        driver!(
            &mut out,
            "FilterLogEvents single term",
            filter_log_events(&config, &group, "ERROR", 10)
        );
        raw!(
            &mut out,
            "FilterLogEvents literal OR (?a ?b)",
            client
                .filter_log_events()
                .log_group_name(&group)
                .filter_pattern("?ERROR ?gamma")
                .limit(10)
                .send()
        );
        raw!(
            &mut out,
            "FilterLogEvents unknown group",
            client
                .filter_log_events()
                .log_group_name(format!("{group}-absent"))
                .limit(10)
                .send()
        );
        driver!(
            &mut out,
            "GetLogGroupFields",
            log_group_fields(&config, &group)
        );

        // Sequence-token plumbing as seen by the driver.
        let token = next_sequence_token(&config, &group, stream).await;
        out.push(format!(
            "DescribeLogStreams uploadSequenceToken: {}",
            match &token {
                Ok(Some(_)) => "OK (present)".to_string(),
                Ok(None) => "OK (absent)".to_string(),
                Err(e) => format!("FAIL: {e:#}"),
            }
        ));

        // --- retention & protection -------------------------------------------
        driver!(
            &mut out,
            "PutRetentionPolicy 7",
            put_retention_policy(&config, &group, 7)
        );
        raw!(
            &mut out,
            "PutRetentionPolicy 13 (not an enum value)",
            client
                .put_retention_policy()
                .log_group_name(&group)
                .retention_in_days(13)
                .send()
        );
        driver!(
            &mut out,
            "DeleteRetentionPolicy",
            delete_retention_policy(&config, &group)
        );
        driver!(
            &mut out,
            "PutLogGroupDeletionProtection on",
            set_deletion_protection(&config, &group, true)
        );
        raw!(
            &mut out,
            "DeleteLogGroup under protection",
            client.delete_log_group().log_group_name(&group).send()
        );
        driver!(
            &mut out,
            "PutLogGroupDeletionProtection off",
            set_deletion_protection(&config, &group, false)
        );

        // --- tags --------------------------------------------------------------
        driver!(
            &mut out,
            "TagLogGroup (deprecated name API)",
            tag_log_group(&config, &group, &[("probe", "1")])
        );
        driver!(
            &mut out,
            "ListTagsLogGroup",
            list_tags_log_group(&config, &group)
        );
        driver!(
            &mut out,
            "UntagLogGroup",
            untag_log_group(&config, &group, &["probe"])
        );
        let arn = log_group_arn(&config, &group).await.unwrap_or_default();
        raw!(
            &mut out,
            "TagResource by ARN",
            client
                .tag_resource()
                .resource_arn(&arn)
                .tags("probe", "1")
                .send()
        );
        raw!(
            &mut out,
            "ListTagsForResource by ARN",
            client.list_tags_for_resource().resource_arn(&arn).send()
        );
        raw!(
            &mut out,
            "ListTagsForResource ':*' ARN",
            client
                .list_tags_for_resource()
                .resource_arn(format!("{arn}:*"))
                .send()
        );
        raw!(
            &mut out,
            "UntagResource by ARN",
            client
                .untag_resource()
                .resource_arn(&arn)
                .tag_keys("probe")
                .send()
        );

        // --- metric filters ----------------------------------------------------
        driver!(
            &mut out,
            "PutMetricFilter",
            put_metric_filter(
                &config,
                &group,
                "probe-metric-filter",
                "ERROR",
                "ProbeErrors",
                "Dashboard/Probe",
                "1"
            )
        );
        driver!(
            &mut out,
            "DescribeMetricFilters",
            describe_metric_filters(&config, &group)
        );
        driver!(
            &mut out,
            "TestMetricFilter",
            test_metric_filter(&config, "ERROR", &["alpha ERROR", "beta ok"])
        );
        driver!(
            &mut out,
            "DeleteMetricFilter",
            delete_metric_filter(&config, &group, "probe-metric-filter")
        );

        // --- destinations & subscriptions --------------------------------------
        let dest_arn = match put_destination(
            &config,
            &dest_name,
            &format!("arn:aws:kinesis:us-east-1::stream/{dest_name}"),
            role,
        )
        .await
        {
            Ok(arn) => {
                out.push("PutDestination: OK".to_string());
                arn
            }
            Err(e) => {
                out.push(format!("PutDestination: FAIL: {e:#}"));
                String::new()
            }
        };
        driver!(
            &mut out,
            "DescribeDestinations",
            describe_destinations(&config)
        );
        driver!(
            &mut out,
            "PutDestinationPolicy",
            set_destination_policy(
                &config,
                &dest_name,
                r#"{"Version":"2012-10-17","Statement":[]}"#
            )
        );
        driver!(
            &mut out,
            "PutSubscriptionFilter to logs destination",
            put_subscription_filter(&config, &group, "probe-sub", "ERROR", &dest_arn, Some(role))
        );
        driver!(
            &mut out,
            "DescribeSubscriptionFilters",
            describe_subscription_filters(&config, &group)
        );
        driver!(
            &mut out,
            "PutSubscriptionFilter to lambda, no role",
            put_subscription_filter(
                &config,
                &group,
                "probe-sub-lambda",
                "ERROR",
                "arn:aws:lambda:us-east-1::function:probe-fn",
                None
            )
        );
        driver!(
            &mut out,
            "PutSubscriptionFilter to kinesis, no role",
            put_subscription_filter(
                &config,
                &group,
                "probe-sub-kinesis",
                "ERROR",
                &format!("arn:aws:kinesis:us-east-1::stream/{dest_name}"),
                None
            )
        );
        driver!(
            &mut out,
            "DeleteSubscriptionFilter",
            delete_subscription_filter(&config, &group, "probe-sub")
        );
        driver!(
            &mut out,
            "DeleteDestination",
            delete_destination(&config, &dest_name)
        );

        // --- queries -----------------------------------------------------------
        // A bare `?ERROR` filter pattern is not the Insights language (it answers
        // MalformedQueryException), and the window is in seconds, not milliseconds.
        match client
            .start_query()
            .log_group_name(&group)
            .query_string("fields @message | filter @message like /ERROR/")
            .start_time(now_ms() / 1000 - 600)
            .end_time(now_ms() / 1000 + 60)
            .send()
            .await
        {
            Ok(resp) => {
                out.push("StartQuery: OK".to_string());
                let qid = resp.query_id().unwrap_or_default().to_string();
                raw!(
                    &mut out,
                    "GetQueryResults",
                    client.get_query_results().query_id(&qid).send()
                );
                raw!(
                    &mut out,
                    "DescribeQueries",
                    client.describe_queries().log_group_name(&group).send()
                );
                raw!(
                    &mut out,
                    "StopQuery",
                    client.stop_query().query_id(&qid).send()
                );
            }
            Err(e) => out.push(format!("StartQuery: FAIL: {:#}", anyhow::Error::new(e))),
        }
        raw!(
            &mut out,
            "StartQuery bad syntax",
            client
                .start_query()
                .log_group_name(&group)
                .query_string("this is not a query")
                .start_time(now_ms() - 600_000)
                .end_time(now_ms())
                .send()
        );
        driver!(
            &mut out,
            "run_query helper",
            run_query(&config, &group, "fields @timestamp | limit 1", 600)
        );

        // --- read-only neighbours ----------------------------------------------
        raw!(
            &mut out,
            "DescribeAccountPolicies",
            client.describe_account_policies().send()
        );
        raw!(
            &mut out,
            "DescribeResourcePolicies",
            client.describe_resource_policies().send()
        );
        raw!(
            &mut out,
            "DescribeExportTasks",
            client.describe_export_tasks().send()
        );
        raw!(
            &mut out,
            "DescribeImportTasks",
            client.describe_import_tasks().send()
        );
        raw!(
            &mut out,
            "ListScheduledQueries",
            client.list_scheduled_queries().send()
        );
        raw!(
            &mut out,
            "DescribeDeliveries",
            client.describe_deliveries().send()
        );
        raw!(
            &mut out,
            "ListIntegrations",
            client.list_integrations().send()
        );

        // --- operations the emulator still scaffolds ---------------------------
        raw!(
            &mut out,
            "ListLogAnomalyDetectors",
            client.list_log_anomaly_detectors().send()
        );
        raw!(
            &mut out,
            "DescribeImportTaskBatches",
            client
                .describe_import_task_batches()
                .import_id("probe-task")
                .send()
        );
        raw!(
            &mut out,
            "CancelImportTask",
            client.cancel_import_task().import_id("probe-task").send()
        );
        raw!(
            &mut out,
            "GetScheduledQueryHistory",
            client
                .get_scheduled_query_history()
                .identifier("probe-query")
                .send()
        );
        raw!(
            &mut out,
            "ListTagsForResource stream ARN",
            client
                .list_tags_for_resource()
                .resource_arn(format!("{arn}:log-stream:probe-stream"))
                .send()
        );

        // --- teardown ------------------------------------------------------------
        for sub in ["probe-sub-lambda", "probe-sub-kinesis"] {
            let _ = delete_subscription_filter(&config, &group, sub).await;
        }
        let _ = delete_metric_filter(&config, &group, "probe-metric-filter").await;
        let _ = delete_destination(&config, &dest_name).await;
        for g in &created {
            let _ = set_deletion_protection(&config, g, false).await;
            let _ = delete_log_group_tree(&config, g).await;
        }
        let residue: Vec<String> = fetch(&config)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.name)
            .filter(|n| n.starts_with("dash-cwl-probe"))
            .collect();

        println!(
            "CloudWatch Logs capability probe:\n{}\nleft behind: {:?}",
            out.join("\n"),
            residue
        );
        assert!(
            residue.is_empty(),
            "probe left log groups behind: {residue:?}"
        );
    }

    #[test]
    fn rejects_names_the_service_cannot_store() {
        assert!(check_group_name("/aws/lambda/dashboard").is_ok());
        assert!(check_group_name("").is_err());
        assert!(check_group_name(&"a".repeat(513)).is_err());
        assert!(check_group_name("bad\u{7}char").is_err());
        assert!(check_group_name("a:b*c").is_err());
    }

    #[test]
    fn formats_sizes_and_timestamps() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
        assert!(format_millis(0).starts_with("1970-01-01"));
    }
}
