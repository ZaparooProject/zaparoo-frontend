// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The tags picker: the "Tags" page of the Games View menu, and the Tags
// field of the Search screen. One form list swaps between a page of
// categories, each showing its chosen value, and a page of one category's
// values (a modal never opens another modal). A pick is kept as it is
// made and Back leaves; the owner refills once, when the picker closes. A
// browse filter is saved per system, a search's tags with the search. The
// category and value rules live in `zaparoo_app::browse_filter`.

use slint::{ComponentHandle, ModelRc, VecModel};
use zaparoo_app::browse_filter::{self as rules, Category, FilterTag, Group};
use zaparoo_core::client::ClientError;
use zaparoo_core::media_types::{DeckInfo, MediaTagsParams, MediaTagsResult, TagInfo};

use crate::router::{
    lock, menu_action, menu_row, menu_row_full, menu_row_keyed, menu_value, Ctx, ListContext,
    Shared,
};
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

/// Whose tags the picker edits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Target {
    /// The browse filter of the system on screen.
    #[default]
    Browse,
    /// The search being edited; its system may be empty (every system).
    Search,
}

/// The tag list for the picker's system, and what the filter was when the
/// picker's owner opened.
#[derive(Debug, Default)]
pub struct Model {
    /// The categories page is waiting on the tags, and whether its row
    /// says so yet (`crate::cue`'s timing).
    row_wait: Option<crate::cue::LocalWait>,
    loading_row: bool,
    /// Bumped per fetch so a late answer cannot fill a newer one.
    seq: u64,
    target: Target,
    system_id: String,
    groups: Vec<Group>,
    load: Load,
    baseline: Vec<String>,
}

/// The active filter for the system on screen, as `media.browse` tags.
pub(crate) fn active_tags(shared: &Shared) -> Vec<String> {
    rules::normalize(shared.persist.games.filter_for(&shared.games.system_id))
}

fn target_system(shared: &Shared) -> String {
    match shared.filter.target {
        Target::Browse => shared.games.system_id.clone(),
        Target::Search => shared.persist.search.system_id.clone(),
    }
}

/// The tags the picker is editing.
fn picker_tags(shared: &Shared) -> Vec<String> {
    match shared.filter.target {
        Target::Browse => active_tags(shared),
        Target::Search => rules::normalize(&shared.persist.search.tags),
    }
}

fn set_picker_tags(shared: &mut Shared, tags: Vec<String>) {
    match shared.filter.target {
        Target::Browse => {
            let system_id = shared.games.system_id.clone();
            shared.persist.games.set_filter(&system_id, tags);
        }
        Target::Search => shared.persist.search.tags = tags,
    }
}

/// Whether the active filter requires a user tag (Lists), which Core
/// answers with hidden games included.
pub(crate) fn shows_hidden(shared: &Shared) -> bool {
    active_tags(shared).iter().any(|t| t.starts_with("user:"))
}

/// The groups loaded for `system_id`; empty before they have loaded or
/// while they belong to another system.
fn groups_for<'a>(shared: &'a Shared, system_id: &str) -> &'a [Group] {
    if shared.filter.system_id == system_id {
        &shared.filter.groups
    } else {
        &[]
    }
}

/// The groups of the system on screen, for the browse filter's cue.
fn groups(shared: &Shared) -> &[Group] {
    groups_for(shared, &shared.games.system_id)
}

/// The groups the picker is offering.
fn picker_groups(shared: &Shared) -> &[Group] {
    match shared.filter.target {
        Target::Browse => groups(shared),
        Target::Search => groups_for(shared, &shared.persist.search.system_id),
    }
}

/// The search's tag groups once they have loaded for its system; None
/// while another system's list, or none, is held.
pub(crate) fn search_groups(shared: &Shared) -> Option<&[Group]> {
    (shared.filter.target == Target::Search
        && shared.filter.load == Load::Ready
        && shared.filter.system_id == shared.persist.search.system_id)
        .then_some(shared.filter.groups.as_slice())
}

