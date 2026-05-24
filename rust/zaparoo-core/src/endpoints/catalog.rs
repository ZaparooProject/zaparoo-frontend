// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `CatalogEndpoint` — single-fetch endpoint for the systems catalog.
// `Args = ()` because the catalog is a global per-connection resource;
// every QML singleton that needs categories or systems shares the one
// `RemoteResource<CatalogData>` the store hands back.
//
// `fetch` does the same shaping the old `systems_catalog::spawn`
// closure did: pull the systems list from Core, sort by name, derive
// the category list. The split into `shape_catalog` exists so the unit
// test can exercise the pipeline without a network call.

use crate::client::{Client, ClientError};
use crate::media_types::{SystemInfo, SystemsParams};
use crate::store::{Endpoint, Tag};
use crate::systems_catalog::CatalogData;
use futures_util::future::BoxFuture;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::info;

#[derive(Debug)]
pub struct CatalogEndpoint;

impl Endpoint for CatalogEndpoint {
    type Args = ();
    type Output = CatalogData;
    const NAME: &'static str = "Catalog";

    fn fetch(
        client: Arc<Client>,
        _args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            let result = client.systems(SystemsParams {}).await?;
            Ok(shape_catalog(result.systems))
        })
    }

    /// Declares the cross-endpoint `Tag::MEDIA_DB` so the catalog is
    /// refetched whenever the store sees an indexing/optimizing run
    /// finish. Without this, the frontend's startup catalog query
    /// (which races Core's first-run DB build) sticks at zero systems
    /// for the rest of the session.
    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME), Tag::MEDIA_DB]
    }
}

/// Apply the canonical sort + category derivation to a freshly-fetched
/// systems list. Pulled out of `fetch` so tests can drive a deterministic
/// fixture without standing up a `Client`.
fn shape_catalog(mut systems: Vec<SystemInfo>) -> CatalogData {
    systems.sort_by_key(|a| a.name.to_lowercase());
    let categories = derive_categories(&systems);
    info!(
        "catalog loaded: {} systems, {} categories",
        systems.len(),
        categories.len()
    );
    CatalogData {
        systems,
        categories,
    }
}

fn derive_categories(systems: &[SystemInfo]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut cats: Vec<String> = Vec::new();
    for s in systems {
        let cat = if s.category.is_empty() {
            "Other".to_string()
        } else {
            s.category.clone()
        };
        let lower = cat.to_lowercase();
        if seen.insert(lower) {
            cats.push(cat);
        }
    }
    cats.sort_by_key(|a| a.to_lowercase());
    cats
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(id: &str, name: &str, category: &str) -> SystemInfo {
        SystemInfo {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            ..SystemInfo::default()
        }
    }

    #[test]
    fn derive_categories_sorts_case_insensitively() {
        let systems = vec![
            sys("a", "A", "Handhelds"),
            sys("b", "B", "arcade"),
            sys("c", "C", "Consoles"),
        ];
        assert_eq!(
            derive_categories(&systems),
            vec!["arcade", "Consoles", "Handhelds"],
        );
    }

    #[test]
    fn derive_categories_dedupes_case_insensitively() {
        let systems = vec![
            sys("a", "A", "Arcade"),
            sys("b", "B", "arcade"),
            sys("c", "C", "ARCADE"),
        ];
        let cats = derive_categories(&systems);
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0], "Arcade"); // first encountered casing wins
    }

    #[test]
    fn derive_categories_synthesizes_other_for_empty() {
        let systems = vec![sys("a", "A", ""), sys("b", "B", "Consoles")];
        assert_eq!(derive_categories(&systems), vec!["Consoles", "Other"]);
    }

    #[test]
    fn shape_catalog_snapshot_matches_fixture() {
        let systems = vec![
            sys("SNES", "Super Nintendo", "Consoles"),
            sys("NES", "Nintendo", "Consoles"),
            sys("Gameboy", "Game Boy", "Handhelds"),
            sys("MAME", "MAME", "arcade"),
            sys("odd", "Odd One", ""),
        ];
        insta::assert_debug_snapshot!(shape_catalog(systems));
    }
}
