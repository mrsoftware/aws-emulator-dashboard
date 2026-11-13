use crate::app_state::Resource;
use anyhow::{Context, Result, bail};
use aws_config::SdkConfig;
use aws_sdk_rds::Client;

pub const LABEL: &str = "RDS";

/// The emulator refuses every other page size with
/// `Invalid value N for MaxRecords. Must be between 20 and 100`, so the driver always asks
/// for the widest page the range allows.
const PAGE_SIZE: i32 = 100;

/// The engine and class the emulator actually accepts: `CreateDBInstance` resolves an
/// omitted `EngineVersion` against its own catalogue (mysql lands on 8.4.4) and refuses a
/// class whose family it publishes no orderable option for, so `db.t3.micro` is the smallest
/// combination that is offered.
pub const DEFAULT_ENGINE: &str = "mysql";
pub const DEFAULT_INSTANCE_CLASS: &str = "db.t3.micro";
pub const DEFAULT_ALLOCATED_STORAGE: i32 = 20;

/// The cluster engine that creates without any further members: `aurora-mysql` takes a
/// provisioned cluster with no storage or class, while a non-Aurora engine such as
/// `postgres` is refused for the Multi-AZ members `AllocatedStorage`,
/// `DBClusterInstanceClass`, `Iops` and `StorageType`.
pub const DEFAULT_CLUSTER_ENGINE: &str = "aurora-mysql";

fn client(config: &SdkConfig) -> Client {
    Client::new(config)
}

fn engine_and_version(engine: Option<&str>, version: Option<&str>) -> String {
    match (engine, version) {
        (Some(engine), Some(version)) => format!("{engine} {version}"),
        (Some(engine), None) => engine.to_string(),
        _ => "unknown".to_string(),
    }
}

/// `status · engine+version · class`.
fn instance_resource(i: &aws_sdk_rds::types::DbInstance) -> Resource {
    Resource {
        name: i.db_instance_identifier().unwrap_or_default().to_string(),
        detail: format!(
            "{} · {} · {}",
            i.db_instance_status().unwrap_or("unknown"),
            engine_and_version(i.engine(), i.engine_version()),
            i.db_instance_class().unwrap_or_default()
        ),
    }
}

fn cluster_resource(c: &aws_sdk_rds::types::DbCluster) -> Resource {
    let members = c.db_cluster_members().len();
    Resource {
        name: c.db_cluster_identifier().unwrap_or_default().to_string(),
        detail: format!(
            "{} · {} · {members} member(s)",
            c.status().unwrap_or("unknown"),
            engine_and_version(c.engine(), c.engine_version()),
        ),
    }
}

fn snapshot_resource(s: &aws_sdk_rds::types::DbSnapshot) -> Resource {
    Resource {
        name: s.db_snapshot_identifier().unwrap_or_default().to_string(),
        detail: format!(
            "{} · {} · {}",
            s.status().unwrap_or("unknown"),
            s.db_instance_identifier().unwrap_or_default(),
            engine_and_version(s.engine(), s.engine_version())
        ),
    }
}

pub async fn fetch(config: &SdkConfig) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = client(config)
            .describe_db_instances()
            .max_records(PAGE_SIZE);
        if let Some(m) = marker.clone() {
            req = req.marker(m);
        }
        let resp = req.send().await.context("DescribeDBInstances")?;
        resources.extend(resp.db_instances().iter().map(instance_resource));
        marker = resp.marker().map(str::to_string);
        if marker.is_none() {
            break;
        }
    }
    Ok(resources)
}

pub async fn fetch_clusters(config: &SdkConfig) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = client(config).describe_db_clusters().max_records(PAGE_SIZE);
        if let Some(m) = marker.clone() {
            req = req.marker(m);
        }
        let resp = req.send().await.context("DescribeDBClusters")?;
        resources.extend(resp.db_clusters().iter().map(cluster_resource));
        marker = resp.marker().map(str::to_string);
        if marker.is_none() {
            break;
        }
    }
    Ok(resources)
}

pub async fn fetch_snapshots(config: &SdkConfig) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = client(config)
            .describe_db_snapshots()
            .max_records(PAGE_SIZE);
        if let Some(m) = marker.clone() {
            req = req.marker(m);
        }
        let resp = req.send().await.context("DescribeDBSnapshots")?;
        resources.extend(resp.db_snapshots().iter().map(snapshot_resource));
        marker = resp.marker().map(str::to_string);
        if marker.is_none() {
            break;
        }
    }
    Ok(resources)
}

