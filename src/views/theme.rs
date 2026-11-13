//! The palette and the shared builders every page draws with.
//!
//! Tokens come from the Stratus Console `.pen` design (`variables` block): a near-black
//! navy ground, a raised card layer, a hairline border, a near-white accent, and four
//! status hues the tables reuse. Page files call these instead of spelling out colours,
//! so the console stays one design when the palette moves.

use crate::app_state::AppState;
use crate::views::root::RootView;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, SharedString, Stateful, TestSupportExt, Window, div,
    prelude::*, px, rgb, rgba,
};

// Background tokens — the .pen `variables` block, verbatim.
pub const PAGE: u32 = 0x0C111D;
pub const SIDEBAR: u32 = 0x101828;
pub const SURFACE: u32 = 0x111A2C;
pub const RAISED: u32 = 0x182230;

// Border
pub const BORDER: u32 = 0x232F42;

// Text
pub const TEXT: u32 = TEXT_PRIMARY;
pub const TEXT_PRIMARY: u32 = 0xF5F7FA;
pub const TEXT_SECONDARY: u32 = 0x98A2B3;
pub const MUTED: u32 = TEXT_SECONDARY;
pub const FAINT: u32 = 0x667085;

// Accents — the accent is the near-white action colour; `ON_ACCENT` is what sits on it.
pub const ACCENT: u32 = 0xF7F9FC;
pub const ON_ACCENT: u32 = 0x0D1117;

// Status colors
pub const OK: u32 = 0x17B26A;
pub const WARN: u32 = 0xF79009;
pub const FAIL: u32 = 0xF04438;
pub const INFO: u32 = 0x7FA8D9;

/// A status colour at the .pen's tint strength (0x21, ~13% alpha), as a 0xRRGGBBAA value
/// [`rgba`] accepts.
pub fn dimmed(color: u32) -> u32 {
    (color << 8) | 0x21
}

/// The accent at its .pen strength (8% white) — nav selection, icon wraps.
pub const ACCENT_DIM_RGBA: u32 = 0xF7F9FC14;

/// The two families the design alternates between. UI text rides the window default
/// (Inter when installed), labels and ids ride the mono.
pub const UI_FONT: &str = "Inter";
pub const MONO_FONT: &str = "IBM Plex Mono";

/// The small uppercase mono label with the design's 0.8px tracking.
pub fn mono_label(text: impl Into<SharedString>) -> AnyElement {
    div()
        .font_family(MONO_FONT)
        .text_size(px(11.0))
        .text_color(rgb(TEXT_SECONDARY))
        .child(text.into())
        .into_any()
}

/// A lucide icon drawn at `size` in `color`, straight out of the bundled catalog.
pub fn icon(name: gpui_kit::assets::IconName, size: f32, color: u32) -> AnyElement {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .child(name)
        .into_any()
}

/// The lucide mark a service group rides in the sidebar and on the services grid. The
/// design maps one icon per group family; an unclassified group gets the generic box.
pub fn group_icon(name: &str) -> gpui_kit::assets::IconName {
    match name {
        "Compute" => gpui_kit::assets::IconName::Cpu,
        "Storage" => gpui_kit::assets::IconName::HardDrive,
        "Database" => gpui_kit::assets::IconName::Database,
        "Networking" => gpui_kit::assets::IconName::Network,
        "Security & Identity" => gpui_kit::assets::IconName::Shield,
        "Analytics" => gpui_kit::assets::IconName::Activity,
        "Machine Learning" => gpui_kit::assets::IconName::Brain,
        "Developer Tools" => gpui_kit::assets::IconName::Code,
        "Management" => gpui_kit::assets::IconName::Settings,
        "Integration" => gpui_kit::assets::IconName::Workflow,
        _ => gpui_kit::assets::IconName::Box,
    }
}

/// A raised panel with the card background and a hairline border.
pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_lg()
        .bg(rgb(RAISED))
        .border_1()
        .border_color(rgb(BORDER))
}

/// A card with the mockup's section heading built in.
pub fn panel(title: impl Into<SharedString>) -> Div {
    card().child(section_label(title))
}

