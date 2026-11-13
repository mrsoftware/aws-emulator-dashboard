//! Every driver call the dashboard turns into a button.
//!
//! The page-level actions that build a resource, delete it, or pull its children live in
//! `views::detail`. This file holds the rest: the calls that only the SDK can make and that
//! a console user would expect to find — read a secret's value, tail a stream, run a query,
//! publish with a subject, list a role. Each operation is one row of a table plus one arm of
//! [`execute`], so the surface an emulator service offers is readable as a list rather than
//! scattered across the views.

use crate::app_state::Resource;
use crate::aws::drivers;
use crate::views::detail::Reload;
use anyhow::{Result, anyhow, bail};
use aws_config::SdkConfig;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Op {
    SecretsReveal,
    SecretsSetValue,
    SecretsDescribe,
    SecretsValueInfo,

    KinesisPutRecord,
    KinesisReadRecords,
    KinesisShardRecords,
    KinesisStreamState,
    KinesisStreamNames,

    LogsCreateStream,
    LogsPutEvents,
    LogsTail,
    LogsFilter,
    LogsQuery,
    LogsRecentQueries,
    LogsStopQuery,
    LogsSetRetention,
    LogsDeleteRetention,
    LogsDeleteTree,
    LogsProtectOn,
    LogsProtectOff,
    LogsGroupFields,
    LogsGroupArn,
    LogsMetricFilters,
    LogsPutMetricFilter,
    LogsDeleteMetricFilter,
    LogsTestMetricFilter,
    LogsSubscriptionFilters,
    LogsPutSubscriptionFilter,
    LogsDeleteSubscriptionFilter,
    LogsDestinations,
    LogsPutDestination,
    LogsDeleteDestination,
    LogsSetDestinationPolicy,
    LogsTags,
    LogsAddTag,
    LogsRemoveTag,

    CwPutMetricData,
    CwPutAlarm,
    CwPutCompositeAlarm,
    CwSetAlarmState,
    CwListMetrics,
    CwNamespaces,
    CwAlarmHistory,
    CwListDashboards,
    CwGetDashboard,
    CwPutDashboard,
    CwDeleteDashboard,
    CwMetricData,

    SnsAttributes,
    SnsSetAttribute,
    SnsPublishSubject,
    SnsConfirm,
    SnsSubscriptions,
    SnsSubAttributes,
    SnsSetSubAttribute,

    RdsInstanceState,
    RdsCreateSnapshot,
    RdsDeleteSnapshot,
    RdsSnapshots,
    RdsClusters,
    RdsClusterState,
    RdsCreateCluster,
    RdsDeleteCluster,
    RdsStartCluster,
    RdsStopCluster,
    RdsSubnetGroups,
    RdsCreateSubnetGroup,
    RdsDeleteSubnetGroup,
    RdsParameterGroups,
    RdsCreateParameterGroup,
    RdsDeleteParameterGroup,

    IamRoles,
    IamCreateRole,
    IamDeleteRole,

    Ec2InstanceState,
}

#[derive(Clone)]
pub struct Field {
    pub label: &'static str,
    pub placeholder: &'static str,
}

#[derive(Clone)]
pub struct Operation {
    pub op: Op,
    pub label: &'static str,
    /// The button needs a selected row: that row's name (and its ARN/URL) is the target.
    pub needs_selection: bool,
    /// The dialog collects the target identifier instead, for the calls whose resource is
    /// not a row of this page — a dashboard, a cluster, a role.
    pub name: Option<Field>,
    pub fields: &'static [Field],
    /// Painted red: this destroys something the page's own delete button does not reach.
    pub danger: bool,
    pub reload: Reload,
}

/// The shape every row of the tables below starts from: a read-only button on the selected
/// row, no extra fields. Each row then overrides only what its call needs.
const BASE: Operation = Operation {
    op: Op::SecretsReveal,
    label: "",
    needs_selection: true,
    name: None,
    fields: &[],
    danger: false,
    reload: Reload::None,
};

