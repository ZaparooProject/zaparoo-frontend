// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Hub's persisted layout: a flat, user-ordered list of tiles stored as
// `[[hub.items]]` in `frontend.toml`. This is the "go all in" replacement
// for the old hide/order-by-composite-key storage — the layout itself is
// now the source of truth for what shows on the Hub and in what order,
// snapshotted from whatever Core reported at first load and hand-editable
// (or, later, edit-UI-editable) from then on. See
// `docs/plans/ui-geometry-refresh.md`'s Hub roadmap for the full design
// discussion this schema comes out of.
//
// Deliberately a plain, reusable entry schema rather than a Hub-specific
// one — `HubItem`/`HubItemKind` describe "an addressable target with an
// optional name/icon override," which is also exactly what a future
// user-authored collection (an arbitrary list of folders/systems/games,
// discussed but explicitly not built yet) would need. Keeping this generic
// now means a collection is a second array of the same item type later,
// not a schema migration. `known`/reconciliation below is the one
// Hub-specific piece — nothing should ever auto-populate a hand-authored
// collection, so that stays out of the shared entry type.
//
// Read side uses plain `toml`+serde (`RawHub` below), like `config.rs`'s
// `RawConfig` — round-tripping formatting only matters on the write side,
// handled by `save_hub_layout` via `toml_edit`.

use serde::Deserialize;
use std::path::Path;

/// What an entry points at. `Collection` is reserved for the not-yet-built
/// virtual-collection feature (see the module doc comment) — accepted on
/// parse so a future build can add it without a schema migration, but
/// nothing renders or targets it yet. `Unknown` preserves any `type` this
/// build doesn't recognise (a newer build's item, or a typo) so it
/// round-trips on save instead of silently vanishing from the user's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubItemKind {
    Category,
    Action,
    System,
    Folder,
    ZapScript,
    Blank,
    Collection,
    Unknown(String),
}

impl HubItemKind {
    fn as_str(&self) -> &str {
        match self {
            Self::Category => "category",
            Self::Action => "action",
            Self::System => "system",
            Self::Folder => "folder",
            Self::ZapScript => "zapscript",
            Self::Blank => "blank",
            Self::Collection => "collection",
            Self::Unknown(raw) => raw.as_str(),
        }
    }

    fn from_str(raw: &str) -> Self {
        match raw {
            "category" => Self::Category,
            "action" => Self::Action,
            "system" => Self::System,
            "folder" => Self::Folder,
            "zapscript" => Self::ZapScript,
            "blank" => Self::Blank,
            "collection" => Self::Collection,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// True for kinds this build actually knows how to render and target.
    /// `Collection` (reserved, unimplemented) and `Unknown` (a newer
    /// build's item, or a typo) still round-trip on save but are skipped
    /// when projecting the layout for display — see `HubLayout::visible`.
    fn renderable(&self) -> bool {
        !matches!(self, Self::Collection | Self::Unknown(_))
    }
}

/// One Hub tile. Fields are used per-kind: `id` for `category`/`action`/
/// `system`, `path` for `folder`, `script` (+ optional `system`/`path` as a
/// cover-art hint) for `zapscript`. `name`/`icon` are optional overrides
/// available on any kind. `Blank` uses none of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubItem {
    pub kind_raw: String,
    pub id: String,
    pub path: String,
    pub script: String,
    pub name: String,
    pub icon: String,
    pub system: String,
}

impl HubItem {
    pub fn kind(&self) -> HubItemKind {
        HubItemKind::from_str(&self.kind_raw)
    }

    fn category(id: &str) -> Self {
        Self {
            kind_raw: HubItemKind::Category.as_str().to_string(),
            id: id.to_string(),
            ..Self::default()
        }
    }

    fn action(id: &str) -> Self {
        Self {
            kind_raw: HubItemKind::Action.as_str().to_string(),
            id: id.to_string(),
            ..Self::default()
        }
    }
}

/// The built-in action ids, in the order a fresh layout seeds them.
/// Visibility of some of these (`resume`, `update`) is conditional and
/// decided at render time in QML from live state (Recents/internet/build
/// flags) — the layout just records that the tile exists and where it
/// sits; see `docs/plans/ui-geometry-refresh.md`.
pub const BUILT_IN_ACTIONS: &[&str] = &["resume", "favorites", "recents", "update", "settings"];

/// The persisted Hub layout.
///
/// `known` is a reconciliation ledger, not a visibility list: every
/// `category`/`action` key the user has EVER seen, whether or not it's
/// currently in `items`. On boot, once Core's category list has actually
/// loaded, `HubLayout::reconcile` diffs the detected categories against
/// `known` — anything new gets appended to `items` (landing at the end,
/// like an app installed on a phone) and added to `known`. A category the
/// user removed from `items` stays in `known`, so it does NOT come back —
/// that's what makes "delete a tile" stick. `known` never tracks
/// `system`/`folder`/`zapscript`/`blank` entries; those are always
/// explicitly user-authored (by hand-editing config, or later an edit UI),
/// never auto-discovered, so there's nothing to reconcile them against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubLayout {
    pub known: Vec<String>,
    pub items: Vec<HubItem>,
}

impl HubLayout {
    /// Layout entries this build can actually render, in order — skips
    /// `Collection`/`Unknown` kinds (see `HubItemKind::renderable`). What
    /// `Browse.HubLayout` exposes to QML.
    pub fn visible(&self) -> impl Iterator<Item = &HubItem> {
        self.items.iter().filter(|item| item.kind().renderable())
    }

