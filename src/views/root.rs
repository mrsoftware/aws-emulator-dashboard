use crate::app_state::{AppState, ConnectionState, ServiceResources, connection_for};
use crate::aws::client::{EmulatorConnection, REGION, runtime};
use crate::aws::drivers;
use crate::aws::services;
use crate::catalog::Sort;
use crate::views::catalog::{self, MANAGED};
use crate::views::control_panel;
use crate::views::detail;
use crate::views::theme;
use crate::views::traffic_log;
use anyhow::{Result, bail};
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, AsyncApp, Context, Entity, FontWeight, ScrollHandle, SharedString, TestSupportExt,
    Window, actions, div, prelude::*, px, rgb,
};
use std::time::Duration;

actions!(dashboard, [RefreshAll]);

pub async fn fetch_service(
    key: &str,
    config: &aws_config::SdkConfig,
) -> Result<Vec<crate::app_state::Resource>> {
    match key {
        "s3" => services::fetch_s3(config).await,
        "dynamodb" => services::fetch_dynamodb(config).await,
        "sqs" => services::fetch_sqs(config).await,
        "ec2" => drivers::ec2::fetch(config).await,
        "iam" => drivers::iam::fetch(config).await,
        "sns" => drivers::sns::fetch(config).await,
        "rds" => drivers::rds::fetch(config).await,
        "logs" => drivers::cloudwatch_logs::fetch(config).await,
        "kinesis" => drivers::kinesis::fetch(config).await,
        "secretsmanager" => drivers::secrets_manager::fetch(config).await,
        "monitoring" => drivers::cloudwatch::fetch(config).await,
        "sts" => drivers::sts::fetch(config).await,
        "ssm" => drivers::ssm::fetch(config).await,
        other => bail!("no driver is wired for {other}"),
    }
}

/// The create behind a page that needs nothing but a name. `s3`, `dynamodb`, `sqs` and `ec2`
/// have their own dialogs because they collect more than one field.
pub async fn create_resource(key: &str, config: &aws_config::SdkConfig, name: &str) -> Result<()> {
    if fetch_service(key, config)
        .await?
        .iter()
        .any(|r| r.name == name)
    {
        bail!("{name} already exists");
    }
    match key {
        "iam" => drivers::iam::create_user(config, name).await,
        "sns" => {
            drivers::sns::create_topic(config, name).await?;
            Ok(())
        }
        "rds" => {
            drivers::rds::create_instance(config, name).await?;
            Ok(())
        }
        "logs" => drivers::cloudwatch_logs::create_log_group(config, name).await,
        // The name-only dialog has no shard field, so a stream is created at the smallest
        // size the API accepts; splitting it later is a shard operation, not this page's.
        "kinesis" => drivers::kinesis::create_stream(config, name, 1).await,
        other => bail!("no create is wired for {other}"),
    }
}

/// The delete behind a page. `detail` is the ARN or URL the listing kept for the row, which
/// is what ARN-addressed services want and the name alone is not enough to recover.
pub async fn delete_resource(
    key: &str,
    config: &aws_config::SdkConfig,
    name: &str,
    detail: &str,
) -> Result<()> {
    match key {
        "ec2" => drivers::ec2::terminate_instance(config, name).await,
        "iam" => drivers::iam::delete_user(config, name).await,
        "sns" => drivers::sns::delete_topic(config, detail).await,
        "rds" => drivers::rds::delete_instance(config, name).await,
        "logs" => drivers::cloudwatch_logs::delete_log_group(config, name).await,
        "kinesis" => drivers::kinesis::delete_stream(config, name).await,
        "secretsmanager" => drivers::secrets_manager::delete_secret(config, name).await,
        "monitoring" => drivers::cloudwatch::delete_alarms(config, &[name.to_string()]).await,
        "ssm" => drivers::ssm::delete(config, name).await,
        other => bail!("no delete is wired for {other}"),
    }
}

/// Child rows of a selected resource: an IAM user's access keys, a topic's subscriptions.
pub async fn fetch_children(
    key: &str,
    config: &aws_config::SdkConfig,
    name: &str,
    detail: &str,
) -> Result<Vec<crate::app_state::Resource>> {
    use crate::app_state::Resource;
    match key {
        "iam" => drivers::iam::list_access_keys(config, name).await,
        "sns" => drivers::sns::list_subscriptions_by_topic(config, detail).await,
        "logs" => Ok(drivers::cloudwatch_logs::list_streams(config, name)
            .await?
            .into_iter()
            .map(|s| Resource {
                name: s.name,
                detail: format!("{} events · last {}", s.first_event_time, s.last_event_time),
            })
            .collect()),
        "kinesis" => Ok(drivers::kinesis::list_shards(config, name)
            .await?
            .into_iter()
            .map(|s| Resource {
                name: s.id,
                detail: if s.parent_id.is_empty() {
                    format!("{} → {}", s.begin_hash_key, s.end_hash_key)
                } else {
                    format!(
                        "child of {} · closed at {}",
                        s.parent_id, s.ending_sequence_number
                    )
                },
            })
            .collect()),
        "secretsmanager" => Ok(drivers::secrets_manager::list_version_ids(config, name)
            .await?
            .into_iter()
            .map(|v| Resource {
                name: v,
                detail: String::new(),
            })
            .collect()),
        "ssm" => drivers::ssm::history(config, name).await,
        _ => Ok(Vec::new()),
    }
}

