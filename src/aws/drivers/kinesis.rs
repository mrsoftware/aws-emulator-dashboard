use crate::app_state::Resource;
use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_kinesis::{Client, types};
use std::collections::HashMap;

pub const LABEL: &str = "Kinesis";

/// Widest page the driver asks for. Every paging member here is an `i32`, and the emulator
/// checks only the lower bound: `limit=0` answers `ValidationException: 1 validation error
/// detected: Value '0' at 'limit' failed to satisfy constraint: Member must have value
/// greater than or equal to 1`, while `limit=1001` and `max_results=10000` are accepted even
/// though the published API ceiling is 1000 and 10000 respectively.
const PAGE_SIZE: i32 = 100;

fn client(config: &SdkConfig) -> Client {
    Client::new(config)
}

/// One shard as `ListShards` reports it. The hash-key bounds are the decimal-string form of
/// 128-bit numbers, so they are kept as strings rather than parsed: a two-shard stream answers
/// `"0"`/`"170141183460469231731687303715884105727"` and the next hundred-and-seventeen
/// trillion-and-one up to `"340282366920938463463374607431768211455"`, i.e. the whole key space
/// split evenly. `parent_id` and `ending_sequence_number` are empty on a freshly created stream.
#[derive(Clone, Debug, Default)]
pub struct ShardInfo {
    pub id: String,
    pub parent_id: String,
    pub begin_hash_key: String,
    pub end_hash_key: String,
    /// Empty while the shard is still open and accepting records.
    pub ending_sequence_number: String,
}

/// Where a `PutRecord` landed: the shard it was routed to and the sequence number that
/// shard assigned it.
#[derive(Clone, Debug, Default)]
pub struct RecordLocation {
    pub shard_id: String,
    pub sequence_number: String,
}

/// A record as `GetRecords` returns it. `data` is the payload decoded as UTF-8 with
/// lossy fallback, because the dashboard only ever shows it.
#[derive(Clone, Debug, Default)]
pub struct RecordInfo {
    pub sequence_number: String,
    pub partition_key: String,
    pub data: String,
    pub byte_len: usize,
}

/// A `GetRecords` page. The next iterator is part of the response, not the request, so a
/// caller that wants to keep reading has to carry it forward.
#[derive(Clone, Debug, Default)]
pub struct RecordsPage {
    pub records: Vec<RecordInfo>,
    pub next_shard_iterator: Option<String>,
    pub millis_behind_latest: Option<i64>,
}

fn shard_info(shard: &types::Shard) -> ShardInfo {
    let hash = shard.hash_key_range();
    let range = shard.sequence_number_range();
    ShardInfo {
        id: shard.shard_id().to_string(),
        parent_id: shard.parent_shard_id().unwrap_or_default().to_string(),
        begin_hash_key: hash
            .map(|h| h.starting_hash_key().to_string())
            .unwrap_or_default(),
        end_hash_key: hash
            .map(|h| h.ending_hash_key().to_string())
            .unwrap_or_default(),
        ending_sequence_number: range
            .and_then(|r| r.ending_sequence_number())
            .unwrap_or_default()
            .to_string(),
    }
}

fn record_info(record: &types::Record) -> RecordInfo {
    let data = record.data().as_ref();
    RecordInfo {
        sequence_number: record.sequence_number().to_string(),
        partition_key: record.partition_key().unwrap_or_default().to_string(),
        data: String::from_utf8_lossy(data).to_string(),
        byte_len: data.len(),
    }
}

/// `Status · Mode · ?` from a `ListStreams.StreamSummary`, used when every describe call for
/// that stream failed: this response carries the status and the mode but no shard count.
fn listed_resource(summary: &types::StreamSummary) -> Resource {
    Resource {
        name: summary.stream_name().to_string(),
        detail: format!(
            "{} · {} · ? shard(s)",
            summary.stream_status().as_str(),
            stream_mode(summary.stream_mode_details())
        ),
    }
}

/// An absent `StreamModeDetails` is provisioned billing in the real API, so the driver
/// says so instead of leaving the field blank.
fn stream_mode(details: Option<&types::StreamModeDetails>) -> String {
    details
        .map(|d| d.stream_mode().as_str().to_string())
        .unwrap_or_else(|| "PROVISIONED".to_string())
}

/// Every stream name, following `NextToken` to the end.
///
/// aws-sdk-kinesis 1.132 exposes `ListStreams` pagination as `next_token` /
/// `NextToken`; there is no `NextSharingToken` member on this operation.
async fn list_all_streams(config: &SdkConfig) -> Result<Vec<String>> {
    Ok(list_all_streams_with_summaries(config).await?.0)
}