pub fn page_header(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(22.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_PRIMARY))
                .child(title.into()),
        )
        .child(
            div()
                .font_family(MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(TEXT_SECONDARY))
                .child(subtitle.into()),
        )
}

/// The small uppercase label above a group of rows.
pub fn section_label(text: impl Into<SharedString>) -> AnyElement {
    mono_label(text)
}

pub fn muted(text: impl Into<SharedString>) -> AnyElement {
    div()
        .text_sm()
        .text_color(rgb(TEXT_SECONDARY))
        .child(text.into())
        .into_any()
}

/// What a column shows when the emulator has no answer for it — a dash, never a guess.
pub fn unavailable() -> AnyElement {
    div().text_color(rgb(FAINT)).child("—").into_any()
}

pub fn dot(color: u32) -> AnyElement {
    div().size(px(6.0)).rounded_full().bg(rgb(color)).into_any()
}

/// The white-tinted square a service mark sits in.
pub fn tile_monogram(monogram: impl Into<SharedString>) -> AnyElement {
    div()
        .w(px(34.0))
        .h(px(34.0))
        .rounded_md()
        .bg(rgba(ACCENT_DIM_RGBA))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(ACCENT))
        .child(monogram.into())
        .into_any()
}

/// The small rounded status pill: a dot, a mono label, and the status colour at tint
/// strength behind them.
pub fn pill(label: impl Into<SharedString>, color: u32) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_0p5()
        .rounded_full()
        .bg(rgba(dimmed(color)))
        .child(dot(color))
        .child(
            div()
                .font_family(MONO_FONT)
                .text_size(px(10.0))
                .text_color(rgb(color))
                .child(label.into()),
        )
        .into_any()
}

/// The count badge that sits beside a title or a nav label.
pub fn count_badge(count: usize) -> AnyElement {
    div()
        .px_2()
        .rounded_md()
        .bg(rgb(RAISED))
        .font_family(MONO_FONT)
        .text_size(px(11.0))
        .text_color(rgb(TEXT_SECONDARY))
        .child(count.to_string())
        .into_any()
}

#[derive(Clone, Copy)]
pub enum Kind {
    /// Orange: the one action a page wants taken.
    Primary,
    /// Bordered and transparent: Refresh, Cancel.
    Ghost,
    /// Raised grey: a secondary action.
    Neutral,
    /// Red: destroys something.
    Danger,
}

/// The box every [`button`] chip is painted in.
fn paint(d: Stateful<Div>, kind: Kind) -> Stateful<Div> {
    let d = d
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer();
    match kind {
        Kind::Primary => d.bg(rgb(ACCENT)).text_color(rgb(ON_ACCENT)),
        Kind::Ghost => d
            .text_color(rgb(TEXT_PRIMARY))
            .border_1()
            .border_color(rgb(BORDER)),
        Kind::Neutral => d
            .bg(rgb(RAISED))
            .border_1()
            .border_color(rgb(BORDER))
            .text_color(rgb(TEXT_PRIMARY)),
        Kind::Danger => d
            .bg(rgb(FAIL))
            .text_color(rgb(PAGE))
            .border_1()
            .border_color(rgb(FAIL)),
    }
}

/// A clickable chip with the console's styling and `test_support` already attached, so a
/// headless test can find it by id.
pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    kind: Kind,
    cx: &mut Context<RootView>,
    on_click: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
) -> AnyElement {
    let id = id.into();
    paint(div().id(id.clone()).test_support(), kind)
        .child(label.into())
        .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
        .into_any()
}

/// The tile the mockups put in front of a service. Backward compatibility alias.
pub fn tile(monogram: impl Into<SharedString>) -> AnyElement {
    tile_monogram(monogram)
}

