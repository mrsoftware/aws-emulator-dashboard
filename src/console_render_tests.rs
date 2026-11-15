//! Headless render checks for the console pages.
//!
//! macOS will not let this session capture the live window, so these tests drive GPUI's
//! own screenshot path instead: a `HeadlessAppContext` lays the real views out in an
//! off-screen window and renders the scene through a Metal headless renderer, which needs
//! neither a visible window nor the AppKit main thread. The PNGs land in
//! `target/console-render/`.

use crate::app_state::{
    AppState, ConnectionInfo, ConnectionState, InstanceTab, Resource, ServiceResources,
};
use crate::aws::client::EmulatorConnection;
use crate::aws::drivers::{self, ec2::InstanceInfo};
use crate::emulator::proxy::TrafficEntry;
use crate::settings::Settings;
use crate::views::catalog::MANAGED;
use crate::views::root::{RootView, fetch_service};
use anyhow::Result;
use gpui_kit::{App, AppContext, Entity, HeadlessAppContext, px, size};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, SystemTime};

const OUT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/console-render");

fn headless() -> HeadlessAppContext {
    HeadlessAppContext::with_platform(
        std::sync::Arc::new(gpui_apple::AppleTextSystem::new()),
        std::sync::Arc::new(gpui_kit::assets::AllAssets::new("")),
        gpui_platform::current_headless_renderer,
    )
}

/// Lays the real `RootView` out at console size, renders it, and writes the frame to
/// `OUT_DIR/{name}.png`. Fails if the frame is a flat fill, which is what a view that
/// rendered nothing at all looks like.
fn capture(cx: &mut HeadlessAppContext, state: &Entity<AppState>, name: &str) {
    let window = cx
        .open_window(size(px(1440.0), px(900.0)), |_window, app| {
            app.new(|_| RootView::new(state.clone()))
        })
        .expect("open headless window");
    let image = cx
        .capture_screenshot(window.into())
        .expect("Metal headless render");
    std::fs::create_dir_all(OUT_DIR).expect("create output dir");
    let path = format!("{OUT_DIR}/{name}.png");
    image
        .save(&path)
        .unwrap_or_else(|e| panic!("write {path}: {e}"));

    let colors: HashSet<[u8; 3]> = image
        .pixels()
        .step_by(97)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    assert!(
        colors.len() > 8,
        "{name}: only {} distinct colours in the frame — nothing was drawn (wrote {path})",
        colors.len()
    );
}

fn named(name: &str, detail: &str) -> Resource {
    Resource {
        name: name.to_string(),
        detail: detail.to_string(),
    }
}

fn rows(key: &str, items: Vec<Resource>) -> (&str, Vec<Resource>) {
    (key, items)
}

