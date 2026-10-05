// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Keeps the Hub's pinned games and folders on Core's current rows. A tile
// stores the identifiers Core reported when it was pinned; this pass asks
// Core where that media is now and writes back what it answers, so a tile
// pinned before Core reported a relative path gains one, and a tile whose
// games moved to another drive gets its absolute path (the cover key and the
// folder route) corrected once Core has rescanned.
//
// Nothing here derives an identifier. Every value written is one Core
// returned for the tile's own media, and a tile Core cannot place is left
// exactly as it was.

use slint::ComponentHandle;
use zaparoo_app::hub::{self as rules, LayoutItem, Located, Refresh};
use zaparoo_core::client::Client;
use zaparoo_core::hub_layout::{HubItem, HubLayout};
use zaparoo_core::media_types::{MediaBrowseParams, MediaLookupParams, MediaMetaParams};

use crate::router::{lock, Ctx};
use crate::App;

/// The questions the pass asks Core. `None` is "Core has no such media",
/// whether it said so or could not be reached.
trait Lookups {
    /// The indexed, present media row at `(system, path)`. `path` may be
    /// absolute or launcher-relative.
    async fn media(&self, system: &str, path: &str) -> Option<Located>;
    /// The media row Core's title matcher resolves `name` to.
    async fn title(&self, system: &str, name: &str) -> Option<Located>;
    /// The folder at `path` (absolute or launcher-relative), when it holds
    /// indexed content for `system`.
    async fn folder(&self, system: &str, path: &str) -> Option<Located>;
}

impl Lookups for Client {
    async fn media(&self, system: &str, path: &str) -> Option<Located> {
        let media = self
            .media_meta(MediaMetaParams {
                system: system.to_string(),
                path: path.to_string(),
                ..MediaMetaParams::default()
            })
            .await
            .ok()?
            .media;
        (!media.is_missing && !media.path.is_empty()).then(|| Located {
            path: media.path,
            relative: media.relative_path.unwrap_or_default(),
            script: media.zap_script,
        })
    }

    async fn title(&self, system: &str, name: &str) -> Option<Located> {
        let matched = self
            .media_lookup(MediaLookupParams {
                system: system.to_string(),
                name: name.to_string(),
                fuzzy_system: None,
            })
            .await
            .ok()?
            .matched?;
        Some(Located {
            path: matched.path,
            relative: matched.relative_path.unwrap_or_default(),
            script: matched.zap_script,
        })
    }

    async fn folder(&self, system: &str, path: &str) -> Option<Located> {
        let result = self
            .media_browse(MediaBrowseParams {
                path: path.to_string(),
                systems: vec![system.to_string()],
                max_results: Some(1),
                ..MediaBrowseParams::default()
            })
            .await
            .ok()?;
        (!result.entries.is_empty() && !result.path.is_empty()).then(|| Located {
            path: result.path,
            relative: result.relative_path.unwrap_or_default(),
            script: String::new(),
        })
    }
}

fn layout_item(item: &HubItem) -> LayoutItem {
    LayoutItem {
        kind: item.kind_raw.clone(),
        id: item.id.clone(),
        path: item.path.clone(),
        relative: item.relative.clone(),
        script: item.script.clone(),
        name: item.name.clone(),
        icon: item.icon.clone(),
        system: item.system.clone(),
    }
}

/// Where Core says a pinned game is. Its stored path is asked first: an
/// exact reference that is also the common case. A game that is no longer
/// there is found by its relative path. A tile from before relative paths
/// has neither, so it falls back to Core's title matcher, whose answer
/// counts only when it names the same file.
async fn locate_game(core: &impl Lookups, item: &HubItem) -> Option<Located> {
    if let Some(found) = core.media(&item.system, &item.path).await {
        return Some(found);
    }
    if !item.relative.is_empty() {
        return core.media(&item.system, &item.relative).await;
    }
    if item.name.is_empty() {
        return None;
    }
    let found = core.title(&item.system, &item.name).await?;
    rules::same_file_name(&item.path, &found.path).then_some(found)
}

