use crate::app_state::{AppState, Dialog};
use crate::catalog::{self, Group, Service, Sort};
use crate::views::root::RootView;
use crate::views::theme;
use gpui_kit::{AnyElement, Context, FontWeight, div, prelude::*, px, rgb};

/// Tier A: services with a driver this build actually calls. Grows as drivers land.
/// The key is the signing name the emulator routes on, which is what `selected_service` holds.
/// The label comes from the driver that serves the page, so the sidebar cannot drift away
/// from the name the driver reports for itself.
pub const MANAGED: [(&str, &str); 11] = [
    ("s3", "S3"),
    ("dynamodb", "DynamoDB"),
    ("sqs", "SQS"),
    ("ec2", crate::aws::drivers::ec2::LABEL),
    ("iam", crate::aws::drivers::iam::LABEL),
    ("sns", crate::aws::drivers::sns::LABEL),
    ("rds", crate::aws::drivers::rds::LABEL),
    ("logs", crate::aws::drivers::cloudwatch_logs::LABEL),
    ("kinesis", crate::aws::drivers::kinesis::LABEL),
    (
        "secretsmanager",
        crate::aws::drivers::secrets_manager::LABEL,
    ),
    ("monitoring", crate::aws::drivers::cloudwatch::LABEL),
];

/// The create button a page offers, with the dialog it opens. A page with no entry here is
/// read-only: nothing in this build can provision that resource.
pub fn create_action(key: &str) -> Option<(&'static str, Dialog)> {
    match key {
        "s3" => Some(("New bucket", Dialog::CreateBucket)),
        "dynamodb" => Some(("New table", Dialog::CreateTable)),
        "sqs" => Some(("New queue", Dialog::CreateQueue)),
        "ec2" => Some(("Launch Instance", Dialog::LaunchInstance)),
        "iam" => Some(("New user", Dialog::CreateNamed)),
        "sns" => Some(("New topic", Dialog::CreateNamed)),
        "rds" => Some(("New DB instance", Dialog::CreateNamed)),
        "logs" => Some(("New log group", Dialog::CreateNamed)),
        "kinesis" => Some(("New stream", Dialog::CreateNamed)),
        "secretsmanager" => Some(("New secret", Dialog::CreateSecret)),
        _ => None,
    }
}

/// The noun a delete button or a duplicate-create message should use for this page.
pub fn resource_noun(key: &str) -> Option<&'static str> {
    match key {
        "s3" => Some("bucket"),
        "dynamodb" => Some("table"),
        "sqs" => Some("queue"),
        "ec2" => Some("instance"),
        "iam" => Some("user"),
        "sns" => Some("topic"),
        "rds" => Some("DB instance"),
        "logs" => Some("log group"),
        "kinesis" => Some("stream"),
        "secretsmanager" => Some("secret"),
        "monitoring" => Some("alarm"),
        _ => None,
    }
}

pub fn is_managed(signing_name: &str) -> bool {
    MANAGED.iter().any(|(key, _)| *key == signing_name)
}

/// How many services a grid card draws before it says "+N more". The mockup's cards are four
/// lines deep; the list view is what shows the rest.
const GRID_ROWS: usize = 4;

/// The width the mockup lays its category cards out at, in one wrapping row.
const CARD_WIDTH: f32 = 420.0;

/// The height of the toolbar strip and of the box each of its chips is laid out in.
const CHIP_HEIGHT: f32 = 34.0;

/// Column widths shared by a group's header strip and its rows, so the table lines up.
const LIST_COLUMNS: &[(Option<f32>, &str)] = &[
    (None, "SERVICE"),
    (Some(150.0), "SIGNING NAME"),
    (Some(190.0), "PROTOCOL"),
    (Some(60.0), "OPS"),
    (Some(60.0), "STATUS"),
];

/// The three tiers a service sits in, told apart by one dot: a page this dashboard drives, a
/// service the emulator answers that has no page here, and one the running build refuses with
/// `UnknownService`.
fn status_color(service: &Service) -> u32 {
    if is_managed(&service.signing_name) {
        theme::OK
    } else if service.registered {
        theme::INFO
    } else {
        theme::FAIL
    }
}

/// The two letters on a category tile. A multi-word category gives its initials, so
/// `Security & Identity` reads `SI`; a single word gives its first two letters.
fn monogram(name: &str) -> String {
    let initials: Vec<char> = name
        .split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .collect();
    let out = if initials.len() > 1 {
        format!("{}{}", initials[0], initials[1])
    } else {
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .take(2)
            .collect()
    };
    if out.is_empty() {
        "?".to_string()
    } else {
        out.to_uppercase()
    }
}

/// `Name` and `Operations` are what the chips read; the ids stay the short `sort-name` and
/// `sort-ops` that a test or a binding wants.
fn sort_id(sort: Sort) -> &'static str {
    match sort {
        Sort::Name => "name",
        Sort::Ops => "ops",
    }
}

