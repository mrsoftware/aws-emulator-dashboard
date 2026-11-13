use crate::app_state::{AppState, ConnectionState};
use crate::aws::client::runtime;
use crate::views::root::RootView;
use crate::views::theme;
use gpui_kit::{
    AnyElement, Context, Entity, SharedString, Window, div, prelude::*, px, rgb,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum Action {
    Start,
    Stop,
    Restart,
}

pub fn start(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    run_action(state, cx, Action::Start);
}

pub fn stop(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    run_action(state, cx, Action::Stop);
}

pub fn restart(state: &Entity<AppState>, cx: &mut Context<RootView>) {
    run_action(state, cx, Action::Restart);
}

fn run_action(state: &Entity<AppState>, cx: &mut Context<RootView>, action: Action) {
    let (manager, settings, allowed) = {
        let s = state.read(cx);
        let allowed = !s.emulator.busy
            && match action {
                Action::Start => !s.emulator.running,
                Action::Stop => s.emulator.running,
                Action::Restart => true,
            };
        (s.manager.clone(), s.settings.clone(), allowed)
    };
    if !allowed {
        return;
    }
    state.update(cx, |s, cx| {
        s.emulator.busy = true;
        s.emulator.last_error = None;
        cx.notify();
    });
    let state_clone = state.clone();
    cx.spawn(async move |_, cx| {
        let result = runtime()
            .spawn(async move {
                match action {
                    Action::Start => manager.start(&settings).await.map(Some),
                    Action::Stop => manager.stop().await.map(|_| None),
                    Action::Restart => manager.restart(&settings).await.map(Some),
                }
            })
            .await;
        let result = match result {
            Ok(r) => r,
            Err(e) => Err(anyhow::anyhow!("emulator action failed: {e}")),
        };
        cx.update(|cx| {
            state_clone.update(cx, |s, cx| {
                s.emulator.busy = false;
                match result {
                    Ok(Some(info)) => {
                        tracing::info!(endpoint = %info.endpoint_url, "emulator started");
                        s.emulator.running = true;
                        s.emulator.started_at = Some(Instant::now());
                        s.connection =
                            ConnectionState::Connected(crate::app_state::ConnectionInfo {
                                endpoint: info.endpoint_url.clone(),
                                ca_path: info.ca_file.clone(),
                            });
                        s.emulator.startup = Some(info);
                    }
                    Ok(None) => {
                        tracing::info!("emulator stopped");
                        s.emulator.running = false;
                        s.emulator.started_at = None;
                        s.emulator.startup = None;
                        s.connection = ConnectionState::Disconnected;
                        s.resources.clear();
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "emulator action failed");
                        s.emulator.last_error = Some(format!("{e:#}"));
                    }
                }
                cx.notify();
            })
        });
    })
    .detach();
}

/// What the process manager answers for the emulator it runs: its state, which backend holds
/// it, and the addresses a client needs to reach it. Nothing here is measured anywhere else —
/// a field the manager has no answer for says so instead of showing a number.
pub fn render(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(process_card(state, cx))
        .child(options_card(state, cx))
        .when_some(state.emulator.last_error.clone(), |d, err| {
            d.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::FAIL))
                    .child(format!("Error: {err}")),
            )
        })
        .into_any()
}