/// A state transition the emulator performs for a named resource. Every verb a page offers
/// has to land here; an unsupported one is an error rather than a silent success.
pub async fn run_lifecycle(
    key: &str,
    verb: &str,
    config: &aws_config::SdkConfig,
    name: &str,
) -> Result<()> {
    match (key, verb) {
        ("ec2", "start") => drivers::ec2::start_instance(config, name).await,
        ("ec2", "stop") => drivers::ec2::stop_instance(config, name).await,
        ("ec2", "terminate") => drivers::ec2::terminate_instance(config, name).await,
        ("rds", "start") => drivers::rds::start_instance(config, name).await,
        ("rds", "stop") => drivers::rds::stop_instance(config, name).await,
        ("rds", "reboot") => drivers::rds::reboot_instance(config, name).await,
        (other, verb) => bail!("{other} does not support {verb} in this build"),
    }
}

pub struct RootView {
    pub state: Entity<AppState>,
    /// The page body and the nav are separate scroll containers, each bounded by the space
    /// the other leaves. A headless test reads `max_offset` off `page_scroll` to prove the
    /// body actually overflows.
    pub page_scroll: ScrollHandle,
    pub nav_scroll: ScrollHandle,
}

impl RootView {
    pub fn new(state: Entity<AppState>) -> Self {
        Self {
            state,
            page_scroll: ScrollHandle::new(),
            nav_scroll: ScrollHandle::new(),
        }
    }

    pub fn refresh(state: &Entity<AppState>, cx: &mut Context<Self>) {
        state.update(cx, |state, cx| {
            state.connection = ConnectionState::Connecting;
            cx.notify();
        });
        let state_clone = state.clone();
        cx.spawn(async move |_, cx| {
            let port: u16 = cx.update(|cx| state_clone.read(cx).settings.default_port);
            let conn = EmulatorConnection::from_port(port);
            let endpoint = conn.endpoint.clone();
            let ca_path = conn.ca_path.clone();
            let result = runtime()
                .spawn(async move { conn.health().await })
                .await
                .map_err(|e| anyhow::anyhow!("health task failed: {e}"))
                .and_then(|r| r);
            match result {
                Ok(_body) => {
                    tracing::info!(%endpoint, "emulator health check ok");
                    cx.update(|cx| {
                        state_clone.update(cx, |state, cx| {
                            state.connection =
                                ConnectionState::Connected(crate::app_state::ConnectionInfo {
                                    endpoint,
                                    ca_path,
                                });
                            cx.notify();
                        })
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "emulator health check failed");
                    cx.update(|cx| {
                        state_clone.update(cx, |state, cx| {
                            state.connection = ConnectionState::Error(format!("{e:#}"));
                            cx.notify();
                        })
                    });
                }
            }
        })
        .detach();
    }

    pub fn refresh_resources(state: &Entity<AppState>, cx: &mut Context<Self>) {
        state.update(cx, |state, cx| {
            for (key, _) in MANAGED {
                state.resources.entry(key.to_string()).or_default().loading = true;
            }
            cx.notify();
        });
        let state_clone = state.clone();
        cx.spawn(async move |_, cx| {
            fetch_inventory(&state_clone, cx).await;
        })
        .detach();
    }

    pub fn start_polling(state: &Entity<AppState>, cx: &mut Context<Self>) {
        let interval = state.read(cx).settings.poll_interval_ms;
        let proxy = state.read(cx).proxy.clone();
        let state_clone = state.clone();
        cx.spawn(async move |_, cx| {
            loop {
                fetch_inventory(&state_clone, cx).await;
                let entries = proxy.log.drain().await;
                if !entries.is_empty() {
                    tracing::info!(count = entries.len(), "traffic entries captured");
                    cx.update(|cx| {
                        state_clone.update(cx, |s, cx| {
                            for e in entries {
                                s.traffic.push_front(e);
                            }
                            while s.traffic.len() > crate::emulator::proxy::MAX_ENTRIES {
                                s.traffic.pop_back();
                            }
                            cx.notify();
                        })
                    });
                }
                cx.background_executor()
                    .timer(Duration::from_millis(interval))
                    .await;
            }
        })
        .detach();
    }
}

pub async fn fetch_inventory(state: &Entity<AppState>, cx: &mut AsyncApp) {
    let conn = cx.update(|cx| connection_for(state.read(cx)));
    let Some(conn) = conn else {
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                for (key, _) in MANAGED {
                    if let Some(r) = state.resources.get_mut(key) {
                        r.loading = false;
                    }
                }
                cx.notify();
            })
        });
        return;
    };

    let result = runtime()
        .spawn(async move {
            let config = conn.build_aws_config().await?;
            let mut fetched = Vec::with_capacity(MANAGED.len());
            for (key, _) in MANAGED {
                fetched.push((key, fetch_service(key, &config).await));
            }
            // The Instances page needs fields a (name, detail) pair cannot carry.
            let instances = drivers::ec2::list_instances(&config).await;
            anyhow::Ok((fetched, instances))
        })
        .await
        .map_err(|e| anyhow::anyhow!("inventory task failed: {e}"))
        .and_then(|r| r);

    cx.update(|cx| {
        state.update(cx, |state, cx| {
            match result {
                Ok((fetched, instances)) => {
                    match instances {
                        Ok(items) => {
                            state.instances = items;
                            state.instances_error = None;
                        }
                        Err(e) => state.instances_error = Some(format!("{e:#}")),
                    }
                    for (key, res) in fetched {
                        let entry = match res {
                            Ok(items) => ServiceResources {
                                loading: false,
                                error: None,
                                items,
                            },
                            Err(e) => ServiceResources {
                                loading: false,
                                error: Some(format!("{e:#}")),
                                items: Vec::new(),
                            },
                        };
                        tracing::debug!(service = key, error = entry.error.is_some(), "fetched");
                        state.resources.insert(key.to_string(), entry);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "inventory refresh failed");
                    state.instances_error = Some(format!("{e:#}"));
                    for (key, _) in MANAGED {
                        state.resources.insert(
                            key.to_string(),
                            ServiceResources {
                                loading: false,
                                error: Some(format!("{e:#}")),
                                items: Vec::new(),
                            },
                        );
                    }
                }
            }
            cx.notify();
        })
    });
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx).clone();

        div()
            .id("root")
            .on_action(cx.listener(|this, _: &RefreshAll, _, cx| {
                Self::refresh(&this.state, cx);
                Self::refresh_resources(&this.state, cx);
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme::PAGE))
            .text_color(rgb(theme::TEXT))
            .text_size(px(13.0))
            .font_family(theme::UI_FONT)
            .child(topbar(&state))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .flex_row()
                    .child(sidebar(&state, &self.nav_scroll, cx))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .flex_col()
                            // The body is bounded by what the header leaves, not by the window.
                            // Measured with `.size_full()` here it took the full height, so nothing
                            // ever overflowed and no page could scroll.
                            .overflow_hidden()
                            .child(header(&state, cx))
                            .child(
                                div()
                                    .id("page-scroll")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.page_scroll)
                                    .p_5()
                                    .child(page_body(&state, cx)),
                            ),
                    ),
            )
    }
}

