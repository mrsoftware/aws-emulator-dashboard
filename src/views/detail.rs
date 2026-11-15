use crate::app_state::{AppState, Dialog, connection_for};
use crate::aws::client::{EmulatorConnection, REGION, runtime};
use crate::aws::manage;
use crate::views::catalog::{MANAGED, resource_noun};
use crate::views::operations::{self, OpOutcome, Operation};
use crate::views::root::RootView;
use crate::views::theme;
use gpui_kit::{
    AnyElement, AsyncApp, Context, Div, Entity, Focusable, FontWeight, PathPromptOptions,
    SharedString, Stateful, Window, div, prelude::*, px, rgb, rgba,
};
use std::future::Future;

/// The widths of the object table, shared by its header strip and its rows.
const OBJECT_COLUMNS: &[(Option<f32>, &str)] = &[
    (None, "KEY"),
    (Some(90.0), "SIZE"),
    (Some(180.0), "MODIFIED"),
    (Some(230.0), ""),
];

/// The widths of the item table.
const ITEM_COLUMNS: &[(Option<f32>, &str)] = &[(None, "ITEM"), (Some(90.0), "")];

/// The widths of the received-message table.
const MESSAGE_COLUMNS: &[(Option<f32>, &str)] = &[
    (None, "BODY"),
    (Some(220.0), "MESSAGE ID"),
    (Some(90.0), ""),
];

/// The widths of the child table: an access key, a subscription, a stream, a shard.
const CHILD_COLUMNS: &[(Option<f32>, &str)] =
    &[(None, "NAME"), (Some(260.0), "DETAILS"), (Some(90.0), "")];

/// The dimmed page behind an open dialog: the page colour at ~70% opacity.
const BACKDROP: u32 = (theme::PAGE << 8) | 0xB3;

#[derive(Clone)]
enum DetailAction {
    Upload,
    RefreshDetail,
    DeleteResource,
    PutItemDialog,
    SendMessageDialog,
    Receive,
    DownloadObject(String),
    DeleteObject(String),
    DeleteItem(String),
    DeleteMessage(String),
    CreateChild,
    /// STS: assume the selected role and verify the minted credential. The mint is
    /// recorded dashboard-side; STS has no list operation to answer for it.
    AssumeRole,
    /// SSM: read the selected parameter's value, decrypted when it is a SecureString.
    /// The value lands in the one-time reveal banner and is never stored.
    RevealParameter,
    DeleteChild {
        name: String,
        detail: String,
    },
    Lifecycle(&'static str),
    PublishDialog,
    SubscribeDialog,
    DismissReveal,
    ClearOutput,
    /// One row of the operations table: either the call runs at once or it opens a dialog
    /// for the arguments its descriptor declares.
    RunOp(Operation),
}

#[derive(Clone, Copy)]
pub(crate) enum Reload {
    None,
    Inventory,
    Detail,
}

pub fn select(state: &Entity<AppState>, cx: &mut Context<RootView>, name: String) {
    state.update(cx, |s, cx| {
        s.selected_resource = Some(name);
        s.detail = Default::default();
        s.detail.loading = true;
        s.action_error = None;
        cx.notify();
    });
    let state_clone = state.clone();
    cx.spawn(async move |_, cx| fetch_detail(&state_clone, cx).await)
        .detach();
}

/// The page a managed sidebar row opens: the live listing, the operations the selected row
/// offers, and its detail. The create button is not here — the header owns it, the way the
/// console puts one action at the top right of every page.
pub fn render_page(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let key = state.selected_service.clone();
    let label = MANAGED
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, l)| *l)
        .unwrap_or("Resources");
    let noun = resource_noun(&key).unwrap_or("resource");
    let count = state.service(&key).items.len();

    div()
        .relative()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            theme::card()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(theme::TEXT_PRIMARY))
                                .child(label.to_string()),
                        )
                        .child(theme::count_badge(count))
                        .child(
                            div()
                                .ml_auto()
                                .font_family(theme::MONO_FONT)
                                .text_size(px(11.0))
                                .text_color(rgb(theme::TEXT_SECONDARY))
                                .child(format!("{count} {noun}s listed · region {REGION}")),
                        ),
                )
                .child(resource_table(state, &key, cx)),
        )
        .children(render_operations(state, cx))
        .children(render_detail(state, cx))
        .children(dialog_overlay(state, cx))
        .into_any()
}

fn resource_table(state: &AppState, key: &str, cx: &mut Context<RootView>) -> AnyElement {
    let resources = state.service(key);
    if let Some(err) = &resources.error {
        return div()
            .text_color(rgb(theme::FAIL))
            .child(format!("The listing failed: {err}"))
            .into_any();
    }
    if resources.items.is_empty() {
        return theme::muted(if resources.loading {
            "Loading…"
        } else {
            "Nothing listed yet — the header's create button makes the first one."
        });
    }
    div()
        .flex()
        .flex_col()
        .child(theme::table_head(&[
            (Some(320.0), "NAME"),
            (None, "DETAILS"),
        ]))
        .children(resources.items.iter().map(|r| {
            let name = r.name.clone();
            let selected = state.selected_resource.as_deref() == Some(name.as_str());
            table_row(SharedString::from(format!("row-{name}")), selected)
                .on_click(cx.listener(move |this, _, _, cx| select(&this.state, cx, name.clone())))
                .child(mono_cell(Some(320.0), theme::TEXT_PRIMARY, r.name.clone()))
                .child(mono_cell(None, theme::TEXT_SECONDARY, r.detail.clone()))
                .into_any()
        }))
        .into_any()
}

enum DetailData {
    Objects(Vec<manage::ObjectInfo>),
    Table {
        key_schema: Vec<(String, String)>,
        items: Vec<String>,
    },
    Children(Vec<crate::app_state::Resource>),
    Empty,
}