fn process_card(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let emu = &state.emulator;
    let running = emu.running;
    let busy = emu.busy;

    let (badge_text, badge_color) = if busy {
        ("Working...", theme::WARN)
    } else if running {
        ("Running", theme::OK)
    } else {
        ("Stopped", theme::FAINT)
    };

    let backend = emu
        .startup
        .as_ref()
        .and_then(|i| i.backend)
        .map(|b| b.label())
        .unwrap_or("not started");
    let instance = emu
        .startup
        .as_ref()
        .map(|i| {
            if let Some(c) = &i.container {
                format!("container {c}")
            } else if let Some(p) = i.pid {
                format!("PID {p}")
            } else {
                String::new()
            }
        })
        .unwrap_or_default();
    let uptime = emu
        .started_at
        .map(|t| format_uptime(t.elapsed()))
        .unwrap_or_default();

    let port = state.settings.default_port;
    let creds_path = dirs::home_dir()
        .map(|h| {
            h.join(format!(".aws/emulator/{port}/emulator_credentials"))
                .display()
                .to_string()
        })
        .unwrap_or_default();
    let endpoint = emu
        .startup
        .as_ref()
        .map(|i| i.endpoint_url.clone())
        .unwrap_or_else(|| format!("https://127.0.0.1:{port}"));
    let ca_path = emu
        .startup
        .as_ref()
        .map(|i| i.ca_file.display().to_string())
        .unwrap_or_else(|| format!("~/.aws/emulator/{port}/ca.pem"));
    let health_url = emu
        .startup
        .as_ref()
        .map(|i| i.health_check_url.clone())
        .unwrap_or_else(|| format!("https://127.0.0.1:{port}/__admin/health"));

    // A grey chip is an action the manager would refuse, not a disabled control: pressing it
    // still goes through `run_action`, which is the single place that decides.
    let start_kind = if busy || running {
        theme::Kind::Neutral
    } else {
        theme::Kind::Primary
    };
    let stop_kind = if busy || !running {
        theme::Kind::Neutral
    } else {
        theme::Kind::Danger
    };
    let restart_kind = if busy {
        theme::Kind::Neutral
    } else {
        theme::Kind::Ghost
    };

    theme::card()
        .child(theme::section_label("Process"))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(theme::pill(badge_text, badge_color))
                .child(mono_note(backend))
                .when(!instance.is_empty(), |d| d.child(mono_note(instance.clone())))
                .when(!uptime.is_empty(), |d| {
                    d.child(mono_note(format!("up {uptime}")))
                }),
        )
        .child(info_row("Endpoint", &endpoint))
        .child(info_row("Health", &health_url))
        .child(info_row("Credentials", &creds_path))
        .child(info_row("CA certificate", &ca_path))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(theme::chip_box(theme::button(
                    "start",
                    "Start",
                    start_kind,
                    cx,
                    |this, _window, cx| start(&this.state, cx),
                )))
                .child(theme::chip_box(theme::button(
                    "stop",
                    "Stop",
                    stop_kind,
                    cx,
                    |this, _window, cx| stop(&this.state, cx),
                )))
                .child(theme::chip_box(theme::button(
                    "restart",
                    "Restart",
                    restart_kind,
                    cx,
                    |this, _window, cx| restart(&this.state, cx),
                ))),
        )
        .into_any()
}

/// The switches this dashboard writes to its own settings file. They take effect on the next
/// start, which is what the heading has always said.
fn options_card(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let port = state.settings.default_port;
    let idle = state.settings.idle_timeout_secs;
    let auth_on = state.settings.auth_enabled;
    let telemetry_on = state.settings.telemetry_enabled;

    theme::card()
        .child(theme::section_label("Options — apply on next start"))
        .child(setting_row(
            "Port",
            stepper(
                "port-dec",
                port.to_string(),
                "port-inc",
                |this, _window, cx| {
                    update_option(&this.state, cx, |s| {
                        s.settings.default_port =
                            s.settings.default_port.saturating_sub(1).max(1024);
                    })
                },
                |this, _window, cx| {
                    update_option(&this.state, cx, |s| {
                        s.settings.default_port = s.settings.default_port.saturating_add(1);
                    })
                },
                cx,
            ),
        ))
        .child(setting_row(
            "Auth (SigV4)",
            theme::chip_box(theme::tab(
                "auth-toggle",
                if auth_on { "on" } else { "off" },
                None,
                auth_on,
                cx,
                |this, cx| {
                    update_option(&this.state, cx, |s| {
                        s.settings.auth_enabled = !s.settings.auth_enabled;
                    })
                },
            )),
        ))
        .child(setting_row(
            "Telemetry",
            theme::chip_box(theme::tab(
                "telemetry-toggle",
                if telemetry_on { "on" } else { "off" },
                None,
                telemetry_on,
                cx,
                |this, cx| {
                    update_option(&this.state, cx, |s| {
                        s.settings.telemetry_enabled = !s.settings.telemetry_enabled;
                    })
                },
            )),
        ))
        .child(setting_row(
            "Idle timeout (s, 0 = off)",
            stepper(
                "idle-dec",
                idle.to_string(),
                "idle-inc",
                |this, _window, cx| {
                    update_option(&this.state, cx, |s| {
                        s.settings.idle_timeout_secs =
                            s.settings.idle_timeout_secs.saturating_sub(60);
                    })
                },
                |this, _window, cx| {
                    update_option(&this.state, cx, |s| {
                        s.settings.idle_timeout_secs =
                            (s.settings.idle_timeout_secs + 60).min(31_536_000);
                    })
                },
                cx,
            ),
        ))
        .into_any()
}

