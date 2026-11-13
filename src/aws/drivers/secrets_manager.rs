use crate::app_state::Resource;
use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_secretsmanager::Client;
use aws_smithy_types::date_time::Format;
use std::collections::HashMap;

pub const LABEL: &str = "Secrets Manager";

/// The widest page `ListSecrets` and `ListSecretVersionIds` accept. `MaxResults` is
/// modelled as `1..=100` and the emulator enforces that boundary on the way in, answering
/// anything outside it with
/// `1 validation error detected: Value '1000' at 'maxResults' failed to satisfy
/// constraint: Member must have value less than or equal to 100`
/// (and the matching `greater than or equal to 1` for `0`), so the driver only ever asks
/// for the largest page the service will hand back.
const PAGE_SIZE: i32 = 100;

fn client(config: &SdkConfig) -> Client {
    Client::new(config)
}

/// ISO-8601 timestamp, or `unknown` when the response omits the field.
fn stamp(value: Option<&aws_smithy_types::DateTime>) -> String {
    value
        .and_then(|d| d.fmt(Format::DateTime).ok())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The staging labels attached to the secret's versions, sorted and deduped. The service
/// returns them as a `versionId -> [label]` map, and map iteration order is not stable,
/// so a listing has to impose one.
fn stage_labels(map: Option<&HashMap<String, Vec<String>>>) -> Vec<String> {
    let mut labels: Vec<String> = map
        .map(|m| m.values().flatten().cloned().collect())
        .unwrap_or_default();
    labels.sort();
    labels.dedup();
    labels
}

/// The third field of a row: the labels a version carries, or — when none do — whether the
/// secret is rotating, because a stage-less listing has nothing else worth showing.
fn stage_or_rotation(labels: &[String], rotation_enabled: Option<bool>) -> String {
    if !labels.is_empty() {
        return labels.join(",");
    }
    match rotation_enabled {
        Some(true) => "rotation enabled".to_string(),
        Some(false) => "rotation disabled".to_string(),
        None => "no stages".to_string(),
    }
}

/// `last changed · secret type · stages`, the shape the dashboard shows for a secret.
///
/// Measured: this emulator leaves `SecretType` out of both `ListSecrets` and
/// `DescribeSecret`, so the middle segment reads `unknown` for every secret it holds. It is
/// reported as missing rather than guessed at as `Standard`, which is what a real account
/// would answer for the same call.
fn row(
    name: &str,
    last_changed: Option<&aws_smithy_types::DateTime>,
    secret_type: Option<&str>,
    labels: &[String],
    rotation_enabled: Option<bool>,
) -> Resource {
    Resource {
        name: name.to_string(),
        detail: format!(
            "{} · {} · {}",
            stamp(last_changed),
            secret_type.unwrap_or("unknown"),
            stage_or_rotation(labels, rotation_enabled)
        ),
    }
}

fn list_entry_resource(entry: &aws_sdk_secretsmanager::types::SecretListEntry) -> Resource {
    row(
        entry.name().unwrap_or_default(),
        entry.last_changed_date(),
        entry.r#type(),
        &stage_labels(entry.secret_versions_to_stages()),
        entry.rotation_enabled(),
    )
}

/// Every version, newest page first as the service orders them, followed across `NextToken`.
async fn all_entries(
    config: &SdkConfig,
) -> Result<Vec<aws_sdk_secretsmanager::types::SecretListEntry>> {
    let client = client(config);
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_secrets().max_results(PAGE_SIZE);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListSecrets")?;
        out.extend_from_slice(resp.secret_list());
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(out)
}

/// Every secret this account holds, one `Resource` each: the secret name in the name,
/// `last changed · type · stages` in the detail. Never a value.
///
/// Measured: a secret sitting in its recovery window is NOT in this listing — it appears
/// only when `IncludePlannedDeletion` is set, which this driver does not set, so a delete
/// that the service accepted already looks like a removal here.
pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    Ok(all_entries(config)
        .await?
        .iter()
        .map(list_entry_resource)
        .collect())
}