pub fn render(state: &AppState, cx: &mut Context<RootView>) -> AnyElement {
    let query = state.input_search.read(cx).value();
    let query = query.trim();
    let groups = catalog::browse(query, state.catalog_sort);
    let hits = catalog::matched(query);
    let grid = state.catalog_grid;
    let selected = state.selected_service.clone();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(toolbar(state, grid, cx))
        // The header's subtitle already carries the totals and the region, so the body adds a
        // line only for what a search changes: how many services answer to it, or none.
        .when(!query.is_empty() && !groups.is_empty(), |d| {
            d.child(theme::muted(format!(
                "{hits} services match {query:?}, grouped below."
            )))
        })
        .when(groups.is_empty(), |d| {
            d.child(theme::muted(if query.is_empty() {
                "The baked catalog holds no services.".to_string()
            } else {
                format!("No service matches {query:?}.")
            }))
        })
        .child(if grid {
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_3()
                .children(groups.iter().map(|group| grid_card(group, &selected, cx)))
                .into_any()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .children(groups.iter().map(|group| list_card(group, &selected, cx)))
                .into_any()
        })
        .into_any()
}

/// The strip above the cards: the search field takes the room, the sort chips and the
/// grid/list toggle sit at its right.
fn toolbar(state: &AppState, grid: bool, cx: &mut Context<RootView>) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .gap_2()
        // The field paints its own box; the frame only gives it the room and the mono face
        // the design asks of whatever is typed into it.
        .child(
            div()
                .flex_1()
                .font_family(theme::MONO_FONT)
                .child(state.input_search.clone()),
        )
        .children(Sort::ALL.iter().map(|sort| {
            let sort = *sort;
            chip_frame(theme::tab(
                format!("sort-{}", sort_id(sort)),
                sort.label().to_string(),
                None,
                state.catalog_sort == sort,
                cx,
                move |this, cx| {
                    this.state.update(cx, |s, cx| {
                        s.catalog_sort = sort;
                        cx.notify();
                    });
                },
            ))
        }))
        .child(chip_frame(theme::button(
            "view-mode",
            if grid { "Grid view" } else { "List view" },
            theme::Kind::Neutral,
            cx,
            move |this, _window, cx| {
                this.state.update(cx, |s, cx| {
                    s.catalog_grid = !s.catalog_grid;
                    cx.notify();
                });
            },
        )))
        .into_any()
}

/// The frame a toolbar chip is laid out in. `theme::tab` and `theme::button` hand back a
/// finished element that asks for no height, and a child that asks for none in this strip
/// takes the whole cross axis of the row, so the chip is given a box of its own here. The
/// frame also carries the mono face the design gives the toolbar's labels.
fn chip_frame(chip: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .font_family(theme::MONO_FONT)
        .text_size(px(12.0))
        .child(chip)
        .into_any()
}

/// The line every card opens with: the group's mark, its name, and how many services the
/// catalog puts in it.
fn card_head(group: &Group) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(theme::icon(
            theme::group_icon(&group.name),
            15.0,
            theme::TEXT_SECONDARY,
        ))
        .child(
            div()
                .flex_1()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(theme::TEXT_PRIMARY))
                .child(group.name.clone()),
        )
        .child(
            div()
                .font_family(theme::MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(group.total.to_string()),
        )
        .into_any()
}

fn grid_card(group: &Group, selected: &str, cx: &mut Context<RootView>) -> AnyElement {
    let hidden = group.items.len().saturating_sub(GRID_ROWS);
    theme::card()
        .w(px(CARD_WIDTH))
        .gap_1()
        .child(card_head(group))
        .children(
            group
                .items
                .iter()
                .take(GRID_ROWS)
                .map(|service| grid_row(service, selected, cx)),
        )
        .when(hidden > 0, |d| {
            // Not a link: a card cannot page through a group, the list view is what shows the
            // rest, and naming that is the honest version of the mockup's "+N more".
            d.child(
                div()
                    .font_family(theme::MONO_FONT)
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED))
                    .child(format!("+{hidden} more — switch to list view to see all")),
            )
        })
        .into_any()
}

fn list_card(group: &Group, selected: &str, cx: &mut Context<RootView>) -> AnyElement {
    theme::card()
        .gap_1()
        .child(card_head(group))
        .child(theme::table_head(LIST_COLUMNS))
        .children(
            group
                .items
                .iter()
                .map(|service| list_row(service, selected, cx)),
        )
        .into_any()
}

/// One service inside a card: its name, its operation count, and the tier dot.
fn grid_row(service: &Service, selected: &str, cx: &mut Context<RootView>) -> AnyElement {
    let key = service.signing_name.clone();
    theme::row(format!("svc-{key}"), selected == key.as_str())
        .hover(|style| style.bg(rgb(theme::RAISED)))
        .child(
            div()
                .flex_1()
                .text_size(px(13.0))
                .text_color(rgb(theme::TEXT_PRIMARY))
                .child(service.title.clone()),
        )
        .child(
            div()
                .font_family(theme::MONO_FONT)
                .text_size(px(11.0))
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(service.ops.to_string()),
        )
        .child(theme::dot(status_color(service)))
        .on_click(cx.listener(move |this, _, _, cx| {
            // Through `open_page`, so a switch drops the row selection, the detail panel and
            // any half-typed dialog, exactly as the sidebar's rows do.
            this.state.update(cx, |s, cx| s.open_page(&key, cx));
        }))
        .into_any()
}