pub async fn fetch_detail(state: &Entity<AppState>, cx: &mut AsyncApp) {
    let (conn, service, name, resource_detail) = cx.update(|cx| {
        let s = state.read(cx);
        (
            connection_for(s),
            s.selected_service.clone(),
            s.selected_resource.clone(),
            s.selected_resource().map(|r| r.detail).unwrap_or_default(),
        )
    });
    let (Some(conn), Some(name)) = (conn, name) else {
        cx.update(|cx| {
            state.update(cx, |s, cx| {
                s.detail.loading = false;
                cx.notify();
            })
        });
        return;
    };
    let result = runtime()
        .spawn(async move {
            let config = conn.build_aws_config().await?;
            match service.as_str() {
                "s3" => Ok(DetailData::Objects(
                    manage::list_objects(&config, &name).await?,
                )),
                "dynamodb" => {
                    let key_schema = manage::describe_key_schema(&config, &name).await?;
                    let items = manage::scan_items(&config, &name, 100).await?;
                    Ok(DetailData::Table { key_schema, items })
                }
                "iam" | "sns" | "logs" | "kinesis" | "secretsmanager" | "ssm" => {
                    Ok(DetailData::Children(
                        crate::views::root::fetch_children(
                            &service,
                            &config,
                            &name,
                            &resource_detail,
                        )
                        .await?,
                    ))
                }
                _ => Ok(DetailData::Empty),
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("detail task failed: {e}"))
        .and_then(|r| r);
    cx.update(|cx| {
        state.update(cx, |s, cx| {
            s.detail.loading = false;
            match result {
                Ok(DetailData::Objects(objects)) => {
                    s.detail.error = None;
                    s.detail.objects = objects;
                }
                Ok(DetailData::Table { key_schema, items }) => {
                    s.detail.error = None;
                    s.detail.key_schema = key_schema;
                    s.detail.items = items;
                }
                Ok(DetailData::Children(children)) => {
                    s.detail.error = None;
                    s.detail.children = children;
                }
                Ok(DetailData::Empty) => {
                    s.detail.error = None;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "detail refresh failed");
                    s.detail.error = Some(format!("{e:#}"));
                }
            }
            cx.notify();
        })
    });
}

pub(crate) fn run<T: Send + 'static>(
    state: &Entity<AppState>,
    cx: &mut Context<RootView>,
    task: impl Future<Output = anyhow::Result<T>> + Send + 'static,
    apply: impl FnOnce(&mut AppState, T) + Send + 'static,
    reload: Reload,
) {
    state.update(cx, |s, cx| {
        s.action_busy = true;
        s.action_error = None;
        cx.notify();
    });
    let state_clone = state.clone();
    cx.spawn(async move |_, cx| {
        let result = runtime().spawn(task).await;
        let result = match result {
            Ok(r) => r,
            Err(e) => Err(anyhow::anyhow!("action task failed: {e}")),
        };
        cx.update(|cx| {
            state_clone.update(cx, |s, cx| {
                s.action_busy = false;
                match result {
                    Ok(v) => {
                        tracing::info!("management action succeeded");
                        apply(s, v);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "management action failed");
                        s.action_error = Some(format!("{e:#}"));
                    }
                }
                let entity = cx.entity();
                if matches!(reload, Reload::Detail) {
                    s.detail.loading = true;
                    let e = entity.clone();
                    cx.spawn(async move |_, cx| fetch_detail(&e, cx).await)
                        .detach();
                }
                if matches!(reload, Reload::Inventory) {
                    cx.spawn(async move |_, cx| {
                        crate::views::root::fetch_inventory(&entity, cx).await
                    })
                    .detach();
                }
                cx.notify();
            })
        });
    })
    .detach();
}

fn set_error(state: &Entity<AppState>, cx: &mut Context<RootView>, msg: &str) {
    state.update(cx, |s, cx| {
        s.action_error = Some(msg.to_string());
        cx.notify();
    });
}

pub fn open_dialog(
    state: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<RootView>,
    dialog: Dialog,
) {
    state.update(cx, |s, cx| {
        s.dialog = Some(dialog);
        s.dialog_op = None;
        s.action_error = None;
        for input in [
            &s.input_name,
            &s.input_pk,
            &s.input_sk,
            &s.input_json,
            &s.input_body,
            &s.input_protocol,
            &s.input_endpoint,
            &s.input_instance_type,
        ] {
            input.update(cx, |f, cx| f.clear(window, cx));
        }
        cx.notify();
    });
    let s = state.read(cx);
    let first = match dialog {
        Dialog::PutItem => &s.input_json,
        Dialog::SendMessage | Dialog::Publish => &s.input_body,
        Dialog::Subscribe => &s.input_protocol,
        Dialog::LaunchInstance => &s.input_instance_type,
        _ => &s.input_name,
    };
    let handle = first.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
}

/// Open the argument dialog for one operation: the fields it declares get their own
/// labels, and the name slot appears only when the operation asks for an identifier the
/// page is not already sitting on.
fn open_operation(
    state: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<RootView>,
    operation: Operation,
) {
    state.update(cx, |s, cx| {
        s.dialog = Some(Dialog::Operation);
        s.action_error = None;
        for input in [
            &s.input_name,
            &s.input_json,
            &s.input_body,
            &s.input_ops[0],
            &s.input_ops[1],
            &s.input_ops[2],
            &s.input_ops[3],
        ] {
            input.update(cx, |f, cx| f.clear(window, cx));
        }
        for (index, field) in operation.fields.iter().enumerate() {
            if let Some(input) = s.input_ops.get(index) {
                input.update(cx, |f, _| f.set_placeholder(field.placeholder));
            }
        }
        s.dialog_op = Some(operation.clone());
        cx.notify();
    });
    let s = state.read(cx);
    let first = if operation.name.is_some() {
        &s.input_name
    } else {
        &s.input_ops[0]
    };
    let handle = first.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
}

pub fn close_dialog(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    state.update(cx, |s, cx| {
        s.dialog = None;
        s.dialog_op = None;
        cx.notify();
    });
}

/// Make one operation's SDK call. `typed_name` is the identifier an operation collects when
/// it does not act on the selected row; `fields` are its declared arguments, and the typed
/// name leads the list when the operation has a name slot — the order `execute` reads.
///
/// The arguments are never logged: an operation's field can be a secret value, and a log
/// line would outlive the reveal banner that is supposed to be its only appearance.
fn fire(
    state: &Entity<AppState>,
    cx: &mut Context<RootView>,
    operation: Operation,
    typed_name: String,
    fields: Vec<String>,
) {
    let (conn, selected_name, handle) = {
        let s = state.read(cx);
        let Some(conn) = connection_for(s) else {
            set_error(state, cx, "Connect to the emulator first");
            return;
        };
        let (name, handle) = s
            .selected_resource()
            .map(|r| (r.name, r.detail))
            .unwrap_or_default();
        if operation.needs_selection && name.is_empty() {
            set_error(state, cx, "Select a resource first");
            return;
        }
        (conn, name, handle)
    };
    let target = operations::target_of(&operation, &selected_name, &typed_name);
    let needs_typed_name = operation.name.is_some();
    if needs_typed_name {
        let label = operation
            .name
            .as_ref()
            .map(|f| f.label)
            .unwrap_or("identifier");
        if target.is_empty() {
            set_error(state, cx, &format!("{label} is required"));
            return;
        }
    }
    let op = operation.op;
    let reload = operation.reload;
    let mut args = Vec::new();
    if needs_typed_name {
        args.push(typed_name.clone());
    }
    args.extend(fields);
    let shown = target.clone();
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            tracing::info!("running a dashboard operation");
            operations::execute(op, &c, &target, &handle, &args).await
        },
        move |s, outcome| {
            s.dialog = None;
            s.dialog_op = None;
            match outcome {
                OpOutcome::Done => {
                    s.detail.output.clear();
                }
                OpOutcome::Rows(rows) => s.detail.output = rows,
                OpOutcome::Secret(value) => {
                    s.detail.revealed = Some(format!(
                        "{shown}: {value} — copy it now, this banner is the only place it appears."
                    ))
                }
            }
        },
        reload,
    );
}