/// The topbar strip the design draws across the whole console: the brand mark, and on the
/// right the region the SDK is pointed at. The design's global search, bell and avatar
/// wait for a working source — an empty search box or an empty bell would be a lie.
fn topbar(state: &AppState) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .h(px(56.0))
        .px_4()
        .bg(rgb(theme::SIDEBAR))
        .border_b_1()
        .border_color(rgb(theme::BORDER))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .child(
                    div()
                        .size(px(28.0))
                        .rounded_lg()
                        .bg(rgb(theme::ACCENT))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(theme::ON_ACCENT))
                                .child("S"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Stratus Console"),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(theme::BORDER))
                .px_2p5()
                .py_1p5()
                .child(theme::icon(
                    gpui_kit::assets::IconName::Globe,
                    14.0,
                    theme::INFO,
                ))
                .child(
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(12.0))
                        .child(REGION.to_string()),
                ),
        )
        .child(
            div()
                .ml_auto()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(11.0))
                .font_family(theme::MONO_FONT)
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(format!(
                    "{} services · account 0000-0000-0000",
                    crate::catalog::counts().0
                )),
        )
        .when(
            matches!(state.connection, ConnectionState::Connected(_)),
            |d| d.child(theme::dot(theme::OK)),
        )
        .into_any()
}

/// The page for the route in `selected_service`, rendered inside the body's scroll container.
fn page_body(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let selected = state.selected_service.as_str();
    match selected {
        "control" => control_panel::render(state, cx),
        "traffic" => traffic_log::render(state, cx),
        "overview" => crate::views::overview::render(state, cx),
        "instances" => crate::views::instances::render(state, cx),
        "catalog" => catalog::render(state, cx),
        other if catalog::is_managed(other) => detail::render_page(state, cx),
        other => catalog::render_focus(state)
            .unwrap_or_else(|| div().child(format!("Unknown page: {other}")).into_any()),
    }
}

/// The oversized ghost title the mockups put above every page, with the line that says what
/// the number in it means.
fn chrome(state: &AppState) -> (String, String) {
    match state.selected_service.as_str() {
        "overview" => (
            "Overview".to_string(),
            format!("Emulated AWS control plane · region {REGION}"),
        ),
        "instances" => (
            "Instances".to_string(),
            format!(
                "{} known to DescribeInstances · EC2 · region {REGION}",
                state.instances.len()
            ),
        ),
        "catalog" => {
            let (total, _, _) = crate::catalog::counts();
            let groups = crate::catalog::browse("", Sort::Name).len();
            (
                "Services".to_string(),
                format!("{total} services across {groups} groups · emulator region {REGION}"),
            )
        }
        "control" => (
            "Emulator control".to_string(),
            "start, stop and health of the local emulator".to_string(),
        ),
        "traffic" => (
            "Traffic".to_string(),
            format!(
                "{} requests captured · proxy {}",
                state.traffic.len(),
                if state.proxy_running {
                    "running".to_string()
                } else {
                    "stopped".to_string()
                }
            ),
        ),
        other if catalog::is_managed(other) => (
            MANAGED
                .iter()
                .find(|(k, _)| *k == other)
                .map(|(_, l)| l.to_string())
                .unwrap_or_else(|| other.to_string()),
            format!(
                "{} resources · region {REGION}",
                state.service(other).items.len()
            ),
        ),
        other => match crate::catalog::find(other) {
            Some(service) => (
                service.title.clone(),
                format!(
                    "{} · {} operations · {}",
                    service.signing_name,
                    service.ops,
                    if service.registered {
                        "served by the build, no page yet"
                    } else {
                        "not served by the running build"
                    }
                ),
            ),
            None => ("Unknown page".to_string(), other.to_string()),
        },
    }
}