async fn locate_folder(core: &impl Lookups, item: &HubItem) -> Option<Located> {
    if let Some(found) = core.folder(&item.system, &item.path).await {
        return Some(found);
    }
    if item.relative.is_empty() {
        return None;
    }
    core.folder(&item.system, &item.relative).await
}

/// The tile as Core's current answer makes it, or `None` when the tile is
/// not one this pass covers, Core cannot place it, or nothing changes.
async fn refreshed(core: &impl Lookups, item: &HubItem) -> Option<HubItem> {
    let current = layout_item(item);
    let found = match rules::refresh_kind(&current)? {
        Refresh::Game => locate_game(core, item).await?,
        Refresh::Folder => locate_folder(core, item).await?,
    };
    let next = rules::with_located(&current, &found)?;
    Some(HubItem {
        path: next.path,
        relative: next.relative,
        script: next.script,
        ..item.clone()
    })
}

/// Write each refreshed tile over the item it was computed from. An item
/// the user moved keeps its update; one they edited or removed meanwhile
/// no longer matches and is skipped.
fn apply_updates(layout: &mut HubLayout, updates: Vec<(HubItem, HubItem)>) -> bool {
    let mut changed = false;
    for (before, after) in updates {
        if let Some(slot) = layout.items.iter_mut().find(|item| **item == before) {
            *slot = after;
            changed = true;
        }
    }
    changed
}