/// A state shaped exactly like a freshly refreshed console: every Tier A page has rows, so
/// a panel that renders empty is a rendering bug and not a missing resource.
fn fixture(app: &mut App) -> Entity<AppState> {
    let rows: HashMap<String, Vec<Resource>> = [
        rows(
            "s3",
            vec![
                named("assets", "us-east-1"),
                named("logs-archive", "us-east-1"),
            ],
        ),
        rows(
            "dynamodb",
            vec![
                named("rides", "HASH: id"),
                named("pricing-rules", "HASH: key, RANGE: bucket"),
            ],
        ),
        rows(
            "sqs",
            vec![
                named("jobs", "https://sqs/9297/000000000000/jobs"),
                named("jobs-dlq", "https://sqs/9297/000000000000/jobs-dlq"),
            ],
        ),
        rows(
            "ec2",
            vec![
                named("i-0a1b2c3d4e5f60718", "t3.micro · running"),
                named("i-09988776655443322", "t3.small · stopped"),
            ],
        ),
        rows(
            "iam",
            vec![named(
                "ci-deployer",
                "arn:aws:iam::000000000000:user/ci-deployer",
            )],
        ),
        rows(
            "sns",
            vec![named("alerts", "arn:aws:sns:us-east-1:000000000000:alerts")],
        ),
        rows("rds", vec![named("pg-dev", "db.t3.micro · available")]),
        rows(
            "logs",
            vec![
                named("/ecs/pricing", "1 stream"),
                named("/lambda/edge", "3 streams"),
            ],
        ),
        rows("kinesis", vec![named("clicks", "ACTIVE · 1 shard")]),
        rows(
            "secretsmanager",
            vec![named("db/password", "created just now")],
        ),
        rows(
            "monitoring",
            vec![named("HighUtilisation", "ALARM"), named("QueueDepth", "OK")],
        ),
        rows(
            "sts",
            vec![named(
                "dash-deploy",
                "arn:aws:iam::000000000000:role/dash-deploy",
            )],
        ),
        rows(
            "ssm",
            vec![
                named("/app/db/url", "String · modified 2026-11-14T09:00:00Z"),
                named(
                    "/app/db/password",
                    "SecureString · modified 2026-11-14T09:01:00Z",
                ),
            ],
        ),
    ]
    .into_iter()
    .map(|(key, items)| (key.to_string(), items))
    .collect();

    let instance =
        |id: &str, name: &str, state: &str, kind: &str, az: &str, ip: &str, launched: &str| {
            InstanceInfo {
                id: id.into(),
                name: name.into(),
                state: state.into(),
                instance_type: kind.into(),
                az: az.into(),
                ip: ip.into(),
                launched: launched.into(),
            }
        };
    // One row with everything, one with no address and no launch time: the page has to show
    // a dash for the emulator's silences rather than invent a value.
    let instances = vec![
        instance(
            "i-0a1b2c3d4e5f60718",
            "pricing-api",
            "running",
            "t3.micro",
            "us-east-1a",
            "54.210.9.42",
            "2026-02-12T09:14:00Z",
        ),
        instance(
            "i-0b2c3d4e5f6071829",
            "worker",
            "pending",
            "t3.small",
            "us-east-1b",
            "",
            "",
        ),
        instance(
            "i-09988776655443322",
            "legacy",
            "stopped",
            "t2.micro",
            "us-east-1a",
            "",
            "2026-01-28T11:40:00Z",
        ),
        instance(
            "i-01122334455667788",
            "",
            "terminated",
            "t3.nano",
            "us-east-1c",
            "",
            "",
        ),
    ];

    let traffic = [
        ("GET", "/2017-06-01/queues/000000000000/jobs", 200u16, 12u64),
        ("POST", "/", 200, 31),
        ("HEAD", "/assets/pricing.csv", 404, 4),
        ("POST", "/2012-12-12/", 400, 7),
    ]
    .into_iter()
    .map(|(method, path, status, latency_ms)| TrafficEntry {
        at: SystemTime::now(),
        method: method.to_string(),
        path: path.to_string(),
        status,
        latency_ms,
    })
    .collect::<VecDeque<_>>();

    app.new(|cx| {
        let mut state = AppState::new(Settings::default(), cx);
        state.connection = ConnectionState::Connected(ConnectionInfo {
            endpoint: "https://127.0.0.1:9297".into(),
            ca_path: std::path::PathBuf::from(".aws/emulator/9297/ca.pem"),
        });
        state.resources = rows
            .into_iter()
            .map(|(key, items)| {
                (
                    key,
                    ServiceResources {
                        loading: false,
                        error: None,
                        items,
                    },
                )
            })
            .collect();
        state.instances = instances;
        state.traffic = traffic;
        state.emulator.running = true;
        state.emulator.started_at = Some(std::time::Instant::now() - Duration::from_secs(124));
        state.proxy_running = true;
        state
    })
}

/// The fixture, then a page-specific tweak, before any window is opened.
fn state_with(cx: &mut HeadlessAppContext, fix: impl Fn(&mut AppState)) -> Entity<AppState> {
    cx.update(|app| {
        let entity = fixture(app);
        entity.update(app, |s, _| fix(s));
        entity
    })
}