/// The tab strip: a label, an optional count, and the accent border when it is the one
/// page showing.
pub fn tab(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    count: Option<usize>,
    active: bool,
    cx: &mut Context<RootView>,
    on_click: impl Fn(&mut RootView, &mut Context<RootView>) + 'static,
) -> AnyElement {
    let id = id.into();
    div()
        .id(id.clone())
        .test_support()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .border_1()
        .border_color(rgb(if active { ACCENT } else { BORDER }))
        .bg(rgb(if active { RAISED } else { SIDEBAR }))
        .text_color(rgb(if active { TEXT_PRIMARY } else { TEXT_SECONDARY }))
        .child(label.into())
        .when_some(count, |d, c| d.child(c.to_string()))
        .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
        .into_any()
}

/// A row of the left navigation: leading icon, label, count, and the raised tint when
/// open.
pub fn nav_row(
    key: impl Into<SharedString>,
    icon: AnyElement,
    label: impl Into<SharedString>,
    count: Option<usize>,
    selected: bool,
    cx: &mut Context<RootView>,
    on_click: impl Fn(&mut AppState, &mut Context<AppState>) + 'static,
) -> AnyElement {
    let key = key.into();
    div()
        .id(key)
        .test_support()
        .flex()
        .items_center()
        .gap_2p5()
        .px_2p5()
        .py_1p5()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if selected { RAISED } else { SIDEBAR }))
        .text_color(rgb(if selected {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        }))
        .child(div().w(px(20.0)).flex().justify_center().child(icon))
        .child(div().flex_1().child(label.into()))
        .when_some(count, |d, c| {
            d.child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(px(11.0))
                    .text_color(rgb(FAINT))
                    .child(c.to_string()),
            )
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.state.update(cx, |s, cx| on_click(s, cx));
        }))
        .into_any()
}

/// The table column header strip. Each column is a width and a label; a `None` width lets
/// the last column take whatever is left.
pub fn table_head(columns: &[(Option<f32>, &str)]) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(36.0))
        .px_3()
        .bg(rgb(RAISED))
        .border_b_1()
        .border_color(rgb(BORDER))
        .text_size(px(10.0))
        .text_color(rgb(TEXT_SECONDARY))
        .children(columns.iter().map(|(w, label)| {
            let d = div().font_family(MONO_FONT).child(label.to_string());
            match w {
                Some(w) => d.w(px(*w)),
                None => d.flex_1(),
            }
        }))
        .into_any()
}

/// One cell of a table row, sized the way [`table_head`] sized its header.
pub fn cell(width: Option<f32>, body: impl Into<Div>) -> Div {
    let d = body.into();
    match width {
        Some(w) => d.w(px(w)),
        None => d.flex_1(),
    }
}

/// A table row on the surface background, with the hover raise the mockups show.
pub fn row(id: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    div()
        .id(id.into())
        .test_support()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if selected { RAISED } else { SURFACE }))
}

/// The four-up metric card at the top of Overview.
pub fn stat(
    id: &str,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    note: impl Into<SharedString>,
    note_color: u32,
) -> AnyElement {
    let label: SharedString = label.into();
    let value: SharedString = value.into();
    let note: SharedString = note.into();
    div()
        .id(SharedString::from(format!("stat-{id}")))
        .test_support()
        .aria_label(format!("{label}: {value} — {note}"))
        .flex_1()
        .min_w(px(220.0))
        .flex()
        .flex_col()
        .gap_2p5()
        .p_4()
        .rounded_lg()
        .bg(rgb(RAISED))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .font_family(MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(TEXT_SECONDARY))
                .child(label.clone()),
        )
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(26.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_PRIMARY))
                .child(value),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .font_family(MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(note_color))
                .child(note),
        )
        .into_any()
}

/// The height a control row lays its chips out at.
pub const CHIP_HEIGHT: f32 = 36.0;

/// The box a [`button`] or a [`tab`] goes in inside a strip. Both hand back a finished chip
/// that asks for no height of its own, and a child that asks for none takes the whole cross
/// axis of the row it sits in, so the strip paints as a column of blocks instead of a row of
/// chips. The frame gives the chip a height of its own to sit inside.
pub fn chip_box(chip: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .child(chip)
        .into_any()
}

/// The background a status pill puts behind its colour — the status hue at tint strength.
pub fn tint(color: u32) -> u32 {
    dimmed(color)
}
