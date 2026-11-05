// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `DecksEndpoint` (the deck list, optionally with one game's membership),
// `DeckEndpoint` (one deck with its items) and the deck mutations. Every
// mutation, and every `decks.changed`, refreshes both kinds, and the
// browse and search results whose rows carry `user:deck:<id>` membership.

use crate::client::{Client, ClientError};
use crate::collections::{Deck, DeckNewParams, DeckUpdateParams, DecksParams, DecksResult};
use crate::endpoints::media_browse::MediaBrowseEndpoint;
use crate::endpoints::media_search::MediaSearchEndpoint;
use crate::store::{Endpoint, Mutation, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct DecksEndpoint;

impl Endpoint for DecksEndpoint {
    type Args = DecksParams;
    type Output = DecksResult;
    const NAME: &'static str = "Decks";

    fn cache_bytes(output: &Self::Output) -> Option<usize> {
        Some(super::retained::bytes(output))
    }

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.decks(args).await })
    }
}

#[derive(Debug)]
pub struct DeckEndpoint;

impl Endpoint for DeckEndpoint {
    /// The deck id.
    type Args = String;
    type Output = Deck;
    const NAME: &'static str = "Deck";

    fn cache_bytes(output: &Self::Output) -> Option<usize> {
        Some(super::retained::bytes(output))
    }

    fn fetch(
        client: Arc<Client>,
        deck_id: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.deck_get(&deck_id).await })
    }

    fn provides(deck_id: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::specific(Self::NAME, deck_id.clone())]
    }
}

/// What a change to one deck, or to the deck list, makes stale: the decks,
/// and the browse and search rows whose `user:deck:<id>` tags name them.
#[must_use]
pub fn deck_tags(deck_id: Option<&str>) -> Vec<Tag> {
    vec![
        Tag::any(DecksEndpoint::NAME),
        deck_id.map_or(Tag::any(DeckEndpoint::NAME), |id| {
            Tag::specific(DeckEndpoint::NAME, id)
        }),
        Tag::any(MediaBrowseEndpoint::NAME),
        Tag::any(MediaSearchEndpoint::NAME),
    ]
}

#[derive(Debug)]
pub struct DeckNewMutation;

impl Mutation for DeckNewMutation {
    type Args = DeckNewParams;
    type Output = Deck;

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.deck_new(args).await })
    }

    fn invalidates(_args: &Self::Args, result: &Self::Output) -> Vec<Tag> {
        deck_tags(Some(&result.deck_id))
    }
}

#[derive(Debug)]
pub struct DeckUpdateMutation;

impl Mutation for DeckUpdateMutation {
    type Args = DeckUpdateParams;
    type Output = Deck;

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.deck_update(args).await })
    }

    fn invalidates(args: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
        deck_tags(Some(&args.deck_id))
    }
}

#[derive(Debug)]
pub struct DeckDeleteMutation;

impl Mutation for DeckDeleteMutation {
    /// The deck id.
    type Args = String;
    type Output = ();

    fn run(
        client: Arc<Client>,
        deck_id: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.deck_delete(&deck_id).await })
    }

    fn invalidates(deck_id: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
        deck_tags(Some(deck_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deck_change_refreshes_the_list_and_only_that_deck() {
        let tags = DeckUpdateMutation::invalidates(
            &DeckUpdateParams {
                deck_id: "abc".into(),
                ..DeckUpdateParams::default()
            },
            &Deck::default(),
        );
        assert_eq!(
            tags,
            vec![
                Tag::any("Decks"),
                Tag::specific("Deck", "abc"),
                Tag::any("MediaBrowse"),
                Tag::any("MediaSearch"),
            ]
        );
        assert_eq!(
            deck_tags(None),
            vec![
                Tag::any("Decks"),
                Tag::any("Deck"),
                Tag::any("MediaBrowse"),
                Tag::any("MediaSearch"),
            ]
        );
    }
}