fn header(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let (title, subtitle) = chrome(state);
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px_5()
        .py_4()
        .border_b_1()
        .border_color(rgb(theme::RAISED))
        .child(theme::page_header(title, subtitle))
        .child(
            div()
                .ml_auto()
                .flex()
                .items_center()
                .gap_2()
                .child(theme::button(
                    "refresh",
                    "Refresh",
                    theme::Kind::Ghost,
                    cx,
                    |this, _, cx| {
                        RootView::refresh(&this.state, cx);
                        RootView::refresh_resources(&this.state, cx);
                    },
                ))
                .children(primary_action(state, cx)),
        )
        .into_any()
}

/// The one orange action the mockups put at the top right, when this build can back it. A
/// page with nothing to create — an unmanaged service, or the read-only alarms page — has no
/// button rather than one that lies.
fn primary_action(state: &AppState, cx: &mut Context<RootView>) -> Option<AnyElement> {
    let key = state.selected_service.as_str();
    if key == "instances" {
        return Some(crate::views::instances::launch_action(cx));
    }
    let (label, dialog) = catalog::create_action(key)?;
    Some(theme::button(
        SharedString::from(format!("create-{key}")),
        format!("+ {label}"),
        theme::Kind::Primary,
        cx,
        move |this, window, cx| detail::open_dialog(&this.state, window, cx, dialog),
    ))
}

/// The routes the console mockup leads the sidebar with, with their lucide marks.
/// `lambda` is not served by this build; it is here because the mockup shows it, and its
/// page says so rather than pretending.
const FAVORITES: [(&str, &str, IconName); 5] = [
    ("overview", "Dashboard", IconName::LayoutDashboard),
    ("instances", "Instances", IconName::Server),
    ("s3", "S3 Buckets", IconName::HardDrive),
    ("lambda", "Lambda", IconName::Zap),
    ("rds", "RDS", IconName::Database),
];

fn sidebar(state: &AppState, nav_scroll: &ScrollHandle, cx: &mut Context<RootView>) -> AnyElement {
    let selected = state.selected_service.as_str();
    let groups = crate::catalog::browse("", Sort::Name);
    div()
        .w(px(240.0))
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme::SIDEBAR))
        .border_r_1()
        .border_color(rgb(theme::BORDER))
        .child(
            div()
                .id("nav-scroll")
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(nav_scroll)
                .flex()
                .flex_col()
                .gap_0p5()
                .pt_3()
                .px_2p5()
                .pb_4()
                .child(
                    div()
                        .px_2p5()
                        .pb_1p5()
                        .child(theme::section_label("FAVORITES")),
                )
                .children(FAVORITES.iter().map(|(key, label, icon)| {
                    nav(key, label, *icon, nav_count(state, key), selected, cx)
                }))
                .child(
                    div()
                        .px_2p5()
                        .pt_3()
                        .pb_1p5()
                        .child(theme::section_label("EMULATOR")),
                )
                .child(nav(
                    "control",
                    "Emulator control",
                    IconName::Settings,
                    None,
                    selected,
                    cx,
                ))
                .child(nav(
                    "traffic",
                    "Traffic",
                    IconName::Activity,
                    Some(state.traffic.len()),
                    selected,
                    cx,
                ))
                .child(
                    div()
                        .px_2p5()
                        .pt_3()
                        .pb_1p5()
                        .child(theme::section_label("ALL SERVICES")),
                )
                .children(
                    groups
                        .iter()
                        .flat_map(|group| group_rows(group, state, selected, cx)),
                ),
        )
        .child(footer(state))
        .into_any()
}

/// The number a nav row carries: running instances for the Instances route, listed resources
/// for a managed service, nothing for a page that has no count.
fn nav_count(state: &AppState, key: &str) -> Option<usize> {
    match key {
        "instances" => Some(
            state
                .instances
                .iter()
                .filter(|i| i.state == "running")
                .count(),
        ),
        other if catalog::is_managed(other) => Some(state.service(other).items.len()),
        _ => None,
    }
}

fn nav(
    key: &str,
    label: &str,
    icon: IconName,
    count: Option<usize>,
    selected: &str,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let key = key.to_string();
    theme::nav_row(
        key.clone(),
        theme::icon(
            icon,
            16.0,
            if selected == key.as_str() {
                theme::ACCENT
            } else {
                theme::FAINT
            },
        ),
        label.to_string(),
        count,
        selected == key.as_str(),
        cx,
        move |s, cx| s.open_page(&key, cx),
    )
}

