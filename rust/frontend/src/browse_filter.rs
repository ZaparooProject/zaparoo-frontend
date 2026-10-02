// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Games browse filter: the "Filter" page of the View menu. One panel
// swaps between a page of categories and a page of one category's values
// (a modal never opens another modal). The choice is saved per system and
// applied to the list once, when the picker closes. The category and
// value rules live in `zaparoo_app::browse_filter`.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::browse_filter::{self as rules, Category, FilterTag, Group};
use zaparoo_core::client::ClientError;
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{MediaTagsParams, MediaTagsResult};

use crate::router::{lock, menu_row, menu_row_full, menu_row_keyed, Ctx, ListContext, Shared};
use crate::App;

/// Rows the list picker shows at once: its viewport is 60% of the screen
/// height over 7% rows with a 1% gap (`ListPickerModal`), whatever the
/// resolution.
const PICKER_ROWS: usize = 7;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Load {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed,
}

/// The tag list for the system on screen, and what the filter was when the
/// View menu opened.
#[derive(Debug, Default)]
pub struct Model {
    /// Bumped per fetch so a late answer cannot fill a newer one.
    seq: u64,
    system_id: String,
    groups: Vec<Group>,
    load: Load,
    baseline: Vec<String>,
}

/// The active filter for the system on screen, as `media.browse` tags.
pub(crate) fn active_tags(shared: &Shared) -> Vec<String> {
    rules::normalize(shared.persist.games.filter_for(&shared.games.system_id))
}

/// Whether the active filter requires a user tag (Lists), which Core
/// answers with hidden games included.
pub(crate) fn shows_hidden(shared: &Shared) -> bool {
    active_tags(shared).iter().any(|t| t.starts_with("user:"))
}

/// The groups of the system on screen; empty before they have loaded or
/// while they belong to another system.
fn groups(shared: &Shared) -> &[Group] {
    if shared.filter.system_id == shared.games.system_id {
        &shared.filter.groups
    } else {
        &[]
    }
}

fn label_for(groups: &[Group], tag: &str) -> Option<String> {
    let (tag_type, value) = tag.split_once(':')?;
    groups
        .iter()
        .filter(|g| g.category.tag_type() == tag_type)
        .flat_map(|g| g.values.iter())
        .find(|v| v.value == value)
        .map(|v| v.label.clone())
}

/// "Platformer +1": the header and View menu cue. None without a filter.
pub(crate) fn summary(shared: &Shared) -> Option<String> {
    let tags = active_tags(shared);
    rules::summary(&tags, |tag| label_for(groups(shared), tag))
}

/// The View menu's filter rows: the page entry, and a way to drop an active
/// filter without opening it.
pub(crate) fn view_rows(shared: &Shared) -> Vec<crate::MenuEntry> {
    let mut rows = vec![menu_row_keyed(
        "filter",
        "filter",
        &summary(shared).unwrap_or_default(),
    )];
    if !active_tags(shared).is_empty() {
        rows.push(menu_row("filter_clear"));
    }
    rows
}

/// Called as the View menu opens: remember what the filter is now and
/// fetch the system's tags, so the pages are ready by the time they open.
pub(crate) fn begin(ctx: &Ctx, app: &App) {
    let (system_id, seq) = {
        let mut shared = lock(&ctx.shared);
        let system_id = shared.games.system_id.clone();
        shared.filter.baseline = shared.persist.games.filter_for(&system_id).to_vec();
        if shared.filter.system_id != system_id {
            shared.filter.groups.clear();
            shared.filter.system_id.clone_from(&system_id);
        }
        shared.filter.seq += 1;
        // A list from an earlier visit stays on screen while this refreshes.
        shared.filter.load = if shared.filter.groups.is_empty() {
            Load::Loading
        } else {
            Load::Ready
        };
        (system_id, shared.filter.seq)
    };
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let outcome = client
            .media_tags(MediaTagsParams {
                systems: vec![system_id],
            })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| landed(&ctx2, &app, seq, outcome));
    });
}