/// The stream names in listing order, beside the `StreamSummary` rows `ListStreams` already
/// returned, so a failed per-stream describe still yields a useful detail line.
async fn list_all_streams_with_summaries(
    config: &SdkConfig,
) -> Result<(Vec<String>, HashMap<String, types::StreamSummary>)> {
    let mut names = Vec::new();
    let mut summaries = HashMap::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client(config).list_streams().limit(PAGE_SIZE);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListStreams")?;
        for name in resp.stream_names() {
            names.push(name.clone());
        }
        for summary in resp.stream_summaries() {
            summaries.insert(summary.stream_name().to_string(), summary.clone());
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok((names, summaries))
}

/// One row per stream: `Status · Mode · ShardCount`.
///
/// `ListStreams` alone gives names, so each stream costs exactly one extra call. A call that
/// fails for one stream — a stream deleted between the two calls, for instance — must not drop
/// the whole page, so the row degrades through `DescribeStream` to the summary `ListStreams`
/// already returned. That last step is not hypothetical: this emulator does fill
/// `StreamSummaries` in, with the status and the mode (measured: one name, one summary, status
/// `ACTIVE`), so a stream with no working describe still shows up with something true about it.
pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    let (names, summaries) = list_all_streams_with_summaries(config).await?;
    let mut resources = Vec::with_capacity(names.len());
    for name in names {
        let resource = match describe_stream_detail(config, &name).await {
            Some(detail) => Resource { name, detail },
            None => summaries
                .get(&name)
                .map(listed_resource)
                .unwrap_or(Resource {
                    name,
                    detail: "unknown · ? · ? shard(s)".to_string(),
                }),
        };
        resources.push(resource);
    }
    Ok(resources)
}

/// `Status · Mode · N shard(s)` for one stream, from `DescribeStreamSummary`. Measured, that
/// call answers in every state a stream passes through — `CREATING`, `ACTIVE`, `UPDATING` and
/// `DELETING` — so the `DescribeStream` step below is a guard against a call that fails for a
/// reason the matrix did not produce, not a routine second choice.
async fn describe_stream_detail(config: &SdkConfig, name: &str) -> Option<String> {
    let summary = client(config)
        .describe_stream_summary()
        .stream_name(name)
        .send()
        .await
        .ok()?
        .stream_description_summary()
        .cloned();
    if let Some(summary) = summary {
        return Some(detail_line(
            summary.stream_status().as_str(),
            &stream_mode(summary.stream_mode_details()),
            summary.open_shard_count(),
        ));
    }
    let described = client(config)
        .describe_stream()
        .stream_name(name)
        .limit(PAGE_SIZE)
        .send()
        .await
        .ok()?
        .stream_description()
        .cloned()?;
    // `DescribeStream` returns the shard list, not a count, and it pages: with
    // `HasMoreShards` the count below is a floor, not the total.
    let shards = described.shards().len() as i32;
    Some(detail_line(
        described.stream_status().as_str(),
        &stream_mode(described.stream_mode_details()),
        shards,
    ))
}

fn detail_line(status: &str, mode: &str, shard_count: i32) -> String {
    format!("{status} · {mode} · {shard_count} shard(s)")
}

pub async fn list_stream_names(config: &SdkConfig) -> Result<Vec<String>> {
    list_all_streams(config).await
}

/// Answers `false` only for the error the emulator actually raises on an unknown name,
/// `ResourceNotFoundException: Stream <name> under account 000000000000 not found.`;
/// `DescribeStreamSummary` is the cheapest probe that raises it, because `ListStreams` has no
/// `ResourceNotFoundException` variant at all and so cannot tell the two apart. Any other
/// refusal is returned as an error rather than reported as "does not exist".
/// Live tests only — `create_resource` pre-checks through `fetch`.
#[cfg(test)]
pub async fn stream_exists(config: &SdkConfig, name: &str) -> Result<bool> {
    match client(config)
        .describe_stream_summary()
        .stream_name(name)
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(e)
            if e.as_service_error()
                .is_some_and(|s| s.is_resource_not_found_exception()) =>
        {
            Ok(false)
        }
        Err(e) => Err(anyhow::Error::new(e).context("DescribeStreamSummary")),
    }
}

/// Current stream status. All four values the model defines were observed on this emulator:
/// `CREATING` straight after `CreateStream`, `ACTIVE` a second later, `UPDATING` after
/// `UpdateShardCount`, `DELETING` straight after `DeleteStream` — and a name that is gone
/// answers `ResourceNotFoundException` rather than an empty status.
pub async fn stream_state(config: &SdkConfig, name: &str) -> Result<String> {
    let resp = client(config)
        .describe_stream_summary()
        .stream_name(name)
        .send()
        .await
        .context("DescribeStreamSummary")?;
    resp.stream_description_summary()
        .map(|s| s.stream_status().as_str().to_string())
        .context("DescribeStreamSummary returned no stream status")
}