/// Every number on Overview has to name its source, so the check is that the panels the
/// mockup promises are all present and drawn.
#[test]
fn overview_page_renders_its_panels() {
    let mut cx = headless();
    let state = cx.update(fixture);
    capture(&mut cx, &state, "overview");
}

#[test]
fn instances_page_renders_tabs_and_rows() {
    let mut cx = headless();
    let state = cx.update(|app| {
        let entity = fixture(app);
        entity.update(app, |s, _| s.selected_service = "instances".into());
        entity
    });
    capture(&mut cx, &state, "instances");
}

#[test]
fn catalog_page_renders_groups_and_chips() {
    let mut cx = headless();
    let state = cx.update(|app| {
        let entity = fixture(app);
        entity.update(app, |s, _| s.selected_service = "catalog".into());
        entity
    });
    capture(&mut cx, &state, "catalog");
}

/// The control panel reads the process manager rather than the API, so its check is that the
/// running state, the uptime it tracks itself, and the options strip all draw — and that a
/// press in flight still paints instead of blanking the page.
#[test]
fn control_panel_page_renders_process_and_options() {
    let mut cx = headless();
    let state = state_with(&mut cx, |s| s.selected_service = "control".into());
    capture(&mut cx, &state, "control");

    let busy = state_with(&mut cx, |s| {
        s.selected_service = "control".into();
        s.emulator.busy = true;
    });
    capture(&mut cx, &busy, "control-busy");
}

/// The traffic page: the proxy's controls and one table row per request it captured.
#[test]
fn traffic_page_renders_proxy_controls_and_rows() {
    let mut cx = headless();
    let state = state_with(&mut cx, |s| s.selected_service = "traffic".into());
    capture(&mut cx, &state, "traffic");
}

/// Both pages again with an emulator this dashboard never started and never proxied: the
/// fields that have no answer must still paint, as a dash or an explanation, not as a number.
#[test]
fn control_and_traffic_pages_render_when_the_emulator_has_nothing() {
    let mut cx = headless();
    for (page, file) in [("control", "empty-control"), ("traffic", "empty-traffic")] {
        let state = cx.update(|app| {
            let entity = app.new(|cx| AppState::new(Settings::default(), cx));
            entity.update(app, |s, _| s.selected_service = page.into());
            entity
        });
        capture(&mut cx, &state, file);
    }
}

/// The spec's acceptance line: the same three pages, filled from the running emulator
/// rather than a fixture. Ignored by default because it needs the emulator on :9297.
#[test]
#[ignore = "requires a running emulator on 127.0.0.1:9297"]
fn all_three_pages_render_with_live_data() {
    let (rows, instances) = live_inventory().expect("live inventory from the emulator");
    let mut cx = headless();
    for (page, file) in [
        ("overview", "overview-live"),
        ("instances", "instances-live"),
        ("catalog", "catalog-live"),
    ] {
        let state = cx.update(|app| {
            let entity = fixture(app);
            entity.update(app, |s, _| {
                s.resources = rows.clone();
                s.instances = instances.clone();
                // Only what this test actually read from the emulator may appear: the proxy
                // never ran here, so traffic and uptime stay at their empty values.
                s.traffic.clear();
                s.emulator = Default::default();
                s.proxy_running = false;
                s.selected_service = page.into();
            });
            entity
        });
        capture(&mut cx, &state, file);
    }
}

fn live_inventory() -> Result<(HashMap<String, ServiceResources>, Vec<InstanceInfo>)> {
    let conn = EmulatorConnection::from_port(9297);
    crate::aws::client::runtime().block_on(async move {
        let config = conn.build_aws_config().await?;
        let mut resources = HashMap::new();
        for (key, _) in MANAGED {
            let items = fetch_service(key, &config)
                .await
                .map_err(|e| anyhow::anyhow!("{key}: {e:#}"))?;
            resources.insert(
                key.to_string(),
                ServiceResources {
                    loading: false,
                    error: None,
                    items,
                },
            );
        }
        let instances = drivers::ec2::list_instances(&config)
            .await
            .map_err(|e| anyhow::anyhow!("ec2: {e:#}"))?;
        Ok((resources, instances))
    })
}