/// One secret as the same row [`fetch`] produces, for a detail pane that renders a single
/// secret without the whole listing.
pub async fn describe_secret(config: &SdkConfig, id: &str) -> Result<Resource> {
    let resp = client(config)
        .describe_secret()
        .secret_id(id)
        .send()
        .await
        .context("DescribeSecret")?;
    Ok(row(
        resp.name().unwrap_or(id),
        resp.last_changed_date(),
        resp.r#type(),
        &stage_labels(resp.version_ids_to_stages()),
        resp.rotation_enabled(),
    ))
}

/// Whether a secret with that name or ARN is there, answered from `DescribeSecret`: this
/// emulator refuses a missing one with
/// `ResourceNotFoundException: Secrets Manager can't find the specified secret.`, and only
/// that refusal counts as absent — any other failure is returned as an error rather than
/// silently read as "no such secret".
///
/// Measured: a secret whose deletion is still inside its recovery window answers TRUE here,
/// even though [`fetch`] has already dropped it from the listing. A create of that name
/// while the window runs is refused, so `false` is the only answer that licenses a create.
pub async fn secret_exists(config: &SdkConfig, id: &str) -> Result<bool> {
    match client(config).describe_secret().secret_id(id).send().await {
        Ok(_) => Ok(true),
        Err(err)
            if err
                .as_service_error()
                .is_some_and(|e| e.is_resource_not_found_exception()) =>
        {
            Ok(false)
        }
        Err(err) => Err(anyhow::Error::new(err).context("DescribeSecret")),
    }
}

/// Create a secret holding `secret_string` and return the handle every other call here
/// takes: the ARN the service assigned, falling back to `id` when the response omits one —
/// a create that succeeded is never reported as a failure for that reason alone.
///
/// `CreateSecret` is the one write that names its secret by `Name` rather than `SecretId`.
/// `SecretString` and `SecretBinary` are both optional in the model — at least one is
/// required by the service — and this driver always sends the string member. The SDK
/// generates the `ClientRequestToken` that this emulator insists on for a create carrying a
/// value, and uses it as the first version's id.
///
/// Measured: unlike S3, DynamoDB, SQS and SNS in this emulator, a duplicate `CreateSecret`
/// is REFUSED, not accepted —
/// `ResourceExistsException: The operation failed because the secret <name> already
/// exists.` — and the value already stored survives the attempt untouched. So
/// [`secret_exists`] before a create is not protecting the stored value here; it is what
/// lets the dashboard say "that name is taken" instead of surfacing a service exception.
/// A name whose delete is still inside its recovery window is a third answer:
/// `InvalidRequestException: You can't create this secret because a secret with the name
/// <name> is already scheduled for deletion.`
pub async fn create_secret(config: &SdkConfig, id: &str, secret_string: &str) -> Result<String> {
    let resp = client(config)
        .create_secret()
        .name(id)
        .secret_string(secret_string)
        .send()
        .await
        .context("CreateSecret")?;
    Ok(resp
        .arn()
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string()))
}

/// Store a new version of an existing secret. The service returns the new version id; this
/// driver drops it, because [`get_secret_value`] reports it as the version now carrying
/// `AWSCURRENT` and [`list_version_ids`] reports every one.
///
/// Measured: an update mints a second version and moves `AWSCURRENT` to it — the superseded
/// version stays listed — while an update of a name the account does not hold is refused
/// with `ResourceNotFoundException: Secrets Manager can't find the specified secret.`, the
/// same refusal `DescribeSecret` gives. `PutSecretValue` behaves identically.
pub async fn update_secret(config: &SdkConfig, id: &str, secret_string: &str) -> Result<()> {
    client(config)
        .update_secret()
        .secret_id(id)
        .secret_string(secret_string)
        .send()
        .await
        .context("UpdateSecret")?;
    Ok(())
}