fn landed(ctx: &Ctx, app: &App, seq: u64, outcome: Result<MediaTagsResult, ClientError>) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.filter.seq != seq {
            return;
        }
        match outcome {
            Ok(result) => {
                let tags: Vec<FilterTag> = result
                    .tags
                    .iter()
                    .map(|t| FilterTag {
                        tag_type: t.tag_type.clone(),
                        value: t.tag.clone(),
                        label: t.label.clone(),
                        count: t.count,
                    })
                    .collect();
                shared.filter.groups = rules::group(&tags);
                shared.filter.load = Load::Ready;
            }
            Err(e) => {
                tracing::warn!(error = %e.message, "media.tags failed");
                if shared.filter.groups.is_empty() {
                    shared.filter.load = Load::Failed;
                }
            }
        }
    }
    // The categories page may be open on its loading row.
    let on_categories = matches!(
        lock(&ctx.shared).list_context,
        ListContext::FilterCategories
    );
    if on_categories && app.global::<crate::Overlays>().get_list_open() {
        let rows = category_rows(&lock(&ctx.shared));
        app.global::<crate::Overlays>()
            .set_list_entries(ModelRc::new(VecModel::from(rows)));
    }
}

/// The value selected for `category`, as its display label.
fn selected_label(shared: &Shared, category: Category) -> String {
    let tags = active_tags(shared);
    let Some(value) = rules::selected(&tags, category) else {
        return String::new();
    };
    let tag = format!("{}:{value}", category.tag_type());
    rules::display_label(&label_for(groups(shared), &tag).unwrap_or_default(), value)
}

fn category_rows(shared: &Shared) -> Vec<crate::MenuEntry> {
    let groups = groups(shared);
    if groups.is_empty() {
        let key = match shared.filter.load {
            Load::Failed => "filter:failed",
            Load::Idle | Load::Loading => "filter:loading",
            Load::Ready => "filter:none",
        };
        return vec![menu_row_full("none", key, "", false, "")];
    }
    let mut rows: Vec<crate::MenuEntry> = groups
        .iter()
        .map(|g| {
            let id = g.category.id();
            menu_row_keyed(
                &format!("cat:{id}"),
                &format!("filter_cat:{id}"),
                &selected_label(shared, g.category),
            )
        })
        .collect();
    if !active_tags(shared).is_empty() {
        rows.push(menu_row("filter_clear"));
    }
    // Last, so one Up from the top row (the list wraps) finishes the form.
    rows.push(menu_row("filter_apply"));
    rows
}

/// `Any`, then every value of `category`; the second half of the pair is
/// the index of the current choice.
fn value_rows(shared: &Shared, category: Category) -> (Vec<crate::MenuEntry>, usize) {
    let tags = active_tags(shared);
    let current = rules::selected(&tags, category);
    let mut rows = vec![menu_row_keyed("any", "filter:any", "")];
    let mut index = 0;
    if let Some(group) = groups(shared).iter().find(|g| g.category == category) {
        for value in &group.values {
            if current == Some(value.value.as_str()) {
                index = rows.len();
            }
            let mut row = menu_row_full(&format!("v:{}", value.value), "", &value.label, true, "");
            if value.count > 0 {
                row.detail = SharedString::from(value.count.to_string());
            }
            rows.push(row);
        }
    }
    (rows, index)
}

fn present(
    ctx: &Ctx,
    app: &App,
    context: ListContext,
    title: &str,
    rows: Vec<crate::MenuEntry>,
    index: usize,
) {
    crate::router::present_list(ctx, app, context, title, rows);
    app.global::<crate::Overlays>()
        .set_list_index(i32::try_from(index).unwrap_or(0));
}

/// The categories page, with `focus` pre-selected (the category just left).
fn present_categories(ctx: &Ctx, app: &App, focus: Option<Category>) {
    let (rows, index) = {
        let shared = lock(&ctx.shared);
        let rows = category_rows(&shared);
        let index = focus
            .and_then(|c| {
                let id = format!("cat:{}", c.id());
                rows.iter().position(|r| r.id == id.as_str())
            })
            .unwrap_or(0);
        (rows, index)
    };
    present(
        ctx,
        app,
        ListContext::FilterCategories,
        "title:filter",
        rows,
        index,
    );
}

fn present_values(ctx: &Ctx, app: &App, category: Category) {
    let (rows, index) = value_rows(&lock(&ctx.shared), category);
    present(
        ctx,
        app,
        ListContext::FilterValues(category),
        &format!("title:filter_cat:{}", category.id()),
        rows,
        index,
    );
}