pub fn confirm_dialog(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let (
        dialog,
        conn,
        service,
        name,
        pk,
        sk,
        json,
        body,
        table,
        resource_url,
        instance_type,
        protocol,
        endpoint,
        operation,
        op_values,
    ) = {
        let s = state.read(cx);
        (
            s.dialog,
            connection_for(s),
            s.selected_service.clone(),
            s.input_name.read(cx).value(),
            s.input_pk.read(cx).value(),
            s.input_sk.read(cx).value(),
            s.input_json.read(cx).value(),
            s.input_body.read(cx).value(),
            s.selected_resource.clone(),
            s.selected_resource().map(|r| r.detail),
            s.input_instance_type.read(cx).value(),
            s.input_protocol.read(cx).value(),
            s.input_endpoint.read(cx).value(),
            s.dialog_op.clone(),
            s.input_ops
                .iter()
                .map(|input| input.read(cx).value())
                .collect::<Vec<String>>(),
        )
    };
    let Some(dialog) = dialog else { return };
    if let Dialog::Operation = dialog {
        let Some(operation) = operation else {
            close_dialog(state, cx);
            return;
        };
        let typed_name = name;
        let fields: Vec<String> = op_values.into_iter().take(operation.fields.len()).collect();
        fire(state, cx, operation, typed_name, fields);
        return;
    }
    let Some(conn) = conn else {
        set_error(state, cx, "Connect to the emulator first");
        return;
    };
    let name = name.trim().to_string();
    let pk = pk.trim().to_string();
    let sk = sk.trim().to_string();
    match dialog {
        Dialog::Operation => unreachable!("handled with its descriptor above"),
        Dialog::CreateBucket => {
            if name.is_empty() {
                set_error(state, cx, "Bucket name is required");
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    if manage::bucket_exists(&c, &name).await? {
                        anyhow::bail!("bucket {name} already exists");
                    }
                    manage::create_bucket(&c, &name).await
                },
                |s, _| s.dialog = None,
                Reload::Inventory,
            );
        }
        Dialog::CreateTable => {
            if name.is_empty() || pk.is_empty() {
                set_error(state, cx, "Table name and partition key are required");
                return;
            }
            if pk == sk {
                set_error(
                    state,
                    cx,
                    "Sort key must be a different attribute than the partition key",
                );
                return;
            }
            let sk = if sk.is_empty() { None } else { Some(sk) };
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    if manage::table_exists(&c, &name).await? {
                        anyhow::bail!("table {name} already exists");
                    }
                    manage::create_table(&c, &name, &pk, sk.as_deref()).await
                },
                |s, _| s.dialog = None,
                Reload::Inventory,
            );
        }
        Dialog::CreateQueue => {
            if name.is_empty() {
                set_error(state, cx, "Queue name is required");
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    if manage::queue_exists(&c, &name).await? {
                        anyhow::bail!("queue {name} already exists");
                    }
                    manage::create_queue(&c, &name).await
                },
                |s, _| s.dialog = None,
                Reload::Inventory,
            );
        }
        Dialog::PutItem => {
            let Some(table) = table else {
                set_error(state, cx, "Select a table first");
                return;
            };
            if json.trim().is_empty() {
                set_error(state, cx, "Item JSON is required");
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    manage::put_item(&c, &table, &json).await
                },
                |s, _| s.dialog = None,
                Reload::Detail,
            );
        }
        Dialog::SendMessage => {
            let Some(url) = resource_url else {
                set_error(state, cx, "Select a queue first");
                return;
            };
            if body.trim().is_empty() {
                set_error(state, cx, "Message body is required");
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    manage::send_message(&c, &url, &body).await
                },
                |s, _| s.dialog = None,
                Reload::None,
            );
        }
        Dialog::LaunchInstance => {
            let instance_type = instance_type.trim().to_string();
            if instance_type.is_empty() {
                set_error(state, cx, "Instance type is required, for example t3.micro");
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    crate::aws::drivers::ec2::create_instance(&c, &instance_type).await
                },
                |s, id| {
                    s.dialog = None;
                    tracing::info!(instance = %id, "launched instance");
                },
                Reload::Inventory,
            );
        }
        Dialog::CreateNamed => {
            let Some(noun) = crate::views::catalog::resource_noun(&service) else {
                set_error(state, cx, "Open a service page first");
                return;
            };
            if name.is_empty() {
                set_error(state, cx, &format!("{noun} name is required"));
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    crate::views::root::create_resource(&service, &c, &name).await
                },
                |s, _| s.dialog = None,
                Reload::Inventory,
            );
        }
        Dialog::CreateSecret => {
            if name.is_empty() {
                set_error(state, cx, "Secret name is required");
                return;
            }
            if json.trim().is_empty() {
                set_error(
                    state,
                    cx,
                    "A secret needs a value; text or JSON, your choice",
                );
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    // The listing drops a secret parked in its deletion window while
                    // `CreateSecret` still refuses its name, so only `DescribeSecret`
                    // can predict this failure.
                    if crate::aws::drivers::secrets_manager::secret_exists(&c, &name).await? {
                        anyhow::bail!(
                            "{name} already exists, or is scheduled for deletion and can still be recovered"
                        );
                    }
                    crate::aws::drivers::secrets_manager::create_secret(&c, &name, &json)
                        .await
                        .map(|_| ())
                },
                |s, _| s.dialog = None,
                Reload::Inventory,
            );
        }
        Dialog::CreateParameter => {
            if name.is_empty() {
                set_error(state, cx, "Parameter name is required");
                return;
            }
            if json.trim().is_empty() {
                set_error(state, cx, "A parameter needs a value");
                return;
            }
            let parameter_type = if protocol.trim().is_empty() {
                crate::aws::drivers::ssm::DEFAULT_PARAMETER_TYPE.to_string()
            } else {
                protocol.trim().to_string()
            };
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    // PutParameter with no Overwrite refuses an existing name, but the
                    // listing pre-check states it the way every other create does.
                    if crate::aws::drivers::ssm::fetch(&c)
                        .await?
                        .iter()
                        .any(|r| r.name == name)
                    {
                        anyhow::bail!("{name} already exists");
                    }
                    crate::aws::drivers::ssm::create(&c, &name, &json, &parameter_type).await
                },
                |s, _| s.dialog = None,
                Reload::Inventory,
            );
        }
        Dialog::Publish => {
            let Some(arn) = resource_url else {
                set_error(state, cx, "Select a topic first");
                return;
            };
            if body.trim().is_empty() {
                set_error(state, cx, "Message is required");
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    crate::aws::drivers::sns::publish(&c, &arn, None, &body)
                        .await
                        .map(|_| ())
                },
                |s, _| s.dialog = None,
                Reload::None,
            );
        }
        Dialog::Subscribe => {
            let Some(arn) = resource_url else {
                set_error(state, cx, "Select a topic first");
                return;
            };
            let protocol = protocol.trim().to_string();
            let endpoint = endpoint.trim().to_string();
            if protocol.is_empty() || endpoint.is_empty() {
                set_error(state, cx, "Protocol and endpoint are both required");
                return;
            }
            run(
                state,
                cx,
                async move {
                    let c = conn.build_aws_config().await?;
                    crate::aws::drivers::sns::subscribe(&c, &arn, &protocol, &endpoint)
                        .await
                        .map(|_| ())
                },
                |s, _| s.dialog = None,
                Reload::Detail,
            );
        }
    }
}