/// The operations the page offers. `selection` is whether a row of this service is open,
/// which is what the row-scoped buttons need.
pub fn operations(service: &str, selection: bool) -> Vec<Operation> {
    let table: Vec<Operation> = match service {
        "secretsmanager" => vec![
            Operation {
                op: Op::SecretsReveal,
                label: "Reveal value",
                ..BASE
            },
            Operation {
                op: Op::SecretsDescribe,
                label: "Describe",
                ..BASE
            },
            Operation {
                op: Op::SecretsValueInfo,
                label: "Value metadata",
                ..BASE
            },
            Operation {
                op: Op::SecretsSetValue,
                label: "Set value",
                reload: Reload::Detail,
                fields: &[Field {
                    label: "New value",
                    placeholder: "text stored as the current version",
                }],
                ..BASE
            },
        ],
        "kinesis" => vec![
            Operation {
                op: Op::KinesisReadRecords,
                label: "Read records",
                ..BASE
            },
            Operation {
                op: Op::KinesisStreamState,
                label: "Stream status",
                ..BASE
            },
            Operation {
                op: Op::KinesisStreamNames,
                label: "All streams",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::KinesisShardRecords,
                label: "Read shard",
                fields: &[
                    Field {
                        label: "Shard id",
                        placeholder: "shardId-000000000000",
                    },
                    Field {
                        label: "From",
                        placeholder: "TRIM_HORIZON or LATEST",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::KinesisPutRecord,
                label: "Put record",
                reload: Reload::Detail,
                fields: &[
                    Field {
                        label: "Partition key",
                        placeholder: "chooses the shard",
                    },
                    Field {
                        label: "Data",
                        placeholder: "record payload",
                    },
                ],
                ..BASE
            },
        ],
        "logs" => vec![
            Operation {
                op: Op::LogsTail,
                label: "Tail events",
                fields: &[Field {
                    label: "Stream",
                    placeholder: "log stream name",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsFilter,
                label: "Filter events",
                fields: &[Field {
                    label: "Pattern",
                    placeholder: r#"[level, msg]"#,
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsQuery,
                label: "Run query",
                fields: &[Field {
                    label: "Query",
                    placeholder: "fields @timestamp, @message | sort @timestamp desc",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsRecentQueries,
                label: "Queries",
                ..BASE
            },
            Operation {
                op: Op::LogsStopQuery,
                label: "Stop query",
                fields: &[Field {
                    label: "Query id",
                    placeholder: "from the Queries list",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsGroupFields,
                label: "Field names",
                ..BASE
            },
            Operation {
                op: Op::LogsGroupArn,
                label: "Group ARN",
                ..BASE
            },
            Operation {
                op: Op::LogsCreateStream,
                label: "Create stream",
                reload: Reload::Detail,
                fields: &[Field {
                    label: "Stream",
                    placeholder: "new log stream name",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsPutEvents,
                label: "Put events",
                reload: Reload::Detail,
                fields: &[
                    Field {
                        label: "Stream",
                        placeholder: "existing or new stream",
                    },
                    Field {
                        label: "Message",
                        placeholder: "one log event",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::LogsSetRetention,
                label: "Set retention",
                fields: &[Field {
                    label: "Days",
                    placeholder: "1, 3, 7, 30, 365…",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsDeleteRetention,
                label: "No retention policy",
                ..BASE
            },
            Operation {
                op: Op::LogsProtectOn,
                label: "Protect from delete",
                ..BASE
            },
            Operation {
                op: Op::LogsProtectOff,
                label: "Allow delete",
                ..BASE
            },
            Operation {
                op: Op::LogsMetricFilters,
                label: "Metric filters",
                ..BASE
            },
            Operation {
                op: Op::LogsPutMetricFilter,
                label: "Put metric filter",
                name: Some(Field {
                    label: "Filter name",
                    placeholder: "errors-to-metric",
                }),
                fields: &[
                    Field {
                        label: "Pattern",
                        placeholder: r#"[level = "ERROR"]"#,
                    },
                    Field {
                        label: "Metric",
                        placeholder: "ErrorCount",
                    },
                    Field {
                        label: "Namespace",
                        placeholder: "DashboardDemo",
                    },
                    Field {
                        label: "Value",
                        placeholder: "1",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::LogsDeleteMetricFilter,
                label: "Delete metric filter",
                danger: true,
                fields: &[Field {
                    label: "Filter name",
                    placeholder: "from the Metric filters list",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsTestMetricFilter,
                label: "Test pattern",
                needs_selection: false,
                fields: &[
                    Field {
                        label: "Pattern",
                        placeholder: r#"[ip, time, code]"#,
                    },
                    Field {
                        label: "Sample event",
                        placeholder: "1.2.3.4 00:00 200",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::LogsSubscriptionFilters,
                label: "Subscription filters",
                ..BASE
            },
            Operation {
                op: Op::LogsPutSubscriptionFilter,
                label: "Put subscription filter",
                name: Some(Field {
                    label: "Filter name",
                    placeholder: "to-my-destination",
                }),
                fields: &[
                    Field {
                        label: "Pattern",
                        placeholder: "[ip, time, code]",
                    },
                    Field {
                        label: "Destination ARN",
                        placeholder: "from the Destinations list",
                    },
                    Field {
                        label: "Role ARN",
                        placeholder: "optional",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::LogsDeleteSubscriptionFilter,
                label: "Delete subscription filter",
                danger: true,
                fields: &[Field {
                    label: "Filter name",
                    placeholder: "from the Subscription filters list",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsDestinations,
                label: "Destinations",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::LogsPutDestination,
                label: "Put destination",
                needs_selection: false,
                name: Some(Field {
                    label: "Destination name",
                    placeholder: "my-sink",
                }),
                fields: &[
                    Field {
                        label: "Target ARN",
                        placeholder: "kinesis stream or firehose ARN",
                    },
                    Field {
                        label: "Role ARN",
                        placeholder: "ARN allowed to write the target",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::LogsDeleteDestination,
                label: "Delete destination",
                needs_selection: false,
                danger: true,
                fields: &[Field {
                    label: "Destination name",
                    placeholder: "from the Destinations list",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsSetDestinationPolicy,
                label: "Set destination policy",
                needs_selection: false,
                fields: &[
                    Field {
                        label: "Destination name",
                        placeholder: "from the Destinations list",
                    },
                    Field {
                        label: "Access policy",
                        placeholder: r#"{"Version":"2012-10-17","Statement":[...]}"#,
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::LogsTags,
                label: "Tags",
                ..BASE
            },
            Operation {
                op: Op::LogsAddTag,
                label: "Add tag",
                reload: Reload::Detail,
                fields: &[
                    Field {
                        label: "Key",
                        placeholder: "team",
                    },
                    Field {
                        label: "Value",
                        placeholder: "pricing",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::LogsRemoveTag,
                label: "Remove tag",
                reload: Reload::Detail,
                danger: true,
                fields: &[Field {
                    label: "Key",
                    placeholder: "team",
                }],
                ..BASE
            },
            Operation {
                op: Op::LogsDeleteTree,
                label: "Delete group and streams",
                danger: true,
                reload: Reload::Inventory,
                fields: &[Field {
                    label: "Type the group name",
                    placeholder: "confirms the whole-tree delete",
                }],
                ..BASE
            },
        ],
        "monitoring" => vec![
            Operation {
                op: Op::CwListMetrics,
                label: "Metrics",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::CwNamespaces,
                label: "Namespaces",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::CwAlarmHistory,
                label: "Alarm history",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::CwListDashboards,
                label: "Dashboards",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::CwMetricData,
                label: "Read datapoints",
                needs_selection: false,
                fields: &[
                    Field {
                        label: "Namespace",
                        placeholder: "DashboardDemo",
                    },
                    Field {
                        label: "Metric",
                        placeholder: "Requests",
                    },
                    Field {
                        label: "Statistic",
                        placeholder: "Average, Sum, Maximum… (blank = Average)",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::CwPutMetricData,
                label: "Put metric data",
                needs_selection: false,
                fields: &[
                    Field {
                        label: "Namespace",
                        placeholder: "DashboardDemo",
                    },
                    Field {
                        label: "Metric",
                        placeholder: "Requests",
                    },
                    Field {
                        label: "Value",
                        placeholder: "42",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::CwPutAlarm,
                label: "Put alarm",
                needs_selection: false,
                reload: Reload::Inventory,
                name: Some(Field {
                    label: "Alarm name",
                    placeholder: "requests-high",
                }),
                fields: &[
                    Field {
                        label: "Namespace",
                        placeholder: "DashboardDemo",
                    },
                    Field {
                        label: "Metric",
                        placeholder: "Requests",
                    },
                    Field {
                        label: "Threshold",
                        placeholder: "100",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::CwPutCompositeAlarm,
                label: "Put composite alarm",
                needs_selection: false,
                reload: Reload::Inventory,
                name: Some(Field {
                    label: "Alarm name",
                    placeholder: "anything-is-bad",
                }),
                fields: &[Field {
                    label: "Rule",
                    placeholder: r#"ALARM("requests-high")"#,
                }],
                ..BASE
            },
            Operation {
                op: Op::CwGetDashboard,
                label: "Read dashboard",
                needs_selection: false,
                fields: &[Field {
                    label: "Dashboard name",
                    placeholder: "from the Dashboards list",
                }],
                ..BASE
            },
            Operation {
                op: Op::CwPutDashboard,
                label: "Put dashboard",
                needs_selection: false,
                name: Some(Field {
                    label: "Dashboard name",
                    placeholder: "my-dashboard",
                }),
                fields: &[Field {
                    label: "Body JSON",
                    placeholder: r#"{"widgets":[]}"#,
                }],
                ..BASE
            },
            Operation {
                op: Op::CwDeleteDashboard,
                label: "Delete dashboard",
                needs_selection: false,
                danger: true,
                fields: &[Field {
                    label: "Dashboard name",
                    placeholder: "from the Dashboards list",
                }],
                ..BASE
            },
            Operation {
                op: Op::CwSetAlarmState,
                label: "Set alarm state",
                reload: Reload::Inventory,
                fields: &[
                    Field {
                        label: "State",
                        placeholder: "OK, ALARM or INSUFFICIENT_DATA",
                    },
                    Field {
                        label: "Reason",
                        placeholder: "why the state changed",
                    },
                ],
                ..BASE
            },
        ],
        "sns" => vec![
            Operation {
                op: Op::SnsAttributes,
                label: "Topic attributes",
                ..BASE
            },
            Operation {
                op: Op::SnsSubscriptions,
                label: "All subscriptions",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::SnsSubAttributes,
                label: "Subscription attributes",
                needs_selection: false,
                fields: &[Field {
                    label: "Subscription ARN",
                    placeholder: "from the All subscriptions list",
                }],
                ..BASE
            },
            Operation {
                op: Op::SnsSetSubAttribute,
                label: "Set subscription attribute",
                needs_selection: false,
                reload: Reload::Detail,
                fields: &[
                    Field {
                        label: "Subscription ARN",
                        placeholder: "from the All subscriptions list",
                    },
                    Field {
                        label: "Attribute",
                        placeholder: "RawMessageDelivery, FilterPolicy…",
                    },
                    Field {
                        label: "Value",
                        placeholder: "true, or a JSON policy",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::SnsSetAttribute,
                label: "Set attribute",
                reload: Reload::Detail,
                fields: &[
                    Field {
                        label: "Attribute",
                        placeholder: "DisplayName, Policy, DeliveryPolicy…",
                    },
                    Field {
                        label: "Value",
                        placeholder: "empty clears a policy",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::SnsPublishSubject,
                label: "Publish with subject",
                fields: &[
                    Field {
                        label: "Subject",
                        placeholder: "shown by email endpoints",
                    },
                    Field {
                        label: "Message",
                        placeholder: "message body",
                    },
                ],
                ..BASE
            },
            Operation {
                op: Op::SnsConfirm,
                label: "Confirm subscription",
                reload: Reload::Detail,
                fields: &[Field {
                    label: "Token",
                    placeholder: "the SubscribeUrl token the endpoint received",
                }],
                ..BASE
            },
        ],
        "rds" => vec![
            Operation {
                op: Op::RdsInstanceState,
                label: "Instance status",
                ..BASE
            },
            Operation {
                op: Op::RdsClusters,
                label: "Clusters",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::RdsSnapshots,
                label: "Snapshots",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::RdsSubnetGroups,
                label: "Subnet groups",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::RdsParameterGroups,
                label: "Parameter groups",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::RdsClusterState,
                label: "Cluster status",
                needs_selection: false,
                fields: &[Field {
                    label: "Cluster id",
                    placeholder: "from the Clusters list",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsCreateSnapshot,
                label: "Create snapshot",
                reload: Reload::Inventory,
                fields: &[Field {
                    label: "Snapshot id",
                    placeholder: "my-snapshot",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsCreateCluster,
                label: "Create cluster",
                needs_selection: false,
                reload: Reload::Inventory,
                name: Some(Field {
                    label: "Cluster id",
                    placeholder: "my-cluster",
                }),
                fields: &[Field {
                    label: "Engine",
                    placeholder: "aurora-mysql, aurora-postgresql",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsDeleteCluster,
                label: "Delete cluster",
                needs_selection: false,
                danger: true,
                reload: Reload::Inventory,
                fields: &[Field {
                    label: "Cluster id",
                    placeholder: "from the Clusters list",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsStartCluster,
                label: "Start cluster",
                needs_selection: false,
                reload: Reload::Inventory,
                fields: &[Field {
                    label: "Cluster id",
                    placeholder: "from the Clusters list",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsStopCluster,
                label: "Stop cluster",
                needs_selection: false,
                reload: Reload::Inventory,
                fields: &[Field {
                    label: "Cluster id",
                    placeholder: "from the Clusters list",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsCreateSubnetGroup,
                label: "Create subnet group",
                needs_selection: false,
                reload: Reload::Inventory,
                name: Some(Field {
                    label: "Group name",
                    placeholder: "my-subnets",
                }),
                ..BASE
            },
            Operation {
                op: Op::RdsDeleteSubnetGroup,
                label: "Delete subnet group",
                needs_selection: false,
                danger: true,
                reload: Reload::Inventory,
                fields: &[Field {
                    label: "Group name",
                    placeholder: "from the Subnet groups list",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsCreateParameterGroup,
                label: "Create parameter group",
                needs_selection: false,
                reload: Reload::Inventory,
                name: Some(Field {
                    label: "Group name",
                    placeholder: "my-params",
                }),
                fields: &[Field {
                    label: "Family",
                    placeholder: "mysql8.0",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsDeleteParameterGroup,
                label: "Delete parameter group",
                needs_selection: false,
                danger: true,
                reload: Reload::Inventory,
                fields: &[Field {
                    label: "Group name",
                    placeholder: "from the Parameter groups list",
                }],
                ..BASE
            },
            Operation {
                op: Op::RdsDeleteSnapshot,
                label: "Delete snapshot",
                needs_selection: false,
                danger: true,
                fields: &[Field {
                    label: "Snapshot id",
                    placeholder: "from the Snapshots list",
                }],
                ..BASE
            },
        ],
        "iam" => vec![
            Operation {
                op: Op::IamRoles,
                label: "Roles",
                needs_selection: false,
                ..BASE
            },
            Operation {
                op: Op::IamCreateRole,
                label: "Create role",
                needs_selection: false,
                reload: Reload::Detail,
                fields: &[Field {
                    label: "Role name",
                    placeholder: "my-role",
                }],
                ..BASE
            },
            Operation {
                op: Op::IamDeleteRole,
                label: "Delete role",
                needs_selection: false,
                danger: true,
                reload: Reload::Detail,
                fields: &[Field {
                    label: "Role name",
                    placeholder: "from the Roles list",
                }],
                ..BASE
            },
        ],
        "ec2" => vec![Operation {
            op: Op::Ec2InstanceState,
            label: "Instance status",
            ..BASE
        }],
        _ => Vec::new(),
    };
    table
        .into_iter()
        .filter(|o| !o.needs_selection || selection)
        .collect()
}

/// What a call gives back that the page should show.
pub enum OpOutcome {
    Done,
    /// Lines of text for the Operations card.
    Rows(Vec<String>),
    /// A value that must never be logged or stored: it goes to the reveal banner only.
    Secret(String),
}

fn arg(args: &[String], index: usize, label: &str) -> Result<String> {
    let value = args
        .get(index)
        .map(|v| v.trim())
        .unwrap_or_default()
        .to_string();
    if value.is_empty() {
        bail!("{label} is required");
    }
    Ok(value)
}

fn number<T: std::str::FromStr>(args: &[String], index: usize, label: &str) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    let text = arg(args, index, label)?;
    text.parse::<T>()
        .map_err(|e| anyhow!("{label} is not a number: {e}"))
}

fn rows(items: Vec<Resource>) -> OpOutcome {
    OpOutcome::Rows(
        items
            .into_iter()
            .map(|r| {
                if r.detail.is_empty() {
                    r.name
                } else {
                    format!("{} · {}", r.name, r.detail)
                }
            })
            .collect(),
    )
}

fn one(line: String) -> OpOutcome {
    OpOutcome::Rows(vec![line])
}

fn none() -> OpOutcome {
    OpOutcome::Done
}

/// The call itself. `target` is the selected row's name, `handle` the ARN or URL its listing
/// kept, and `typed` the identifier the dialog collected when the operation is not about the
/// selected row.
pub async fn execute(
    op: Op,
    config: &SdkConfig,
    target: &str,
    handle: &str,
    args: &[String],
) -> Result<OpOutcome> {
    match op {
        Op::SecretsReveal => {
            let bytes = drivers::secrets_manager::reveal_secret_value(config, target).await?;
            Ok(OpOutcome::Secret(
                String::from_utf8_lossy(&bytes).to_string(),
            ))
        }
        Op::SecretsSetValue => {
            let value = arg(args, 0, "New value")?;
            drivers::secrets_manager::update_secret(config, target, &value).await?;
            Ok(none())
        }
        Op::SecretsDescribe => {
            let described = drivers::secrets_manager::describe_secret(config, target).await?;
            Ok(one(described.detail))
        }
        Op::SecretsValueInfo => {
            let info = drivers::secrets_manager::get_secret_value(config, target).await?;
            Ok(one(format!(
                "version {} · {} · {} bytes · {} · {}",
                info.version_id,
                info.created_date,
                info.byte_len,
                if info.is_binary { "binary" } else { "text" },
                info.version_stages.join(",")
            )))
        }

        Op::KinesisPutRecord => {
            let key = arg(args, 0, "Partition key")?;
            let data = arg(args, 1, "Data")?;
            let where_it_went =
                drivers::kinesis::put_record(config, target, &key, data.as_bytes()).await?;
            Ok(one(format!(
                "stored on {} as sequence {}",
                where_it_went.shard_id, where_it_went.sequence_number
            )))
        }
        Op::KinesisReadRecords => {
            let records = drivers::kinesis::read_records(config, target, 50).await?;
            Ok(OpOutcome::Rows(
                records
                    .into_iter()
                    .map(|r| {
                        format!(
                            "{} · key {} · {} bytes · {}",
                            r.sequence_number, r.partition_key, r.byte_len, r.data
                        )
                    })
                    .collect(),
            ))
        }
        Op::KinesisStreamState => Ok(one(drivers::kinesis::stream_state(config, target).await?)),
        Op::KinesisShardRecords => {
            let shard = arg(args, 0, "Shard id")?;
            let from = arg(args, 1, "From")?;
            let iterator =
                drivers::kinesis::get_shard_iterator(config, target, &shard, &from).await?;
            let page = drivers::kinesis::get_records(config, &iterator, 50).await?;
            let mut lines: Vec<String> = page
                .records
                .iter()
                .map(|r| {
                    format!(
                        "{} · key {} · {} bytes · {}",
                        r.sequence_number, r.partition_key, r.byte_len, r.data
                    )
                })
                .collect();
            lines.push(format!(
                "{} record(s) on {shard} · {} ms behind latest · more to read: {}",
                page.records.len(),
                page.millis_behind_latest.unwrap_or(0),
                page.next_shard_iterator.is_some()
            ));
            Ok(OpOutcome::Rows(lines))
        }
        Op::KinesisStreamNames => Ok(OpOutcome::Rows(
            drivers::kinesis::list_stream_names(config)
                .await?
                .into_iter()
                .collect(),
        )),

        Op::LogsCreateStream => {
            let stream = arg(args, 0, "Stream")?;
            drivers::cloudwatch_logs::create_log_stream(config, target, &stream).await?;
            Ok(none())
        }
        Op::LogsPutEvents => {
            let stream = arg(args, 0, "Stream")?;
            let message = arg(args, 1, "Message")?;
            drivers::cloudwatch_logs::put_log_events(config, target, &stream, &[message.as_str()])
                .await?;
            Ok(none())
        }
        Op::LogsTail => {
            let stream = arg(args, 0, "Stream")?;
            let events = drivers::cloudwatch_logs::get_log_events(config, target, &stream).await?;
            Ok(OpOutcome::Rows(
                events
                    .into_iter()
                    .map(|e| {
                        format!(
                            "{} · {}",
                            crate::aws::drivers::cloudwatch_logs::format_millis(e.timestamp),
                            e.message
                        )
                    })
                    .collect(),
            ))
        }
        Op::LogsFilter => {
            let pattern = arg(args, 0, "Pattern")?;
            let events =
                drivers::cloudwatch_logs::filter_log_events(config, target, &pattern, 100).await?;
            Ok(OpOutcome::Rows(
                events
                    .into_iter()
                    .map(|e| {
                        format!(
                            "{} · {} · {}",
                            crate::aws::drivers::cloudwatch_logs::format_millis(e.timestamp),
                            e.stream,
                            e.message
                        )
                    })
                    .collect(),
            ))
        }
        Op::LogsQuery => {
            let query = arg(args, 0, "Query")?;
            let results = drivers::cloudwatch_logs::run_query(config, target, &query, 3600).await?;
            Ok(OpOutcome::Rows(
                results
                    .into_iter()
                    .map(|row| {
                        row.fields
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect(),
            ))
        }
        Op::LogsRecentQueries => {
            let queries = drivers::cloudwatch_logs::describe_queries(config, target).await?;
            Ok(OpOutcome::Rows(
                queries
                    .into_iter()
                    .map(|q| format!("{} · {} · {}", q.id, q.status, q.query))
                    .collect(),
            ))
        }
        Op::LogsStopQuery => {
            let id = arg(args, 0, "Query id")?;
            drivers::cloudwatch_logs::stop_query(config, &id).await?;
            Ok(none())
        }
        Op::LogsSetRetention => {
            let days = number::<i32>(args, 0, "Days")?;
            drivers::cloudwatch_logs::put_retention_policy(config, target, days).await?;
            Ok(none())
        }
        Op::LogsDeleteRetention => {
            drivers::cloudwatch_logs::delete_retention_policy(config, target).await?;
            Ok(none())
        }
        Op::LogsDeleteTree => {
            let typed = arg(args, 0, "Group name confirmation")?;
            if typed != target {
                bail!("type {target} exactly to delete the group and every stream in it");
            }
            drivers::cloudwatch_logs::delete_log_group_tree(config, target).await?;
            Ok(none())
        }
        Op::LogsProtectOn => {
            drivers::cloudwatch_logs::set_deletion_protection(config, target, true).await?;
            Ok(none())
        }
        Op::LogsProtectOff => {
            drivers::cloudwatch_logs::set_deletion_protection(config, target, false).await?;
            Ok(none())
        }
        Op::LogsGroupFields => {
            let fields = drivers::cloudwatch_logs::log_group_fields(config, target).await?;
            Ok(OpOutcome::Rows(
                fields
                    .into_iter()
                    .map(|f| format!("{} · {}%", f.name, f.percent))
                    .collect(),
            ))
        }
        Op::LogsGroupArn => Ok(one(
            drivers::cloudwatch_logs::log_group_arn(config, target).await?
        )),
        Op::LogsMetricFilters => {
            let filters = drivers::cloudwatch_logs::describe_metric_filters(config, target).await?;
            Ok(OpOutcome::Rows(
                filters
                    .into_iter()
                    .map(|f| format!("{} · {} · {}", f.filter_name, f.filter_pattern, f.metric))
                    .collect(),
            ))
        }
        Op::LogsPutMetricFilter => {
            let name = arg(args, 0, "Filter name")?;
            let pattern = arg(args, 1, "Pattern")?;
            let metric = arg(args, 2, "Metric")?;
            let namespace = arg(args, 3, "Namespace")?;
            let value = arg(args, 4, "Value")?;
            drivers::cloudwatch_logs::put_metric_filter(
                config, target, &name, &pattern, &metric, &namespace, &value,
            )
            .await?;
            Ok(none())
        }
        Op::LogsDeleteMetricFilter => {
            let name = arg(args, 0, "Filter name")?;
            drivers::cloudwatch_logs::delete_metric_filter(config, target, &name).await?;
            Ok(none())
        }
        Op::LogsTestMetricFilter => {
            let pattern = arg(args, 0, "Pattern")?;
            let sample = arg(args, 1, "Sample event")?;
            let matches =
                drivers::cloudwatch_logs::test_metric_filter(config, &pattern, &[sample.as_str()])
                    .await?;
            Ok(OpOutcome::Rows(
                matches
                    .into_iter()
                    .map(|m| {
                        if m.is_empty() {
                            "no match".to_string()
                        } else {
                            m.iter()
                                .map(|(k, v)| format!("{k}={v}"))
                                .collect::<Vec<_>>()
                                .join(" ")
                        }
                    })
                    .collect(),
            ))
        }
        Op::LogsSubscriptionFilters => {
            let filters =
                drivers::cloudwatch_logs::describe_subscription_filters(config, target).await?;
            Ok(OpOutcome::Rows(
                filters
                    .into_iter()
                    .map(|f| {
                        format!(
                            "{} · {} → {} (role {})",
                            f.filter_name, f.filter_pattern, f.destination_arn, f.role_arn
                        )
                    })
                    .collect(),
            ))
        }
        Op::LogsPutSubscriptionFilter => {
            let name = arg(args, 0, "Filter name")?;
            let pattern = arg(args, 1, "Pattern")?;
            let destination = arg(args, 2, "Destination ARN")?;
            let role = args.get(3).map(|s| s.trim()).filter(|s| !s.is_empty());
            drivers::cloudwatch_logs::put_subscription_filter(
                config,
                target,
                &name,
                &pattern,
                &destination,
                role,
            )
            .await?;
            Ok(none())
        }
        Op::LogsDeleteSubscriptionFilter => {
            let name = arg(args, 0, "Filter name")?;
            drivers::cloudwatch_logs::delete_subscription_filter(config, target, &name).await?;
            Ok(none())
        }
        Op::LogsDestinations => {
            let destinations = drivers::cloudwatch_logs::describe_destinations(config).await?;
            Ok(OpOutcome::Rows(
                destinations
                    .into_iter()
                    .map(|d| {
                        format!(
                            "{} · {} · target {} · role {}",
                            d.name, d.arn, d.target_arn, d.role_arn
                        )
                    })
                    .collect(),
            ))
        }
        Op::LogsPutDestination => {
            let name = arg(args, 0, "Destination name")?;
            let target_arn = arg(args, 1, "Target ARN")?;
            let role_arn = arg(args, 2, "Role ARN")?;
            let arn =
                drivers::cloudwatch_logs::put_destination(config, &name, &target_arn, &role_arn)
                    .await?;
            Ok(one(arn))
        }
        Op::LogsDeleteDestination => {
            let name = arg(args, 0, "Destination name")?;
            drivers::cloudwatch_logs::delete_destination(config, &name).await?;
            Ok(none())
        }
        Op::LogsSetDestinationPolicy => {
            let name = arg(args, 0, "Destination name")?;
            let policy = arg(args, 1, "Access policy")?;
            drivers::cloudwatch_logs::set_destination_policy(config, &name, &policy).await?;
            Ok(none())
        }
        Op::LogsTags => {
            let tags = drivers::cloudwatch_logs::list_tags_log_group(config, target).await?;
            Ok(OpOutcome::Rows(
                tags.into_iter()
                    .map(|t| format!("{}={}", t.key, t.value))
                    .collect(),
            ))
        }
        Op::LogsAddTag => {
            let key = arg(args, 0, "Key")?;
            let value = arg(args, 1, "Value")?;
            drivers::cloudwatch_logs::tag_log_group(config, target, &[(&key, &value)]).await?;
            Ok(none())
        }
        Op::LogsRemoveTag => {
            let key = arg(args, 0, "Key")?;
            drivers::cloudwatch_logs::untag_log_group(config, target, &[key.as_str()]).await?;
            Ok(none())
        }

        Op::CwPutMetricData => {
            let namespace = arg(args, 0, "Namespace")?;
            let metric = arg(args, 1, "Metric")?;
            let value = number::<f64>(args, 2, "Value")?;
            drivers::cloudwatch::put_metric_data(config, &namespace, &metric, value).await?;
            Ok(one(format!("published {metric} = {value} in {namespace}")))
        }
        Op::CwPutAlarm => {
            let name = arg(args, 0, "Alarm name")?;
            let namespace = arg(args, 1, "Namespace")?;
            let metric = arg(args, 2, "Metric")?;
            let threshold = number::<f64>(args, 3, "Threshold")?;
            drivers::cloudwatch::put_metric_alarm(config, &name, &namespace, &metric, threshold)
                .await?;
            Ok(none())
        }
        Op::CwPutCompositeAlarm => {
            let name = arg(args, 0, "Alarm name")?;
            let rule = arg(args, 1, "Rule")?;
            drivers::cloudwatch::put_composite_alarm(config, &name, &rule).await?;
            Ok(none())
        }
        Op::CwSetAlarmState => {
            let state = arg(args, 0, "State")?;
            let reason = arg(args, 1, "Reason")?;
            drivers::cloudwatch::set_alarm_state(config, target, &state, &reason).await?;
            Ok(none())
        }
        Op::CwListMetrics => Ok(rows(drivers::cloudwatch::list_metrics(config).await?)),
        Op::CwNamespaces => Ok(OpOutcome::Rows(
            drivers::cloudwatch::list_namespaces(config).await?,
        )),
        Op::CwAlarmHistory => Ok(rows(
            drivers::cloudwatch::describe_alarm_history(config).await?,
        )),
        Op::CwListDashboards => Ok(rows(drivers::cloudwatch::list_dashboards(config).await?)),
        Op::CwMetricData => {
            let namespace = arg(args, 0, "Namespace")?;
            let metric = arg(args, 1, "Metric")?;
            let stat = args
                .get(2)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("Average")
                .to_string();
            let datapoints =
                drivers::cloudwatch::get_metric_data(config, &namespace, &metric, &stat, 3).await?;
            Ok(rows(datapoints))
        }
        Op::CwGetDashboard => {
            let name = arg(args, 0, "Dashboard name")?;
            Ok(one(drivers::cloudwatch::get_dashboard(config, &name).await?))
        }
        Op::CwPutDashboard => {
            let name = arg(args, 0, "Dashboard name")?;
            let body = arg(args, 1, "Body JSON")?;
            drivers::cloudwatch::put_dashboard(config, &name, &body).await?;
            Ok(none())
        }
        Op::CwDeleteDashboard => {
            let name = arg(args, 0, "Dashboard name")?;
            drivers::cloudwatch::delete_dashboards(config, &[name]).await?;
            Ok(none())
        }

        Op::SnsAttributes => {
            let attributes = drivers::sns::get_topic_attributes(config, handle).await?;
            Ok(OpOutcome::Rows(
                attributes
                    .into_iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect(),
            ))
        }
        Op::SnsSetAttribute => {
            let name = arg(args, 0, "Attribute")?;
            let value = args
                .get(1)
                .map(|v| v.trim())
                .unwrap_or_default()
                .to_string();
            drivers::sns::set_topic_attribute(config, handle, &name, &value).await?;
            Ok(none())
        }
        Op::SnsPublishSubject => {
            let subject = arg(args, 0, "Subject")?;
            let message = arg(args, 1, "Message")?;
            let id = drivers::sns::publish(config, handle, Some(&subject), &message).await?;
            Ok(one(format!("published as message {id}")))
        }
        Op::SnsConfirm => {
            let token = arg(args, 0, "Token")?;
            let arn = drivers::sns::confirm_subscription(config, handle, &token).await?;
            Ok(one(arn))
        }
        Op::SnsSubscriptions => Ok(rows(drivers::sns::list_subscriptions(config).await?)),
        Op::SnsSubAttributes => {
            let arn = arg(args, 0, "Subscription ARN")?;
            let attributes = drivers::sns::get_subscription_attributes(config, &arn).await?;
            Ok(OpOutcome::Rows(
                attributes
                    .into_iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect(),
            ))
        }
        Op::SnsSetSubAttribute => {
            let arn = arg(args, 0, "Subscription ARN")?;
            let name = arg(args, 1, "Attribute")?;
            let value = arg(args, 2, "Value")?;
            drivers::sns::set_subscription_attribute(config, &arn, &name, &value).await?;
            Ok(none())
        }

        Op::RdsInstanceState => Ok(one(drivers::rds::instance_state(config, target).await?)),
        Op::RdsCreateSnapshot => {
            let id = arg(args, 0, "Snapshot id")?;
            drivers::rds::create_snapshot(config, target, &id).await?;
            Ok(none())
        }
        Op::RdsDeleteSnapshot => {
            let id = arg(args, 0, "Snapshot id")?;
            drivers::rds::delete_snapshot(config, &id).await?;
            Ok(none())
        }
        Op::RdsSnapshots => Ok(rows(drivers::rds::fetch_snapshots(config).await?)),
        Op::RdsClusters => Ok(rows(drivers::rds::fetch_clusters(config).await?)),
        Op::RdsClusterState => {
            let id = arg(args, 0, "Cluster id")?;
            Ok(one(drivers::rds::cluster_state(config, &id).await?))
        }
        Op::RdsCreateCluster => {
            let id = arg(args, 0, "Cluster id")?;
            let engine = args
                .get(1)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            match engine {
                Some(engine) => drivers::rds::create_cluster_with(config, &id, &engine).await?,
                None => drivers::rds::create_cluster(config, &id).await?,
            };
            Ok(none())
        }
        Op::RdsDeleteCluster => {
            let id = arg(args, 0, "Cluster id")?;
            drivers::rds::delete_cluster(config, &id).await?;
            Ok(none())
        }
        Op::RdsStartCluster => {
            let id = arg(args, 0, "Cluster id")?;
            drivers::rds::start_cluster(config, &id).await?;
            Ok(none())
        }
        Op::RdsStopCluster => {
            let id = arg(args, 0, "Cluster id")?;
            drivers::rds::stop_cluster(config, &id).await?;
            Ok(none())
        }
        Op::RdsSubnetGroups => Ok(rows(drivers::rds::fetch_subnet_groups(config).await?)),
        Op::RdsCreateSubnetGroup => {
            let name = arg(args, 0, "Group name")?;
            drivers::rds::create_subnet_group(config, &name).await?;
            Ok(none())
        }
        Op::RdsDeleteSubnetGroup => {
            let name = arg(args, 0, "Group name")?;
            drivers::rds::delete_subnet_group(config, &name).await?;
            Ok(none())
        }
        Op::RdsParameterGroups => Ok(rows(drivers::rds::fetch_parameter_groups(config).await?)),
        Op::RdsCreateParameterGroup => {
            let name = arg(args, 0, "Group name")?;
            let family = arg(args, 1, "Family")?;
            drivers::rds::create_parameter_group(config, &name, &family).await?;
            Ok(none())
        }
        Op::RdsDeleteParameterGroup => {
            let name = arg(args, 0, "Group name")?;
            drivers::rds::delete_parameter_group(config, &name).await?;
            Ok(none())
        }

        Op::IamRoles => Ok(rows(drivers::iam::fetch_roles(config).await?)),
        Op::IamCreateRole => {
            let name = arg(args, 0, "Role name")?;
            drivers::iam::create_role(config, &name).await?;
            Ok(none())
        }
        Op::IamDeleteRole => {
            let name = arg(args, 0, "Role name")?;
            drivers::iam::delete_role(config, &name).await?;
            Ok(none())
        }

        Op::Ec2InstanceState => Ok(one(drivers::ec2::instance_state(config, target).await?)),
    }
}

/// The identifier an operation acts on, in the order `execute` reads it: the dialog's name
/// slot when the operation declares one, otherwise the selected row's name.
pub fn target_of(operation: &Operation, selected_name: &str, typed_name: &str) -> String {
    match operation.name {
        Some(_) => typed_name.trim().to_string(),
        None => selected_name.to_string(),
    }
}
