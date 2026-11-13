use crate::app_state::{AppState, Dialog, InstanceTab, connection_for};
use crate::aws::drivers::ec2::InstanceInfo;
use crate::views::detail;
use crate::views::root::RootView;
use crate::views::theme;
use gpui_kit::{AnyElement, Context, Entity, FontWeight, div, prelude::*, px, rgb};

/// Column widths shared by the header strip and every row, so the table lines up.
const COLUMNS: &[(Option<f32>, &str)] = &[
    (Some(240.0), "NAME"),
    (Some(120.0), "STATE"),
    (Some(110.0), "TYPE"),
    (Some(110.0), "AZ"),
    (Some(130.0), "IPV4"),
    (Some(150.0), "CPU"),
    (Some(130.0), "LAUNCHED"),
    (None, ""),
];

fn act(state: &Entity<AppState>, cx: &mut Context<RootView>, verb: &'static str, id: String) {
    let conn = state.update(cx, |s, _| connection_for(s));
    let Some(conn) = conn else {
        state.update(cx, |s, cx| {
            s.action_error = Some("Connect to the emulator first".into());
            cx.notify();
        });
        return;
    };
    state.update(cx, |s, cx| {
        s.instance_menu = None;
        cx.notify();
    });
    detail::run(
        state,
        cx,
        async move {
            let c = conn.build_aws_config().await?;
            crate::views::root::run_lifecycle("ec2", verb, &c, &id).await
        },
        move |_, _| {},
        detail::Reload::Inventory,
    );
}

pub fn render(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let filter = state.input_filter.read(cx).value().to_lowercase();
    let tab = state.instance_tab;
    let visible: Vec<&InstanceInfo> = state
        .instances
        .iter()
        .filter(|i| tab.matches(&i.state))
        .filter(|i| {
            filter.is_empty()
                || i.id.to_lowercase().contains(&filter)
                || i.name.to_lowercase().contains(&filter)
                || i.instance_type.to_lowercase().contains(&filter)
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(toolbar(state, tab, cx))
        .when_some(state.instances_error.clone(), |d, err| {
            d.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::FAIL))
                    .child(format!("DescribeInstances failed: {err}")),
            )
        })
        .when_some(state.action_error.clone(), |d, err| {
            d.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::FAIL))
                    .child(format!("Error: {err}")),
            )
        })
        .when(state.action_busy, |d| {
            d.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::WARN))
                    .child("Working... the emulator advances instance states on a clock."),
            )
        })
        .child(
            if state.instances.is_empty() && state.instances_error.is_none() {
                theme::muted("No instances — launch one.")
            } else if visible.is_empty() {
                theme::muted("No instance matches this tab and filter.")
            } else {
                table(&visible, state, cx)
            },
        )
        .into_any()
}

/// The state tabs and the right-aligned filter field, one strip above the table.
fn toolbar(state: &AppState, tab: InstanceTab, cx: &mut Context<RootView>) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .children(InstanceTab::ALL.iter().map(|t| {
            let t = *t;
            let count = state
                .instances
                .iter()
                .filter(|i| t.matches(&i.state))
                .count();
            theme::tab(
                format!("tab-{}", t.label()),
                t.label().to_string(),
                Some(count),
                tab == t,
                cx,
                move |this, cx| {
                    this.state.update(cx, |s, cx| {
                        s.instance_tab = t;
                        s.instance_menu = None;
                        cx.notify();
                    });
                },
            )
        }))
        .child(
            div()
                .ml_auto()
                .w(px(280.0))
                .child(state.input_filter.clone()),
        )
        .into_any()
}

/// The table: a header strip and one row per visible instance, each row followed by its
/// opened action row when the menu is open.
fn table(instances: &[&InstanceInfo], state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    theme::card()
        .gap_1()
        .child(theme::table_head(COLUMNS))
        .children(instances.iter().map(|i| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(instance_row(i, state, cx))
                .when(state.instance_menu.as_deref() == Some(i.id.as_str()), |d| {
                    d.child(menu_row(&i.id, &i.state, cx))
                })
        }))
        .into_any()
}