fn dispatch(
    state: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<RootView>,
    action: DetailAction,
) {
    match action {
        DetailAction::Upload => upload_object(state, cx),
        DetailAction::RefreshDetail => {
            state.update(cx, |s, cx| {
                s.detail.loading = true;
                cx.notify();
            });
            let state_clone = state.clone();
            cx.spawn(async move |_, cx| fetch_detail(&state_clone, cx).await)
                .detach();
        }
        DetailAction::DeleteResource => delete_selected(state, cx),
        DetailAction::PutItemDialog => open_dialog(state, window, cx, Dialog::PutItem),
        DetailAction::SendMessageDialog => open_dialog(state, window, cx, Dialog::SendMessage),
        DetailAction::Receive => receive_messages(state, cx),
        DetailAction::DownloadObject(key) => download_object(state, cx, key),
        DetailAction::DeleteObject(key) => delete_object(state, cx, key),
        DetailAction::DeleteItem(item) => delete_item(state, cx, item),
        DetailAction::DeleteMessage(receipt) => delete_message(state, cx, receipt),
        DetailAction::CreateChild => create_child(state, cx),
        DetailAction::AssumeRole => assume_selected(state, cx),
        DetailAction::RevealParameter => reveal_parameter(state, cx),
        DetailAction::Lifecycle(verb) => run_lifecycle(state, cx, verb),
        DetailAction::DeleteChild { name, detail } => delete_child(state, cx, name, detail),
        DetailAction::PublishDialog => open_dialog(state, window, cx, Dialog::Publish),
        DetailAction::SubscribeDialog => open_dialog(state, window, cx, Dialog::Subscribe),
        DetailAction::DismissReveal => {
            state.update(cx, |s, cx| {
                s.detail.revealed = None;
                cx.notify();
            });
        }
        DetailAction::ClearOutput => {
            state.update(cx, |s, cx| {
                s.detail.output.clear();
                cx.notify();
            });
        }
        DetailAction::RunOp(operation) => {
            if operation.name.is_none() && operation.fields.is_empty() {
                fire(state, cx, operation, String::new(), Vec::new());
            } else {
                open_operation(state, window, cx, operation);
            }
        }
    }
}

fn selected(
    state: &Entity<AppState>,
    cx: &mut Context<RootView>,
) -> Option<(EmulatorConnection, String, String)> {
    let s = state.read(cx);
    let conn = connection_for(s)?;
    let res = s.selected_resource()?;
    Some((conn, s.selected_service.clone(), res.name))
}

/// The selected row plus the handle its listing kept in `detail`: a queue URL, a topic ARN.
/// Name-addressed calls use `selected`; ARN-addressed ones need this.
fn selected_row(
    state: &Entity<AppState>,
    cx: &mut Context<RootView>,
) -> Option<(EmulatorConnection, String, String, String)> {
    let s = state.read(cx);
    let conn = connection_for(s)?;
    let res = s.selected_resource()?;
    Some((conn, s.selected_service.clone(), res.name, res.detail))
}

fn delete_selected(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let Some((conn, service, res, url)) = selected_row(state, cx) else {
        return;
    };
    let task = async move {
        let c = conn.build_aws_config().await?;
        match service.as_str() {
            "s3" => manage::delete_bucket(&c, &res).await,
            "dynamodb" => manage::delete_table(&c, &res).await,
            "sqs" => manage::delete_queue(&c, &url).await,
            other => crate::views::root::delete_resource(other, &c, &res, &url).await,
        }
    };
    run(
        state,
        cx,
        task,
        |s, _| {
            s.selected_resource = None;
            s.detail = Default::default();
        },
        Reload::Inventory,
    );
}

/// Create a child of the selected resource. An access key secret is returned exactly once by
/// the service, so it lands in `detail.revealed` for the user to copy and is never logged.
fn create_child(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let Some((conn, service, name, _)) = selected_row(state, cx) else {
        return;
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            match service.as_str() {
                "iam" => {
                    let (id, secret) =
                        crate::aws::drivers::iam::create_access_key(&c, &name).await?;
                    Ok((id, secret))
                }
                other => Err(anyhow::anyhow!("no child create for {other}")),
            }
        },
        |s, (id, secret)| {
            s.detail.revealed = Some(format!(
                "Access key {id} created. Secret: {secret} — copy it now, it cannot be read back."
            ));
        },
        Reload::Detail,
    );
}

/// Assume the selected role through the STS workbench. The minted credential is
/// verified inside the driver's call; what lands here is the fact of the mint and
/// the identity it speaks for. The secret is never stored: the reveal line says so.
fn assume_selected(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let Some((conn, _, _, arn)) = selected_row(state, cx) else {
        return;
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            crate::aws::drivers::sts::assume(
                &c,
                &arn,
                crate::aws::drivers::sts::DEFAULT_SESSION_NAME,
            )
            .await
        },
        |s, mint| {
            s.minted_sessions.push(mint.clone());
            s.detail.revealed = Some(format!(
                "Assumed role {} as session {}. Identity: {}. Credentials expire: {}. \
                 Access key {} minted; its secret is not stored.",
                mint.role_name,
                mint.session_name,
                mint.identity_arn,
                mint.expires,
                mint.access_key_id,
            ));
        },
        Reload::None,
    );
}

/// Read the selected parameter's value through `GetParameter(WithDecryption=true)`.
/// A SecureString decrypts only if the emulator's KMS gate permits the caller; its
/// refusal is the error the user reads. The value is shown once and never stored.
fn reveal_parameter(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let Some((conn, _, name, _)) = selected_row(state, cx) else {
        return;
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            let (type_, value) = crate::aws::drivers::ssm::reveal(&c, &name).await?;
            Ok((name, type_, value))
        },
        |s, (name, type_, value)| {
            s.detail.revealed = Some(format!("Parameter {name}: ({type_}) {value}"));
        },
        Reload::None,
    );
}