fn group_rows(
    group: &crate::catalog::Group,
    state: &AppState,
    selected: &str,
    cx: &mut Context<RootView>,
) -> Vec<AnyElement> {
    let open = state.open_groups.contains(&group.name);
    let manageable: Vec<&str> = group
        .items
        .iter()
        .map(|s| s.signing_name.as_str())
        .filter(|key| catalog::is_managed(key))
        .collect();
    let mut rows = vec![group_header(
        &group.name,
        group.total,
        !manageable.is_empty(),
        open,
        cx,
    )];
    if open {
        rows.extend(manageable.iter().map(|key| {
            let label = MANAGED
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, l)| l.to_string())
                .unwrap_or_else(|| {
                    crate::catalog::find(key)
                        .map(|s| s.title.clone())
                        .unwrap_or_else(|| key.to_string())
                });
            nav(
                key,
                &label,
                theme::group_icon(&group.name),
                Some(state.service(key).items.len()),
                selected,
                cx,
            )
        }));
        if group.total > manageable.len() {
            let rest = group.total - manageable.len();
            rows.push(
                div()
                    .id(SharedString::from(format!("group-more-{}", group.name)))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .child(theme::muted(format!("+{rest} more in the catalog")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.update(cx, |s, cx| s.open_page("catalog", cx));
                    }))
                    .into_any(),
            );
        }
    }
    rows
}

fn group_header(
    name: &str,
    total: usize,
    manageable: bool,
    open: bool,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let name_owned = name.to_string();
    div()
        .id(SharedString::from(format!("group-{name_owned}")))
        .test_support()
        .flex()
        .items_center()
        .gap_2()
        .px_2p5()
        .py_1p5()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if open { theme::RAISED } else { theme::SIDEBAR }))
        .child(theme::icon(
            if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
            12.0,
            theme::FAINT,
        ))
        .child(theme::icon(theme::group_icon(name), 15.0, theme::FAINT))
        .child(div().flex_1().child(name_owned.clone()))
        .when(manageable, |d| d.child(theme::dot(theme::OK)))
        .child(
            div()
                .font_family(theme::MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(theme::FAINT))
                .child(total.to_string()),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.state
                .update(cx, |s, cx| s.toggle_group(&name_owned, cx));
        }))
        .into_any()
}

fn footer(state: &AppState) -> AnyElement {
    let (color, text) = match &state.connection {
        ConnectionState::Disconnected => (theme::FAINT, "not checked".to_string()),
        ConnectionState::Connecting => (theme::WARN, "connecting…".to_string()),
        ConnectionState::Connected(info) => (theme::OK, info.endpoint.clone()),
        ConnectionState::Error(err) => (theme::FAIL, format!("unreachable: {}", short(err))),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(rgb(theme::RAISED))
        .child(theme::dot(color))
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme::MUTED))
                .child(text)
                .overflow_hidden(),
        )
        .into_any()
}