/// One service across the whole page, with the two fields a card has no room for.
fn list_row(service: &Service, selected: &str, cx: &mut Context<RootView>) -> AnyElement {
    let key = service.signing_name.clone();
    let signing_name = service.signing_name.clone();
    let ops = service.ops.to_string();
    let color = status_color(service);
    theme::row(format!("svc-{key}"), selected == key.as_str())
        .hover(|style| style.bg(rgb(theme::RAISED)))
        .child(theme::cell(
            LIST_COLUMNS[0].0,
            div()
                .text_size(px(13.0))
                .text_color(rgb(theme::TEXT_PRIMARY))
                .child(service.title.clone()),
        ))
        .child(theme::cell(
            LIST_COLUMNS[1].0,
            div()
                .font_family(theme::MONO_FONT)
                .text_size(px(12.0))
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(signing_name),
        ))
        .child(theme::cell(
            LIST_COLUMNS[2].0,
            match &service.protocol {
                Some(protocol) => div()
                    .font_family(theme::MONO_FONT)
                    .text_size(px(12.0))
                    .text_color(rgb(theme::TEXT_SECONDARY))
                    .child(protocol.clone()),
                // A service the model gives no protocol for is a real absence, so it gets
                // the dash, not a word that pretends to be an answer.
                None => div().child(theme::unavailable()),
            },
        ))
        .child(theme::cell(
            LIST_COLUMNS[3].0,
            div()
                .font_family(theme::MONO_FONT)
                .text_size(px(12.0))
                .text_color(rgb(theme::TEXT_SECONDARY))
                .child(ops),
        ))
        .child(theme::cell(
            LIST_COLUMNS[4].0,
            div().flex().justify_center().child(theme::dot(color)),
        ))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.state.update(cx, |s, cx| s.open_page(&key, cx));
        }))
        .into_any()
}

/// The Tier B landing panel: what the emulator models and what the dashboard can do with it.
pub fn render_focus(state: &AppState) -> Option<AnyElement> {
    let service = catalog::find(&state.selected_service)?;
    let managed = is_managed(&service.signing_name);
    let module = catalog::driver_module(&service.sdk_id);
    // The verdict chip says which of the three tiers this service is in; the line under the
    // fields says what the dashboard can and cannot do about it. Neither one is friendlier
    // than the truth.
    let (verdict, color) = if !service.registered {
        ("not in build", theme::FAIL)
    } else if managed {
        ("managed", theme::OK)
    } else {
        ("served, no page", theme::INFO)
    };
    let explanation = if !service.registered {
        "Not emulated: the running build answers UnknownService for this signing name. \
         Nothing the dashboard can do will make it answer."
            .to_string()
    } else if managed {
        "Managed: this page lists live resources.".to_string()
    } else {
        format!(
            "Not manageable yet: a page here needs the `{}` crate and a `src/aws/drivers/{module}.rs` \
             driver. Nothing is faked in its place — start the proxy on Traffic and send requests \
             yourself to see them land.",
            catalog::sdk_crate(&service.sdk_id)
        )
    };

    Some(
        theme::card()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(theme::tile(monogram(&service.title)))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child(service.title.clone()),
                    )
                    .child(theme::pill(verdict, color))
                    .child(
                        div()
                            .ml_auto()
                            .text_sm()
                            .text_color(rgb(theme::MUTED))
                            .child(format!(
                                "{} · {} operations",
                                service.signing_name, service.ops
                            )),
                    ),
            )
            .child(labelled("Category", service.category.clone()))
            .child(labelled(
                "Protocol",
                service.protocol.clone().unwrap_or("no protocol".into()),
            ))
            .child(labelled(
                "API version",
                service.version.clone().unwrap_or_else(|| "unknown".into()),
            ))
            .child(labelled("Emulator crate", service.crate_name.clone()))
            .child(labelled(
                "SDK crate",
                if managed {
                    catalog::sdk_crate(&service.sdk_id)
                } else {
                    format!("{} (not a dependency)", catalog::sdk_crate(&service.sdk_id))
                },
            ))
            .child(div().text_sm().text_color(rgb(color)).child(explanation))
            .into_any(),
    )
}

fn labelled(label: &str, value: String) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .gap_2()
        .child(
            div()
                .w(px(120.0))
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child(label.to_string()),
        )
        .child(div().text_sm().child(value))
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_services_are_all_registered_in_the_running_build() {
        for (key, label) in MANAGED {
            let service = catalog::find(key)
                .unwrap_or_else(|| panic!("{key} must be in the baked catalog for {label}"));
            assert!(service.registered, "{key} is managed but not served");
        }
    }
}
