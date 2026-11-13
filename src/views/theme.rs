//! The palette and the shared builders every page draws with.
//!
//! Colours are sampled from the console mockups: a navy ground, a darker sidebar, raised
//! cards, AWS orange for anything actionable, and four status hues the tables reuse. Page
//! files call these instead of spelling out colours, so the console stays one design when
//! the palette moves.

use crate::app_state::AppState;
use crate::views::root::RootView;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, SharedString, Stateful, TestSupportExt, Window, div,
    prelude::*, px, rgb,
};

pub const PAGE: u32 = 0x161c28;
pub const SIDEBAR: u32 = 0x10151d;
pub const CARD: u32 = 0x1b2334;
pub const RAISED: u32 = 0x262e3f;
pub const BORDER: u32 = 0x262e3f;
pub const NAV_ACTIVE: u32 = 0x2d2519;

pub const TEXT: u32 = 0xe8edf5;
pub const MUTED: u32 = 0x939daf;
pub const FAINT: u32 = 0x555d6c;
/// The oversized page title, drawn dark enough to read as furniture rather than content.
pub const GHOST: u32 = 0x2b3548;

pub const ACCENT: u32 = 0xff9900;
/// Text sitting on orange.
pub const ON_ACCENT: u32 = 0x14100a;

pub const OK: u32 = 0x3fb950;
pub const WARN: u32 = 0xe3b341;
pub const FAIL: u32 = 0xf87171;
pub const INFO: u32 = 0x6cb6ff;

/// The background a status pill puts behind its colour, so the pill reads as a tinted chip
/// and not as a block of saturated paint.
pub fn tint(color: u32) -> u32 {
    match color {
        OK => 0x1b2f2d,
        WARN => 0x2d2519,
        FAIL => 0x38222a,
        INFO => 0x1e2a3c,
        ACCENT => 0x2d2519,
        _ => RAISED,
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
        .bg(rgb(CARD))
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
                .text_3xl()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(GHOST))
                .child(title.into()),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(subtitle.into()),
        )
}

/// The small uppercase label above a group of rows.
pub fn section_label(text: impl Into<SharedString>) -> AnyElement {
    div()
        .text_xs()
        .text_color(rgb(FAINT))
        .child(text.into())
        .into_any()
}

pub fn muted(text: impl Into<SharedString>) -> AnyElement {
    div()
        .text_sm()
        .text_color(rgb(MUTED))
        .child(text.into())
        .into_any()
}

/// What a column shows when the emulator has no answer for it — a dash, never a guess.
pub fn unavailable() -> AnyElement {
    div().text_color(rgb(FAINT)).child("—").into_any()
}

pub fn dot(color: u32) -> AnyElement {
    div().size(px(8.0)).rounded_full().bg(rgb(color)).into_any()
}

/// The orange-tinted square the mockups put in front of a service or a stat.
pub fn tile(monogram: impl Into<SharedString>) -> AnyElement {
    div()
        .w(px(34.0))
        .h(px(34.0))
        .rounded_md()
        .bg(rgb(tint(ACCENT)))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(ACCENT))
        .child(monogram.into())
        .into_any()
}

/// The small rounded status chip: OK, WARN, FAIL, running, stopped.
pub fn pill(label: impl Into<SharedString>, color: u32) -> AnyElement {
    div()
        .px_2()
        .py_0p5()
        .rounded_md()
        .bg(rgb(tint(color)))
        .text_color(rgb(color))
        .child(label.into())
        .into_any()
}

/// The count badge that sits beside a title or a nav label.
pub fn count_badge(count: usize) -> AnyElement {
    div()
        .px_2()
        .rounded_md()
        .bg(rgb(RAISED))
        .text_color(rgb(MUTED))
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

/// The box every [`button`] chip is painted in. The vertical padding is deliberately the
/// small step: with it the chip's own height is [`CHIP_HEIGHT`], so a strip that frames the
/// chip at that height gets a chip that fits instead of one that overflows it.
fn paint(d: Stateful<Div>, kind: Kind) -> Stateful<Div> {
    let d = d
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_0p5()
        .rounded_md()
        .cursor_pointer();
    match kind {
        Kind::Primary => d.bg(rgb(ACCENT)).text_color(rgb(ON_ACCENT)),
        Kind::Ghost => d
            .bg(rgb(PAGE))
            .text_color(rgb(TEXT))
            .border_1()
            .border_color(rgb(RAISED)),
        Kind::Neutral => d.bg(rgb(RAISED)).text_color(rgb(TEXT)),
        Kind::Danger => d
            .bg(rgb(tint(FAIL)))
            .text_color(rgb(FAIL))
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

/// The tab strip: a label, an optional count, and the orange underline when it is the one
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
        .py_0p5()
        .rounded_md()
        .cursor_pointer()
        .border_1()
        .border_color(rgb(if active { ACCENT } else { BORDER }))
        .bg(rgb(if active { tint(ACCENT) } else { CARD }))
        .text_color(rgb(if active { ACCENT } else { MUTED }))
        .child(label.into())
        .when_some(count, |d, c| d.child(c.to_string()))
        .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
        .into_any()
}

/// A row of the left navigation: icon tile, label, count, and the amber tint when open.
pub fn nav_row(
    key: impl Into<SharedString>,
    monogram: impl Into<SharedString>,
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
        .gap_2()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(if selected { NAV_ACTIVE } else { PAGE }))
        .text_color(rgb(if selected { ACCENT } else { TEXT }))
        .child(div().w(px(20.0)).text_xs().child(monogram.into()))
        .child(div().flex_1().child(label.into()))
        .when_some(count, |d, c| {
            d.child(div().text_xs().text_color(rgb(MUTED)).child(c.to_string()))
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
        .px_3()
        .py_2()
        .text_xs()
        .text_color(rgb(FAINT))
        .children(columns.iter().map(|(w, label)| {
            let d = div().child(label.to_string());
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

/// A table row on the card background, with the hover raise the mockups show.
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
        .bg(rgb(if selected { RAISED } else { CARD }))
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
        .gap_2()
        .p_4()
        .rounded_lg()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgb(BORDER))
        .child(div().text_xs().text_color(rgb(FAINT)).child(label.clone()))
        .child(div().text_2xl().text_color(rgb(TEXT)).child(value))
        .child(div().text_sm().text_color(rgb(note_color)).child(note))
        .into_any()
}

/// The height a control row lays its chips out at.
pub const CHIP_HEIGHT: f32 = 34.0;

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