    /// True when this layout has never been seeded at all — the state a
    /// brand-new install (or a config predating this feature) starts in.
    /// Distinct from "seeded but the user emptied it": an empty `items`
    /// with a non-empty `known` is a deliberate empty Hub, not a fresh
    /// install, and must NOT be re-seeded. Exposed to QML (as
    /// `Browse.HubLayout.is_unseeded()`) so the bootstrap placeholder branch
    /// can key off this instead of `item_count() == 0` — with remove-leaves-
    /// a-gap plus trailing-blank trimming, a seeded layout the user has
    /// emptied out (every item a trailing blank, all trimmed away) is a
    /// real, reachable `item_count() == 0` state now, and must render as an
    /// empty Hub, not snap back to fake placeholder tiles.
    pub fn is_unseeded(&self) -> bool {
        self.known.is_empty() && self.items.is_empty()
    }

    /// Reconcile against Core's currently-detected category list. Returns
    /// `true` when the layout changed (the caller should persist).
    ///
    /// Two cases, both additive-only — this never removes an item or a
    /// `known` entry, so an unreachable/mid-scan Core (which reports zero
    /// or a partial category list) can never destroy a hand-arranged
    /// layout:
    ///
    /// - **Unseeded** (`is_unseeded`): take a full snapshot — every
    ///   detected category not in `migrate_hidden_categories`, then every
    ///   built-in action — as the layout's starting point. Skipped entirely
    ///   when `detected_categories` is empty, so a first-ever boot with
    ///   Core down or mid-scan does not seed an actions-only layout; the
    ///   caller should call this again on the next real category refresh
    ///   until it returns `true`.
    /// - **Already seeded**: append any detected category whose
    ///   `"category:<id>"` key isn't in `known` yet — same "lands at the
    ///   end" rule, new items only.
    pub fn reconcile(
        &mut self,
        detected_categories: &[String],
        migrate_hidden_categories: &[String],
    ) -> bool {
        if self.is_unseeded() {
            if detected_categories.is_empty() {
                return false;
            }
            // Resume seeds first, ahead of every category, so it lands in
            // the top-left cell by default — the highest-value action, and
            // the one the startup focus special case (`HubScreen.
            // focusResumeIfVisible`) already seats on regardless of where
            // it sits. Depends on `BUILT_IN_ACTIONS[0] == "resume"`.
            let resume_id = BUILT_IN_ACTIONS[0];
            self.known.push(format!("action:{resume_id}"));
            self.items.push(HubItem::action(resume_id));
            for id in detected_categories {
                let key = format!("category:{id}");
                self.known.push(key.clone());
                if !migrate_hidden_categories.iter().any(|hidden| hidden == id) {
                    self.items.push(HubItem::category(id));
                }
            }
            for id in &BUILT_IN_ACTIONS[1..] {
                self.known.push(format!("action:{id}"));
                self.items.push(HubItem::action(id));
            }
            return true;
        }

        let mut changed = false;
        for id in detected_categories {
            let key = format!("category:{id}");
            if self.known.iter().any(|k| k == &key) {
                continue;
            }
            self.known.push(key);
            self.items.push(HubItem::category(id));
            changed = true;
        }
        changed
    }