fn delete_child(
    state: &Entity<AppState>,
    cx: &mut Context<RootView>,
    child: String,
    child_detail: String,
) {
    let Some((conn, service, name, _)) = selected_row(state, cx) else {
        return;
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            match service.as_str() {
                "iam" => crate::aws::drivers::iam::delete_access_key(&c, &name, &child).await,
                "sns" => crate::aws::drivers::sns::unsubscribe(&c, &child_detail).await,
                "logs" => {
                    crate::aws::drivers::cloudwatch_logs::delete_log_stream(&c, &name, &child).await
                }
                other => Err(anyhow::anyhow!("no child delete for {other}")),
            }
        },
        |_, _| {},
        Reload::Detail,
    );
}

/// Whether a child row of this service has a delete call behind it.
fn child_is_deletable(service: &str) -> bool {
    matches!(service, "iam" | "sns" | "logs")
}

fn upload_object(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let Some((conn, _, bucket)) = selected(state, cx) else {
        return;
    };
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Choose a file to upload".into()),
    });
    run(
        state,
        cx,
        async move {
            let paths = receiver
                .await
                .map_err(|_| anyhow::anyhow!("file prompt failed"))?
                .map_err(|e| anyhow::anyhow!("file prompt: {e}"))?;
            let path = paths
                .and_then(|p| p.into_iter().next())
                .ok_or_else(|| anyhow::anyhow!("upload cancelled"))?;
            let key = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "upload".to_string());
            let c = conn.build_aws_config().await?;
            manage::put_object(&c, &bucket, &key, &path).await
        },
        |_, _| {},
        Reload::Detail,
    );
}

fn download_object(state: &Entity<AppState>, cx: &mut Context<RootView>, key: String) {
    let Some((conn, _, bucket)) = selected(state, cx) else {
        return;
    };
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default();
    let receiver = cx.prompt_for_new_path(&dir, Some(&key));
    let dest_key = key.clone();
    run(
        state,
        cx,
        async move {
            let dest = receiver
                .await
                .map_err(|_| anyhow::anyhow!("save prompt failed"))?
                .map_err(|e| anyhow::anyhow!("save prompt: {e}"))?
                .ok_or_else(|| anyhow::anyhow!("download cancelled"))?;
            let c = conn.build_aws_config().await?;
            manage::get_object(&c, &bucket, &dest_key, &dest).await
        },
        |_, _| {},
        Reload::None,
    );
}

fn delete_object(state: &Entity<AppState>, cx: &mut Context<RootView>, key: String) {
    let Some((conn, _, bucket)) = selected(state, cx) else {
        return;
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            manage::delete_object(&c, &bucket, &key).await
        },
        |_, _| {},
        Reload::Detail,
    );
}

fn delete_item(state: &Entity<AppState>, cx: &mut Context<RootView>, item_json: String) {
    let (conn, table, key_schema) = {
        let s = state.read(cx);
        let Some(conn) = connection_for(s) else {
            return;
        };
        let Some(table) = s.selected_resource.clone() else {
            return;
        };
        (conn, table, s.detail.key_schema.clone())
    };
    let key_json = match extract_key(&item_json, &key_schema) {
        Ok(k) => k,
        Err(e) => {
            set_error(state, cx, &format!("{e:#}"));
            return;
        }
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            manage::delete_item(&c, &table, &key_json).await
        },
        |_, _| {},
        Reload::Detail,
    );
}

fn extract_key(item_json: &str, schema: &[(String, String)]) -> anyhow::Result<String> {
    let value: serde_json::Value = serde_json::from_str(item_json)?;
    let mut key = serde_json::Map::new();
    for (attr, _) in schema {
        let v = value
            .get(attr)
            .ok_or_else(|| anyhow::anyhow!("item is missing key attribute {attr}"))?;
        key.insert(attr.clone(), v.clone());
    }
    Ok(serde_json::Value::Object(key).to_string())
}

fn receive_messages(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let (conn, url) = {
        let s = state.read(cx);
        let Some(conn) = connection_for(s) else {
            return;
        };
        let Some(res) = s.selected_resource() else {
            return;
        };
        (conn, res.detail)
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            manage::receive_messages(&c, &url).await
        },
        |s, messages| s.detail.messages = messages,
        Reload::None,
    );
}

fn delete_message(state: &Entity<AppState>, cx: &mut Context<RootView>, receipt: String) {
    let (conn, url) = {
        let s = state.read(cx);
        let Some(conn) = connection_for(s) else {
            return;
        };
        let Some(res) = s.selected_resource() else {
            return;
        };
        (conn, res.detail)
    };
    let receipt_clone = receipt.clone();
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            manage::delete_message(&c, &url, &receipt).await
        },
        move |s, _| {
            s.detail.messages.retain(|m| m.receipt != receipt_clone);
        },
        Reload::None,
    );
}

/// The line a failed call leaves behind, in the one red the palette reserves for failure.
fn error_line(message: String) -> AnyElement {
    div()
        .text_sm()
        .text_color(rgb(theme::FAIL))
        .child(format!("Error: {message}"))
        .into_any()
}

/// The amber line that says a call is still in flight.
fn busy_note() -> AnyElement {
    div()
        .text_sm()
        .text_color(rgb(theme::WARN))
        .child("Working...")
        .into_any()
}

/// A wrapping strip of buttons, each in the box [`theme::chip_box`] gives a control chip: a
/// chip asks for no height of its own, and an unbounded child of a flex row takes the whole
/// cross axis of its parent, so the strip would otherwise paint as a column of blocks.
fn button_strip(buttons: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .children(buttons.into_iter().map(theme::chip_box))
}

/// One line of what a read-only operation answered.
fn output_line(line: &str) -> AnyElement {
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(theme::SURFACE))
        .font_family(theme::MONO_FONT)
        .text_size(px(12.0))
        .text_color(rgb(theme::TEXT_PRIMARY))
        .child(line.to_string())
        .into_any()
}

/// The flush row a table draws inside a card: square corners, one hairline under it, the
/// height the design's rows sit at.
fn table_row(id: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    theme::row(id, selected)
        .h(px(40.0))
        .rounded_none()
        .border_b_1()
        .border_color(rgb(theme::BORDER))
}

/// One text cell of a table row, in the mono the design sets ids, keys and counts in.
fn mono_cell(width: Option<f32>, color: u32, text: impl Into<SharedString>) -> Div {
    theme::cell(
        width,
        div()
            .font_family(theme::MONO_FONT)
            .text_size(px(12.0))
            .text_color(rgb(color))
            .overflow_hidden()
            .child(text.into()),
    )
}

/// The page's one modal: the dialog card floats over the page behind a dimmed backdrop.
fn dialog_overlay(state: &AppState, cx: &mut Context<RootView>) -> Option<AnyElement> {
    state.dialog?;
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .bg(rgba(BACKDROP))
            .pt(px(56.0))
            .children(render_dialog(state, cx))
            .into_any(),
    )
}