/// The header's orange action for this page; the root shell calls it through
/// `root::primary_action` so the button sits at the top right like every other page's.
pub fn launch_action(cx: &mut Context<RootView>) -> AnyElement {
    crate::views::theme::button(
        "launch-instance",
        "+ Launch Instance",
        crate::views::theme::Kind::Primary,
        cx,
        |this, window, cx| detail::open_dialog(&this.state, window, cx, Dialog::LaunchInstance),
    )
}

fn instance_row(i: &InstanceInfo, state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let id = i.id.clone();
    let click_id = id.clone();
    let open_menu = state.instance_menu.as_deref() == Some(id.as_str());
    let name = if i.name.is_empty() {
        i.id.clone()
    } else {
        i.name.clone()
    };

    theme::row(format!("instance-{id}"), open_menu)
        .text_sm()
        .child(theme::cell(
            COLUMNS[0].0,
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                .child(div().text_color(rgb(theme::MUTED)).child(i.id.clone())),
        ))
        .child(theme::cell(
            COLUMNS[1].0,
            div()
                .flex()
                .child(theme::pill(i.state.clone(), state_color(&i.state))),
        ))
        .child(theme::cell(
            COLUMNS[2].0,
            div().child(i.instance_type.clone()),
        ))
        .child(theme::cell(COLUMNS[3].0, div().child(i.az.clone())))
        .child(theme::cell(
            COLUMNS[4].0,
            if i.ip.is_empty() {
                div().child(theme::unavailable())
            } else {
                div().child(i.ip.clone())
            },
        ))
        .child(theme::cell(
            COLUMNS[5].0,
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(theme::unavailable())
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme::MUTED))
                        .child("no cpu metrics"),
                ),
        ))
        .child(theme::cell(
            COLUMNS[6].0,
            match format_launched(&i.launched) {
                Some(when) => div().child(when),
                None => div().child(theme::unavailable()),
            },
        ))
        .child(theme::cell(
            COLUMNS[7].0,
            div().flex().justify_end().child(theme::button(
                format!("menu-{click_id}"),
                "...",
                theme::Kind::Neutral,
                cx,
                move |this, _, cx| {
                    this.state.update(cx, |s, cx| {
                        s.instance_menu = if s.instance_menu.as_deref() == Some(click_id.as_str()) {
                            None
                        } else {
                            Some(click_id.clone())
                        };
                        cx.notify();
                    });
                },
            )),
        ))
        .into_any()
}

/// Pill colour per state family: live work reads green, halted or dying work reads red,
/// and a terminated instance fades out.
fn state_color(state: &str) -> u32 {
    match state {
        "running" => theme::OK,
        // A state the instance is still moving through gets the amber, the way the console
        // colors it; only a state it sits in gets a hue of its own.
        "pending" | "stopping" => theme::WARN,
        "stopped" => theme::FAIL,
        _ => theme::FAINT,
    }
}

fn menu_row(id: &str, state: &str, cx: &mut Context<RootView>) -> AnyElement {
    let actions: Vec<(&str, &str)> = match state {
        "running" => vec![("stop", "Stop instance"), ("terminate", "Terminate")],
        "stopped" => vec![("start", "Start instance"), ("terminate", "Terminate")],
        "terminated" => vec![],
        _ => vec![("terminate", "Terminate")],
    };
    div()
        .flex()
        .flex_row()
        .gap_2()
        .px_3()
        .children(actions.into_iter().map(|(verb, label)| {
            let id = id.to_string();
            let kind = if verb == "terminate" {
                theme::Kind::Danger
            } else {
                theme::Kind::Neutral
            };
            theme::button(
                format!("act-{verb}-{id}"),
                label.to_string(),
                kind,
                cx,
                move |this, _, cx| act(&this.state, cx, verb, id.clone()),
            )
        }))
        .into_any()
}

/// The SDK prints launch times as RFC 3339; the table wants the mockup's "Mon DD, HH:MM".
/// Anything that does not parse to that shape is not rendered as a guess.
fn format_launched(rfc3339: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let b = rfc3339.as_bytes();
    if b.len() < 16
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || ![0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
    {
        return None;
    }
    let month: usize = (b[5] - b'0') as usize * 10 + (b[6] - b'0') as usize;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some(format!(
        "{} {}, {}:{}",
        MONTHS[month - 1],
        &rfc3339[8..10],
        &rfc3339[11..13],
        &rfc3339[14..16]
    ))
}