/// Delete a secret outright.
///
/// `DeleteSecret` answers `InvalidParameterException: You can't use
/// ForceDeleteWithoutRecovery in conjunction with RecoveryWindowInDays.` when both
/// parameters are sent, and takes a window of 7..=30 days only — so exactly one of the two
/// has to be chosen, and neither of them means "delete it now". Answering neither is the
/// documented default of a 30-day window, and measurement shows what that costs here: the
/// secret leaves [`fetch`] at once but keeps answering `DescribeSecret`, keeps
/// [`secret_exists`] true and keeps the name against a later create for the rest of the
/// window, which nothing else in this dashboard can shorten.
///
/// This driver therefore always force-deletes. Measured: the call is accepted, the secret
/// drops out of the listing immediately, and the store finishes the job asynchronously a
/// short moment (about half a second) later — so `DescribeSecret` may still answer true for
/// that instant even though the delete already succeeded. A repeat delete of the same name
/// is a success, never an error, so a retry after a lost response is safe.
pub async fn delete_secret(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .delete_secret()
        .secret_id(id)
        .force_delete_without_recovery(true)
        .send()
        .await
        .context("DeleteSecret")?;
    Ok(())
}

/// What a stored version looks like from the outside: which version it is, when it was
/// written, whether it holds text or bytes, and how big it is.
///
/// There is deliberately no value field. `GetSecretValue` returns the plaintext, and a
/// struct that carried it would put the plaintext one `.clone()` away from being rendered
/// in a table row or written to a log; [`SecretValueInfo`] is what this driver hands the
/// UI. The only way to get the bytes is [`reveal_secret_value`], which a caller has to ask
/// for by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SecretValueInfo {
    pub name: String,
    pub arn: String,
    pub version_id: String,
    pub created_date: String,
    pub version_stages: Vec<String>,
    /// The value came back in `SecretBinary` rather than `SecretString`.
    pub is_binary: bool,
    /// Byte length of the stored value — a size, never the content.
    pub byte_len: usize,
}

async fn get_output(
    config: &SdkConfig,
    id: &str,
) -> Result<aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueOutput> {
    client(config)
        .get_secret_value()
        .secret_id(id)
        .send()
        .await
        .context("GetSecretValue")
}

fn value_info(
    output: &aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueOutput,
) -> SecretValueInfo {
    let (is_binary, byte_len) = match (output.secret_string(), output.secret_binary()) {
        (Some(text), _) => (false, text.len()),
        (None, Some(blob)) => (true, blob.as_ref().len()),
        (None, None) => (false, 0),
    };
    SecretValueInfo {
        name: output.name().unwrap_or_default().to_string(),
        arn: output.arn().unwrap_or_default().to_string(),
        version_id: output.version_id().unwrap_or_default().to_string(),
        created_date: stamp(output.created_date()),
        version_stages: output.version_stages().to_vec(),
        is_binary,
        byte_len,
    }
}

/// Fetch the current version (or the one `id`'s staging label selects) and return only its
/// metadata. The plaintext is read here and dropped here: nothing in this function's
/// result, and nothing on any error path, carries it.
///
/// Measured against the emulator: `version_id` is the UUID the create or update minted,
/// `created_date` comes back in ISO-8601, `version_stages` carries `AWSCURRENT` for the
/// version the call resolved, and a `SecretString` value reports `is_binary = false` with
/// `byte_len` equal to its UTF-8 length.
pub async fn get_secret_value(config: &SdkConfig, id: &str) -> Result<SecretValueInfo> {
    Ok(value_info(&get_output(config, id).await?))
}

/// The plaintext of a stored version, as bytes: the `SecretString` member encoded as UTF-8,
/// or the `SecretBinary` member as it came back.
///
/// This is the only function in the driver that returns a secret, and it exists for one
/// deliberate user action (copy or reveal). Do not call it from a listing, a refresh, or a
/// `fetch`, and do not `println!`, `dbg!`, trace or `Display` what it returns — a secret
/// value in a log is a leaked secret. Callers that only need to know that a value is
/// there, how big it is, or which version it is want [`get_secret_value`] instead.
pub async fn reveal_secret_value(config: &SdkConfig, id: &str) -> Result<Vec<u8>> {
    let output = get_output(config, id).await?;
    match (output.secret_string(), output.secret_binary()) {
        (Some(text), _) => Ok(text.as_bytes().to_vec()),
        (None, Some(blob)) => Ok(blob.as_ref().to_vec()),
        (None, None) => Err(anyhow::anyhow!("GetSecretValue returned no value member"))
            .context("GetSecretValue"),
    }
}

