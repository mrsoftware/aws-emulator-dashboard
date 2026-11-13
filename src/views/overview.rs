//! Dashboard: the four metrics the emulator can actually answer, the traffic the proxy
//! caught, what is failing right now, and the services with the most resources.

use crate::app_state::{AppState, ConnectionState};
use crate::views::catalog::MANAGED;
use crate::views::root::RootView;
use crate::views::theme;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, SharedString, TestSupportExt, div, prelude::*, px, rgb,
};
use std::time::{SystemTime, UNIX_EPOCH};

/// The rows the table shows: the newest traffic, since every entry is pushed at the front.
const RECENT_ROWS: usize = 8;

const TIME_W: f32 = 80.0;
const SERVICE_W: f32 = 110.0;
const EVENT_W: f32 = 170.0;
const STATUS_W: f32 = 110.0;

/// The widths [`activity_rows`] reuses for its cells, so the header and the rows line up.
/// The proxy knows a request's verb and its latency — not the service behind it — so the
/// columns name what the row really carries.
const ACTIVITY_COLUMNS: &[(Option<f32>, &str)] = &[
    (Some(TIME_W), "TIME"),
    (None, "RESOURCE"),
    (Some(SERVICE_W), "METHOD"),
    (Some(EVENT_W), "LATENCY"),
    (Some(STATUS_W), "STATUS"),
];

pub fn render(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(metrics(state))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_start()
                .gap_4()
                .child(recent_activity(state, cx))
                .child(alerts_panel(state)),
        )
        .child(pinned_services(state, cx))
        .into_any()
}

/// The four metric cards matching the design. Every value names a source this build can
/// actually answer: the design's lambda-invocations and monthly-spend cards have no
/// emulator source, so those slots carry real aggregates instead of invented numbers.
fn metrics(state: &AppState) -> AnyElement {
    let running = state
        .instances
        .iter()
        .filter(|i| i.state == "running")
        .count();

    let s3_count = state.service("s3").items.len();
    let dynamodb_count = state.service("dynamodb").items.len();
    let alarm_count = state.service("monitoring").items.len();

    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_4()
        .child(theme::stat(
            "instances",
            "RUNNING INSTANCES",
            running.to_string(),
            format!("of {} known", state.instances.len()),
            theme::OK,
        ))
        .child(theme::stat(
            "s3",
            "S3 BUCKETS",
            s3_count.to_string(),
            format!("{} tables in DynamoDB", dynamodb_count),
            theme::INFO,
        ))
        .child(theme::stat(
            "alarms",
            "CLOUDWATCH ALARMS",
            alarm_count.to_string(),
            format!(
                "{} errors in the last sweep",
                state
                    .resources
                    .values()
                    .filter(|r| r.error.is_some())
                    .count()
            ),
            if alarm_count > 0 {
                theme::OK
            } else {
                theme::INFO
            },
        ))
        .child(theme::stat(
            "traffic",
            "REQUESTS CAPTURED",
            state.traffic.len().to_string(),
            if state.proxy_running {
                "proxy running".to_string()
            } else {
                "proxy stopped".to_string()
            },
            if state.proxy_running {
                theme::OK
            } else {
                theme::WARN
            },
        ))
        .into_any()
}

/// A card with the mockup's heading row: a bold title and, on the right, whatever the panel
/// wants counted or clicked. [`theme::panel`] only carries the small uppercase label, so the
/// two titled panels here use this instead.
fn titled_panel(title: &str, trailing: Option<AnyElement>) -> Div {
    theme::card().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::TEXT))
                    .child(title.to_string()),
            )
            .when_some(trailing, |d, e| d.child(div().ml_auto().child(e))),
    )
}

fn recent_activity(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let view_all = div()
        .id("overview-view-all")
        .test_support()
        .flex()
        .items_center()
        .gap_1()
        .cursor_pointer()
        .text_sm()
        .text_color(rgb(theme::ACCENT))
        .child("View all →")
        .on_click(cx.listener(|this, _, _, cx| {
            this.state.update(cx, |s, cx| s.open_page("traffic", cx));
        }))
        .into_any();

    titled_panel("Recent activity", Some(view_all))
        .flex_1()
        .min_w(px(520.0))
        .gap_0()
        .child(theme::table_head(ACTIVITY_COLUMNS))
        .children(activity_rows(state))
        .when(state.traffic.is_empty(), |d| {
            d.child(div().py_2().child(theme::muted(
                "No requests captured - turn the proxy on in Traffic, or let a page make a call.",
            )))
        })
        .into_any()
}