pub async fn fetch_subnet_groups(config: &SdkConfig) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = client(config)
            .describe_db_subnet_groups()
            .max_records(PAGE_SIZE);
        if let Some(m) = marker.clone() {
            req = req.marker(m);
        }
        let resp = req.send().await.context("DescribeDBSubnetGroups")?;
        for g in resp.db_subnet_groups() {
            resources.push(Resource {
                name: g.db_subnet_group_name().unwrap_or_default().to_string(),
                detail: format!(
                    "{} · {} · {} subnet(s)",
                    g.subnet_group_status().unwrap_or("unknown"),
                    g.db_subnet_group_description().unwrap_or_default(),
                    g.subnets().len()
                ),
            });
        }
        marker = resp.marker().map(str::to_string);
        if marker.is_none() {
            break;
        }
    }
    Ok(resources)
}

pub async fn fetch_parameter_groups(config: &SdkConfig) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = client(config)
            .describe_db_parameter_groups()
            .max_records(PAGE_SIZE);
        if let Some(m) = marker.clone() {
            req = req.marker(m);
        }
        let resp = req.send().await.context("DescribeDBParameterGroups")?;
        for g in resp.db_parameter_groups() {
            resources.push(Resource {
                name: g.db_parameter_group_name().unwrap_or_default().to_string(),
                detail: format!(
                    "{} · {}",
                    g.db_parameter_group_family().unwrap_or_default(),
                    g.description().unwrap_or_default()
                ),
            });
        }
        marker = resp.marker().map(str::to_string);
        if marker.is_none() {
            break;
        }
    }
    Ok(resources)
}

/// `DescribeDBInstances` answers `DBInstanceNotFound` for an identifier the emulator does
/// not hold, so a deleted instance reports an error here rather than an empty status.
pub async fn instance_state(config: &SdkConfig, id: &str) -> Result<String> {
    let resp = client(config)
        .describe_db_instances()
        .db_instance_identifier(id)
        .max_records(PAGE_SIZE)
        .send()
        .await
        .context("DescribeDBInstances")?;
    resp.db_instances()
        .iter()
        .find(|i| i.db_instance_identifier() == Some(id))
        .and_then(|i| i.db_instance_status())
        .map(str::to_string)
        .context("DescribeDBInstances returned no status")
}

pub async fn cluster_state(config: &SdkConfig, id: &str) -> Result<String> {
    let resp = client(config)
        .describe_db_clusters()
        .db_cluster_identifier(id)
        .max_records(PAGE_SIZE)
        .send()
        .await
        .context("DescribeDBClusters")?;
    resp.db_clusters()
        .iter()
        .find(|c| c.db_cluster_identifier() == Some(id))
        .and_then(|c| c.status())
        .map(str::to_string)
        .context("DescribeDBClusters returned no status")
}