pub fn render_dialog(state: &AppState, cx: &mut Context<RootView>) -> Option<AnyElement> {
    let dialog = state.dialog?;
    let operation = state.dialog_op.clone();
    let (title, confirm_label) = match dialog {
        Dialog::CreateBucket => ("New S3 bucket", "Create"),
        Dialog::CreateTable => ("New DynamoDB table", "Create"),
        Dialog::CreateQueue => ("New SQS queue", "Create"),
        Dialog::CreateNamed => ("New resource", "Create"),
        Dialog::CreateSecret => ("New secret", "Create"),
        Dialog::CreateParameter => ("New parameter", "Create"),
        Dialog::PutItem => ("Put item (plain JSON)", "Put"),
        Dialog::SendMessage => ("Send message", "Send"),
        Dialog::LaunchInstance => ("Launch instance", "Launch"),
        Dialog::Publish => ("Publish to topic", "Publish"),
        Dialog::Subscribe => ("Subscribe an endpoint", "Subscribe"),
        Dialog::Operation => (
            operation.as_ref().map(|o| o.label).unwrap_or("Operation"),
            "Run",
        ),
    };
    let mut card = theme::card().gap_2p5().w(px(560.0)).child(
        div()
            .text_size(px(15.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme::TEXT_PRIMARY))
            .child(title),
    );
    match dialog {
        Dialog::CreateBucket | Dialog::CreateQueue | Dialog::CreateNamed => {
            card = card.child(field_row("Name", state.input_name.clone()));
        }
        Dialog::CreateSecret => {
            card = card
                .child(field_row("Name", state.input_name.clone()))
                .child(field_row("Value", state.input_json.clone()));
        }
        Dialog::CreateParameter => {
            card = card
                .child(field_row("Name", state.input_name.clone()))
                .child(field_row("Value", state.input_json.clone()))
                .child(field_row("Type", state.input_protocol.clone()))
                .child(theme::muted(
                    "Type is String, StringList or SecureString; empty means String. \
                     A StringList's items are separated by commas.",
                ));
        }
        Dialog::CreateTable => {
            card = card
                .child(field_row("Table name", state.input_name.clone()))
                .child(field_row("Partition key", state.input_pk.clone()))
                .child(field_row("Sort key", state.input_sk.clone()));
        }
        Dialog::PutItem => {
            card = card
                .child(field_row("Item JSON", state.input_json.clone()))
                .child(theme::muted(
                    "Must include the table's key attributes, e.g. {\"id\": \"1\"}",
                ));
        }
        Dialog::SendMessage => {
            card = card.child(field_row("Body", state.input_body.clone()));
        }
        Dialog::LaunchInstance => {
            card = card.child(field_row(
                "Instance type",
                state.input_instance_type.clone(),
            ));
        }
        Dialog::Publish => {
            card = card.child(field_row("Message", state.input_body.clone()));
        }
        Dialog::Subscribe => {
            card = card
                .child(field_row("Protocol", state.input_protocol.clone()))
                .child(field_row("Endpoint", state.input_endpoint.clone()))
                .child(theme::muted(
                    "The emulator confirms only sqs and lambda endpoints; \
                     anything else stays pending until its owner confirms.",
                ));
        }
        Dialog::Operation => {
            if let Some(op) = operation.as_ref() {
                let acts_on = match (op.needs_selection, &state.selected_resource) {
                    (true, Some(name)) => format!("This call acts on {name}."),
                    (true, None) => "Select a row of this page first.".to_string(),
                    (false, _) => match &state.selected_resource {
                        Some(name) => format!("This call ignores the selected {name}."),
                        None => "This call is for the whole account.".to_string(),
                    },
                };
                card = card.child(theme::muted(acts_on));
                if let Some(field) = &op.name {
                    card = card.child(field_row(field.label, state.input_name.clone()));
                }
                for (index, field) in op.fields.iter().enumerate() {
                    if let Some(input) = state.input_ops.get(index) {
                        card = card.child(field_row(field.label, input.clone()));
                    }
                }
            }
        }
    }
    if let Some(err) = &state.action_error {
        card = card.child(error_line(err.clone()));
    }
    card = card.child(button_strip(vec![
        theme::button(
            "dialog-cancel",
            "Cancel",
            theme::Kind::Ghost,
            cx,
            |this, _, cx| close_dialog(&this.state, cx),
        ),
        theme::button(
            "dialog-confirm",
            confirm_label,
            theme::Kind::Primary,
            cx,
            |this, _, cx| confirm_dialog(&this.state, cx),
        ),
    ]));
    Some(card.into_any())
}

/// One entry of the detail card's action strip: its label, the button style it is painted
/// in, and the call it dispatches. The accent is the call this page wants taken, red the one
/// that destroys something, ghost the ones that only re-read.
fn b(label: &str, kind: theme::Kind, action: DetailAction) -> (String, theme::Kind, DetailAction) {
    (label.to_string(), kind, action)
}

/// The Operations card: the SDK calls this page offers beyond a resource's own create,
/// delete and children, plus whatever the last read-only call answered.
pub fn render_operations(state: &AppState, cx: &mut Context<RootView>) -> Option<AnyElement> {
    let service = state.selected_service.as_str();
    if !crate::views::catalog::is_managed(service) {
        return None;
    }
    let ops = operations::operations(service, state.selected_resource.is_some());
    if ops.is_empty() && state.detail.output.is_empty() {
        return None;
    }
    let mut card = theme::panel("OPERATIONS");
    if state.action_busy {
        card = card.child(busy_note());
    }
    if !ops.is_empty() {
        card = card.child(button_strip(
            ops.into_iter()
                .map(|op| {
                    let kind = if op.danger {
                        theme::Kind::Danger
                    } else {
                        theme::Kind::Neutral
                    };
                    theme::button(
                        format!("op-{}", op.label),
                        op.label,
                        kind,
                        cx,
                        move |this, window, cx| {
                            dispatch(&this.state, window, cx, DetailAction::RunOp(op.clone()))
                        },
                    )
                })
                .collect(),
        ));
    }
    // The open operation dialog shows action_error itself; don't print it twice.
    if let Some(err) = &state.action_error
        && state.dialog.is_none()
    {
        card = card.child(error_line(err.clone()));
    }
    if !state.detail.output.is_empty() {
        card = card.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(11.0))
                        .text_color(rgb(theme::TEXT_SECONDARY))
                        .child(format!("{} result(s)", state.detail.output.len())),
                )
                .child(theme::chip_box(theme::button(
                    "clear-output",
                    "Clear",
                    theme::Kind::Ghost,
                    cx,
                    |this, window, cx| dispatch(&this.state, window, cx, DetailAction::ClearOutput),
                ))),
        );
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(state.detail.output.iter().map(|line| output_line(line))),
        );
    }
    Some(card.into_any())
}