/// One option line: the name at a fixed width, its control beside it.
fn setting_row(label: &str, control: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(220.0))
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child(label.to_string()),
        )
        .child(control)
        .into_any()
}

/// The `-` value `+` control. Both buttons stay clickable at a clamp, exactly as before: the
/// clamp in the handler is what decides whether a press changes anything.
fn stepper(
    dec_id: &str,
    value: String,
    inc_id: &str,
    dec: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    inc: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    cx: &mut Context<RootView>,
) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(theme::chip_box(theme::button(
            dec_id,
            "-",
            theme::Kind::Neutral,
            cx,
            dec,
        )))
        .child(
            div()
                .w(px(72.0))
                .text_center()
                .font_family(theme::MONO_FONT)
                .text_size(px(12.0))
                .text_color(rgb(theme::TEXT_PRIMARY))
                .child(value),
        )
        .child(theme::chip_box(theme::button(
            inc_id,
            "+",
            theme::Kind::Neutral,
            cx,
            inc,
        )))
        .into_any()
}

/// An option change, written to disk and repainted — the same path every control on this page
/// has always taken, gathered in one place instead of six copies.
fn update_option(
    state: &Entity<AppState>,
    cx: &mut Context<RootView>,
    change: impl FnOnce(&mut AppState),
) {
    state.update(cx, |s, cx| {
        change(s);
        if let Err(e) = s.settings.save() {
            tracing::warn!(error = %e, "saving settings");
        }
        cx.notify();
    });
}

/// A label and the address or path the manager reported for it. An empty value is the
/// manager's silence, so it gets a dash rather than a guess.
fn info_row(label: &str, value: &str) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(140.0))
                .font_family(theme::MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(label.to_string()),
        )
        .child(if value.is_empty() {
            div().flex_1().child(theme::unavailable())
        } else {
            div()
                .flex_1()
                .font_family(theme::MONO_FONT)
                .text_size(px(12.0))
                .text_color(rgb(theme::TEXT_PRIMARY))
                .overflow_hidden()
                .child(value.to_string())
        })
        .into_any()
}

/// The secondary mono line the process card's status row rides on: backends, process ids and
/// durations are ids and clocks, not prose.
fn mono_note(text: impl Into<SharedString>) -> AnyElement {
    div()
        .font_family(theme::MONO_FONT)
        .text_size(px(12.0))
        .text_color(rgb(theme::TEXT_SECONDARY))
        .child(text.into())
        .into_any()
}

pub(crate) fn format_uptime(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Overview reads the same clock line, so the one unit that is not a rendering is the one
    /// worth pinning: the format switches at the minute and at the hour, never before.
    #[test]
    fn uptime_names_the_unit_the_clock_has_reached() {
        assert_eq!(format_uptime(Duration::from_secs(9)), "9s");
        assert_eq!(format_uptime(Duration::from_secs(59)), "59s");
        assert_eq!(format_uptime(Duration::from_secs(124)), "2m 4s");
        assert_eq!(format_uptime(Duration::from_secs(3_600)), "1h 0m");
        assert_eq!(format_uptime(Duration::from_secs(3_725)), "1h 2m");
    }
}