    /// Indices into `items` for entries `visible()` yields, in order — the
    /// mapping from a QML-facing visible index back to the real position in
    /// `items`. Needed because `visible()` filters out unrenderable kinds
    /// (`Collection`/`Unknown`) this build never creates but must still
    /// round-trip, so a visible index and an `items` index can diverge
    /// whenever one is present.
    fn visible_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.kind().renderable())
            .map(|(i, _)| i)
            .collect()
    }

    /// Board-model move: swap the `from`-th visible item with whatever
    /// occupies the `to`-th visible position — a tile, or a `blank`. Always
    /// exactly two cells change; nothing else in the layout ever shifts, so
    /// a gap the user deliberately left stays exactly where they left it.
    /// `to` past the current visible end pads with fresh `blank` entries
    /// until it exists (pushing onto a brand-new page mid-move), so the
    /// only rejected input is an out-of-range `from` or `from == to`.
    pub fn place_visible_item(&mut self, from: usize, to: usize) -> bool {
        let indices = self.visible_indices();
        if from >= indices.len() || from == to {
            return false;
        }
        while self.visible_indices().len() <= to {
            self.items.push(HubItem {
                kind_raw: HubItemKind::Blank.as_str().to_string(),
                ..HubItem::default()
            });
        }
        let indices = self.visible_indices();
        self.items.swap(indices[from], indices[to]);
        true
    }

    /// Origin-anchored placement for a Move session. Rebuilds `self` from
    /// `base` — the layout exactly as it stood when the move was armed —
    /// then places the item at `base`'s `origin`-th visible position onto
    /// the `to`-th visible position. Unlike `place_visible_item`, repeated
    /// calls with the same `base`/`origin` and a changing `to` are NOT
    /// chained: every call starts over from `base`, so at most one other
    /// tile is ever displaced from where `base` had it — the one currently
    /// occupying `to` — and any tile a previous call displaced snaps back
    /// home. This is what makes a Move session read as "carrying one tile
    /// around a static board" instead of dragging a trail of swaps behind
    /// it. `origin == to` is a legal no-op that still returns `true` — a
    /// held tile landing back on its own cell is a full restore, not a
    /// rejected move (contrast `place_visible_item`, which rejects
    /// `from == to`). Only an out-of-range `origin` (against `base`) fails.
    pub fn reseat_held_item(&mut self, base: &Self, origin: usize, to: usize) -> bool {
        if origin >= base.visible_indices().len() {
            return false;
        }
        self.clone_from(base);
        if origin == to {
            return true;
        }
        self.place_visible_item(origin, to)
    }

    /// Drop trailing `blank` entries — ones after the last non-blank item.
    /// Leading and interior blanks are the user's deliberate gaps and are
    /// never touched; a trailing blank is indistinguishable on screen from
    /// ordinary page padding, so it doesn't belong in the saved file. Also
    /// what cleans up a blank a move created (to reach a new page) and then
    /// moved away from again.
    pub fn trim_trailing_blanks(&mut self) {
        while matches!(self.items.last(), Some(item) if item.kind() == HubItemKind::Blank) {
            self.items.pop();
        }
    }

    /// Remove the `index`-th visible item by turning its cell into a
    /// `blank` — board-model removal, so nothing after it shifts. `known`
    /// is left untouched for `category`/`action` kinds — see the struct doc
    /// comment for why that's what makes a removal stick instead of the
    /// item reappearing on the next `reconcile`. Trims trailing blanks
    /// afterward (see `trim_trailing_blanks`), so removing the last real
    /// tile on the page shrinks the layout rather than leaving a dangling
    /// blank at the end.
    pub fn remove_visible_item(&mut self, index: usize) -> bool {
        let indices = self.visible_indices();
        let Some(&real) = indices.get(index) else {
            return false;
        };
        self.items[real] = HubItem {
            kind_raw: HubItemKind::Blank.as_str().to_string(),
            ..HubItem::default()
        };
        self.trim_trailing_blanks();
        true
    }

    /// Keys from `known` (`"category:<id>"` / `"action:<id>"`) not
    /// currently present in `items` — the "Add item…" grab bag's contents,
    /// in `known` order. A removed tile only ever comes back through this
    /// path, never automatically (see `reconcile`).
    pub fn available_known(&self) -> Vec<String> {
        self.known
            .iter()
            .filter(|key| {
                !self
                    .items
                    .iter()
                    .any(|item| &format!("{}:{}", item.kind_raw, item.id) == *key)
            })
            .cloned()
            .collect()
    }

    /// Board-model add: a `category`/`action` entry from `known` (see
    /// `available_known`), or a fresh, untracked `blank` spacer
    /// (`kind == "blank"`, `id` ignored — blanks are never tracked in
    /// `known`, see the struct doc comment). Lands on the `target`-th
    /// visible position when that cell is currently a `blank` (the cell the
    /// user's cursor is on); otherwise appended after the last real tile.
    /// Returns `false` for a `category`/`action` key not actually in
    /// `known`, or already present in `items`.
    pub fn add_item(&mut self, kind: &str, id: &str, target: usize) -> bool {
        let is_blank_entry = kind == "blank";
        let entry = if is_blank_entry {
            HubItem {
                kind_raw: HubItemKind::Blank.as_str().to_string(),
                ..HubItem::default()
            }
        } else {
            let key = format!("{kind}:{id}");
            if !self.known.iter().any(|k| k == &key) {
                return false;
            }
            if self
                .items
                .iter()
                .any(|item| item.kind_raw == kind && item.id == id)
            {
                return false;
            }
            match kind {
                "category" => HubItem::category(id),
                "action" => HubItem::action(id),
                _ => return false,
            }
        };
        let indices = self.visible_indices();
        if let Some(&real) = indices.get(target) {
            if self.items[real].kind() == HubItemKind::Blank {
                self.items[real] = entry;
                // Trimming here can only ever remove blanks that come
                // AFTER what was just placed (the invariant: nothing
                // before the placed cell ever shifts), so it's always
                // safe, even for a blank-kind entry replacing another
                // blank — a self-canceling no-op nobody would actually do.
                self.trim_trailing_blanks();
                return true;
            }
        }
        self.items.push(entry);
        // A freshly appended blank spacer IS the trailing entry right
        // now — trimming here would delete the very thing the user just
        // asked to add. Only clean up pre-existing dangling blanks when
        // what landed at the end is a real tile.
        if !is_blank_entry {
            self.trim_trailing_blanks();
        }
        true
    }

    /// Append a fully-specified `system`/`folder`/`zapscript` shortcut
    /// created from a browse screen's "Add to Hub" context-menu action.
    /// Unlike `add_item`, these kinds are never tracked in `known` (see the
    /// struct doc comment — they're always user-authored, never
    /// auto-discovered), so there is no key to validate against here.
    /// Always appends after the last real tile — the same "lands at the
    /// end, like an app just installed" placement `reconcile` uses for a
    /// newly detected category — since a context menu on Systems/Games has
    /// no meaningful Hub cursor cell to target the way the Hub's own "Add
    /// item…" does. Returns `false` only for a `kind` this isn't valid for.
    #[allow(
        clippy::too_many_arguments,
        reason = "one field per HubItem column, mirrors the struct shape directly"
    )]
    pub fn add_target_item(
        &mut self,
        kind: &str,
        id: &str,
        path: &str,
        script: &str,
        name: &str,
        icon: &str,
        system: &str,
    ) -> bool {
        let kind_enum = HubItemKind::from_str(kind);
        if !matches!(
            kind_enum,
            HubItemKind::System | HubItemKind::Folder | HubItemKind::ZapScript
        ) {
            return false;
        }
        self.items.push(HubItem {
            kind_raw: kind.to_string(),
            id: id.to_string(),
            path: path.to_string(),
            script: script.to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            system: system.to_string(),
        });
        // No trim call here, unlike `add_item`'s own non-blank append
        // branch: a straight `push` always lands the new entry at the true
        // end, so any blank that was previously trailing is now interior
        // (ahead of what we just added) rather than trailing — nothing for
        // `trim_trailing_blanks` to find. That also means a pre-existing
        // trailing blank isn't silently swallowed by this call: it's left
        // exactly where it was, same as the rest of this file's "nothing
        // you didn't touch moves" board-model discipline.
        true
    }

    /// Re-seed the layout from scratch — "Reset layout" in the View menu.
    /// Drops every existing item and `known` entry (folders, `ZapScript`
    /// tiles, and blanks included, none of which `known` tracks in the
    /// first place) and reseeds from `detected_categories` plus the
    /// built-in actions, the same starting point a fresh install seeds to.
    /// Unlike `reconcile`, this is unconditional even on an already-seeded
    /// layout; empty `detected_categories` still clears down to just the
    /// built-in actions rather than leaving stale categories behind.
    pub fn reset(&mut self, detected_categories: &[String]) {
        self.known.clear();
        self.items.clear();
        // Resume first — see `reconcile`'s unseeded branch for why.
        let resume_id = BUILT_IN_ACTIONS[0];
        self.known.push(format!("action:{resume_id}"));
        self.items.push(HubItem::action(resume_id));
        for id in detected_categories {
            self.known.push(format!("category:{id}"));
            self.items.push(HubItem::category(id));
        }
        for id in &BUILT_IN_ACTIONS[1..] {
            self.known.push(format!("action:{id}"));
            self.items.push(HubItem::action(id));
        }
    }
}