/// The state transitions this build can actually perform on a resource. A service with no
/// transition backed by a driver call gets no button rather than a guess.
fn lifecycle_buttons(service: &str) -> Vec<(&'static str, DetailAction)> {
    match service {
        "ec2" => vec![
            ("Start instance", DetailAction::Lifecycle("start")),
            ("Stop instance", DetailAction::Lifecycle("stop")),
        ],
        "rds" => vec![
            ("Start", DetailAction::Lifecycle("start")),
            ("Stop", DetailAction::Lifecycle("stop")),
            ("Reboot", DetailAction::Lifecycle("reboot")),
        ],
        _ => vec![],
    }
}

fn run_lifecycle(state: &Entity<AppState>, cx: &mut Context<RootView>, verb: &'static str) {
    let Some((conn, service, name, _)) = selected_row(state, cx) else {
        return;
    };
    run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            crate::views::root::run_lifecycle(&service, verb, &c, &name).await
        },
        |_, _| {},
        Reload::Inventory,
    );
}

/// One line of the dialog form: the label in its own column, the text field taking the room
/// that is left.
fn field_row(label: &str, input: Entity<crate::views::text_field::TextField>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(140.0))
                .child(theme::mono_label(label.to_uppercase())),
        )
        .child(input)
        .into_any()
}