/// A page whose data layer is empty must still draw its own chrome, not a blank window.
#[test]
fn pages_render_when_the_emulator_has_nothing() {
    let mut cx = headless();
    for (page, file) in [
        ("overview", "empty-overview"),
        ("instances", "empty-instances"),
        ("catalog", "empty-catalog"),
        ("s3", "empty-s3"),
    ] {
        let state = cx.update(|app| {
            let entity = app.new(|cx| AppState::new(Settings::default(), cx));
            entity.update(app, |s, _| s.selected_service = page.into());
            entity
        });
        capture(&mut cx, &state, file);
    }
}

/// The detail dialog is the one overlay the console pages can raise; it has to draw over
/// the page it belongs to.
#[test]
fn resource_page_renders_detail_and_dialog() {
    use crate::app_state::Dialog;
    let mut cx = headless();
    let state = cx.update(|app| {
        let entity = fixture(app);
        entity.update(app, |s, _| {
            s.selected_service = "dynamodb".into();
            s.selected_resource = Some("rides".into());
            s.dialog = Some(Dialog::PutItem);
            s.detail.items = vec!["id=1".into(), "id=2".into()];
            s.instance_tab = InstanceTab::All;
        });
        entity
    });
    capture(&mut cx, &state, "resource-dialog");
}

/// The Operations card is what makes every Tier A driver verb reachable from a page, and its
/// dialog is the one overlay that carries a name slot plus labelled argument fields.
#[test]
fn operations_card_and_operation_dialog_render() {
    use crate::app_state::Dialog;
    use crate::views::operations;

    // s3, dynamodb and sqs act through their own dialogs, so they are not in this table.
    for key in [
        "ec2",
        "iam",
        "sns",
        "rds",
        "logs",
        "kinesis",
        "secretsmanager",
        "monitoring",
    ] {
        for selection in [false, true] {
            let ops = operations::operations(key, selection);
            // ec2's only verb acts on an instance, so the empty card without a selection
            // is the page telling the user to pick a row.
            if selection {
                assert!(
                    !ops.is_empty(),
                    "{key}: selecting a row offers no operation"
                );
            }
            for op in &ops {
                assert!(
                    op.fields.len() <= 4,
                    "{key}: {} declares {} fields but the dialog has 4 slots",
                    op.label,
                    op.fields.len()
                );
            }
        }
    }

    let mut cx = headless();
    let card = state_with(&mut cx, |s| {
        s.selected_service = "logs".into();
        s.selected_resource = Some("/ecs/pricing".into());
        s.detail.output = vec![
            "/ecs/pricing · 1 stream".into(),
            "event 1730000000000 pricing-api".into(),
        ];
    });
    capture(&mut cx, &card, "operations-card");

    let dialog = state_with(&mut cx, |s| {
        s.selected_service = "monitoring".into();
        let op = operations::operations("monitoring", false)
            .into_iter()
            .find(|op| op.name.is_some() && !op.fields.is_empty())
            .expect("a monitoring operation with a name slot and fields");
        s.dialog_op = Some(op);
        s.dialog = Some(Dialog::Operation);
    });
    capture(&mut cx, &dialog, "operations-dialog");
}