/// The emulator advances every status on a clock (`creating` -> `available` and
/// `deleting` -> removed both become due 300 ms after the call that asked for them), so a
/// caller must wait for a transition to land before issuing the next one.
/// Live tests only — the UI reloads instead of blocking on a transition.
#[cfg(test)]
pub async fn wait_state(config: &SdkConfig, id: &str, want: &str, secs: u64) -> Result<()> {
    for _ in 0..(secs * 2) {
        if instance_state(config, id).await? == want {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    bail!("db instance {id} did not reach {want} within {secs}s")
}

/// Live tests only — the UI reloads instead of blocking on a transition.
#[cfg(test)]
pub async fn wait_cluster_state(config: &SdkConfig, id: &str, want: &str, secs: u64) -> Result<()> {
    for _ in 0..(secs * 2) {
        if cluster_state(config, id).await? == want {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    bail!("db cluster {id} did not reach {want} within {secs}s")
}

/// The instance stays listed while its delete is owed to the clock, so "gone" is a
/// `DescribeDBInstances` that no longer carries it, not a status.
/// Live tests only — the UI reloads instead of blocking on a transition.
#[cfg(test)]
pub async fn wait_instance_gone(config: &SdkConfig, id: &str, secs: u64) -> Result<()> {
    for _ in 0..(secs * 2) {
        if !fetch(config).await?.iter().any(|r| r.name == id) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    bail!("db instance {id} is still listed after {secs}s")
}

/// Live tests only — the UI reloads instead of blocking on a transition.
#[cfg(test)]
pub async fn wait_cluster_gone(config: &SdkConfig, id: &str, secs: u64) -> Result<()> {
    for _ in 0..(secs * 2) {
        if !fetch_clusters(config).await?.iter().any(|r| r.name == id) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    bail!("db cluster {id} is still listed after {secs}s")
}

/// Creates a DB instance. No master credential is sent: the emulator accepts the create
/// without one and reports none back, so the dashboard never has to carry a password.
pub async fn create_instance(config: &SdkConfig, id: &str) -> Result<String> {
    create_instance_with(
        config,
        id,
        DEFAULT_ENGINE,
        DEFAULT_INSTANCE_CLASS,
        DEFAULT_ALLOCATED_STORAGE,
    )
    .await
}

pub async fn create_instance_with(
    config: &SdkConfig,
    id: &str,
    engine: &str,
    db_instance_class: &str,
    allocated_storage: i32,
) -> Result<String> {
    client(config)
        .create_db_instance()
        .db_instance_identifier(id)
        .db_instance_class(db_instance_class)
        .engine(engine)
        .allocated_storage(allocated_storage)
        .send()
        .await
        .context("CreateDBInstance")?;
    Ok(id.to_string())
}

/// A delete is refused with `FinalDBSnapshotIdentifier is required unless SkipFinalSnapshot
/// is specified.` from every status, so the dashboard's delete always skips the snapshot.
pub async fn delete_instance(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .delete_db_instance()
        .db_instance_identifier(id)
        .skip_final_snapshot(true)
        .send()
        .await
        .context("DeleteDBInstance")?;
    Ok(())
}

pub async fn start_instance(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .start_db_instance()
        .db_instance_identifier(id)
        .send()
        .await
        .context("StartDBInstance")?;
    Ok(())
}

/// Accepted only from `available`; an instance still `creating` is refused with
/// `InvalidDBInstanceState`.
pub async fn stop_instance(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .stop_db_instance()
        .db_instance_identifier(id)
        .send()
        .await
        .context("StopDBInstance")?;
    Ok(())
}

pub async fn reboot_instance(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .reboot_db_instance()
        .db_instance_identifier(id)
        .send()
        .await
        .context("RebootDBInstance")?;
    Ok(())
}

pub async fn create_snapshot(
    config: &SdkConfig,
    instance_id: &str,
    snapshot_id: &str,
) -> Result<()> {
    client(config)
        .create_db_snapshot()
        .db_instance_identifier(instance_id)
        .db_snapshot_identifier(snapshot_id)
        .send()
        .await
        .context("CreateDBSnapshot")?;
    Ok(())
}

pub async fn delete_snapshot(config: &SdkConfig, snapshot_id: &str) -> Result<()> {
    client(config)
        .delete_db_snapshot()
        .db_snapshot_identifier(snapshot_id)
        .send()
        .await
        .context("DeleteDBSnapshot")?;
    Ok(())
}

/// Creates a provisioned Aurora cluster. `aurora-mysql` needs no storage or class members;
/// a non-Aurora engine is refused for the four Multi-AZ members it requires.
pub async fn create_cluster(config: &SdkConfig, id: &str) -> Result<String> {
    create_cluster_with(config, id, DEFAULT_CLUSTER_ENGINE).await
}

pub async fn create_cluster_with(config: &SdkConfig, id: &str, engine: &str) -> Result<String> {
    client(config)
        .create_db_cluster()
        .db_cluster_identifier(id)
        .engine(engine)
        .send()
        .await
        .context("CreateDBCluster")?;
    Ok(id.to_string())
}

pub async fn delete_cluster(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .delete_db_cluster()
        .db_cluster_identifier(id)
        .skip_final_snapshot(true)
        .send()
        .await
        .context("DeleteDBCluster")?;
    Ok(())
}

/// Stops an `available` cluster; the clock settles it in `stopped` a moment later.
/// Deleting a stopped cluster is accepted, starting one back up is not (see
/// [`start_cluster`]).
pub async fn stop_cluster(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .stop_db_cluster()
        .db_cluster_identifier(id)
        .send()
        .await
        .context("StopDBCluster")?;
    Ok(())
}

/// NOT IMPLEMENTED in the emulator: `StartDBCluster` answers HTTP 501 `NotImplemented`
/// (`operation StartDBCluster is scaffolded but not implemented in this emulator`), so a
/// stopped cluster cannot be started back up. Reported, not worked around: prefer leaving a
/// cluster `available`.
pub async fn start_cluster(config: &SdkConfig, id: &str) -> Result<()> {
    client(config)
        .start_db_cluster()
        .db_cluster_identifier(id)
        .send()
        .await
        .context("StartDBCluster")?;
    Ok(())
}

/// A DB subnet group is a set of REAL EC2 subnets covering at least two Availability Zones:
/// the emulator asks EC2 where each id lives and refuses one it does not know with
/// `Some input subnets in :[...] are invalid.`, so the ids come from `DescribeSubnets`.
pub async fn create_subnet_group(config: &SdkConfig, name: &str) -> Result<()> {
    let subnet_ids = available_subnet_ids(config).await?;
    if subnet_ids.len() < 2 {
        bail!(
            "need at least 2 EC2 subnets in different availability zones, found {}",
            subnet_ids.len()
        );
    }
    let mut req = client(config)
        .create_db_subnet_group()
        .db_subnet_group_name(name)
        .db_subnet_group_description("managed by the emulator dashboard");
    for id in &subnet_ids {
        req = req.subnet_ids(id);
    }
    req.send().await.context("CreateDBSubnetGroup")?;
    Ok(())
}

pub async fn delete_subnet_group(config: &SdkConfig, name: &str) -> Result<()> {
    client(config)
        .delete_db_subnet_group()
        .db_subnet_group_name(name)
        .send()
        .await
        .context("DeleteDBSubnetGroup")?;
    Ok(())
}

async fn available_subnet_ids(config: &SdkConfig) -> Result<Vec<String>> {
    let resp = aws_sdk_ec2::Client::new(config)
        .describe_subnets()
        .send()
        .await
        .context("DescribeSubnets")?;
    Ok(resp
        .subnets()
        .iter()
        .filter_map(|s| s.subnet_id().map(str::to_string))
        .collect())
}

pub async fn create_parameter_group(config: &SdkConfig, name: &str, family: &str) -> Result<()> {
    client(config)
        .create_db_parameter_group()
        .db_parameter_group_name(name)
        .db_parameter_group_family(family)
        .description("managed by the emulator dashboard")
        .send()
        .await
        .context("CreateDBParameterGroup")?;
    Ok(())
}

pub async fn delete_parameter_group(config: &SdkConfig, name: &str) -> Result<()> {
    client(config)
        .delete_db_parameter_group()
        .db_parameter_group_name(name)
        .send()
        .await
        .context("DeleteDBParameterGroup")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::client::EmulatorConnection;
    use aws_sdk_rds::error::ProvideErrorMetadata;

    async fn live_config() -> SdkConfig {
        let conn = EmulatorConnection::from_port(9297);
        conn.build_aws_config().await.expect("aws config")
    }

    fn unique(prefix: &str) -> String {
        let secs = std::time::UNIX_EPOCH
            .elapsed()
            .expect("clock before the epoch")
            .as_secs();
        let pid = std::process::id();
        format!("{prefix}{secs:x}{pid:x}").to_ascii_lowercase()
    }

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; launches and deletes a DB instance and a DB cluster"]
    async fn live_rds_lifecycle() {
        let config = live_config().await;
        let id = unique("dashdb");
        let cluster_id = unique("dashcluster");
        let subnet_group = unique("dashsg");
        let parameter_group = unique("dashpg");

        create_instance(&config, &id)
            .await
            .expect("create db instance");

        // The emulator advances the status on a clock, so the instance has to land in
        // `available` before a stop or a delete is accepted.
        wait_state(&config, &id, "available", 30)
            .await
            .expect("wait available");

        let listed = fetch(&config).await.expect("describe db instances");
        let row = listed
            .iter()
            .find(|r| r.name == id)
            .expect("created instance is listed");
        assert!(
            row.detail.starts_with("available · mysql ")
                && row
                    .detail
                    .ends_with(&format!(" · {DEFAULT_INSTANCE_CLASS}")),
            "unexpected detail {:?}",
            row.detail
        );

        let snapshot_id = format!("{id}-snap");
        let outcome = async {
            stop_instance(&config, &id).await.context("stop")?;
            wait_state(&config, &id, "stopped", 30).await?;
            start_instance(&config, &id).await.context("start")?;
            wait_state(&config, &id, "available", 30).await?;
            create_snapshot(&config, &id, &snapshot_id)
                .await
                .context("create snapshot")?;
            wait_state(&config, &id, "available", 30).await?;
            let snaps = fetch_snapshots(&config).await?;
            anyhow::ensure!(
                snaps.iter().any(|r| r.name == snapshot_id),
                "snapshot is not listed"
            );
            Ok::<_, anyhow::Error>(())
        }
        .await;

        // Must run even if `outcome` panicked above.
        let _ = delete_snapshot(&config, &snapshot_id).await;
        delete_instance(&config, &id).await.ok();
        outcome.expect("lifecycle");
        wait_instance_gone(&config, &id, 30)
            .await
            .expect("instance gone after delete");
        let _ = delete_snapshot(&config, &snapshot_id).await;

        let cluster_outcome =
            async {
                create_cluster(&config, &cluster_id)
                    .await
                    .context("create db cluster")?;
                wait_cluster_state(&config, &cluster_id, "available", 30)
                    .await
                    .context("wait cluster available")?;
                let clusters = fetch_clusters(&config).await?;
                anyhow::ensure!(
                    clusters.iter().any(|r| r.name == cluster_id
                        && r.detail.starts_with("available · aurora-mysql ")),
                    "created cluster is not listed as available"
                );
                // Stop is accepted; starting a stopped cluster back up is not implemented in
                // this emulator, so the cluster is deleted while stopped rather than left down.
                stop_cluster(&config, &cluster_id)
                    .await
                    .context("stop cluster")?;
                wait_cluster_state(&config, &cluster_id, "stopped", 30)
                    .await
                    .context("wait cluster stopped")?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
        delete_cluster(&config, &cluster_id).await.ok();
        cluster_outcome.expect("cluster lifecycle");
        wait_cluster_gone(&config, &cluster_id, 30)
            .await
            .expect("cluster gone after delete");

        let group_outcome = async {
            create_subnet_group(&config, &subnet_group)
                .await
                .context("create db subnet group")?;
            let groups = fetch_subnet_groups(&config).await?;
            anyhow::ensure!(
                groups
                    .iter()
                    .any(|r| r.name == subnet_group && r.detail.contains("Complete")),
                "created subnet group is not listed"
            );
            create_parameter_group(&config, &parameter_group, "mysql8.0")
                .await
                .context("create db parameter group")?;
            let params = fetch_parameter_groups(&config).await?;
            anyhow::ensure!(
                params
                    .iter()
                    .any(|r| r.name == parameter_group && r.detail.starts_with("mysql8.0")),
                "created parameter group is not listed"
            );
            Ok::<_, anyhow::Error>(())
        }
        .await;
        delete_subnet_group(&config, &subnet_group).await.ok();
        delete_parameter_group(&config, &parameter_group).await.ok();
        group_outcome.expect("subnet and parameter group lifecycle");
    }

    /// Records what the emulator answers for the calls the dashboard does not manage on the
    /// happy path — the refusals are the report, so they are asserted rather than worked
    /// around.
    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; reads only"]
    async fn live_rds_refusals_are_verbatim() {
        let config = live_config().await;

        let too_small = client(&config)
            .describe_db_instances()
            .max_records(5)
            .send()
            .await;
        match too_small {
            Err(e) => {
                let service = e.as_service_error().expect("a modeled service error");
                assert_eq!(service.code(), Some("InvalidParameterValue"));
                assert_eq!(
                    service.message(),
                    Some("Invalid value 5 for MaxRecords. Must be between 20 and 100")
                );
            }
            Ok(_) => panic!("MaxRecords below the 20..=100 range must be refused"),
        }

        for (engine, class, expected) in [
            ("not-an-engine", "db.t3.micro", "Invalid DB engine"),
            (DEFAULT_ENGINE, "micro", "Invalid DB Instance class: micro"),
        ] {
            let resp = client(&config)
                .create_db_instance()
                .db_instance_identifier(unique("dashbad"))
                .db_instance_class(class)
                .engine(engine)
                .allocated_storage(20)
                .send()
                .await;
            match resp {
                Err(e) => {
                    let service = e.as_service_error().expect("a modeled service error");
                    assert_eq!(service.message(), Some(expected), "{engine}/{class}");
                }
                Ok(_) => panic!("{engine}/{class} must be refused"),
            }
        }

        // HTTP 501 NotImplemented, surfaced through `.context("StartDBCluster")`; nothing
        // here works around it.
        let started = start_cluster(&config, &unique("dashnothing")).await;
        let err = started.expect_err("StartDBCluster is not implemented in this emulator");
        let chain = format!("{err:?}");
        assert!(
            chain.contains("StartDBCluster") && chain.contains("not implemented in this emulator"),
            "the refusal must name the operation: {chain}"
        );
    }
}