pub fn render_detail(state: &AppState, cx: &mut Context<RootView>) -> Option<AnyElement> {
    let res = state.selected_resource()?;
    let service = state.selected_service.as_str();
    let detail = &state.detail;

    let mut card = theme::card().gap_2().child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .font_family(theme::MONO_FONT)
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::TEXT_PRIMARY))
                    .child(res.name.clone()),
            )
            .when(state.action_busy, |d| d.child(busy_note())),
    );

    let buttons: Vec<(String, theme::Kind, DetailAction)> = match service {
        "s3" => vec![
            b("Upload file", theme::Kind::Primary, DetailAction::Upload),
            b("Refresh", theme::Kind::Ghost, DetailAction::RefreshDetail),
            b(
                "Delete bucket",
                theme::Kind::Danger,
                DetailAction::DeleteResource,
            ),
        ],
        "dynamodb" => vec![
            b(
                "Put item",
                theme::Kind::Primary,
                DetailAction::PutItemDialog,
            ),
            b("Scan", theme::Kind::Ghost, DetailAction::RefreshDetail),
            b(
                "Delete table",
                theme::Kind::Danger,
                DetailAction::DeleteResource,
            ),
        ],
        "sqs" => vec![
            b(
                "Send message",
                theme::Kind::Primary,
                DetailAction::SendMessageDialog,
            ),
            b("Receive", theme::Kind::Neutral, DetailAction::Receive),
            b(
                "Delete queue",
                theme::Kind::Danger,
                DetailAction::DeleteResource,
            ),
        ],
        "iam" => vec![
            b(
                "Create access key",
                theme::Kind::Primary,
                DetailAction::CreateChild,
            ),
            b("Refresh", theme::Kind::Ghost, DetailAction::RefreshDetail),
            b(
                "Delete user",
                theme::Kind::Danger,
                DetailAction::DeleteResource,
            ),
        ],
        "sns" => vec![
            b("Publish", theme::Kind::Primary, DetailAction::PublishDialog),
            b(
                "Subscribe",
                theme::Kind::Neutral,
                DetailAction::SubscribeDialog,
            ),
            b("Refresh", theme::Kind::Ghost, DetailAction::RefreshDetail),
            b(
                "Delete topic",
                theme::Kind::Danger,
                DetailAction::DeleteResource,
            ),
        ],
        "ssm" => vec![
            b(
                "Reveal value",
                theme::Kind::Primary,
                DetailAction::RevealParameter,
            ),
            b("Refresh", theme::Kind::Ghost, DetailAction::RefreshDetail),
            b(
                "Delete parameter",
                theme::Kind::Danger,
                DetailAction::DeleteResource,
            ),
        ],
        "sts" => vec![
            b(
                "Assume role",
                theme::Kind::Primary,
                DetailAction::AssumeRole,
            ),
            b("Refresh", theme::Kind::Ghost, DetailAction::RefreshDetail),
        ],
        other => {
            let mut row = lifecycle_buttons(other)
                .into_iter()
                .map(|(label, action)| b(label, theme::Kind::Primary, action))
                .collect::<Vec<_>>();
            row.push(b(
                "Refresh",
                theme::Kind::Ghost,
                DetailAction::RefreshDetail,
            ));
            if let Some(noun) = crate::views::catalog::resource_noun(other) {
                row.push(b(
                    &format!("Delete {noun}"),
                    theme::Kind::Danger,
                    DetailAction::DeleteResource,
                ));
            }
            row
        }
    };
    card = card.child(button_strip(
        buttons
            .into_iter()
            .map(|(label, kind, action)| {
                let id = format!("detail-{label}");
                theme::button(id, label, kind, cx, move |this, window, cx| {
                    dispatch(&this.state, window, cx, action.clone())
                })
            })
            .collect(),
    ));

    // The open dialog renders action_error itself; don't duplicate it below.
    if let Some(err) = &state.action_error
        && state.dialog.is_none()
    {
        card = card.child(error_line(err.clone()));
    }
    if let Some(err) = &detail.error {
        card = card.child(error_line(err.clone()));
    }

    match service {
        "s3" => {
            if detail.loading {
                card = card.child(theme::muted("Loading objects..."));
            } else if detail.objects.is_empty() {
                card = card.child(theme::muted("No objects"));
            } else {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(theme::table_head(OBJECT_COLUMNS))
                        .children(detail.objects.iter().map(|o| {
                            let actions = button_strip(vec![
                                row_button(
                                    format!("dl-{}", o.key),
                                    "Download",
                                    theme::Kind::Neutral,
                                    DetailAction::DownloadObject(o.key.clone()),
                                    cx,
                                ),
                                row_button(
                                    format!("del-{}", o.key),
                                    "Delete",
                                    theme::Kind::Danger,
                                    DetailAction::DeleteObject(o.key.clone()),
                                    cx,
                                ),
                            ]);
                            table_row(format!("object-{}", o.key), false)
                                .child(mono_cell(
                                    OBJECT_COLUMNS[0].0,
                                    theme::TEXT_PRIMARY,
                                    o.key.clone(),
                                ))
                                .child(mono_cell(
                                    OBJECT_COLUMNS[1].0,
                                    theme::TEXT_SECONDARY,
                                    o.size.to_string(),
                                ))
                                .child(mono_cell(
                                    OBJECT_COLUMNS[2].0,
                                    theme::TEXT_SECONDARY,
                                    o.modified.clone(),
                                ))
                                .child(theme::cell(OBJECT_COLUMNS[3].0, actions))
                                .into_any()
                        })),
                );
            }
        }
        "dynamodb" => {
            if !detail.key_schema.is_empty() {
                let schema = detail
                    .key_schema
                    .iter()
                    .map(|(a, t)| format!("{a} ({t})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(theme::mono_label("KEY SCHEMA"))
                        .child(
                            div()
                                .font_family(theme::MONO_FONT)
                                .text_size(px(12.0))
                                .text_color(rgb(theme::TEXT_PRIMARY))
                                .child(schema),
                        ),
                );
            }
            if detail.loading {
                card = card.child(theme::muted("Scanning items..."));
            } else if detail.items.is_empty() {
                card = card.child(theme::muted("No items"));
            } else {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(theme::table_head(ITEM_COLUMNS))
                        .children(detail.items.iter().map(|item| {
                            table_row(format!("item-{item}"), false)
                                .child(mono_cell(
                                    ITEM_COLUMNS[0].0,
                                    theme::TEXT_PRIMARY,
                                    item.clone(),
                                ))
                                .child(theme::cell(
                                    ITEM_COLUMNS[1].0,
                                    button_strip(vec![row_button(
                                        format!("del-item-{item}"),
                                        "Delete",
                                        theme::Kind::Danger,
                                        DetailAction::DeleteItem(item.clone()),
                                        cx,
                                    )]),
                                ))
                                .into_any()
                        })),
                );
            }
        }
        "sqs" => {
            card = card.child(theme::muted(
                "Receive pulls up to 10 messages; delete removes them permanently.",
            ));
            if detail.messages.is_empty() {
                card = card.child(theme::muted("No messages received yet"));
            } else {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(theme::table_head(MESSAGE_COLUMNS))
                        .children(detail.messages.iter().map(|m| {
                            table_row(format!("message-{}", m.id), false)
                                .child(mono_cell(
                                    MESSAGE_COLUMNS[0].0,
                                    theme::TEXT_PRIMARY,
                                    m.body.clone(),
                                ))
                                .child(mono_cell(
                                    MESSAGE_COLUMNS[1].0,
                                    theme::TEXT_SECONDARY,
                                    m.id.clone(),
                                ))
                                .child(theme::cell(
                                    MESSAGE_COLUMNS[2].0,
                                    button_strip(vec![row_button(
                                        format!("del-msg-{}", m.id),
                                        "Delete",
                                        theme::Kind::Danger,
                                        DetailAction::DeleteMessage(m.receipt.clone()),
                                        cx,
                                    )]),
                                ))
                                .into_any()
                        })),
                );
            }
        }
        "iam" | "sns" | "logs" | "kinesis" | "secretsmanager" | "ssm" => {
            if detail.loading {
                card = card.child(theme::muted("Loading..."));
            } else if detail.children.is_empty() {
                let none = match service {
                    "iam" => "No access keys for this user",
                    "sns" => "No subscriptions on this topic",
                    "logs" => "No log streams in this group",
                    "kinesis" => "No shards reported for this stream",
                    "ssm" => "No versions recorded for this parameter",
                    _ => "No versions stored for this secret",
                };
                card = card.child(theme::muted(none));
            } else {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(theme::table_head(CHILD_COLUMNS))
                        .children(detail.children.iter().map(|c| {
                            // A shard and a secret version have no delete call of their own;
                            // offering one would be a button that cannot do anything.
                            let delete = child_is_deletable(service).then(|| {
                                row_button(
                                    format!("del-child-{}", c.name),
                                    "Delete",
                                    theme::Kind::Danger,
                                    DetailAction::DeleteChild {
                                        name: c.name.clone(),
                                        detail: c.detail.clone(),
                                    },
                                    cx,
                                )
                            });
                            table_row(format!("child-{}", c.name), false)
                                .child(mono_cell(
                                    CHILD_COLUMNS[0].0,
                                    theme::TEXT_PRIMARY,
                                    c.name.clone(),
                                ))
                                .child(mono_cell(
                                    CHILD_COLUMNS[1].0,
                                    theme::TEXT_SECONDARY,
                                    c.detail.clone(),
                                ))
                                .child(theme::cell(
                                    CHILD_COLUMNS[2].0,
                                    button_strip(delete.into_iter().collect()),
                                ))
                                .into_any()
                        })),
                );
            }
        }
        "sts" => {
            // The mints are what this dashboard made this run, held in state — the
            // wire has no list of sessions to fetch. Only the selected role's.
            let mints: Vec<_> = state
                .minted_sessions
                .iter()
                .filter(|m| m.role_name == res.name)
                .collect();
            if mints.is_empty() {
                card = card.child(theme::muted(
                    "No sessions assumed from this role in this run",
                ));
            } else {
                card = card.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(theme::table_head(CHILD_COLUMNS))
                        .children(mints.iter().map(|m| {
                            table_row(format!("mint-{}", m.access_key_id), false)
                                .child(mono_cell(
                                    CHILD_COLUMNS[0].0,
                                    theme::TEXT_PRIMARY,
                                    m.session_name.clone(),
                                ))
                                .child(mono_cell(
                                    CHILD_COLUMNS[1].0,
                                    theme::TEXT_SECONDARY,
                                    format!("{} · expires {}", m.identity_arn, m.expires),
                                ))
                                .into_any()
                        })),
                );
            }
        }
        _ => {}
    }
    if let Some(text) = &detail.revealed {
        card = card.child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(theme::BORDER))
                .bg(rgba(theme::tint(theme::WARN)))
                .child(theme::dot(theme::WARN))
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(rgb(theme::TEXT_PRIMARY))
                        .child(text.clone()),
                )
                .child(theme::chip_box(theme::button(
                    "dismiss-reveal",
                    "Dismiss",
                    theme::Kind::Ghost,
                    cx,
                    |this, window, cx| {
                        dispatch(&this.state, window, cx, DetailAction::DismissReveal)
                    },
                ))),
        );
    }
    Some(card.into_any())
}

/// A button at the end of a data row. `id` is the string a test or a binding grabs, so it
/// stays exactly what the row's own call was named before the restyle.
fn row_button(
    id: String,
    label: &'static str,
    kind: theme::Kind,
    action: DetailAction,
    cx: &mut Context<RootView>,
) -> AnyElement {
    theme::button(id, label, kind, cx, move |this, window, cx| {
        dispatch(&this.state, window, cx, action.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::extract_key;

    #[test]
    fn extract_key_builds_composite_key() {
        let schema = vec![
            ("user_id".to_string(), "HASH".to_string()),
            ("created_at".to_string(), "RANGE".to_string()),
        ];
        let item = r#"{"user_id":"u1","created_at":"t2","n":2}"#;
        let key = extract_key(item, &schema).unwrap();
        let v: serde_json::Value = serde_json::from_str(&key).unwrap();
        assert_eq!(v["user_id"], "u1");
        assert_eq!(v["created_at"], "t2");
        assert!(v.get("n").is_none());
    }

    #[test]
    fn extract_key_rejects_missing_attribute() {
        let schema = vec![("id".to_string(), "HASH".to_string())];
        assert!(extract_key(r#"{"other":"x"}"#, &schema).is_err());
    }
}