#[derive(Deserialize, Default)]
struct RawHub {
    #[serde(default)]
    known: Vec<String>,
    #[serde(default)]
    items: Vec<RawHubItem>,
}

#[derive(Deserialize, Default)]
struct RawHubItem {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    script: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    system: String,
}

#[derive(Deserialize, Default)]
struct RawHubRoot {
    #[serde(default)]
    hub: RawHub,
}

/// Load the Hub layout from `frontend.toml`. Read side only — an
/// independent top-level parse of the same file `config.rs::load_config`
/// reads, mirroring how several `Browse.*` singletons already re-read
/// config independently rather than threading a shared parse through
/// (`rust/frontend/src/models/settings.rs`, `crt_video.rs`, etc). A missing
/// or malformed file returns an empty (unseeded) layout, same fallback
/// shape `load_config` uses.
pub fn load_hub_layout(path: &Path) -> HubLayout {
    let raw: RawHubRoot = match std::fs::read_to_string(path) {
        Ok(src) => toml::from_str(&src).unwrap_or_default(),
        Err(_) => RawHubRoot::default(),
    };
    HubLayout {
        known: raw.hub.known,
        items: raw
            .hub
            .items
            .into_iter()
            .map(|raw| HubItem {
                kind_raw: raw.kind,
                id: raw.id,
                path: raw.path,
                script: raw.script,
                name: raw.name,
                icon: raw.icon,
                system: raw.system,
            })
            .collect(),
    }
}