/// Start a pass over the current layout, replacing any pass still running.
/// Called once the catalog is in and again whenever Core finishes changing
/// its media database; a database that is still being built is skipped,
/// since its finish schedules the pass anyway.
pub fn schedule(ctx: &Ctx, app: &App) {
    {
        let status = ctx.store.media_status().subscribe();
        let state = status.borrow();
        if state.indexing || state.optimizing {
            return;
        }
    }
    let targets: Vec<HubItem> = {
        let shared = lock(&ctx.shared);
        shared
            .hub
            .layout
            .items
            .iter()
            .filter(|item| rules::refresh_kind(&layout_item(item)).is_some())
            .cloned()
            .collect()
    };
    if targets.is_empty() {
        return;
    }
    let client = ctx.store.client();
    let weak = app.as_weak();
    let task_ctx = ctx.clone();
    let task = ctx.handle.spawn(async move {
        let mut state = client.connection.subscribe();
        let _ = state
            .wait_for(|s| *s == zaparoo_core::client::ConnectionState::Connected)
            .await;
        let mut updates = Vec::new();
        for item in targets {
            if *task_ctx.dormant.borrow() {
                return;
            }
            if let Some(next) = refreshed(client.as_ref(), &item).await {
                updates.push((item, next));
            }
        }
        if updates.is_empty() {
            return;
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            let changed = {
                let mut shared = lock(&task_ctx.shared);
                let hub = &mut shared.hub;
                // A Move session works from its own snapshot of the layout;
                // the next pass picks these tiles up instead.
                let changed = !hub.move_armed() && apply_updates(&mut hub.layout, updates);
                if changed {
                    hub.save(&task_ctx.handle);
                }
                changed
            };
            if changed {
                crate::hub::rebuild(&task_ctx, &app);
            }
        });
    });
    lock(&ctx.shared).hub.refresh.replace(task);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests should fail fast")]

    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A Core that knows a fixed set of media and folders, and records what
    /// it was asked.
    #[derive(Default)]
    struct FakeCore {
        media: HashMap<(&'static str, &'static str), Located>,
        titles: HashMap<(&'static str, &'static str), Located>,
        folders: HashMap<(&'static str, &'static str), Located>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeCore {
        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }

        fn find(
            &self,
            what: &str,
            map: &HashMap<(&'static str, &'static str), Located>,
            system: &str,
            key: &str,
        ) -> Option<Located> {
            self.asked.lock().unwrap().push(format!("{what} {key}"));
            map.iter()
                .find(|((s, k), _)| *s == system && *k == key)
                .map(|(_, found)| found.clone())
        }
    }

    impl Lookups for FakeCore {
        async fn media(&self, system: &str, path: &str) -> Option<Located> {
            self.find("media", &self.media, system, path)
        }
        async fn title(&self, system: &str, name: &str) -> Option<Located> {
            self.find("title", &self.titles, system, name)
        }
        async fn folder(&self, system: &str, path: &str) -> Option<Located> {
            self.find("folder", &self.folders, system, path)
        }
    }

    fn located(path: &str, relative: &str, script: &str) -> Located {
        Located {
            path: path.into(),
            relative: relative.into(),
            script: script.into(),
        }
    }

    fn game(relative: &str, script: &str, path: &str) -> HubItem {
        HubItem {
            kind_raw: "zapscript".into(),
            system: "NES".into(),
            name: "Zelda".into(),
            relative: relative.into(),
            script: script.into(),
            path: path.into(),
            ..HubItem::default()
        }
    }

    fn folder(relative: &str, path: &str) -> HubItem {
        HubItem {
            kind_raw: "folder".into(),
            system: "SNES".into(),
            relative: relative.into(),
            path: path.into(),
            ..HubItem::default()
        }
    }

    #[tokio::test]
    async fn an_old_pin_gains_the_identifiers_core_reports_for_its_path() {
        let mut core = FakeCore::default();
        core.media.insert(
            ("NES", "/fat/NES/Zelda.nes"),
            located("/fat/NES/Zelda.nes", "NES/Zelda.nes", "@NES/Zelda"),
        );
        let old = game("", "/fat/NES/Zelda.nes", "/fat/NES/Zelda.nes");

        let next = refreshed(&core, &old).await.unwrap();

        assert_eq!(
            next,
            game("NES/Zelda.nes", "@NES/Zelda", "/fat/NES/Zelda.nes")
        );
        assert_eq!(core.asked(), ["media /fat/NES/Zelda.nes"]);
    }

    #[tokio::test]
    async fn a_moved_game_is_found_by_its_relative_path() {
        let mut core = FakeCore::default();
        core.media.insert(
            ("NES", "NES/Zelda.nes"),
            located("/usb/NES/Zelda.nes", "NES/Zelda.nes", "@NES/Zelda"),
        );
        let pinned = game("NES/Zelda.nes", "@NES/Zelda", "/fat/NES/Zelda.nes");

        let next = refreshed(&core, &pinned).await.unwrap();

        assert_eq!(next.path, "/usb/NES/Zelda.nes");
        assert_eq!(next.relative, "NES/Zelda.nes");
        assert_eq!(
            core.asked(),
            ["media /fat/NES/Zelda.nes", "media NES/Zelda.nes"],
            "the title matcher is never consulted for a tile with a relative path"
        );
    }

    #[tokio::test]
    async fn an_up_to_date_tile_is_left_alone() {
        let mut core = FakeCore::default();
        core.media.insert(
            ("NES", "/fat/NES/Zelda.nes"),
            located("/fat/NES/Zelda.nes", "NES/Zelda.nes", "@NES/Zelda"),
        );
        let pinned = game("NES/Zelda.nes", "@NES/Zelda", "/fat/NES/Zelda.nes");
        assert_eq!(refreshed(&core, &pinned).await, None);
    }

    #[tokio::test]
    async fn an_old_core_that_reports_no_portable_form_changes_nothing() {
        let mut core = FakeCore::default();
        core.media.insert(
            ("NES", "/fat/NES/Zelda.nes"),
            located("/fat/NES/Zelda.nes", "", ""),
        );
        let old = game("", "/fat/NES/Zelda.nes", "/fat/NES/Zelda.nes");
        assert_eq!(refreshed(&core, &old).await, None);
    }

    #[tokio::test]
    async fn a_stale_old_pin_is_repaired_only_by_a_title_match_naming_the_same_file() {
        let old = game("", "/fat/NES/Zelda.nes", "/fat/NES/Zelda.nes");

        let mut same_file = FakeCore::default();
        same_file.titles.insert(
            ("NES", "Zelda"),
            located("/usb/games/NES/Zelda.nes", "NES/Zelda.nes", "@NES/Zelda"),
        );
        assert_eq!(
            refreshed(&same_file, &old).await.unwrap(),
            game("NES/Zelda.nes", "@NES/Zelda", "/usb/games/NES/Zelda.nes")
        );

        let mut other_file = FakeCore::default();
        other_file.titles.insert(
            ("NES", "Zelda"),
            located(
                "/usb/games/NES/Zelda (Europe).nes",
                "NES/Zelda (Europe).nes",
                "@NES/Zelda",
            ),
        );
        assert_eq!(refreshed(&other_file, &old).await, None);

        assert_eq!(refreshed(&FakeCore::default(), &old).await, None);
    }

    #[tokio::test]
    async fn a_hand_written_tile_is_never_looked_up() {
        let core = FakeCore::default();
        let custom = HubItem {
            kind_raw: "zapscript".into(),
            system: "NES".into(),
            script: "**launch.random:NES".into(),
            path: "/fat/NES/Zelda.nes".into(),
            ..HubItem::default()
        };
        assert_eq!(refreshed(&core, &custom).await, None);
        assert!(core.asked().is_empty());
    }

    #[tokio::test]
    async fn a_folder_gains_its_relative_path_and_follows_a_move() {
        let mut core = FakeCore::default();
        core.folders.insert(
            ("SNES", "/fat/SNES/USA"),
            located("/fat/SNES/USA", "SNES/USA", ""),
        );
        let old = folder("", "/fat/SNES/USA");
        let upgraded = refreshed(&core, &old).await.unwrap();
        assert_eq!(upgraded, folder("SNES/USA", "/fat/SNES/USA"));

        let mut moved = FakeCore::default();
        moved.folders.insert(
            ("SNES", "SNES/USA"),
            located("/usb/SNES/USA", "SNES/USA", ""),
        );
        let healed = refreshed(&moved, &upgraded).await.unwrap();
        assert_eq!(healed, folder("SNES/USA", "/usb/SNES/USA"));
        assert_eq!(moved.asked(), ["folder /fat/SNES/USA", "folder SNES/USA"]);

        // A stale folder with no relative path has nothing exact to ask by.
        assert_eq!(refreshed(&moved, &old).await, None);
    }

    #[test]
    fn an_update_lands_on_the_item_it_was_computed_from_wherever_it_now_sits() {
        let before = game("", "/fat/NES/Zelda.nes", "/fat/NES/Zelda.nes");
        let after = game("NES/Zelda.nes", "@NES/Zelda", "/fat/NES/Zelda.nes");
        let other = folder("", "/fat/SNES/USA");

        let mut moved = HubLayout {
            items: vec![other.clone(), before.clone()],
            ..HubLayout::default()
        };
        assert!(apply_updates(
            &mut moved,
            vec![(before.clone(), after.clone())]
        ));
        assert_eq!(moved.items, vec![other.clone(), after.clone()]);

        let mut removed = HubLayout {
            items: vec![other.clone()],
            ..HubLayout::default()
        };
        assert!(!apply_updates(
            &mut removed,
            vec![(before.clone(), after.clone())]
        ));
        assert_eq!(removed.items, vec![other.clone()]);

        let renamed = HubItem {
            name: "My Zelda".into(),
            ..before.clone()
        };
        let mut edited = HubLayout {
            items: vec![renamed.clone()],
            ..HubLayout::default()
        };
        assert!(!apply_updates(&mut edited, vec![(before, after)]));
        assert_eq!(edited.items, vec![renamed]);
    }
}