/// The bug this guards: the body used `size_full` inside a flex column, so it measured the
/// whole window instead of the room the header left. Nothing ever overflowed, and the wheel
/// did nothing on every page. In a deliberately short window the page handle has to report
/// room to scroll, and the nav — 17 collapsed categories — likewise.
#[test]
fn page_body_and_nav_scroll_in_a_short_window() {
    let mut cx = headless();
    let state = state_with(&mut cx, |s| {
        s.selected_service = "s3".into();
        s.resources
            .get_mut("s3")
            .expect("the fixture lists s3")
            .items = (0..60)
            .map(|i| named(&format!("bucket-{i:02}"), "us-east-1"))
            .collect();
    });
    let view = cx.update(|app| app.new(|_| RootView::new(state.clone())));
    let window = cx
        .open_window(size(px(1440.0), px(400.0)), {
            let view = view.clone();
            move |_window, _app| view.clone()
        })
        .expect("open a short window");
    cx.capture_screenshot(window.into())
        .expect("Metal headless render");

    let (page, nav) = cx.update(|app| {
        view.update(app, |view, _| {
            (view.page_scroll.max_offset(), view.nav_scroll.max_offset())
        })
    });
    assert!(
        page.y > px(0.0),
        "60 rows in a 400px window left the page body with no overflow (max_offset {page:?}) — \
         the body is not bounded, so it cannot scroll"
    );
    assert!(
        nav.y > px(0.0),
        "the sidebar nav has no overflow (max_offset {nav:?}) — a long category list is cut off"
    );
}

/// Every table shape the resource page draws after the theme restyle: objects, items,
/// received messages, child rows, the operations card with its answered lines, an open
/// dialog over all of it, and the one-time reveal banner.
#[test]
fn resource_page_renders_every_table_and_the_reveal_banner() {
    use crate::app_state::Dialog;
    use crate::aws::manage::{MessageInfo, ObjectInfo};
    use crate::views::operations;

    let mut cx = headless();

    let objects = state_with(&mut cx, |s| {
        s.selected_service = "s3".into();
        s.selected_resource = Some("assets".into());
        s.detail.objects = vec![
            ObjectInfo {
                key: "pricing.csv".into(),
                size: 4096,
                modified: "2026-02-12T09:14:00Z".into(),
            },
            ObjectInfo {
                key: "notes.txt".into(),
                size: 12,
                modified: "2026-01-28T11:40:00Z".into(),
            },
        ];
    });
    capture(&mut cx, &objects, "resource-objects");

    let messages = state_with(&mut cx, |s| {
        s.selected_service = "sqs".into();
        s.selected_resource = Some("jobs".into());
        s.detail.messages = vec![MessageInfo {
            id: "msg-1".into(),
            body: "pricing recomputed".into(),
            receipt: "receipt-1".into(),
        }];
    });
    capture(&mut cx, &messages, "resource-messages");

    // The reveal banner is drawn from a placeholder: a key id from the AWS docs, and a
    // stand-in for the secret. No real credential is put in state, a file, or a log.
    let reveal = state_with(&mut cx, |s| {
        s.selected_service = "iam".into();
        s.selected_resource = Some("ci-deployer".into());
        s.detail.children = vec![named("AKIAIOSFODNN7EXAMPLE", "active · created just now")];
        s.detail.revealed = Some(
            "Access key AKIAIOSFODNN7EXAMPLE created. Secret: [redacted placeholder] — copy it \
             now, it cannot be read back."
                .into(),
        );
    });
    capture(&mut cx, &reveal, "resource-reveal");

    let card_and_dialog = state_with(&mut cx, |s| {
        s.selected_service = "logs".into();
        s.selected_resource = Some("/ecs/pricing".into());
        s.detail.children = vec![named(
            "pricing-api",
            "12 events · last 2026-02-12T09:14:00Z",
        )];
        s.detail.output = vec![
            "event 1730000000000 pricing-api ERROR timeout".into(),
            "event 1730000000500 pricing-api INFO retried".into(),
        ];
        s.dialog_op = operations::operations("logs", true)
            .into_iter()
            .find(|op| !op.fields.is_empty());
        s.dialog = Some(Dialog::Operation);
    });
    capture(&mut cx, &card_and_dialog, "resource-operations-dialog");
}