/// A connection error can be a whole request chain; the sidebar has room for its first line.
fn short(err: &str) -> String {
    err.lines()
        .next()
        .map(|l| l.chars().take(48).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::Resource;
    use crate::aws::manage;
    use anyhow::Context as _;
    use anyhow::ensure;
    use aws_config::SdkConfig;

    async fn config() -> aws_config::SdkConfig {
        EmulatorConnection::from_port(9297)
            .build_aws_config()
            .await
            .expect("aws config")
    }

    /// A name no other run can collide with. IAM passes `_` because an IAM user name allows
    /// no hyphen; every other page accepts `-`.
    fn unique(tag: &str, sep: char) -> String {
        let nanos = std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos();
        format!("dash{sep}wiring{sep}{tag}{sep}{nanos}")
    }

    fn err<E: std::fmt::Display>(ctx: &str, source: E) -> anyhow::Error {
        anyhow::anyhow!("{ctx}: {source}")
    }

    /// The row a page lists for `name`, through the same table the sidebar and the
    /// inventory poll use. A create that never appears in its own listing is the wiring
    /// failure this test exists to catch.
    async fn row(config: &SdkConfig, key: &str, name: &str) -> Result<Resource> {
        let resources = fetch_service(key, config)
            .await
            .map_err(|e| err(&format!("{key} page: listing"), e))?;
        resources
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| anyhow::anyhow!("{key} page: {name} was not listed after create"))
    }

    /// Polls, because a delete can be accepted and the store finish the job a moment later.
    async fn absent(config: &SdkConfig, key: &str, name: &str) -> Result<()> {
        for _ in 0..40 {
            let listed = fetch_service(key, config)
                .await
                .map_err(|e| err(&format!("{key} page: listing"), e))?;
            if !listed.iter().any(|r| r.name == name) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(anyhow::anyhow!(
            "{key} page: {name} was still listed after delete",
        ))
    }

    /// The child rows the detail panel would show, via the panel's own table.
    async fn children(
        config: &SdkConfig,
        key: &str,
        name: &str,
        detail: &str,
    ) -> Result<Vec<String>> {
        let kids = fetch_children(key, config, name, detail)
            .await
            .map_err(|e| err(&format!("{key} page: children of {name}"), e))?;
        Ok(kids.into_iter().map(|r| r.name).collect())
    }

    fn record(failures: &mut Vec<String>, label: &str, outcome: Result<()>) {
        if let Err(e) = outcome {
            failures.push(format!("{label}: {e:#}"));
        }
    }

    /// A verb the tables refuse on purpose has to refuse with a message, never quietly.
    fn refused(outcome: Result<()>, needle: &str) -> Result<()> {
        match outcome {
            Ok(()) => Err(anyhow::anyhow!(
                "expected an error mentioning {needle:?}, got a success"
            )),
            Err(e) if format!("{e:#}").contains(needle) => Ok(()),
            Err(e) => Err(anyhow::anyhow!(
                "expected an error mentioning {needle:?}, got {e:#}"
            )),
        }
    }

    /// A call that had to fail. The message says why, not what the emulator answered — the
    /// answer is not what this asserts.
    fn expect_error(outcome: Result<()>) -> Result<()> {
        match outcome {
            Ok(()) => Err(anyhow::anyhow!("succeeded, and nothing here should have")),
            Err(_) => Ok(()),
        }
    }

    macro_rules! page {
        ($failures:expr, $label:expr, $body:expr, $cleanup:expr) => {{
            let outcome: Result<()> = $body.await;
            record($failures, &format!("{}: round trip", $label), outcome);
            let cleanup: Result<()> = $cleanup.await;
            record($failures, &format!("{}: delete", $label), cleanup);
        }};
    }

    /// Every Tier A page: create the resource the way the page's button does, prove the
    /// listing picked it up, walk its children, then delete it the way the page's delete
    /// button does and prove it left. This is the stand-in for clicking through the running
    /// window — it exercises `fetch_service`, `create_resource`, `fetch_children` and
    /// `delete_resource` with nothing but the emulator between them and the SDK.
    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297; creates and deletes one resource per Tier A page"]
    async fn live_tier_a_wiring_round_trip() {
        let config = config().await;
        let mut failures = Vec::new();

        {
            let name = unique("s3", '-');
            let _ = manage::delete_bucket(&config, &name).await;
            page!(
                &mut failures,
                "s3",
                async {
                    manage::create_bucket(&config, &name).await?;
                    row(&config, "s3", &name).await?;
                    Ok(())
                },
                async {
                    manage::delete_bucket(&config, &name).await?;
                    absent(&config, "s3", &name).await
                }
            );
        }

        {
            let name = unique("dynamodb", '-');
            let _ = manage::delete_table(&config, &name).await;
            page!(
                &mut failures,
                "dynamodb",
                async {
                    manage::create_table(&config, &name, "pk", Some("sk")).await?;
                    row(&config, "dynamodb", &name).await?;
                    Ok(())
                },
                async {
                    manage::delete_table(&config, &name).await?;
                    absent(&config, "dynamodb", &name).await
                }
            );
        }

        {
            let name = unique("sqs", '-');
            let mut url = String::new();
            page!(
                &mut failures,
                "sqs",
                async {
                    manage::create_queue(&config, &name).await?;
                    url = row(&config, "sqs", &name).await?.detail;
                    ensure!(
                        url.starts_with("http"),
                        "sqs page: {name} was listed with no queue URL ({url})"
                    );
                    Ok(())
                },
                async {
                    manage::delete_queue(&config, &url).await?;
                    absent(&config, "sqs", &name).await
                }
            );
        }

        {
            let mut id = String::new();
            page!(
                &mut failures,
                "ec2",
                async {
                    id = drivers::ec2::create_instance(&config, "t3.micro").await?;
                    row(&config, "ec2", &id).await?;
                    drivers::ec2::wait_state(&config, &id, "running", 30).await?;
                    // The state tabs and the row menu both go through `run_lifecycle`.
                    run_lifecycle("ec2", "stop", &config, &id).await?;
                    drivers::ec2::wait_state(&config, &id, "stopped", 30).await?;
                    run_lifecycle("ec2", "start", &config, &id).await?;
                    drivers::ec2::wait_state(&config, &id, "running", 30).await?;
                    Ok(())
                },
                async {
                    delete_resource("ec2", &config, &id, "").await?;
                    // Measured: the emulator leaves the instance listed and walks it through
                    // `shutting-down` before it reaches `terminated`, so the row the page
                    // terminated stays visible for a moment in its new state.
                    match drivers::ec2::wait_state(&config, &id, "terminated", 30).await {
                        Ok(()) => Ok(()),
                        // If the store drops it from the listing instead, that is the same
                        // outcome the page shows.
                        Err(_) => absent(&config, "ec2", &id).await,
                    }
                }
            );
        }

        {
            let name = unique("iam", '_');
            let _ = drivers::iam::delete_user(&config, &name).await;
            page!(
                &mut failures,
                "iam",
                async {
                    create_resource("iam", &config, &name).await?;
                    row(&config, "iam", &name).await?;
                    // An access key is the child row this page can have. Only the id goes
                    // into the listing, so only the id is what this can assert on; the
                    // secret stays in `_secret` and is never printed or stored.
                    let (key_id, _secret) = drivers::iam::create_access_key(&config, &name).await?;
                    let kids = children(&config, "iam", &name, "").await?;
                    ensure!(
                        kids.contains(&key_id),
                        "iam page: {name} listed no access key after create"
                    );
                    drivers::iam::delete_access_key(&config, &name, &key_id).await?;
                    Ok(())
                },
                async {
                    delete_resource("iam", &config, &name, "").await?;
                    absent(&config, "iam", &name).await
                }
            );
        }

        {
            let name = unique("sns", '-');
            let mut arn = String::new();
            page!(
                &mut failures,
                "sns",
                async {
                    create_resource("sns", &config, &name).await?;
                    arn = row(&config, "sns", &name).await?.detail;
                    ensure!(
                        arn.ends_with(&name),
                        "sns page: {name} was listed with the ARN {arn}"
                    );
                    // A topic with no subscriptions still has to answer for its children.
                    children(&config, "sns", &name, &arn).await?;
                    Ok(())
                },
                async {
                    delete_resource("sns", &config, &name, &arn).await?;
                    absent(&config, "sns", &name).await
                }
            );
        }

        {
            let name = unique("rds", '-');
            let _ = drivers::rds::delete_instance(&config, &name).await;
            page!(
                &mut failures,
                "rds",
                async {
                    create_resource("rds", &config, &name).await?;
                    row(&config, "rds", &name).await?;
                    Ok(())
                },
                async {
                    delete_resource("rds", &config, &name, "").await?;
                    absent(&config, "rds", &name).await
                }
            );
        }

        {
            let name = unique("logs", '-');
            let stream = "dash-wiring-stream";
            let _ = drivers::cloudwatch_logs::delete_log_group_tree(&config, &name).await;
            page!(
                &mut failures,
                "logs",
                async {
                    create_resource("logs", &config, &name).await?;
                    row(&config, "logs", &name).await?;
                    drivers::cloudwatch_logs::create_log_stream(&config, &name, stream).await?;
                    let kids = children(&config, "logs", &name, "").await?;
                    ensure!(
                        kids.iter().any(|s| s == stream),
                        "logs page: {name} listed {kids:?} after create_log_stream"
                    );
                    // The child row's delete button, then the group's.
                    drivers::cloudwatch_logs::delete_log_stream(&config, &name, stream).await?;
                    Ok(())
                },
                async {
                    delete_resource("logs", &config, &name, "").await?;
                    absent(&config, "logs", &name).await
                }
            );
        }

        {
            let name = unique("kinesis", '-');
            page!(
                &mut failures,
                "kinesis",
                async {
                    create_resource("kinesis", &config, &name).await?;
                    drivers::kinesis::wait_state(&config, &name, "ACTIVE", 30).await?;
                    row(&config, "kinesis", &name).await?;
                    let kids = children(&config, "kinesis", &name, "").await?;
                    ensure!(!kids.is_empty(), "kinesis page: {name} listed no shard");
                    Ok(())
                },
                async {
                    delete_resource("kinesis", &config, &name, "").await?;
                    absent(&config, "kinesis", &name).await
                }
            );
        }

        {
            let name = unique("secretsmanager", '-');
            let _ = drivers::secrets_manager::delete_secret(&config, &name).await;
            page!(
                &mut failures,
                "secretsmanager",
                async {
                    // The secret dialog collects a value, so this page is deliberately not
                    // in the name-only create table; the value is what the create needs.
                    refused(
                        create_resource("secretsmanager", &config, &name).await,
                        "no create is wired",
                    )?;
                    drivers::secrets_manager::create_secret(&config, &name, r#"{"dash":"wiring"}"#)
                        .await?;
                    row(&config, "secretsmanager", &name).await?;
                    let kids = children(&config, "secretsmanager", &name, "").await?;
                    ensure!(
                        kids.len() == 1,
                        "secretsmanager page: {name} listed {kids:?} versions, expected one"
                    );
                    Ok(())
                },
                async {
                    delete_resource("secretsmanager", &config, &name, "").await?;
                    absent(&config, "secretsmanager", &name).await
                }
            );
        }

        {
            let name = unique("alarms", '-');
            let _ = drivers::cloudwatch::delete_alarms(&config, std::slice::from_ref(&name)).await;
            page!(
                &mut failures,
                "monitoring",
                async {
                    // Read-only page: the dashboard offers no alarm create, so the table
                    // must say so rather than pretend. The alarm the delete is tested
                    // against is put through the driver, the way any CloudWatch client
                    // would put it.
                    refused(
                        create_resource("monitoring", &config, &name).await,
                        "no create is wired",
                    )?;
                    drivers::cloudwatch::put_metric_alarm(
                        &config,
                        &name,
                        "DashWiring",
                        "Errors",
                        1.0,
                    )
                    .await?;
                    row(&config, "monitoring", &name).await?;
                    Ok(())
                },
                async {
                    delete_resource("monitoring", &config, &name, "").await?;
                    absent(&config, "monitoring", &name).await
                }
            );
        }

        {
            let name = unique("sts", '-');
            let _ = drivers::iam::delete_role(&config, &name).await;
            // The account id, for a trust policy that names the caller: the IAM
            // page's default trust policy names only the EC2 service principal,
            // which no principal this dashboard signs with is.
            let caller = aws_sdk_sts::Client::new(&config)
                .get_caller_identity()
                .send()
                .await
                .expect("GetCallerIdentity");
            let caller_arn = caller.arn().expect("caller Arn").to_string();
            let account = caller.account().expect("caller account").to_string();
            let role_arn = format!("arn:aws:iam::{account}:role/{name}");
            let trust = format!(
                r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"AWS":"{caller_arn}"}},"Action":"sts:AssumeRole"}}]}}"#
            );
            page!(
                &mut failures,
                "sts",
                async {
                    aws_sdk_iam::Client::new(&config)
                        .create_role()
                        .role_name(&name)
                        .assume_role_policy_document(&trust)
                        .send()
                        .await
                        .context("CreateRole for the STS workbench")?;
                    row(&config, "sts", &name).await?;
                    let mint = drivers::sts::assume(&config, &role_arn, "dashboard").await?;
                    ensure!(
                        mint.identity_arn
                            .contains(&format!("assumed-role/{name}/dashboard")),
                        "sts page: assuming {name} verified as {}, not the assumed-role session",
                        mint.identity_arn
                    );
                    Ok(())
                },
                async {
                    drivers::iam::delete_role(&config, &name).await?;
                    absent(&config, "sts", &name).await
                }
            );
            record(
                &mut failures,
                "sts assume of an absent role",
                expect_error(
                    drivers::sts::assume(
                        &config,
                        &format!("arn:aws:iam::{account}:role/dash-wiring-absent"),
                        "dashboard",
                    )
                    .await
                    .map(|_| ()),
                ),
            );
        }

        {
            let name = unique("ssm", '-');
            let _ = drivers::ssm::delete(&config, &name).await;
            page!(
                &mut failures,
                "ssm",
                async {
                    drivers::ssm::create(&config, &name, "wiring-value", "String").await?;
                    row(&config, "ssm", &name).await?;
                    let (type_, value) = drivers::ssm::reveal(&config, &name).await?;
                    ensure!(
                        type_ == "String" && value == "wiring-value",
                        "ssm page: reveal read ({type_}, {value:?}), not the put value"
                    );
                    let versions = children(&config, "ssm", &name, "").await?;
                    ensure!(
                        versions.len() == 1 && versions[0].starts_with("version 1"),
                        "ssm page: history listed {versions:?}, expected one version"
                    );
                    Ok(())
                },
                async {
                    delete_resource("ssm", &config, &name, "").await?;
                    absent(&config, "ssm", &name).await
                }
            );
            record(
                &mut failures,
                "ssm reveal of an absent parameter",
                expect_error(drivers::ssm::reveal(&config, &name).await.map(|_| ())),
            );
        }

        // The bail! arms, so an unwired page or verb is an error the user reads rather than
        // a green row that never happened.
        record(
            &mut failures,
            "catalog page with no driver",
            refused(
                fetch_service("apigateway", &config).await.map(|_| ()),
                "no driver is wired",
            ),
        );
        for key in [
            "s3",
            "dynamodb",
            "sqs",
            "secretsmanager",
            "monitoring",
            "ssm",
        ] {
            record(
                &mut failures,
                &format!("{key} has no name-only create"),
                refused(
                    create_resource(key, &config, "nope").await,
                    "no create is wired",
                ),
            );
        }
        for key in [
            "s3",
            "dynamodb",
            "sqs",
            "iam",
            "sns",
            "logs",
            "kinesis",
            "secretsmanager",
        ] {
            record(
                &mut failures,
                &format!("{key} has no lifecycle verb"),
                refused(
                    run_lifecycle(key, "start", &config, "nope").await,
                    "does not support",
                ),
            );
        }
        // A delete of a row that went away underneath the page. Measured: S3, DynamoDB, SQS,
        // SNS, Logs and Kinesis all answer their service's not-found, so the page shows the
        // failure instead of a green row; `DeleteSecret` and `DeleteAlarms` answer success
        // for a name the store does not hold, so those two deletes are quiet by the
        // emulator's own design and only re-poll the listing.
        for key in ["s3", "dynamodb", "sqs", "sns", "logs", "kinesis"] {
            let outcome = delete_resource(
                key,
                &config,
                "dash-wiring-absent",
                "https://example.invalid/dash-wiring-absent",
            );
            record(
                &mut failures,
                &format!("{key} delete of an absent resource"),
                expect_error(outcome.await),
            );
        }
        for key in ["secretsmanager", "monitoring"] {
            record(
                &mut failures,
                &format!("{key} delete of an absent resource is quiet"),
                delete_resource(key, &config, "dash-wiring-absent", "dash-wiring-absent").await,
            );
        }

        assert!(
            failures.is_empty(),
            "Tier A wiring failed on {} of {} pages:\n{}",
            failures.len(),
            MANAGED.len(),
            failures.join("\n")
        );
    }

    /// The registries and the dispatch tables have to agree on what a page offers, or the
    /// sidebar shows a button the page cannot back — or hides one it can.
    #[test]
    fn managed_pages_offer_a_create_everywhere_the_build_can_make_one() {
        for (key, label) in MANAGED {
            assert!(
                catalog::resource_noun(key).is_some(),
                "{label} offers a delete button but has no resource noun to name it"
            );
            if matches!(key, "monitoring" | "sts") {
                assert!(
                    catalog::create_action(key).is_none(),
                    "{label} shows a create button this build cannot back with a dialog"
                );
            } else {
                assert!(
                    catalog::create_action(key).is_some(),
                    "{label} is manageable but the page offers no way to make one"
                );
            }
        }
    }
}
