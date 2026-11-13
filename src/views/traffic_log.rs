//! Traffic: the proxy's own view of what went through it. Every row is a request the proxy
//! actually recorded, so the table shows the fields it captured and nothing else — no rate,
//! no total, no latency it never measured.

use crate::app_state::{AppState, ConnectionState};
use crate::aws::client::{EmulatorConnection, runtime};
use crate::emulator::proxy::MAX_ENTRIES;
use crate::views::root::RootView;
use crate::views::theme;
use gpui_kit::{AnyElement, Context, Entity, div, prelude::*, rgb};
use std::time::{SystemTime, UNIX_EPOCH};

/// Column widths shared by the header strip and every row, so the table lines up. The path
/// column takes whatever the fixed ones leave.
const COLUMNS: &[(Option<f32>, &str)] = &[
    (Some(90.0), "TIME (UTC)"),
    (Some(70.0), "METHOD"),
    (None, "PATH"),
    (Some(90.0), "STATUS"),
    (Some(80.0), "LATENCY"),
];

pub fn toggle(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    let (proxy, settings, running, conn) = {
        let s = state.read(cx);
        let conn = match &s.connection {
            ConnectionState::Connected(info) => Some(EmulatorConnection {
                endpoint: info.endpoint.clone(),
                credentials_path: dirs::home_dir()
                    .map(|h| {
                        h.join(format!(
                            ".aws/emulator/{}/emulator_credentials",
                            s.settings.default_port
                        ))
                    })
                    .unwrap_or_default(),
                ca_path: info.ca_path.clone(),
                auth_enabled: s.settings.auth_enabled,
            }),
            _ => None,
        };
        (s.proxy.clone(), s.settings.clone(), s.proxy_running, conn)
    };

    let state_clone = state.clone();
    if running {
        cx.spawn(async move |_, cx| {
            let _ = runtime().spawn(async move { proxy.stop().await }).await;
            cx.update(|cx| {
                state_clone.update(cx, |s, cx| {
                    s.proxy_running = false;
                    s.settings.proxy_enabled = false;
                    if let Err(e) = s.settings.save() {
                        tracing::warn!(error = %e, "saving settings");
                    }
                    cx.notify();
                })
            });
        })
        .detach();
    } else {
        let Some(conn) = conn else {
            state.update(cx, |s, cx| {
                s.proxy_error = Some("Connect to the emulator before starting the proxy".into());
                cx.notify();
            });
            return;
        };
        cx.spawn(async move |_, cx| {
            let result = runtime()
                .spawn(async move { proxy.start(&settings, &conn).await })
                .await;
            let result = match result {
                Ok(r) => r,
                Err(e) => Err(anyhow::anyhow!("proxy task failed: {e}")),
            };
            cx.update(|cx| {
                state_clone.update(cx, |s, cx| {
                    match result {
                        Ok(_) => {
                            s.proxy_running = true;
                            s.proxy_error = None;
                            s.settings.proxy_enabled = true;
                            if let Err(e) = s.settings.save() {
                                tracing::warn!(error = %e, "saving settings");
                            }
                        }
                        Err(e) => {
                            s.proxy_error = Some(format!("{e:#}"));
                        }
                    }
                    cx.notify();
                })
            });
        })
        .detach();
    }
}

pub fn clear(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    state.update(cx, |s, cx| {
        s.traffic.clear();
        cx.notify();
    });
}

pub fn render(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(proxy_card(state, cx))
        .child(table_card(state))
        .into_any()
}

/// The proxy's state, the one action that changes it, and the address to point a client at.
fn proxy_card(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let running = state.proxy_running;
    let proxy_port = state.settings.proxy_port;

    theme::card()
        .child(theme::section_label("Proxy"))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(theme::pill(
                    if running {
                        "Proxy running"
                    } else {
                        "Proxy stopped"
                    },
                    if running { theme::OK } else { theme::FAINT },
                ))
                .child(theme::chip_box(theme::button(
                    "proxy-toggle",
                    if running { "Stop proxy" } else { "Start proxy" },
                    if running {
                        theme::Kind::Danger
                    } else {
                        theme::Kind::Primary
                    },
                    cx,
                    |this, _window, cx| toggle(&this.state, cx),
                )))
                .child(theme::chip_box(theme::button(
                    "traffic-clear",
                    "Clear",
                    theme::Kind::Ghost,
                    cx,
                    |this, _window, cx| clear(&this.state, cx),
                ))),
        )
        .child(theme::muted(format!(
            "Point your SDK or CLI at http://127.0.0.1:{proxy_port} to capture traffic. \
             Dashboard requests are routed through the proxy while it runs."
        )))
        .when_some(state.proxy_error.clone(), |d, err| {
            d.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::FAIL))
                    .child(format!("Error: {err}")),
            )
        })
        .into_any()
}

/// The captured requests, newest first, in the same table shape Overview's activity card uses.
/// A request the proxy did not see is not in this table, and nothing about one that is gets
/// extrapolated.
fn table_card(state: &AppState) -> AnyElement {
    theme::card()
        .gap_1()
        .child(theme::section_label(format!(
            "{} requests (newest first)",
            state.traffic.len()
        )))
        .when(!state.traffic.is_empty(), |d| {
            // The header strip only appears over rows, exactly as before: an empty capture is
            // the one state with nothing to label.
            d.child(theme::table_head(COLUMNS))
        })
        .when(state.traffic.is_empty(), |d| {
            d.child(theme::muted("No traffic captured yet"))
        })
        .children(
            state
                .traffic
                .iter()
                .enumerate()
                .take(MAX_ENTRIES)
                .map(|(index, entry)| traffic_row(index, entry)),
        )
        .into_any()
}

/// One request as the proxy recorded it: green under 400, red from 400 up, the same reading
/// Overview gives these rows.
fn traffic_row(index: usize, entry: &crate::emulator::proxy::TrafficEntry) -> AnyElement {
    let status_color = if entry.status < 400 {
        theme::OK
    } else {
        theme::FAIL
    };
    theme::row(format!("traffic-{index}"), false)
        .text_sm()
        .child(theme::cell(
            COLUMNS[0].0,
            div()
                .text_color(rgb(theme::MUTED))
                .child(format_time(entry.at)),
        ))
        .child(theme::cell(COLUMNS[1].0, div().child(entry.method.clone())))
        .child(theme::cell(
            COLUMNS[2].0,
            div()
                .text_color(rgb(theme::MUTED))
                .overflow_hidden()
                .child(entry.path.clone()),
        ))
        .child(theme::cell(
            COLUMNS[3].0,
            div().child(theme::pill(entry.status.to_string(), status_color)),
        ))
        .child(theme::cell(
            COLUMNS[4].0,
            div()
                .text_color(rgb(theme::MUTED))
                .child(format!("{} ms", entry.latency_ms)),
        ))
        .into_any()
}

fn format_time(at: SystemTime) -> String {
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day = secs % 86400;
    format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The table has one clock column and no date column, so the time is the UTC time of day
    /// and a capture past midnight rolls over rather than counting up.
    #[test]
    fn time_is_the_utc_clock_of_the_capture() {
        let epoch = SystemTime::UNIX_EPOCH;
        assert_eq!(format_time(epoch + Duration::from_secs(3_725)), "01:02:05");
        assert_eq!(format_time(epoch + Duration::from_secs(90_061)), "01:01:01");
        assert_eq!(format_time(epoch), "00:00:00");
    }
}