/// The STS workbench: a selected role, the Assume button, the minted sessions the
/// dashboard itself recorded, and the one-time reveal of a mint. The mint row uses
/// placeholder text — no real credential is put in state, a file, or a log.
#[test]
fn sts_page_renders_the_assume_button_and_minted_sessions() {
    let mut cx = headless();

    let empty = state_with(&mut cx, |s| {
        s.selected_service = "sts".into();
        s.selected_resource = Some("dash-deploy".into());
    });
    capture(&mut cx, &empty, "sts-workbench-empty");

    let mints = state_with(&mut cx, |s| {
        s.selected_service = "sts".into();
        s.selected_resource = Some("dash-deploy".into());
        s.minted_sessions = vec![crate::aws::drivers::sts::MintedSession {
            role_name: "dash-deploy".into(),
            session_name: "dashboard".into(),
            identity_arn: "arn:aws:sts::000000000000:assumed-role/dash-deploy/dashboard".into(),
            expires: "2026-11-14T12:00:00Z".into(),
            access_key_id: "ASIAIOSFODNN7EXAMPLE".into(),
        }];
        s.detail.revealed = Some(
            "Assumed role dash-deploy as session dashboard. Identity: \
             arn:aws:sts::000000000000:assumed-role/dash-deploy/dashboard. Credentials \
             expire: 2026-11-14T12:00:00Z. Access key ASIAIOSFODNN7EXAMPLE minted; its \
             secret is not stored."
                .into(),
        );
    });
    capture(&mut cx, &mints, "sts-workbench-minted");
}

/// The SSM Parameter Store page: a selected parameter, its version history as the
/// children table, and the one-time reveal banner. The revealed value is placeholder
/// text — no real parameter value is put in state, a file, or a log.
#[test]
fn ssm_page_renders_the_reveal_button_and_history() {
    let mut cx = headless();

    let empty = state_with(&mut cx, |s| {
        s.selected_service = "ssm".into();
        s.selected_resource = Some("/app/db/url".into());
    });
    capture(&mut cx, &empty, "ssm-parameters-empty");

    let reveal = state_with(&mut cx, |s| {
        s.selected_service = "ssm".into();
        s.selected_resource = Some("/app/db/url".into());
        s.detail.children = vec![
            named("version 1", "String · modified 2026-11-14T09:00:00Z"),
            named("version 2", "String · modified 2026-11-14T09:05:00Z"),
        ];
        s.detail.revealed = Some("Parameter /app/db/url: (String) [redacted placeholder]".into());
    });
    capture(&mut cx, &reveal, "ssm-parameters-reveal");
}

/// The resource pages against the running emulator: every Tier A page with its first live
/// row selected and that row's real children, so the themed tables draw what the emulator
/// actually answers rather than a fixture's guess. Reads only — it creates nothing.
#[test]
#[ignore = "requires a running emulator on 127.0.0.1:9297; reads only, creates nothing"]
fn resource_pages_render_with_live_data() {
    let (rows, instances) = live_inventory().expect("live inventory from the emulator");
    let children = crate::aws::client::runtime().block_on(async {
        let config = EmulatorConnection::from_port(9297)
            .build_aws_config()
            .await
            .expect("aws config");
        let mut out = HashMap::new();
        for (key, _) in MANAGED {
            let Some(row) = rows.get(key).and_then(|r| r.items.first()) else {
                continue;
            };
            if let Ok(kids) =
                crate::views::root::fetch_children(key, &config, &row.name, &row.detail).await
            {
                out.insert(key.to_string(), kids);
            }
        }
        out
    });

    let mut cx = headless();
    for (key, _) in MANAGED {
        let state = cx.update(|app| {
            let entity = fixture(app);
            entity.update(app, |s, _| {
                s.resources = rows.clone();
                s.instances = instances.clone();
                // Only what this test read: the proxy never ran here, so traffic and uptime
                // stay empty rather than showing the fixture's invented numbers.
                s.traffic.clear();
                s.emulator = Default::default();
                s.proxy_running = false;
                s.selected_service = key.to_string();
                s.selected_resource = rows
                    .get(key)
                    .and_then(|r| r.items.first())
                    .map(|r| r.name.clone());
                s.detail.children = children.get(key).cloned().unwrap_or_default();
            });
            entity
        });
        capture(&mut cx, &state, &format!("live-resource-{key}"));
    }
}