/// Persist the Hub layout into `frontend.toml`, format-preserving (see
/// `config.rs`'s "Format-preserving writes" section — `[hub]` gets the same
/// treatment as `[settings]`). Rewrites `[hub.known]` and the full
/// `[[hub.items]]` array of tables unconditionally when either changed;
/// unrelated sections and their comments/formatting are untouched.
pub fn save_hub_layout(path: &Path, layout: &HubLayout) -> Result<(), String> {
    let mut doc = crate::config::read_config_document(path)?;
    let before = doc.to_string();

    let hub = crate::config::section_mut(&mut doc, "hub", path)?;

    crate::config::set_string_list(hub, "known", &layout.known);

    let mut items = toml_edit::ArrayOfTables::new();
    for item in &layout.items {
        let mut table = toml_edit::Table::new();
        table.insert("type", toml_edit::value(item.kind_raw.as_str()));
        if !item.id.is_empty() {
            table.insert("id", toml_edit::value(item.id.as_str()));
        }
        if !item.path.is_empty() {
            table.insert("path", toml_edit::value(item.path.as_str()));
        }
        if !item.script.is_empty() {
            table.insert("script", toml_edit::value(item.script.as_str()));
        }
        if !item.name.is_empty() {
            table.insert("name", toml_edit::value(item.name.as_str()));
        }
        if !item.icon.is_empty() {
            table.insert("icon", toml_edit::value(item.icon.as_str()));
        }
        if !item.system.is_empty() {
            table.insert("system", toml_edit::value(item.system.as_str()));
        }
        items.push(table);
    }
    hub.insert("items", toml_edit::Item::ArrayOfTables(items));

    crate::config::write_document_if_changed(path, &before, &doc)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{load_hub_layout, save_hub_layout, HubItem, HubItemKind, HubLayout};
    use std::io::Write;

    fn write_tmp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(contents.as_bytes()).expect("write");
        f
    }

    #[test]
    fn missing_file_returns_unseeded_layout() {
        let layout = load_hub_layout(std::path::Path::new("/nonexistent/frontend.toml"));
        assert!(layout.known.is_empty());
        assert!(layout.items.is_empty());
    }

    #[test]
    fn round_trips_every_field() {
        let f = write_tmp(
            r#"
[hub]
known = ["category:Arcade", "action:resume"]

[[hub.items]]
type = "category"
id = "Arcade"

[[hub.items]]
type = "folder"
path = "/media/fat/games/SNES/Homebrew"
name = "Homebrew"

[[hub.items]]
type = "zapscript"
name = "Random NES"
script = "**launch.random:NES"
icon = "Dice"

[[hub.items]]
type = "blank"
"#,
        );
        let layout = load_hub_layout(f.path());
        assert_eq!(layout.known, vec!["category:Arcade", "action:resume"]);
        assert_eq!(layout.items.len(), 4);
        assert_eq!(layout.items[0].kind(), HubItemKind::Category);
        assert_eq!(layout.items[0].id, "Arcade");
        assert_eq!(layout.items[1].kind(), HubItemKind::Folder);
        assert_eq!(layout.items[1].path, "/media/fat/games/SNES/Homebrew");
        assert_eq!(layout.items[1].name, "Homebrew");
        assert_eq!(layout.items[2].kind(), HubItemKind::ZapScript);
        assert_eq!(layout.items[2].script, "**launch.random:NES");
        assert_eq!(layout.items[2].icon, "Dice");
        assert_eq!(layout.items[3].kind(), HubItemKind::Blank);
    }

    #[test]
    fn unknown_type_parses_as_unknown_and_is_not_renderable() {
        let f = write_tmp("[[hub.items]]\ntype = \"future-kind\"\nid = \"x\"\n");
        let layout = load_hub_layout(f.path());
        assert_eq!(
            layout.items[0].kind(),
            HubItemKind::Unknown("future-kind".to_string())
        );
        assert_eq!(layout.visible().count(), 0);
    }

    #[test]
    fn collection_kind_parses_but_is_not_yet_renderable() {
        let f = write_tmp("[[hub.items]]\ntype = \"collection\"\nid = \"x\"\n");
        let layout = load_hub_layout(f.path());
        assert_eq!(layout.items[0].kind(), HubItemKind::Collection);
        assert_eq!(layout.visible().count(), 0);
    }

    #[test]
    fn visible_skips_unrenderable_kinds_but_keeps_order_of_the_rest() {
        let mut layout = HubLayout::default();
        layout.items.push(HubItem::category("Arcade"));
        layout.items.push(HubItem {
            kind_raw: "collection".to_string(),
            ..HubItem::default()
        });
        layout.items.push(HubItem::action("settings"));
        let visible: Vec<&str> = layout.visible().map(|i| i.id.as_str()).collect();
        assert_eq!(visible, vec!["Arcade", "settings"]);
    }

    #[test]
    fn reconcile_seeds_a_fresh_layout_with_resume_first_then_categories_then_actions() {
        let mut layout = HubLayout::default();
        let changed = layout.reconcile(&["Arcade".to_string(), "Console".to_string()], &[]);
        assert!(changed);
        assert_eq!(
            layout.known,
            vec![
                "action:resume",
                "category:Arcade",
                "category:Console",
                "action:favorites",
                "action:recents",
                "action:update",
                "action:settings",
            ]
        );
        let ids: Vec<&str> = layout.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "resume",
                "Arcade",
                "Console",
                "favorites",
                "recents",
                "update",
                "settings"
            ]
        );
    }

    #[test]
    fn reconcile_does_not_seed_while_core_reports_zero_categories() {
        let mut layout = HubLayout::default();
        let changed = layout.reconcile(&[], &[]);
        assert!(!changed);
        assert!(layout.known.is_empty());
        assert!(layout.items.is_empty());
    }

    #[test]
    fn reconcile_migrates_previously_hidden_categories_as_known_but_absent() {
        let mut layout = HubLayout::default();
        layout.reconcile(
            &["Arcade".to_string(), "Console".to_string()],
            &["Console".to_string()],
        );
        // Known (so it won't re-seed later), but not in items (so it
        // doesn't reappear despite having been hidden pre-migration).
        assert!(layout.known.contains(&"category:Console".to_string()));
        assert!(!layout.items.iter().any(|i| i.id == "Console"));
        assert!(layout.items.iter().any(|i| i.id == "Arcade"));
    }

    #[test]
    fn reconcile_on_an_already_seeded_layout_only_appends_new_categories() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        let seeded_len = layout.items.len();

        // Re-detecting the same category changes nothing.
        assert!(!layout.reconcile(&["Arcade".to_string()], &[]));
        assert_eq!(layout.items.len(), seeded_len);

        // A genuinely new category appends at the END, after the actions
        // the first seed already placed — not grouped back with Arcade.
        assert!(layout.reconcile(&["Arcade".to_string(), "Handheld".to_string()], &[]));
        assert_eq!(layout.items.last().unwrap().id, "Handheld");
        assert!(layout.known.contains(&"category:Handheld".to_string()));
    }

    #[test]
    fn reconcile_never_re_adds_a_removed_item_because_known_still_has_it() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        // Simulate the user deleting the Arcade tile (Phase D): drop from
        // items, `known` untouched.
        layout.items.retain(|i| i.id != "Arcade");
        assert!(!layout.items.iter().any(|i| i.id == "Arcade"));

        // Core reporting Arcade again (a later refresh) must not bring it
        // back — it's already in `known`.
        let changed = layout.reconcile(&["Arcade".to_string()], &[]);
        assert!(!changed);
        assert!(!layout.items.iter().any(|i| i.id == "Arcade"));
    }

    #[test]
    fn place_visible_item_swaps_the_two_cells_only() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["A".to_string(), "B".to_string(), "C".to_string()], &[]);
        // resume, A, B, C, favorites, recents, update, settings
        assert!(layout.place_visible_item(1, 3));
        let ids: Vec<&str> = layout.items.iter().map(|i| i.id.as_str()).collect();
        // A and C trade places; nothing else moves.
        assert_eq!(
            ids,
            vec![
                "resume",
                "C",
                "B",
                "A",
                "favorites",
                "recents",
                "update",
                "settings"
            ]
        );
    }

    #[test]
    fn place_visible_item_onto_an_interior_blank_replaces_only_that_blank() {
        let mut layout = HubLayout::default();
        layout.items.push(HubItem::action("resume"));
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        layout.items.push(HubItem::action("settings"));
        // resume, blank, blank, blank, settings
        assert!(layout.place_visible_item(4, 2));
        let ids: Vec<&str> = layout.items.iter().map(|i| i.id.as_str()).collect();
        let kinds: Vec<HubItemKind> = layout.items.iter().map(HubItem::kind).collect();
        assert_eq!(ids, vec!["resume", "", "settings", "", ""]);
        assert_eq!(
            kinds,
            vec![
                HubItemKind::Action,
                HubItemKind::Blank,
                HubItemKind::Action,
                HubItemKind::Blank,
                HubItemKind::Blank,
            ],
            "only the middle blank was consumed; the other two gaps must not move or vanish"
        );
    }

    #[test]
    fn place_visible_item_off_a_slot_leaves_a_blank_behind() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["A".to_string(), "B".to_string()], &[]);
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        // resume, A, B, favorites, recents, update, settings, blank
        assert!(layout.place_visible_item(1, 7));
        assert_eq!(
            layout.items[1].kind(),
            HubItemKind::Blank,
            "A's old cell must become a blank, not shift anything else"
        );
        assert_eq!(layout.items[7].id, "A");
    }

    #[test]
    fn place_visible_item_past_the_end_pads_with_blanks() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["A".to_string()], &[]);
        let count_before = layout.items.len();
        assert!(layout.place_visible_item(0, count_before + 2));
        assert_eq!(layout.items.len(), count_before + 3);
        assert_eq!(layout.items[count_before + 2].id, "resume");
        assert_eq!(layout.items[0].kind(), HubItemKind::Blank);
    }

    #[test]
    fn place_visible_item_no_ops_on_out_of_range_from_or_identical_indices() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["A".to_string()], &[]);
        let before = layout.items.clone();
        assert!(!layout.place_visible_item(0, 0));
        assert!(!layout.place_visible_item(999, 0));
        assert_eq!(layout.items, before);
    }

    #[test]
    fn reseat_held_item_displaces_only_the_tile_currently_under_the_press() {
        let mut base = HubLayout::default();
        base.reconcile(&["A".to_string(), "B".to_string(), "C".to_string()], &[]);
        // resume, A, B, C, favorites, recents, update, settings
        let mut layout = base.clone();
        assert!(layout.reseat_held_item(&base, 1, 2));
        let ids: Vec<&str> = layout.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "resume",
                "B",
                "A",
                "C",
                "favorites",
                "recents",
                "update",
                "settings"
            ],
            "one step: A and B trade places"
        );

        // A second press further along must NOT chain off the first
        // press's result -- B must snap back home and only C, the tile now
        // under the press, gets displaced.
        assert!(layout.reseat_held_item(&base, 1, 3));
        let ids: Vec<&str> = layout.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "resume",
                "C",
                "B",
                "A",
                "favorites",
                "recents",
                "update",
                "settings"
            ],
            "B must return home; only C (currently under the press) is displaced"
        );
    }

    #[test]
    fn reseat_held_item_onto_its_own_origin_is_a_full_restore() {
        let mut base = HubLayout::default();
        base.reconcile(&["A".to_string(), "B".to_string()], &[]);
        let mut layout = base.clone();
        assert!(layout.reseat_held_item(&base, 1, 4));
        assert_ne!(
            layout.items, base.items,
            "sanity: the move actually changed something"
        );
        assert!(
            layout.reseat_held_item(&base, 1, 1),
            "landing back on the origin must succeed, unlike place_visible_item's from == to rejection"
        );
        assert_eq!(layout.items, base.items, "must reproduce base exactly");
    }

    #[test]
    fn reseat_held_item_past_the_end_does_not_accumulate_padding() {
        let mut base = HubLayout::default();
        base.reconcile(&["A".to_string()], &[]);
        let base_len = base.items.len();
        let mut layout = base.clone();
        assert!(layout.reseat_held_item(&base, 0, base_len + 2));
        assert_eq!(
            layout.items.len(),
            base_len + 3,
            "one press past the end pads exactly to the target"
        );

        // Stepping back inside base's range must drop that padding again,
        // not add to it -- each call rebuilds from `base`, it never chains.
        assert!(layout.reseat_held_item(&base, 0, 1));
        assert_eq!(
            layout.items.len(),
            base_len,
            "padding must not accumulate across presses"
        );
    }

    #[test]
    fn reseat_held_item_leaves_interior_blanks_the_user_left_untouched() {
        let mut base = HubLayout::default();
        base.items.push(HubItem::action("resume"));
        base.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        base.items.push(HubItem::action("settings"));
        // resume, blank, settings -- moving "resume" across the interior
        // blank onto "settings" must not disturb the blank.
        let mut layout = base.clone();
        assert!(layout.reseat_held_item(&base, 0, 2));
        let ids: Vec<&str> = layout.items.iter().map(|i| i.id.as_str()).collect();
        let kinds: Vec<HubItemKind> = layout.items.iter().map(HubItem::kind).collect();
        assert_eq!(ids, vec!["settings", "", "resume"]);
        assert_eq!(
            kinds,
            vec![HubItemKind::Action, HubItemKind::Blank, HubItemKind::Action,],
            "the interior blank must stay in place, untouched; only resume/settings trade"
        );
    }

    #[test]
    fn reseat_held_item_rejects_an_out_of_range_origin() {
        let mut base = HubLayout::default();
        base.reconcile(&["A".to_string()], &[]);
        let mut layout = base.clone();
        let before = layout.items.clone();
        assert!(!layout.reseat_held_item(&base, 999, 0));
        assert_eq!(
            layout.items, before,
            "a rejected reseat must not mutate `self`"
        );
    }

    #[test]
    fn trim_trailing_blanks_drops_only_the_run_after_the_last_real_item() {
        let mut layout = HubLayout::default();
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        layout.items.push(HubItem::action("resume"));
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        layout.trim_trailing_blanks();
        let kinds: Vec<HubItemKind> = layout.items.iter().map(HubItem::kind).collect();
        assert_eq!(
            kinds,
            vec![HubItemKind::Blank, HubItemKind::Action],
            "the leading blank must survive; only the trailing run is dropped"
        );
    }

    #[test]
    fn remove_visible_item_leaves_a_gap_and_shifts_nothing() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string(), "Console".to_string()], &[]);
        // resume, Arcade, Console, favorites, recents, update, settings
        assert!(layout.remove_visible_item(1));
        assert_eq!(layout.items[1].kind(), HubItemKind::Blank);
        assert_eq!(layout.items[2].id, "Console");
        assert!(layout.known.contains(&"category:Arcade".to_string()));
    }

    #[test]
    fn remove_visible_item_at_the_end_trims_rather_than_leaving_a_dangling_blank() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        let last = layout.items.len() - 1;
        assert_eq!(layout.items[last].id, "settings");
        assert!(layout.remove_visible_item(last));
        assert_eq!(layout.items.last().unwrap().id, "update");
    }

    #[test]
    fn remove_visible_item_out_of_range_is_a_no_op() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        assert!(!layout.remove_visible_item(999));
    }

    #[test]
    fn available_known_lists_only_known_keys_missing_from_items() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        assert!(layout.available_known().is_empty());
        let arcade_index = layout
            .items
            .iter()
            .position(|i| i.id == "Arcade")
            .expect("Arcade seeded");
        layout.remove_visible_item(arcade_index);
        assert_eq!(layout.available_known(), vec!["category:Arcade"]);
    }

    #[test]
    fn add_item_restores_a_removed_known_entry_into_the_gap_it_left() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        let arcade_index = layout
            .items
            .iter()
            .position(|i| i.id == "Arcade")
            .expect("Arcade seeded");
        layout.remove_visible_item(arcade_index);
        assert_eq!(layout.items[arcade_index].kind(), HubItemKind::Blank);
        assert!(layout.add_item("category", "Arcade", arcade_index));
        assert_eq!(layout.items[arcade_index].id, "Arcade");
        assert!(layout.available_known().is_empty());
    }

    #[test]
    fn add_item_targeting_a_non_blank_cell_appends_after_the_last_tile_instead() {
        let mut layout = HubLayout::default();
        layout.reconcile(
            &[
                "Arcade".to_string(),
                "Console".to_string(),
                "Handheld".to_string(),
            ],
            &[],
        );
        let handheld_index = layout
            .items
            .iter()
            .position(|i| i.id == "Handheld")
            .expect("Handheld seeded");
        layout.remove_visible_item(handheld_index);
        let before = layout.items.clone();
        // Target the "resume" cell (index 0) — not a blank, so the new
        // entry must not displace it.
        assert!(layout.add_item("category", "Handheld", 0));
        assert_eq!(&layout.items[..before.len()], before.as_slice());
        assert_eq!(layout.items.last().unwrap().id, "Handheld");
    }

    #[test]
    fn add_item_rejects_unknown_keys_and_duplicates() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        let end = layout.items.len();
        assert!(!layout.add_item("category", "Console", end));
        assert!(!layout.add_item("category", "Arcade", end));
    }

    #[test]
    fn add_item_blank_appended_at_the_end_is_not_immediately_trimmed_away() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        let end = layout.items.len();
        assert!(layout.add_item("blank", "", end));
        assert_eq!(
            layout.items.last().map(HubItem::kind),
            Some(HubItemKind::Blank),
            "a blank spacer the user just asked to add must survive, not be trimmed on arrival"
        );
    }

    #[test]
    fn add_item_blank_always_adds_an_untracked_spacer() {
        let mut layout = HubLayout::default();
        assert!(layout.add_item("blank", "", 0));
        assert!(layout.add_item("blank", "", 1));
        assert_eq!(
            layout
                .items
                .iter()
                .filter(|i| i.kind() == HubItemKind::Blank)
                .count(),
            2
        );
        assert!(layout.known.is_empty());
    }

    #[test]
    fn add_target_item_appends_a_system_shortcut() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        let before_len = layout.items.len();
        assert!(layout.add_target_item("system", "NES", "", "", "", "", ""));
        assert_eq!(layout.items.len(), before_len + 1);
        let added = layout.items.last().unwrap();
        assert_eq!(added.kind(), HubItemKind::System);
        assert_eq!(added.id, "NES");
    }

    #[test]
    fn add_target_item_appends_a_folder_shortcut_with_system_hint() {
        let mut layout = HubLayout::default();
        assert!(layout.add_target_item(
            "folder",
            "",
            "/media/fat/games/SNES/Homebrew",
            "",
            "",
            "",
            "SNES"
        ));
        let added = layout.items.last().unwrap();
        assert_eq!(added.kind(), HubItemKind::Folder);
        assert_eq!(added.path, "/media/fat/games/SNES/Homebrew");
        assert_eq!(added.system, "SNES");
    }

    #[test]
    fn add_target_item_appends_a_game_shortcut_with_cached_name() {
        let mut layout = HubLayout::default();
        assert!(layout.add_target_item(
            "zapscript",
            "",
            "/media/fat/games/NES/Zelda.nes",
            "/media/fat/games/NES/Zelda.nes",
            "The Legend of Zelda",
            "",
            "NES"
        ));
        let added = layout.items.last().unwrap();
        assert_eq!(added.kind(), HubItemKind::ZapScript);
        assert_eq!(added.script, "/media/fat/games/NES/Zelda.nes");
        assert_eq!(added.name, "The Legend of Zelda");
        assert_eq!(added.system, "NES");
    }

    #[test]
    fn add_target_item_rejects_a_kind_it_is_not_valid_for() {
        let mut layout = HubLayout::default();
        assert!(!layout.add_target_item("category", "Arcade", "", "", "", "", ""));
        assert!(!layout.add_target_item("action", "resume", "", "", "", "", ""));
        assert!(!layout.add_target_item("blank", "", "", "", "", "", ""));
        assert!(layout.items.is_empty());
    }

    #[test]
    fn add_target_item_never_consults_known() {
        // system/folder/zapscript are never tracked in `known` -- a target
        // shortcut must succeed against a layout that has never seen this
        // id before, unlike `add_item`'s category/action path.
        let mut layout = HubLayout::default();
        assert!(layout.known.is_empty());
        assert!(layout.add_target_item("system", "NES", "", "", "", "", ""));
        assert!(
            layout.known.is_empty(),
            "target shortcuts must not touch known"
        );
    }

    #[test]
    fn add_target_item_appends_after_a_preexisting_trailing_blank_without_touching_it() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        let blank_index = layout.items.len() - 1;
        assert!(layout.add_target_item("system", "NES", "", "", "", "", ""));
        assert_eq!(
            layout.items[blank_index].kind(),
            HubItemKind::Blank,
            "a straight append lands after the blank, not into it -- the blank must be left alone"
        );
        assert_eq!(layout.items.last().unwrap().kind(), HubItemKind::System);
    }

    #[test]
    fn reset_wipes_and_reseeds_from_detected_categories() {
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        layout.items.push(HubItem {
            kind_raw: "blank".to_string(),
            ..HubItem::default()
        });
        layout.reset(&["Console".to_string()]);
        let ids: Vec<&str> = layout.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "resume",
                "Console",
                "favorites",
                "recents",
                "update",
                "settings"
            ]
        );
        assert!(!layout.items.iter().any(|i| i.kind() == HubItemKind::Blank));
        assert_eq!(
            layout.known,
            vec![
                "action:resume",
                "category:Console",
                "action:favorites",
                "action:recents",
                "action:update",
                "action:settings"
            ]
        );
    }

    #[test]
    fn move_add_remove_never_shift_a_tile_that_was_not_touched() {
        // The board-model invariant: no cell you didn't explicitly target
        // ever changes position, whether that's from a move, a removal, or
        // an add landing elsewhere.
        let mut layout = HubLayout::default();
        layout.reconcile(
            &[
                "Arcade".to_string(),
                "Console".to_string(),
                "Handheld".to_string(),
            ],
            &[],
        );
        // resume, Arcade, Console, Handheld, favorites, recents, update, settings
        let snapshot = |l: &HubLayout| -> Vec<(usize, String)> {
            l.items
                .iter()
                .enumerate()
                .map(|(i, item)| (i, format!("{}:{}", item.kind_raw, item.id)))
                .collect()
        };

        // Move: swap indices 2 and 6. Every other index must be identical.
        let before = snapshot(&layout);
        assert!(layout.place_visible_item(2, 6));
        let after = snapshot(&layout);
        for (i, key) in &before {
            if *i == 2 || *i == 6 {
                continue;
            }
            assert_eq!(
                after[*i].1, *key,
                "index {i} moved from a swap it wasn't part of"
            );
        }

        // Remove: only index 1 changes.
        let before = snapshot(&layout);
        assert!(layout.remove_visible_item(1));
        let after = snapshot(&layout);
        for (i, key) in &before {
            if *i == 1 {
                continue;
            }
            assert_eq!(
                after[*i].1, *key,
                "index {i} moved from a removal elsewhere"
            );
        }

        // Add landing on the gap the removal (of Arcade, originally at
        // index 1) just left: only index 1 changes.
        let before = snapshot(&layout);
        assert!(layout.add_item("category", "Arcade", 1));
        let after = snapshot(&layout);
        for (i, key) in &before {
            if *i == 1 {
                continue;
            }
            assert_eq!(after[*i].1, *key, "index {i} moved from an add elsewhere");
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("frontend.toml");
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        layout.items.push(HubItem {
            kind_raw: "zapscript".to_string(),
            name: "Random NES".to_string(),
            script: "**launch.random:NES".to_string(),
            icon: "Dice".to_string(),
            ..HubItem::default()
        });
        save_hub_layout(&path, &layout).expect("save");

        let reloaded = load_hub_layout(&path);
        assert_eq!(reloaded.known, layout.known);
        assert_eq!(reloaded.items, layout.items);
    }

    #[test]
    fn save_preserves_comments_and_is_a_noop_when_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("frontend.toml");
        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        save_hub_layout(&path, &layout).expect("initial save");

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open for marker");
        file.write_all(b"\n# a user comment\n")
            .expect("append marker");
        drop(file);
        let before = std::fs::read_to_string(&path).expect("read before");

        save_hub_layout(&path, &layout).expect("no-op save");
        let after = std::fs::read_to_string(&path).expect("read after");
        assert_eq!(
            before, after,
            "unchanged save must not touch the file, comment included"
        );
    }

    #[test]
    fn save_preserves_other_config_sections() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("frontend.toml");
        std::fs::write(&path, "[core]\nendpoint = \"ws://example.com/api\"\n").expect("seed");

        let mut layout = HubLayout::default();
        layout.reconcile(&["Arcade".to_string()], &[]);
        save_hub_layout(&path, &layout).expect("save");

        let contents = std::fs::read_to_string(&path).expect("read");
        assert!(contents.contains("ws://example.com/api"));
    }
}