/// The View menu's Filter row.
pub(crate) fn open(ctx: &Ctx, app: &App) {
    present_categories(ctx, app, None);
}

fn set_selection(ctx: &Ctx, category: Category, value: Option<&str>) {
    let mut shared = lock(&ctx.shared);
    let system_id = shared.games.system_id.clone();
    let next = rules::with_selection(shared.persist.games.filter_for(&system_id), category, value);
    shared.persist.games.set_filter(&system_id, next);
}

fn clear_selection(ctx: &Ctx) {
    let mut shared = lock(&ctx.shared);
    let system_id = shared.games.system_id.clone();
    shared.persist.games.set_filter(&system_id, Vec::new());
}

/// The View menu's Clear filter row: the list closes with the menu.
pub(crate) fn clear(ctx: &Ctx, app: &App) {
    clear_selection(ctx);
    apply(ctx, app);
}

/// Refill the list when the filter differs from the one the View menu opened
/// with. One refill however many picks were made.
pub(crate) fn apply(ctx: &Ctx, app: &App) {
    let changed = {
        let mut shared = lock(&ctx.shared);
        let system_id = shared.games.system_id.clone();
        let now = shared.persist.games.filter_for(&system_id).to_vec();
        let changed = now != shared.filter.baseline;
        shared.filter.baseline = now;
        changed
    };
    if changed {
        crate::games::refilter(ctx, app);
    }
}

/// Accept on a filter page. The panel stays open and swaps pages.
pub(crate) fn accept(ctx: &Ctx, app: &App, context: &ListContext, id: &str) {
    match context {
        ListContext::FilterCategories => {
            if id == "filter_apply" {
                close(ctx, app);
            } else if id == "filter_clear" {
                clear_selection(ctx);
                close(ctx, app);
            } else if let Some(category) = id.strip_prefix("cat:").and_then(Category::from_id) {
                present_values(ctx, app, category);
            }
        }
        ListContext::FilterValues(category) => {
            let value = id.strip_prefix("v:");
            if id == "any" || value.is_some() {
                set_selection(ctx, *category, value);
                present_categories(ctx, app, Some(*category));
            }
        }
        _ => {}
    }
}

/// Back on a filter page: from a category's values to the categories, from
/// the categories out of the picker without applying. `close_all` (the View
/// button) leaves from either page.
pub(crate) fn back(ctx: &Ctx, app: &App, context: &ListContext, close_all: bool) {
    match context {
        ListContext::FilterValues(category) if !close_all => {
            present_categories(ctx, app, Some(*category));
        }
        _ => discard(ctx, app),
    }
}

/// Leave the picker with the filter as it was when the View menu opened.
fn discard(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let system_id = shared.games.system_id.clone();
        let baseline = shared.filter.baseline.clone();
        shared.persist.games.set_filter(&system_id, baseline);
    }
    app.global::<crate::Overlays>().set_list_open(false);
}

fn close(ctx: &Ctx, app: &App) {
    app.global::<crate::Overlays>().set_list_open(false);
    apply(ctx, app);
}