/// A search tag's display label: Core's when the list is loaded, otherwise
/// the raw value made readable.
pub(crate) fn search_tag_label(shared: &Shared, tag: &str) -> String {
    let value = tag.split_once(':').map_or(tag, |(_, value)| value);
    let label = search_groups(shared)
        .and_then(|groups| label_for(groups, tag))
        .unwrap_or_default();
    rules::display_label(&label, value)
}

/// The search's filter as one line ("Platformer +1"). None without one.
pub(crate) fn search_summary(shared: &Shared) -> Option<String> {
    let tags = rules::normalize(&shared.persist.search.tags);
    rules::summary(&tags, |tag| {
        search_groups(shared).and_then(|groups| label_for(groups, tag))
    })
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
    begin_for(ctx, app, Target::Browse);
}

/// The same for either owner. The Search screen calls it on entry and on a
/// change of system, so its chips can be labelled and pruned.
pub(crate) fn begin_for(ctx: &Ctx, app: &App, target: Target) {
    let (system_id, seq) = {
        let mut shared = lock(&ctx.shared);
        let retarget = shared.filter.target != target;
        shared.filter.target = target;
        let system_id = target_system(&shared);
        shared.filter.baseline = picker_tags(&shared);
        if retarget || shared.filter.system_id != system_id {
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
                systems: Some(system_id)
                    .filter(|id| !id.is_empty())
                    .into_iter()
                    .collect(),
            })
            .await;
        // Core tags a deck's members by its id alone. Only a library that
        // has such tags asks for the names, and a Core too old to list
        // decks simply offers none.
        let has_decks = outcome.as_ref().is_ok_and(|result| {
            result
                .tags
                .iter()
                .any(|t| t.tag_type == "user" && rules::deck_id(&t.tag).is_some())
        });
        let decks = if has_decks {
            match client.decks().await {
                Ok(result) => result.decks,
                Err(e) => {
                    tracing::warn!(error = %e.message, "decks failed");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let _ = weak.upgrade_in_event_loop(move |app| landed(&ctx2, &app, seq, outcome, &decks));
    });
}

/// Answer the tag fetch in flight with `(type, value, label, count)` tags,
/// for a test.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn landed_for_test(ctx: &Ctx, app: &App, tags: &[(&str, &str, &str, i64)]) {
    let seq = lock(&ctx.shared).filter.seq;
    let tags: Vec<serde_json::Value> = tags
        .iter()
        .map(|(tag_type, tag, label, count)| {
            serde_json::json!({ "type": tag_type, "tag": tag, "label": label, "count": count })
        })
        .collect();
    let result = serde_json::from_value(serde_json::json!({ "tags": tags }));
    if let Ok(result) = result {
        landed(ctx, app, seq, Ok(result), &[]);
    }
}

/// A tag as the rules group it. A deck's membership tag takes the deck's
/// name as its label; Core matches deck ids without regard to case.
fn filter_tag(tag: &TagInfo, decks: &[DeckInfo]) -> FilterTag {
    let deck_name = (tag.tag_type == "user")
        .then(|| rules::deck_id(&tag.tag))
        .flatten()
        .and_then(|id| decks.iter().find(|d| d.deck_id.eq_ignore_ascii_case(id)))
        .map(|d| d.name.trim())
        .filter(|name| !name.is_empty());
    FilterTag {
        tag_type: tag.tag_type.clone(),
        value: tag.tag.clone(),
        label: deck_name.map_or_else(|| tag.label.clone(), ToString::to_string),
        count: tag.count,
    }
}