fn activity_rows(state: &AppState) -> Vec<AnyElement> {
    state
        .traffic
        .iter()
        .take(RECENT_ROWS)
        .enumerate()
        .map(|(index, entry)| {
            let status_color = if entry.status < 400 {
                theme::OK
            } else {
                theme::FAIL
            };
            theme::row(format!("activity-{index}"), false)
                .h(px(40.0))
                .rounded_none()
                .border_b_1()
                .border_color(rgb(theme::BORDER))
                .child(theme::cell(
                    Some(TIME_W),
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(12.0))
                        .text_color(rgb(theme::TEXT_SECONDARY))
                        .child(clock(entry.at)),
                ))
                .child(theme::cell(
                    None,
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(12.0))
                        .text_color(rgb(theme::TEXT_PRIMARY))
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(entry.path.clone()),
                ))
                .child(theme::cell(
                    Some(SERVICE_W),
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(12.0))
                        .text_color(rgb(theme::TEXT_SECONDARY))
                        .child(entry.method.clone()),
                ))
                .child(theme::cell(
                    Some(EVENT_W),
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(12.0))
                        .text_color(rgb(theme::TEXT_SECONDARY))
                        .child(format!("{} ms", entry.latency_ms)),
                ))
                .child(theme::cell(
                    Some(STATUS_W),
                    div()
                        .flex()
                        .child(theme::pill(entry.status.to_string(), status_color)),
                ))
                .into_any()
        })
        .collect()
}

/// Everything the console knows is failing: a service whose listing errored, the instance
/// table, the proxy, the last action and the connection itself. All of it is an error, so all
/// of it gets the failure dot — the mockup's warn-level rows have no source here.
fn alerts(state: &AppState) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (key, label) in MANAGED {
        if let Some(err) = &state.service(key).error {
            out.push(format!("{label}: {err}"));
        }
    }
    if let Some(err) = &state.instances_error {
        out.push(format!("DescribeInstances: {err}"));
    }
    if let Some(err) = &state.proxy_error {
        out.push(format!("Proxy: {err}"));
    }
    if let Some(err) = &state.action_error {
        out.push(format!("Last action: {err}"));
    }
    if let ConnectionState::Error(err) = &state.connection {
        out.push(format!("Connection: {err}"));
    }
    out
}

fn alerts_panel(state: &AppState) -> AnyElement {
    let items = alerts(state);
    // The count is a reading, not a button, so it stays off the accent: orange would promise
    // an action the header does not offer.
    let active = div()
        .text_sm()
        .text_color(rgb(if items.is_empty() {
            theme::MUTED
        } else {
            theme::FAIL
        }))
        .child(format!("{} active", items.len()))
        .into_any();

    titled_panel("Alerts", Some(active))
        .w(px(320.0))
        .when(items.is_empty(), |d| {
            d.child(theme::muted("Nothing to report."))
        })
        .children(items.into_iter().map(alert_row))
        .into_any()
}

fn alert_row(message: String) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(theme::pill("FAIL", theme::FAIL))
        .child(
            div()
                .flex_1()
                .text_size(px(13.0))
                .text_color(rgb(theme::TEXT))
                .overflow_hidden()
                .child(message),
        )
        .into_any()
}

/// The six services with the most resources, each a card that opens its own page.
fn pinned_services(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let mut ranked: Vec<(&str, &str, usize, bool)> = MANAGED
        .iter()
        .map(|(key, label)| {
            let svc = state.service(key);
            (
                *key,
                *label,
                svc.items.len(),
                // A listing that errored is red; one that answered is green, even when it
                // answered with nothing.
                svc.error.is_none(),
            )
        })
        .collect();
    ranked.sort_by_key(|row| std::cmp::Reverse(row.2));

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(theme::section_label("PINNED SERVICES"))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_3()
                .children(
                    ranked
                        .into_iter()
                        .take(6)
                        .map(|(key, label, count, listing_ok)| {
                            pinned_card(key, label, count, listing_ok, cx)
                        }),
                ),
        )
        .into_any()
}

fn pinned_card(
    key: &str,
    label: &str,
    count: usize,
    listing_ok: bool,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let target = key.to_string();
    div()
        .id(SharedString::from(format!("pin-{key}")))
        .test_support()
        .w(px(220.0))
        .flex()
        .flex_col()
        .gap_3()
        .p_3p5()
        .rounded_lg()
        .bg(rgb(theme::RAISED))
        .border_1()
        .border_color(rgb(theme::BORDER))
        .cursor_pointer()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(theme::tile(monogram(label)))
                .child(div().ml_auto().child(theme::dot(if listing_ok {
                    theme::OK
                } else {
                    theme::FAIL
                }))),
        )
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .child(label.to_string()),
        )
        .child(
            div()
                .font_family(theme::MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(format!(
                    "{count} resource{}",
                    if count == 1 { "" } else { "s" }
                )),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            let target = target.clone();
            this.state.update(cx, |s, cx| s.open_page(&target, cx));
        }))
        .into_any()
}

/// The two-letter tile the mockup's icon would occupy.
fn monogram(title: &str) -> String {
    let mut out: String = title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(2)
        .collect();
    if out.is_empty() {
        out.push('?');
    }
    out.make_ascii_uppercase();
    out
}

/// The wall clock, printed the way the Traffic page prints it.
fn clock(at: SystemTime) -> String {
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() % 86_400)
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}