/// Left and Right page a long list of values.
pub(crate) fn page(app: &App, action: &str) {
    let overlays = app.global::<crate::Overlays>();
    let len = slint::Model::row_count(&overlays.get_list_entries());
    let index = usize::try_from(overlays.get_list_index()).unwrap_or(0);
    let next = rules::page_step(index, len, PICKER_ROWS, action == actions::RIGHT);
    overlays.set_list_index(i32::try_from(next).unwrap_or(0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_app::browse_filter::FilterValue;

    fn value(value: &str, label: &str, count: i64) -> FilterValue {
        FilterValue {
            value: value.into(),
            label: label.into(),
            count,
        }
    }

    fn shared_with(groups: Vec<Group>, filter: &[&str]) -> Shared {
        let mut shared = Shared::new(
            zaparoo_core::persist::PersistedState::default(),
            false,
            vec![],
            vec![],
            String::new(),
            std::path::PathBuf::new(),
        );
        shared.games.system_id = "NES".into();
        shared.filter.system_id = "NES".into();
        shared.filter.groups = groups;
        shared.filter.load = Load::Ready;
        shared
            .persist
            .games
            .set_filter("NES", filter.iter().map(|s| (*s).to_string()).collect());
        shared
    }

    fn genre_group() -> Group {
        Group {
            category: Category::Genre,
            values: vec![value("action", "Action", 90), value("rpg", "Rpg", 0)],
        }
    }

    fn ids(rows: &[crate::MenuEntry]) -> Vec<String> {
        rows.iter().map(|r| r.id.to_string()).collect()
    }

    #[test]
    fn category_rows_show_the_choice_and_offer_clear_only_when_active() {
        let idle = shared_with(vec![genre_group()], &[]);
        assert_eq!(ids(&category_rows(&idle)), ["cat:genre", "filter_apply"]);
        assert_eq!(category_rows(&idle)[0].label, "");

        let active = shared_with(vec![genre_group()], &["genre:rpg"]);
        let rows = category_rows(&active);
        assert_eq!(ids(&rows), ["cat:genre", "filter_clear", "filter_apply"]);
        assert_eq!(rows[0].label, "Rpg");
        assert_eq!(rows[0].label_key, "filter_cat:genre");
    }

    #[test]
    fn category_rows_say_why_there_is_nothing_to_pick() {
        for (load, key) in [
            (Load::Loading, "filter:loading"),
            (Load::Failed, "filter:failed"),
            (Load::Ready, "filter:none"),
        ] {
            let mut shared = shared_with(Vec::new(), &[]);
            shared.filter.load = load;
            let rows = category_rows(&shared);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].label_key, key);
            assert!(!rows[0].enabled);
        }
    }

    #[test]
    fn value_rows_lead_with_any_and_focus_the_current_choice() {
        let shared = shared_with(vec![genre_group()], &["genre:rpg"]);
        let (rows, index) = value_rows(&shared, Category::Genre);
        assert_eq!(ids(&rows), ["any", "v:action", "v:rpg"]);
        assert_eq!(index, 2);
        assert_eq!(rows[1].detail, "90");
        assert_eq!(rows[2].detail, "");

        let idle = shared_with(vec![genre_group()], &[]);
        assert_eq!(value_rows(&idle, Category::Genre).1, 0);
    }

    #[test]
    fn groups_of_another_system_are_ignored() {
        let mut shared = shared_with(vec![genre_group()], &["genre:action"]);
        shared.games.system_id = "SNES".into();
        shared
            .persist
            .games
            .set_filter("SNES", vec!["genre:rpg".into()]);
        assert!(groups(&shared).is_empty());
        // Without its tag list the cue falls back to the raw value.
        assert_eq!(summary(&shared).as_deref(), Some("Rpg"));
    }

    #[test]
    fn browse_requests_carry_only_this_systems_clean_filter() {
        let mut shared = shared_with(Vec::new(), &["region:us", "bogus:x", "genre:rpg"]);
        shared
            .persist
            .games
            .set_filter("SNES", vec!["year:1994".into()]);
        assert_eq!(active_tags(&shared), ["genre:rpg", "region:us"]);
        shared.games.system_id = "SNES".into();
        assert_eq!(active_tags(&shared), ["year:1994"]);
        shared.games.system_id = "Genesis".into();
        assert!(active_tags(&shared).is_empty());
        // Flat lists clear the system id, so they never inherit a filter.
        shared.games.system_id.clear();
        assert!(active_tags(&shared).is_empty());
    }

    #[test]
    fn only_a_lists_filter_keeps_hidden_games_in_view() {
        let plain = shared_with(Vec::new(), &["genre:rpg"]);
        assert!(!shows_hidden(&plain));
        let lists = shared_with(Vec::new(), &["genre:rpg", "user:favorite"]);
        assert!(shows_hidden(&lists));
    }

    #[test]
    fn summary_uses_core_labels_once_loaded() {
        let shared = shared_with(
            vec![Group {
                category: Category::Genre,
                values: vec![value("action:platformer", "Platform games", 3)],
            }],
            &["genre:action:platformer", "year:1994"],
        );
        assert_eq!(summary(&shared).as_deref(), Some("Platform games +1"));
        assert_eq!(view_rows(&shared).len(), 2);
    }
}