fn landed(
    ctx: &Ctx,
    app: &App,
    seq: u64,
    outcome: Result<MediaTagsResult, ClientError>,
    decks: &[DeckInfo],
) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.filter.seq != seq {
            return;
        }
        match outcome {
            Ok(result) => {
                let tags: Vec<FilterTag> =
                    result.tags.iter().map(|t| filter_tag(t, decks)).collect();
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
    // The categories page may be open on its loading row, which stays
    // out its hold if it only just appeared.
    let wait = lock(&ctx.shared).filter.row_wait.take();
    let ctx2 = ctx.clone();
    let finish = move |app: &App| {
        lock(&ctx2.shared).filter.loading_row = false;
        refresh_categories(&ctx2, app);
    };
    match wait {
        Some(wait) => crate::cue::end_local(app, wait, finish),
        None => finish(app),
    }
    let target = lock(&ctx.shared).filter.target;
    if target == Target::Search {
        crate::search::tags_loaded(ctx, app);
    }
}

/// The value selected for `category`, as its display label.
fn selected_label(shared: &Shared, category: Category) -> String {
    let tags = picker_tags(shared);
    let Some(value) = rules::selected(&tags, category) else {
        return String::new();
    };
    let tag = format!("{}:{value}", category.tag_type());
    rules::display_label(
        &label_for(picker_groups(shared), &tag).unwrap_or_default(),
        value,
    )
}

fn category_rows(shared: &Shared) -> Vec<crate::MenuEntry> {
    let groups = picker_groups(shared);
    if groups.is_empty() {
        let key = match shared.filter.load {
            Load::Failed => "filter:failed",
            // Blank until the wait is long enough to be worth a word.
            Load::Idle | Load::Loading if !shared.filter.loading_row => "filter:pending",
            Load::Idle | Load::Loading => "filter:loading",
            Load::Ready => "filter:none",
        };
        return vec![menu_row_full("none", key, "", false, "")];
    }
    let mut rows: Vec<crate::MenuEntry> = groups
        .iter()
        .map(|g| {
            let id = g.category.id();
            let chosen = selected_label(shared, g.category);
            menu_value(
                &format!("cat:{id}"),
                &format!("title:filter_cat:{id}"),
                "",
                &chosen,
                if chosen.is_empty() { "filter:any" } else { "" },
            )
        })
        .collect();
    // The one action, set apart at the foot.
    if !picker_tags(shared).is_empty() {
        rows.push(menu_action("filter_clear"));
    }
    rows
}

/// `Any`, then every value of `category`; the second half of the pair is
/// the index of the current choice.
fn value_rows(shared: &Shared, category: Category) -> (Vec<crate::MenuEntry>, usize) {
    let tags = picker_tags(shared);
    let current = rules::selected(&tags, category);
    let mut rows = vec![menu_row_keyed("any", "filter:any", "")];
    let mut index = 0;
    if let Some(group) = picker_groups(shared)
        .iter()
        .find(|g| g.category == category)
    {
        for value in &group.values {
            if current == Some(value.value.as_str()) {
                index = rows.len();
            }
            let count = if value.count > 0 {
                value.count.to_string()
            } else {
                String::new()
            };
            rows.push(menu_value(
                &format!("v:{}", value.value),
                "",
                &value.label,
                &count,
                "",
            ));
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
    crate::router::present_form_list(ctx, app, context, title, rows, index);
}

/// Redraw the categories page's rows where it is the page on screen.
fn refresh_categories(ctx: &Ctx, app: &App) {
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

/// Start the loading row's wait when the page opens on tags still in
/// flight: the row is blank, then says "Loading tags…" if it lasts.
fn arm_loading_row(ctx: &Ctx, app: &App) {
    let stale = {
        let mut shared = lock(&ctx.shared);
        let waiting = shared.filter.groups.is_empty()
            && matches!(shared.filter.load, Load::Idle | Load::Loading);
        if !waiting {
            return;
        }
        shared.filter.loading_row = false;
        shared.filter.row_wait.take()
    };
    if let Some(stale) = stale {
        crate::cue::abandon_local(stale);
    }
    let ctx2 = ctx.clone();
    let wait = crate::cue::begin_local(app, move |app| {
        {
            let mut shared = lock(&ctx2.shared);
            if !matches!(shared.filter.load, Load::Idle | Load::Loading) {
                return;
            }
            shared.filter.loading_row = true;
        }
        refresh_categories(&ctx2, app);
    });
    lock(&ctx.shared).filter.row_wait = Some(wait);
}

/// The categories page, with `focus` pre-selected (the category just left).
fn present_categories(ctx: &Ctx, app: &App, focus: Option<Category>) {
    arm_loading_row(ctx, app);
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

/// The View menu's Filter row, or the Search screen's Tags field.
pub(crate) fn open(ctx: &Ctx, app: &App) {
    present_categories(ctx, app, None);
}

fn set_selection(ctx: &Ctx, category: Category, value: Option<&str>) {
    let mut shared = lock(&ctx.shared);
    let next = rules::with_selection(&picker_tags(&shared), category, value);
    set_picker_tags(&mut shared, next);
}

fn clear_selection(ctx: &Ctx) {
    set_picker_tags(&mut lock(&ctx.shared), Vec::new());
}

/// The View menu's Clear tags row: the list closes with the menu.
pub(crate) fn clear(ctx: &Ctx, app: &App) {
    clear_selection(ctx);
    apply(ctx, app);
}

/// Refill the owner when the filter differs from the one the picker opened
/// with. One refill however many picks were made.
pub(crate) fn apply(ctx: &Ctx, app: &App) {
    let (changed, target) = {
        let mut shared = lock(&ctx.shared);
        let now = picker_tags(&shared);
        let changed = now != shared.filter.baseline;
        shared.filter.baseline = now;
        (changed, shared.filter.target)
    };
    if !changed {
        return;
    }
    match target {
        Target::Browse => crate::games::refilter(ctx, app),
        Target::Search => crate::search::tags_changed(ctx, app),
    }
}

/// Accept on a filter page. The panel stays open and swaps pages.
pub(crate) fn accept(ctx: &Ctx, app: &App, context: &ListContext, id: &str) {
    match context {
        ListContext::FilterCategories => {
            if id == "filter_clear" {
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

/// Back on a picker page: from a category's values to the categories, from
/// the categories out of the picker, keeping what was picked. `close_all`
/// (the View button) leaves from either page.
pub(crate) fn back(ctx: &Ctx, app: &App, context: &ListContext, close_all: bool) {
    match context {
        ListContext::FilterValues(category) if !close_all => {
            present_categories(ctx, app, Some(*category));
        }
        _ => close(ctx, app),
    }
}

fn close(ctx: &Ctx, app: &App) {
    app.global::<crate::Overlays>().set_list_open(false);
    apply(ctx, app);
}

/// Left and Right, or the shoulder buttons, page a long list of values.
pub(crate) fn page(app: &App, forward: bool) {
    let overlays = app.global::<crate::Overlays>();
    let len = slint::Model::row_count(&overlays.get_list_entries());
    let index = usize::try_from(overlays.get_list_index()).unwrap_or(0);
    let next = rules::page_step(index, len, PICKER_ROWS, forward);
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
            values: vec![value("action", "Action", 90), value("rpg", "rpg", 0)],
        }
    }

    fn ids(rows: &[crate::MenuEntry]) -> Vec<String> {
        rows.iter().map(|r| r.id.to_string()).collect()
    }

    #[test]
    fn category_rows_show_the_choice_and_offer_clear_only_when_active() {
        let idle = shared_with(vec![genre_group()], &[]);
        let rows = category_rows(&idle);
        assert_eq!(ids(&rows), ["cat:genre"]);
        // Nothing chosen reads "Any", which is the view's word to translate.
        assert_eq!(
            (rows[0].detail.as_str(), rows[0].detail_key.as_str()),
            ("", "filter:any")
        );

        let active = shared_with(vec![genre_group()], &["genre:rpg"]);
        let rows = category_rows(&active);
        assert_eq!(ids(&rows), ["cat:genre", "filter_clear"]);
        assert_eq!(
            (rows[0].detail.as_str(), rows[0].detail_key.as_str()),
            ("rpg", "")
        );
        assert_eq!(rows[0].label_key, "title:filter_cat:genre");
        // Clear is an action set apart from the categories, not one of them.
        assert_eq!(rows[0].role, crate::MenuRole::Option);
        assert_eq!(rows[1].role, crate::MenuRole::Action);
    }

    #[test]
    fn category_rows_say_why_there_is_nothing_to_pick() {
        // A wait says nothing until it has lasted long enough to be one.
        for (load, said, key) in [
            (Load::Loading, false, "filter:pending"),
            (Load::Loading, true, "filter:loading"),
            (Load::Failed, false, "filter:failed"),
            (Load::Ready, false, "filter:none"),
        ] {
            let mut shared = shared_with(Vec::new(), &[]);
            shared.filter.load = load;
            shared.filter.loading_row = said;
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
        assert_eq!(summary(&shared).as_deref(), Some("rpg"));
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
    fn the_search_target_edits_the_searchs_own_tags_and_system() {
        let mut shared = shared_with(vec![genre_group()], &["genre:action"]);
        shared.filter.target = Target::Search;
        // The groups on hand belong to NES; the search covers every system.
        assert!(picker_groups(&shared).is_empty());
        assert!(search_groups(&shared).is_none());
        assert_eq!(
            search_tag_label(&shared, "genre:run-and-gun"),
            "run-and-gun"
        );

        shared.persist.search.system_id = "NES".into();
        assert_eq!(picker_groups(&shared).len(), 1);
        assert!(search_groups(&shared).is_some());
        assert_eq!(search_tag_label(&shared, "genre:rpg"), "rpg");

        set_picker_tags(&mut shared, vec!["genre:rpg".into()]);
        assert_eq!(picker_tags(&shared), ["genre:rpg"]);
        assert_eq!(shared.persist.search.tags, ["genre:rpg"]);
        // The browse filter of the same system is untouched.
        assert_eq!(active_tags(&shared), ["genre:action"]);
        let rows = category_rows(&shared);
        assert_eq!(ids(&rows), ["cat:genre", "filter_clear"]);
        assert_eq!(rows[0].detail, "rpg");
    }

    #[test]
    fn a_deck_tag_takes_the_decks_name_however_its_id_is_cased() {
        let tag = |tag_type: &str, value: &str| -> TagInfo {
            serde_json::from_value(serde_json::json!({ "type": tag_type, "tag": value }))
                .unwrap_or_else(|_| unreachable!("a fixed tag deserialises"))
        };
        let decks = vec![
            DeckInfo {
                deck_id: "0K3V9X2RQ7BM".into(),
                name: " Couch co-op ".into(),
                item_count: 8,
            },
            DeckInfo {
                deck_id: "blank".into(),
                name: "  ".into(),
                item_count: 1,
            },
        ];
        let named = filter_tag(&tag("user", "deck:0k3v9x2rq7bm"), &decks);
        assert_eq!(named.label, "Couch co-op");
        // A deck with no name, or one the list does not hold, stays
        // unnamed, and the rules leave it out.
        assert_eq!(filter_tag(&tag("user", "deck:blank"), &decks).label, "");
        assert_eq!(filter_tag(&tag("user", "deck:gone"), &decks).label, "");
        // Only a deck tag is looked up.
        assert_eq!(
            filter_tag(&tag("genre", "deck:0k3v9x2rq7bm"), &decks).label,
            ""
        );
        let groups = rules::group(&[
            named,
            filter_tag(&tag("user", "deck:gone"), &decks),
            filter_tag(&tag("user", "favorite"), &decks),
        ]);
        let labels: Vec<&str> = groups[0].values.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(labels, ["Favorites", "Couch co-op"]);
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