/// The emulator advances stream statuses on a clock, so callers must wait for a transition
/// to land before issuing the next one (mirrors `ec2::wait_state`). Live tests only.
#[cfg(test)]
pub async fn wait_state(config: &SdkConfig, name: &str, want: &str, secs: u64) -> Result<()> {
    for _ in 0..(secs * 2) {
        if stream_state(config, name).await? == want {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    anyhow::bail!("stream {name} did not reach {want} within {secs}s")
}

/// Creates a stream with `shard_count` shards.
///
/// aws-sdk-kinesis 1.132 gives `CreateStreamInput` a required `shard_count(i32)`; there is no
/// `consumer_count` member on this operation, and an on-demand stream is asked for with
/// `stream_mode_details(...)`, not by dropping the shard count. What the emulator enforces:
///
/// * `shard_count = 0` → `ValidationException: 1 validation error detected: Value '0' at
///   'shardCount' failed to satisfy constraint: Member must have value greater than or equal to 1`
/// * a count past the account's 1000-shard ceiling → `LimitExceededException: This request
///   would exceed the shard limit for the account … Current shard count for the account: N.
///   Limit: 1000.` (`DescribeLimits` reports that ceiling)
/// * a name already taken → `ResourceInUseException: stream <name> already exists in this
///   account and region; delete it before creating another under that name` — so a create is
///   *not* idempotent here, unlike SNS `CreateTopic`.
/// * the published 1..=60-character name bound is not checked: a 61-character name creates.
///
/// The stream reports `CREATING` the instant this returns and `ACTIVE` within a second, and the
/// calls that act on it refuse any other state — `DeleteStream` on a stream that is not `ACTIVE`
/// answers `ResourceInUseException: Stream <name> under account 000000000000 not ACTIVE,
/// instead in state <STATE>` — so a caller that wants to keep going waits with [`wait_state`].
pub async fn create_stream(config: &SdkConfig, name: &str, shard_count: i32) -> Result<()> {
    client(config)
        .create_stream()
        .stream_name(name)
        .shard_count(shard_count)
        .send()
        .await
        .context("CreateStream")?;
    Ok(())
}

/// Deletes a stream. Only an `ACTIVE` stream accepts this: a stream in any other state answers
/// `ResourceInUseException: Stream <name> under account 000000000000 not ACTIVE, instead in
/// state DELETING` (or `UPDATING`), so a caller that just called `UpdateShardCount` has to wait
/// rather than retry blindly. Deleting a name that is already gone answers
/// `ResourceNotFoundException`, so this is not idempotent — use [`stream_exists`] first if the
/// caller needs "gone is fine".
pub async fn delete_stream(config: &SdkConfig, name: &str) -> Result<()> {
    client(config)
        .delete_stream()
        .stream_name(name)
        .send()
        .await
        .context("DeleteStream")?;
    Ok(())
}

/// Every shard of a stream, following `NextToken` to the end.
///
/// `ListShards` paginates through `next_token` in this SDK version — the `NextShardIterator`
/// member is what `GetRecords` returns, not what `ListShards` accepts. The name and the token
/// are mutually exclusive: a continuation that repeats `StreamName` is refused with
/// `InvalidArgumentException: StreamName cannot be specified together with NextToken, which
/// already identifies what is being listed and where the listing resumes`, so only the first
/// page carries the name.
pub async fn list_shards(config: &SdkConfig, name: &str) -> Result<Vec<ShardInfo>> {
    let mut shards = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client(config).list_shards().max_results(PAGE_SIZE);
        match token.clone() {
            Some(t) => req = req.next_token(t),
            None => req = req.stream_name(name),
        }
        let resp = req.send().await.context("ListShards")?;
        for shard in resp.shards() {
            shards.push(shard_info(shard));
        }
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(shards)
}

/// Writes one record and reports where it landed.
///
/// The emulator does hash the partition key: `"pk-1"` went to `shardId-000000000001` and
/// `"dashboard-key"` to `shardId-000000000000` of the same two-shard stream. The sequence
/// number is a fixed-width 33-digit decimal, so it is usable as an opaque cursor but is not an
/// ordered integer across shards. A stream that is not there answers `ResourceNotFoundException`
/// like every other call.
pub async fn put_record(
    config: &SdkConfig,
    stream: &str,
    partition_key: &str,
    data: &[u8],
) -> Result<RecordLocation> {
    let resp = client(config)
        .put_record()
        .stream_name(stream)
        .partition_key(partition_key)
        .data(aws_smithy_types::Blob::from(data.to_vec()))
        .send()
        .await
        .context("PutRecord")?;
    Ok(RecordLocation {
        shard_id: resp.shard_id().to_string(),
        sequence_number: resp.sequence_number().to_string(),
    })
}

/// An iterator over `shard` starting at `iterator_type` (`"TRIM_HORIZON"` or `"LATEST"`),
/// which [`get_records`] then consumes.
///
/// The emulator validates the type against the extra members the other types need, verbatim:
/// `InvalidArgumentException: StartingSequenceNumber is required with the AT_SEQUENCE_NUMBER
/// shard iterator type` and `InvalidArgumentException: StartingSequenceNumber can only be used
/// with the AT_SEQUENCE_NUMBER or AFTER_SEQUENCE_NUMBER shard iterator types, not with
/// TRIM_HORIZON`. Only the two types this helper can satisfy without an extra member are
/// offered here; a seek needs the SDK builder directly.
/// An iterator is opaque, single-use and expires, so fetch it right before reading.
pub async fn get_shard_iterator(
    config: &SdkConfig,
    stream: &str,
    shard_id: &str,
    iterator_type: &str,
) -> Result<String> {
    let resp = client(config)
        .get_shard_iterator()
        .stream_name(stream)
        .shard_id(shard_id)
        .shard_iterator_type(types::ShardIteratorType::from(iterator_type))
        .send()
        .await
        .context("GetShardIterator")?;
    resp.shard_iterator()
        .map(str::to_string)
        .context("GetShardIterator returned no iterator")
}

/// Reads one page of records and the iterator that continues from it.
///
/// `limit` is checked only from below (`limit = 0` answers `ValidationException: 1 validation
/// error detected: Value '0' at 'limit' failed to satisfy constraint: Member must have value
/// greater than or equal to 1`); 1, 500, 501 and 10 000 all work, the last of them past the
/// published ceiling. A garbage iterator answers `InvalidArgumentException: Invalid ShardIterator.`
/// — the emulator never raises the `ExpiredIteratorException` a real region would, so a reader
/// that has to re-take an iterator should treat that message as the expiry signal.
pub async fn get_records(
    config: &SdkConfig,
    shard_iterator: &str,
    limit: i32,
) -> Result<RecordsPage> {
    let resp = client(config)
        .get_records()
        .shard_iterator(shard_iterator)
        .limit(limit)
        .send()
        .await
        .context("GetRecords")?;
    Ok(RecordsPage {
        records: resp.records().iter().map(record_info).collect(),
        next_shard_iterator: resp.next_shard_iterator().map(str::to_string),
        millis_behind_latest: resp.millis_behind_latest(),
    })
}

/// Reads what a stream holds on its first open shard, from the beginning of its retention.
/// The data-plane convenience the stream page wants: shards and payload in one call.
pub async fn read_records(config: &SdkConfig, stream: &str, limit: i32) -> Result<Vec<RecordInfo>> {
    let shard = list_shards(config, stream)
        .await?
        .into_iter()
        .find(|s| s.ending_sequence_number.is_empty())
        .context("ListShards returned no open shard to read from")?;
    let iterator = get_shard_iterator(config, stream, &shard.id, "TRIM_HORIZON").await?;
    Ok(get_records(config, &iterator, limit).await?.records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    async fn config() -> SdkConfig {
        let conn = EmulatorConnection::from_port(9297);
        conn.build_aws_config().await.expect("aws config")
    }

    /// A failure's verbatim text. `SdkError`'s own `Display` is only the variant name — a
    /// service failure reads literally `service error` — so the emulator's message has to
    /// be read off the error chain down to the typed exception, which repeats itself one
    /// link deeper. An `anyhow::Error` made by `.context("OpName")` walks the same chain,
    /// so both go through `Into<anyhow::Error>` instead of a trait that cannot cover both.
    fn verbatim<E>(err: E) -> String
    where
        E: Into<anyhow::Error>,
    {
        fn is_noise(rendered: &str) -> bool {
            matches!(rendered, "" | "service error" | "response error")
        }
        let error: anyhow::Error = err.into();
        let mut parts: Vec<String> = Vec::new();
        for cause in error.chain() {
            let rendered = cause.to_string();
            if is_noise(&rendered) || parts.last().is_some_and(|previous| *previous == rendered) {
                continue;
            }
            parts.push(rendered);
        }
        parts.join(": ")
    }

    /// A name no sibling run can collide with, and short enough to take a suffix without
    /// brushing the 1..=60-character stream-name bound.
    fn test_stream_name(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("dash-kinesis-{tag}-{nanos}")
    }

    /// A stream accepts `DeleteStream` only in `ACTIVE`; in any other state the emulator
    /// answers `ResourceInUseException: Stream <name> under account <acct> not ACTIVE,
    /// instead in state <STATE>`, so the wait is what makes the first attempt land. Asking
    /// the name back afterwards is not the test — a name the emulator will not describe at
    /// all looks identical to a stream that is gone — so the delete's own error decides.
    async fn force_delete(config: &SdkConfig, name: &str) {
        let _ = wait_state(config, name, "ACTIVE", 30).await;
        for _ in 0..40 {
            match delete_stream(config, name).await {
                Ok(()) => return,
                Err(e) => {
                    let text = verbatim(e);
                    if text.contains("not found") {
                        return;
                    }
                    println!("cleanup retry after {text}");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    }

    /// Prints what every Kinesis call the dashboard would want answers with. The names of
    /// created streams are unique, and each mutating probe deletes what it made.
    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297"]
    async fn live_kinesis_support_matrix() {
        let config = config().await;
        let c = Client::new(&config);

        macro_rules! probe {
            ($name:literal, $e:expr) => {{
                let outcome = $e.await;
                match outcome {
                    Ok(_) => println!("{:<38} OK", $name),
                    Err(e) => println!("{:<38} FAIL {}", $name, verbatim(e)),
                }
            }};
        }

        probe!("ListStreams", c.list_streams().send());
        probe!("ListStreams(limit=1)", c.list_streams().limit(1).send());
        probe!("ListStreams(limit=100)", c.list_streams().limit(100).send());
        probe!(
            "ListStreams(limit=1000)",
            c.list_streams().limit(1000).send()
        );
        probe!(
            "ListStreams(bad next_token)",
            c.list_streams().next_token("bogus").send()
        );
        probe!("DescribeLimits", c.describe_limits().send());
        if let Ok(limits) = c.describe_limits().send().await {
            println!(
                "  DescribeLimits: shard_limit={}, open_shard_count={}, \
                 on_demand_stream_count={}/{}, channel_count={}/{}",
                limits.shard_limit(),
                limits.open_shard_count(),
                limits.on_demand_stream_count(),
                limits.on_demand_stream_count_limit(),
                limits.channel_count().unwrap_or_default(),
                limits.channel_count_limit().unwrap_or_default(),
            );
        }
        probe!(
            "DescribeAccountSettings",
            c.describe_account_settings().send()
        );
        probe!("ListChannels", c.list_channels().send());
        probe!(
            "DescribeStreamSummary(absent)",
            c.describe_stream_summary()
                .stream_name("definitely-not-here")
                .send()
        );
        probe!(
            "DescribeStream(absent)",
            c.describe_stream()
                .stream_name("definitely-not-here")
                .send()
        );
        probe!(
            "ListShards(absent)",
            c.list_shards().stream_name("definitely-not-here").send()
        );

        let name = test_stream_name("matrix");
        let created = create_stream(&config, &name, 2).await;
        match created {
            Ok(()) => {
                println!("{:<38} OK", "CreateStream(shard_count=2)");
                let created_immediately = stream_state(&config, &name).await;
                println!("  state right after CreateStream: {created_immediately:?}");
                probe!(
                    "CreateStream(duplicate)",
                    c.create_stream().stream_name(&name).shard_count(2).send()
                );
                let state = wait_state(&config, &name, "ACTIVE", 30).await;
                println!(
                    "wait ACTIVE: {state:?} -> {}",
                    stream_state(&config, &name).await.unwrap_or_default()
                );
                // `fetch` falls back to this member when both describes fail, so whether the
                // emulator fills it in decides if that branch is real or dead code.
                match c.list_streams().limit(100).send().await {
                    Ok(page) => println!(
                        "  ListStreams with a live stream: {} name(s), {} summaries, first summary = {:?}",
                        page.stream_names().len(),
                        page.stream_summaries().len(),
                        page.stream_summaries().first().map(|s| (
                            s.stream_name().to_string(),
                            s.stream_status().as_str().to_string()
                        )),
                    ),
                    Err(e) => println!("  ListStreams summaries FAIL {}", verbatim(e)),
                }
                println!(
                    "  row while ACTIVE: {:?}",
                    fetch(&config)
                        .await
                        .map(|rows| rows.into_iter().find(|r| r.name == name).map(|r| r.detail))
                );
                probe!(
                    "DescribeStreamSummary",
                    c.describe_stream_summary().stream_name(&name).send()
                );
                probe!(
                    "DescribeStream",
                    c.describe_stream().stream_name(&name).send()
                );
                probe!("ListShards", c.list_shards().stream_name(&name).send());
                probe!(
                    "ListShards(max_results=1)",
                    c.list_shards().stream_name(&name).max_results(1).send()
                );
                probe!(
                    "ListShards(max_results=10000)",
                    c.list_shards().stream_name(&name).max_results(10000).send()
                );
                probe!(
                    "ListTagsForStream",
                    c.list_tags_for_stream().stream_name(&name).send()
                );
                let stream_arn = c
                    .describe_stream_summary()
                    .stream_name(&name)
                    .send()
                    .await
                    .ok()
                    .and_then(|r| {
                        r.stream_description_summary()
                            .map(|s| s.stream_arn().to_string())
                    });
                probe!("ListStreamConsumers", async {
                    let arn = stream_arn
                        .clone()
                        .context("no ARN to point ListStreamConsumers at")?;
                    let _ = c.list_stream_consumers().stream_arn(arn).send().await?;
                    Ok::<_, anyhow::Error>(())
                });
                let written = match put_record(&config, &name, "pk-1", b"matrix payload").await {
                    Ok(loc) => {
                        println!(
                            "{:<38} OK shard={} seq={}",
                            "PutRecord", loc.shard_id, loc.sequence_number
                        );
                        Some(loc)
                    }
                    Err(e) => {
                        println!("{:<38} FAIL {}", "PutRecord", verbatim(e));
                        None
                    }
                };
                // The fields `ShardInfo` promises the dashboard, as the emulator returns them.
                match list_shards(&config, &name).await {
                    Ok(shards) => println!("  list_shards: {shards:?}"),
                    Err(e) => println!("  list_shards FAIL {}", verbatim(e)),
                }
                // Iterator positions, measured for every type the model defines rather than
                // only the two this driver uses, so the doc comment states facts.
                for kind in [
                    "TRIM_HORIZON",
                    "LATEST",
                    "AT_SEQUENCE_NUMBER",
                    "AFTER_SEQUENCE_NUMBER",
                    "AT_TIMESTAMP",
                ] {
                    let seq = written.as_ref().map(|w| w.sequence_number.clone());
                    for with_seq in [false, true] {
                        let outcome = async {
                            let mut req = c
                                .get_shard_iterator()
                                .stream_name(&name)
                                .shard_id("shardId-000000000000")
                                .shard_iterator_type(types::ShardIteratorType::from(kind));
                            if with_seq {
                                let seq = seq.clone().context("no record was written")?;
                                req = req.starting_sequence_number(seq);
                            }
                            let _ = req.send().await?;
                            Ok::<_, anyhow::Error>(())
                        }
                        .await;
                        let label = format!(
                            "GetShardIterator({kind}{})",
                            if with_seq { "+seq" } else { "" }
                        );
                        match outcome {
                            Ok(()) => println!("{:<38} OK", label),
                            Err(e) => println!("{:<38} FAIL {}", label, verbatim(e)),
                        }
                    }
                }
                probe!(
                    "PutRecord(on missing stream)",
                    c.put_record()
                        .stream_name("definitely-not-here")
                        .partition_key("pk")
                        .data(aws_smithy_types::Blob::from(b"x".to_vec()))
                        .send()
                );
                let iterator =
                    get_shard_iterator(&config, &name, "shardId-000000000000", "TRIM_HORIZON")
                        .await;
                match iterator {
                    Ok(it) => {
                        println!("{:<38} OK", "GetShardIterator(TRIM_HORIZON)");
                        probe!("GetRecords", get_records(&config, &it, 100));
                    }
                    Err(e) => println!("{:<38} FAIL {}", "GetShardIterator", verbatim(e)),
                }
                probe!(
                    "GetRecords(bad iterator)",
                    c.get_records().shard_iterator("bogus").send()
                );
                // The `bogus` iterator above fails before `limit` is ever looked at, so the
                // bound is only measurable with a real one. Iterators are single-use, hence
                // a fresh one per probe.
                for limit in [0, 1, 500, 501, 10000] {
                    let label = format!("GetRecords(limit={limit})");
                    match get_shard_iterator(&config, &name, "shardId-000000000000", "LATEST").await
                    {
                        Ok(iterator) => {
                            let outcome = c
                                .get_records()
                                .shard_iterator(iterator)
                                .limit(limit)
                                .send()
                                .await;
                            match outcome {
                                Ok(page) => println!(
                                    "{:<38} OK {} record(s), millis_behind_latest={:?}",
                                    label,
                                    page.records().len(),
                                    page.millis_behind_latest()
                                ),
                                Err(e) => println!("{:<38} FAIL {}", label, verbatim(e)),
                            }
                        }
                        Err(e) => println!("{:<38} no iterator: {}", label, verbatim(e)),
                    }
                }
                probe!(
                    "UpdateShardCount(no scaling_type)",
                    c.update_shard_count()
                        .stream_name(&name)
                        .target_shard_count(4)
                        .send()
                );
                probe!(
                    "UpdateShardCount(UNIFORM_SCALING)",
                    c.update_shard_count()
                        .stream_name(&name)
                        .target_shard_count(4)
                        .scaling_type(types::ScalingType::UniformScaling)
                        .send()
                );
                println!(
                    "  row while UPDATING: {:?}",
                    fetch(&config)
                        .await
                        .map(|rows| rows.into_iter().find(|r| r.name == name).map(|r| r.detail))
                );
                // The token one `ListShards` page hands back has to be accepted by the next.
                match c
                    .list_shards()
                    .stream_name(&name)
                    .max_results(1)
                    .send()
                    .await
                {
                    Ok(page) => {
                        println!(
                            "  ListShards page 1: shards={:?} token={}",
                            page.shards()
                                .iter()
                                .map(|s| s.shard_id().to_string())
                                .collect::<Vec<_>>(),
                            page.next_token().is_some()
                        );
                        let round_trip = async {
                            let token = page
                                .next_token()
                                .context("single-shard page returned no token")?
                                .to_string();
                            let _ = c
                                .list_shards()
                                .stream_name(&name)
                                .max_results(1)
                                .next_token(token)
                                .send()
                                .await?;
                            Ok::<_, anyhow::Error>(())
                        };
                        probe!("ListShards(token round trip)", round_trip);
                    }
                    Err(e) => println!("ListShards page 1 FAIL {}", verbatim(e)),
                }
                probe!(
                    "IncreaseStreamRetentionPeriod",
                    c.increase_stream_retention_period()
                        .stream_name(&name)
                        .retention_period_hours(48)
                        .send()
                );
                // UpdateStreamMode takes the ARN, not the name, in this API version.
                probe!("UpdateStreamMode(ON_DEMAND)", async {
                    let arn = stream_arn.context("no ARN to point UpdateStreamMode at")?;
                    let details = types::StreamModeDetails::builder()
                        .stream_mode(types::StreamMode::OnDemand)
                        .build()
                        .context("StreamModeDetails build")?;
                    let _ = c
                        .update_stream_mode()
                        .stream_arn(arn)
                        .stream_mode_details(details)
                        .send()
                        .await?;
                    Ok::<_, anyhow::Error>(())
                });
                println!(
                    "row after mode update: {:?}",
                    fetch(&config)
                        .await
                        .map(|rows| rows.into_iter().find(|r| r.name == name).map(|r| r.detail))
                );
                // Delete last: everything above needs the stream to still be there.
                probe!("DeleteStream", delete_stream(&config, &name));
                println!(
                    "  state right after DeleteStream: {:?}",
                    stream_state(&config, &name).await.map_err(verbatim)
                );
            }
            Err(e) => println!("{:<38} FAIL {}", "CreateStream(shard_count=2)", verbatim(e)),
        }
        force_delete(&config, &name).await;

        // Bounds, measured rather than assumed. Each accepted value really does create a
        // stream, so every one of these names is cleaned up right below.
        for count in [0, 1, 5000, 5001] {
            let bounded = format!("{}-sc{count}", test_stream_name("bound"));
            match c
                .create_stream()
                .stream_name(&bounded)
                .shard_count(count)
                .send()
                .await
            {
                Ok(_) => println!(
                    "{:<38} OK (a stream really was made)",
                    format!("CreateStream(shard_count={count})")
                ),
                Err(e) => println!(
                    "{:<38} FAIL {}",
                    format!("CreateStream(shard_count={count})"),
                    verbatim(e)
                ),
            }
            force_delete(&config, &bounded).await;
        }
        // The emulator does not enforce the 60-character name bound, so this probe really
        // does create a 61-character stream. It runs first so a previous run's leftover is
        // gone before the probe, and the name is deleted again at the end.
        let long_name = "a".repeat(61);
        force_delete(&config, &long_name).await;
        probe!(
            "CreateStream(name 61 chars)",
            c.create_stream()
                .stream_name(long_name.clone())
                .shard_count(1)
                .send()
        );
        probe!(
            "ListStreams(limit=1001)",
            c.list_streams().limit(1001).send()
        );
        force_delete(&config, &long_name).await;

        // Deleting twice: the second call has nothing left to delete.
        let once = test_stream_name("twice");
        if create_stream(&config, &once, 1).await.is_ok() {
            let _ = wait_state(&config, &once, "ACTIVE", 30).await;
            probe!("DeleteStream(first)", delete_stream(&config, &once));
            probe!("DeleteStream(second)", delete_stream(&config, &once));
        }
        force_delete(&config, &once).await;

        // `limit` below the account's stream count is the only way to see whether the
        // emulator actually pages, because an empty account never needs to.
        let filler: Vec<String> = (0..3)
            .map(|i| format!("{}-pg{i}", test_stream_name("page")))
            .collect();
        for stream in &filler {
            let _ = create_stream(&config, stream, 1).await;
        }
        let mut token: Option<String> = None;
        let mut pages = 0;
        let mut names: Vec<String> = Vec::new();
        loop {
            let mut req = c.list_streams().limit(1);
            if let Some(t) = token.clone() {
                req = req.next_token(t);
            }
            let Ok(resp) = req.send().await else {
                println!("ListStreams(limit=1) page {pages} FAIL");
                break;
            };
            pages += 1;
            names.extend(resp.stream_names().iter().cloned());
            token = resp.next_token().map(str::to_string);
            println!(
                "  page {pages}: has_more_streams={} token={} names={:?}",
                resp.has_more_streams(),
                token.is_some(),
                resp.stream_names()
            );
            if token.is_none() || pages > 20 {
                break;
            }
        }
        let unique = names.iter().collect::<std::collections::HashSet<_>>().len();
        println!(
            "pagination walk: {pages} page(s), {} names, {unique} unique",
            names.len()
        );
        for stream in &filler {
            force_delete(&config, stream).await;
        }
        // More shards than one page, so the driver has to follow its own ListShards token.
        let wide = format!("{}-wide", test_stream_name("page"));
        match create_stream(&config, &wide, 101).await {
            Ok(()) => {
                let _ = wait_state(&config, &wide, "ACTIVE", 60).await;
                match list_shards(&config, &wide).await {
                    Ok(shards) => {
                        println!("list_shards over 101 shards: {} returned", shards.len())
                    }
                    Err(e) => println!("list_shards over 101 shards FAIL {}", verbatim(e)),
                }
            }
            Err(e) => println!("CreateStream(101 shards) FAIL {}", verbatim(e)),
        }
        force_delete(&config, &wide).await;

        // Runs whether or not any probe above landed anything.
        force_delete(&config, &name).await;
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes a stream"]
    async fn live_kinesis_lifecycle() {
        let config = config().await;
        let name = test_stream_name("live");

        create_stream(&config, &name, 2)
            .await
            .expect("create stream");

        // Guaranteed cleanup: the guarded block below only ever returns, it never panics,
        // so the delete runs even when an intermediate check fails.
        let outcome = async {
            wait_state(&config, &name, "ACTIVE", 30).await?;
            anyhow::ensure!(stream_exists(&config, &name).await?, "stream should exist");
            anyhow::ensure!(
                !stream_exists(&config, "definitely-not-here").await?,
                "absent stream should not exist"
            );

            let rows = fetch(&config).await?;
            let row = rows
                .iter()
                .find(|r| r.name == name)
                .context("created stream is not listed by fetch()")?;
            assert_shape(row, &name)?;

            let shards = list_shards(&config, &name).await?;
            anyhow::ensure!(shards.len() == 2, "expected 2 shards, got {shards:?}");
            anyhow::ensure!(
                shards[0].id == "shardId-000000000000",
                "unexpected first shard id {}",
                shards[0].id
            );

            let placed = put_record(&config, &name, "dashboard-key", b"hello kinesis").await?;
            anyhow::ensure!(
                shards.iter().any(|s| s.id == placed.shard_id),
                "PutRecord routed to unknown shard {}",
                placed.shard_id
            );

            let iterator =
                get_shard_iterator(&config, &name, &placed.shard_id, "TRIM_HORIZON").await?;
            let page = get_records(&config, &iterator, 100).await?;
            anyhow::ensure!(
                page.records
                    .iter()
                    .any(|r| r.data == "hello kinesis" && r.partition_key == "dashboard-key"),
                "read-back records: {:?}",
                page.records
            );
            anyhow::ensure!(
                page.next_shard_iterator.is_some(),
                "GetRecords returned no next iterator"
            );

            let read_back = read_records(&config, &name, 100).await?;
            anyhow::ensure!(
                read_back
                    .iter()
                    .any(|r| r.sequence_number == placed.sequence_number),
                "read_records missed the written record: {read_back:?}"
            );

            Ok::<_, anyhow::Error>(placed)
        }
        .await;

        delete_stream(&config, &name).await.ok();
        force_delete(&config, &name).await;
        let placed = outcome.expect("lifecycle");

        wait_gone(&config, &name).await.expect("stream gone");
        assert!(
            !stream_exists(&config, &name).await.expect("stream exists"),
            "stream {name} still answers DescribeStreamSummary after delete"
        );
        println!("lifecycle ok, record landed at {placed:?}");
    }

    /// `Status · Mode · N shard(s)`, as `fetch` documents it.
    fn assert_shape(row: &Resource, name: &str) -> Result<()> {
        let parts: Vec<&str> = row.detail.split(" · ").collect();
        anyhow::ensure!(
            parts.len() == 3 && !parts[0].is_empty() && parts[2].ends_with("shard(s)"),
            "unexpected detail for {name}: {}",
            row.detail
        );
        Ok(())
    }

    /// A delete lands on the emulator's clock too: the stream reports `DELETING` first and
    /// then disappears from `ListStreams`.
    async fn wait_gone(config: &SdkConfig, name: &str) -> Result<()> {
        for _ in 0..60 {
            if !list_stream_names(config).await?.iter().any(|n| n == name) {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        anyhow::bail!("stream {name} is still listed after delete")
    }
}