/// The version ids a secret carries, pages followed. `ListSecretVersionIds` reports the
/// version ids only — the values stay in `GetSecretValue`.
pub async fn list_version_ids(config: &SdkConfig, id: &str) -> Result<Vec<String>> {
    let client = client(config);
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client
            .list_secret_version_ids()
            .secret_id(id)
            .max_results(PAGE_SIZE);
        if let Some(t) = token.clone() {
            req = req.next_token(t);
        }
        let resp = req.send().await.context("ListSecretVersionIds")?;
        out.extend(
            resp.versions()
                .iter()
                .filter_map(|v| v.version_id().map(str::to_string)),
        );
        token = resp.next_token().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;

    async fn config() -> SdkConfig {
        let conn = EmulatorConnection::from_port(9297);
        conn.build_aws_config().await.expect("aws config")
    }

    /// The namespace every secret in these tests lives in, so a run that dies half-way
    /// can be cleaned up by the next one without touching anybody else's secrets. Each
    /// test adds its own tag: `cargo test` runs them in parallel, and a sweep must never
    /// delete the secret the other test is still acting on.
    const PREFIX: &str = "dashboard/secrets-";

    fn unique(tag: &str) -> String {
        format!(
            "{PREFIX}{tag}-{}",
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        )
    }

    /// Force-delete every secret under `prefix`, including one parked in a recovery
    /// window, and report how many went away.
    async fn sweep(config: &SdkConfig, prefix: &str) -> Vec<String> {
        let client = Client::new(config);
        let mut removed = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut req = client
                .list_secrets()
                .max_results(PAGE_SIZE)
                .include_planned_deletion(true);
            if let Some(t) = token.clone() {
                req = req.next_token(t);
            }
            let Ok(resp) = req.send().await else {
                break;
            };
            for entry in resp.secret_list() {
                if let Some(name) = entry.name().filter(|n| n.starts_with(prefix))
                    && delete_secret(config, name).await.is_ok()
                {
                    removed.push(name.to_string());
                }
            }
            token = resp.next_token().map(str::to_string);
            if token.is_none() {
                break;
            }
        }
        removed
    }

    /// Walk an error's `source()` chain and print every layer, because the layer that
    /// carries the emulator's own words is never the outermost one. Two adjacent layers
    /// that say the same thing — the modeled error and its exception struct both `Display`
    /// the message — collapse into one.
    fn chain_of(err: &(dyn std::error::Error + 'static)) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut current = Some(err);
        while let Some(layer) = current {
            let text = layer.to_string();
            if parts.last().map(String::as_str) != Some(text.as_str()) {
                parts.push(text);
            }
            current = layer.source();
        }
        parts.join(": ")
    }

    /// The two error types a probe result can carry: the SDK's own, and the `anyhow` one a
    /// driver function returns.
    trait ProbeError {
        fn probe_text(&self) -> String;
    }

    impl<E, R> ProbeError for aws_sdk_secretsmanager::error::SdkError<E, R>
    where
        E: std::error::Error + 'static,
        R: std::fmt::Debug + 'static,
    {
        fn probe_text(&self) -> String {
            chain_of(self)
        }
    }

    impl ProbeError for anyhow::Error {
        fn probe_text(&self) -> String {
            let mut out = String::new();
            for cause in self.chain() {
                let text = chain_of(cause);
                if out.is_empty() {
                    out = text;
                } else if !text.is_empty() && !out.ends_with(&text) {
                    out.push_str(": ");
                    out.push_str(&text);
                }
            }
            out
        }
    }

    /// Prints what every Secrets Manager call the dashboard would want answers with,
    /// including the verbatim error for anything it refuses. Every secret this creates is
    /// deleted before the test returns.
    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297"]
    async fn live_secrets_manager_support_matrix() {
        let config = config().await;
        let c = Client::new(&config);
        let prefix = format!("{PREFIX}matrix-");
        println!(
            "sweep before: {} leftover secret(s)",
            sweep(&config, &prefix).await.len()
        );

        // Both macros print the whole cause chain, because an `SdkError`'s own `Display` is
        // the words "service error" and the emulator's message sits one `source()` deeper —
        // the verbatim refusal is only visible once the chain is walked.
        macro_rules! probe {
            ($name:literal, $e:expr) => {{
                match $e.await {
                    Ok(_) => println!("{:<38} OK", $name),
                    Err(e) => println!("{:<38} FAIL {}", $name, e.probe_text()),
                }
            }};
        }

        // Same as `probe!`, but prints the value a driver helper answers with instead of
        // swallowing it.
        macro_rules! answer {
            ($name:literal, $e:expr) => {{
                match $e.await {
                    Ok(v) => println!("{:<38} -> {v:?}", $name),
                    Err(e) => println!("{:<38} FAIL {}", $name, e.probe_text()),
                }
            }};
        }
        println!("--- read-only ---");
        probe!("ListSecrets", c.list_secrets().send());
        probe!(
            "ListSecrets(max_results=100)",
            c.list_secrets().max_results(100).send()
        );
        probe!(
            "ListSecrets(max_results=1)",
            c.list_secrets().max_results(1).send()
        );
        probe!(
            "ListSecrets(max_results=0)",
            c.list_secrets().max_results(0).send()
        );
        probe!(
            "ListSecrets(max_results=1000)",
            c.list_secrets().max_results(1000).send()
        );
        probe!(
            "ListSecrets(include_planned_deletion)",
            c.list_secrets().include_planned_deletion(true).send()
        );
        probe!(
            "ListSecrets(filter name)",
            c.list_secrets()
                .filters(
                    aws_sdk_secretsmanager::types::Filter::builder()
                        .key(aws_sdk_secretsmanager::types::FilterNameStringType::Name)
                        .values("dashboard")
                        .build()
                )
                .send()
        );
        probe!(
            "DescribeSecret(missing)",
            c.describe_secret().secret_id("definitely-not-here").send()
        );
        probe!(
            "GetSecretValue(missing)",
            c.get_secret_value().secret_id("definitely-not-here").send()
        );
        probe!(
            "ListSecretVersionIds(missing)",
            c.list_secret_version_ids()
                .secret_id("definitely-not-here")
                .send()
        );
        probe!(
            "DeleteSecret(missing)",
            c.delete_secret()
                .secret_id("definitely-not-here")
                .force_delete_without_recovery(true)
                .send()
        );
        probe!(
            "UpdateSecret(missing)",
            c.update_secret()
                .secret_id("definitely-not-here")
                .secret_string("x")
                .send()
        );
        probe!(
            "PutSecretValue(missing)",
            c.put_secret_value()
                .secret_id("definitely-not-here")
                .secret_string("x")
                .send()
        );
        probe!(
            "BatchGetSecretValue(any)",
            c.batch_get_secret_value().send()
        );
        probe!(
            "GetRandomPassword",
            c.get_random_password().exclude_punctuation(true).send()
        );
        probe!(
            "RestoreSecret(missing)",
            c.restore_secret().secret_id("definitely-not-here").send()
        );
        probe!(
            "GetResourcePolicy(missing)",
            c.get_resource_policy()
                .secret_id("definitely-not-here")
                .send()
        );
        probe!(
            "DeleteResourcePolicy(missing)",
            c.delete_resource_policy()
                .secret_id("definitely-not-here")
                .send()
        );
        probe!(
            "ValidateResourcePolicy(missing)",
            c.validate_resource_policy()
                .secret_id("definitely-not-here")
                .resource_policy("{\"Version\":\"2012-10-17\"}")
                .send()
        );
        probe!(
            "RotateSecret(missing)",
            c.rotate_secret().secret_id("definitely-not-here").send()
        );
        probe!(
            "CancelRotateSecret(missing)",
            c.cancel_rotate_secret()
                .secret_id("definitely-not-here")
                .send()
        );

        println!("--- paging shape ---");
        let page1 = c.list_secrets().max_results(1).send().await;
        match page1 {
            Ok(resp) => println!(
                "ListSecrets(max_results=1): {} entry(s), next_token present={}",
                resp.secret_list().len(),
                resp.next_token().is_some()
            ),
            Err(e) => println!(
                "ListSecrets(max_results=1) paging probe FAIL {}",
                e.probe_text()
            ),
        }

        println!("--- writing to an existing secret ---");
        let id = unique("matrix");
        probe!("CreateSecret", create_secret(&config, &id, "first-value"));
        probe!(
            "DescribeSecret(created)",
            c.describe_secret().secret_id(&id).send()
        );
        answer!("secret_exists(created)", secret_exists(&config, &id));
        answer!(
            "secret_exists(missing)",
            secret_exists(&config, "definitely-not-here-either")
        );
        probe!("fetch(lists created)", fetch(&config));
        probe!("GetSecretValue(created)", get_secret_value(&config, &id));
        probe!(
            "ListSecretVersionIds(created)",
            list_version_ids(&config, &id)
        );
        probe!("UpdateSecret", update_secret(&config, &id, "second-value"));
        probe!(
            "PutSecretValue",
            c.put_secret_value()
                .secret_id(&id)
                .secret_string("third-value")
                .send()
        );
        probe!(
            "TagResource",
            c.tag_resource()
                .secret_id(&id)
                .tags(
                    aws_sdk_secretsmanager::types::Tag::builder()
                        .key("dashboard")
                        .value("yes")
                        .build()
                )
                .send()
        );
        probe!(
            "UntagResource",
            c.untag_resource()
                .secret_id(&id)
                .tag_keys("dashboard")
                .send()
        );
        probe!(
            "PutResourcePolicy",
            c.put_resource_policy()
                .secret_id(&id)
                .resource_policy("{\"Version\":\"2012-10-17\",\"Statement\":[]}")
                .send()
        );
        probe!(
            "GetResourcePolicy(created)",
            c.get_resource_policy().secret_id(&id).send()
        );
        probe!(
            "DeleteResourcePolicy(created)",
            c.delete_resource_policy().secret_id(&id).send()
        );
        probe!(
            "UpdateSecretVersionStage",
            c.update_secret_version_stage()
                .secret_id(&id)
                .version_stage("DASHBOARDTEST")
                .move_to_version_id(
                    list_version_ids(&config, &id)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .next()
                        .unwrap_or_default(),
                )
                .send()
        );
        probe!(
            "RotateSecret(created)",
            c.rotate_secret().secret_id(&id).send()
        );

        println!("--- duplicate CreateSecret (overwrite trap) ---");
        // The trap: a duplicate create may be accepted and clobber the stored value.
        // Read the value back and compare it to the known literals — never print it.
        let before = reveal_secret_value(&config, &id).await.unwrap_or_default();
        let versions_before = list_version_ids(&config, &id).await.unwrap_or_default();
        let dupe = c
            .create_secret()
            .name(&id)
            .secret_string("duplicate-value")
            .send()
            .await;
        match dupe {
            Ok(_) => println!("{:<38} OK", "CreateSecret(duplicate name)"),
            Err(e) => println!(
                "{:<38} FAIL {}",
                "CreateSecret(duplicate name)",
                e.probe_text()
            ),
        }
        // Whether it was accepted or refused, the question the dashboard cares about is
        // the same: is the value that was there still there?
        let after = reveal_secret_value(&config, &id).await.unwrap_or_default();
        let versions_after = list_version_ids(&config, &id).await.unwrap_or_default();
        println!(
            "duplicate create outcome: value is {}",
            if after == b"duplicate-value".to_vec() {
                "the DUPLICATE's value (create overwrote it)"
            } else if after == before {
                "the ORIGINAL value (old value survived)"
            } else if after.is_empty() {
                "unreadable (GetSecretValue failed)"
            } else {
                "neither original nor duplicate, by length only"
            }
        );
        println!(
            "duplicate create version count: {} -> {} (before -> after)",
            versions_before.len(),
            versions_after.len()
        );

        println!("--- deletion shape ---");
        probe!("delete_secret(force)", delete_secret(&config, &id));
        println!(
            "listed after force delete: {}",
            fetch(&config)
                .await
                .unwrap_or_default()
                .iter()
                .any(|r| r.name == id)
        );
        // Force delete is not instantaneous in this emulator: the purge runs behind the
        // call, so measure how long `DescribeSecret` keeps answering.
        println!(
            "exists right after force delete: {}",
            secret_exists(&config, &id).await.unwrap_or_default()
        );
        for _ in 0..20 {
            if !secret_exists(&config, &id).await.unwrap_or_default() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        println!(
            "exists after waiting up to 2s: {}",
            secret_exists(&config, &id).await.unwrap_or_default()
        );
        let windowed = unique("matrix");
        if let Err(e) = create_secret(&config, &windowed, "value").await {
            println!(
                "{:<38} FAIL {}",
                "CreateSecret(window subject)",
                e.probe_text()
            );
        }
        probe!(
            "DeleteSecret(recovery_window_in_days=7)",
            c.delete_secret()
                .secret_id(&windowed)
                .recovery_window_in_days(7)
                .send()
        );
        println!(
            "listed after windowed delete (default): {}",
            fetch(&config)
                .await
                .unwrap_or_default()
                .iter()
                .any(|r| r.name == windowed)
        );
        println!(
            "listed after windowed delete (include_planned_deletion): {}",
            c.list_secrets()
                .include_planned_deletion(true)
                .send()
                .await
                .map(|r| r
                    .secret_list()
                    .iter()
                    .any(|e| e.name() == Some(windowed.as_str())))
                .unwrap_or(false)
        );
        println!(
            "DescribeSecret after windowed delete: {}",
            c.describe_secret()
                .secret_id(&windowed)
                .send()
                .await
                .err()
                .map(|e| e.probe_text())
                .unwrap_or_else(|| "OK".to_string())
        );
        println!(
            "secret_exists during the window: {}",
            secret_exists(&config, &windowed).await.unwrap_or_default()
        );
        println!(
            "GetSecretValue during the window: {}",
            get_secret_value(&config, &windowed)
                .await
                .map(|info| format!("len={}", info.byte_len))
                .unwrap_or_else(|e| e.probe_text())
        );
        println!(
            "CreateSecret same name during window: {}",
            c.create_secret()
                .name(&windowed)
                .secret_string("again")
                .send()
                .await
                .err()
                .map(|e| e.probe_text())
                .unwrap_or_else(|| "accepted".to_string())
        );
        // Both parameters at once is a client-side error in real AWS; measure here.
        let both = unique("matrix");
        if let Err(e) = create_secret(&config, &both, "value").await {
            println!(
                "{:<38} FAIL {}",
                "CreateSecret(both-params subject)",
                e.probe_text()
            );
        }
        probe!(
            "DeleteSecret(window AND force)",
            c.delete_secret()
                .secret_id(&both)
                .recovery_window_in_days(7)
                .force_delete_without_recovery(true)
                .send()
        );
        probe!(
            "DeleteSecret(neither param)",
            c.delete_secret().secret_id(&both).send()
        );
        probe!(
            "RestoreSecret(after neither)",
            c.restore_secret().secret_id(&both).send()
        );
        let _ = delete_secret(&config, &both).await;
        let _ = delete_secret(&config, &windowed).await;

        // Nothing in this test panics, but a run killed by the harness would leave secrets
        // behind; this is the belt-and-braces pass over the test's own namespace.
        println!(
            "sweep after: {} secret(s) removed",
            sweep(&config, &prefix).await.len()
        );
    }

    /// create -> describe -> get -> update -> list versions -> delete, with the delete
    /// guaranteed even when a check in the middle fails: every step reports through `?`, so
    /// the async block returns a `Result` instead of panicking and the delete below always
    /// runs.
    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes a secret"]
    async fn live_secrets_manager_lifecycle() {
        let config = config().await;
        let prefix = format!("{PREFIX}life-");
        let id = unique("life");

        let arn = create_secret(&config, &id, "lifecycle-first")
            .await
            .expect("create secret");
        println!("create returned handle: {arn}");

        let outcome = async {
            anyhow::ensure!(
                secret_exists(&config, &id).await?,
                "secret_exists should be true after create"
            );

            let described = describe_secret(&config, &id).await.context("describe")?;
            anyhow::ensure!(described.name == id, "describe echoes the name");
            println!("describe detail: {}", described.detail);

            let listed = fetch(&config).await.context("list")?;
            let row = listed
                .iter()
                .find(|r| r.name == id)
                .context("created secret is listed")?;
            println!("listed detail: {}", row.detail);
            anyhow::ensure!(
                row.detail.contains("AWSCURRENT") || row.detail.contains("rotation"),
                "the listing detail is {:?}",
                row.detail
            );

            let first = get_secret_value(&config, &id).await.context("get")?;
            println!(
                "get info: version={} created={} len={} binary={}",
                first.version_id, first.created_date, first.byte_len, first.is_binary
            );
            anyhow::ensure!(
                !first.version_id.is_empty(),
                "GetSecretValue should name the version it read"
            );
            anyhow::ensure!(
                first.version_stages.contains(&"AWSCURRENT".to_string()),
                "current version stages: {:?}",
                first.version_stages
            );
            // Byte length of the fixture literal, never the value itself.
            anyhow::ensure!(
                first.byte_len == "lifecycle-first".len() && !first.is_binary,
                "a string secret should read as {} bytes, not {} (binary={})",
                "lifecycle-first".len(),
                first.byte_len,
                first.is_binary
            );

            // The plaintext is compared against the fixture and never printed — not even
            // through a `PartialEq` panic message, which is why this is `ensure!`.
            let plain = reveal_secret_value(&config, &id).await.context("reveal")?;
            anyhow::ensure!(
                plain == "lifecycle-first".as_bytes(),
                "reveal returned {} unexpected bytes",
                plain.len()
            );

            update_secret(&config, &id, "lifecycle-second")
                .await
                .context("update")?;
            let updated = get_secret_value(&config, &id)
                .await
                .context("get after update")?;
            anyhow::ensure!(
                updated.byte_len == "lifecycle-second".len(),
                "the updated version reads as {} bytes",
                updated.byte_len
            );
            anyhow::ensure!(
                updated.version_id != first.version_id,
                "update should mint a new current version (both {})",
                first.version_id
            );

            let versions = list_version_ids(&config, &id)
                .await
                .context("list versions")?;
            println!("version count after update: {}", versions.len());
            anyhow::ensure!(
                versions.contains(&updated.version_id),
                "the current version should be listed"
            );
            anyhow::ensure!(
                versions.len() >= 2,
                "the superseded version should still be listed"
            );

            Ok::<_, anyhow::Error>(())
        }
        .await;

        // Runs whether or not the block above returned an error.
        delete_secret(&config, &id).await.ok();

        // Force delete lands on the emulator's clock: the purge is a background step, so
        // `DescribeSecret` keeps answering for a moment after the call returns.
        let gone = wait_gone(&config, &id).await.expect("wait gone");
        println!("force delete cleared DescribeSecret within 4s: {gone}");

        let leftovers = sweep(&config, &prefix).await;
        println!(
            "sweep after lifecycle: {} secret(s) removed",
            leftovers.len()
        );
        outcome.expect("lifecycle");

        assert!(gone, "force delete should remove the secret outright");
        let after = fetch(&config).await.expect("list after delete");
        assert!(
            !after.iter().any(|r| r.name == id),
            "deleted secret is still listed"
        );
    }

    /// Poll until the emulator's purge has caught up with a force delete.
    async fn wait_gone(config: &SdkConfig, id: &str) -> Result<bool> {
        for _ in 0..40 {
            if !secret_exists(config, id).await? {
                return Ok(true);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Ok(false)
    }
}
