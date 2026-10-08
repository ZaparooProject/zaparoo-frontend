// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaTagsUpdateMutation` — add/remove mutable user tags for a media row.

use crate::client::{Client, ClientError};
use crate::endpoints::{
    media_browse::MediaBrowseEndpoint, media_favorites::MediaFavoritesEndpoint,
    media_history::MediaHistoryEndpoint, media_search::MediaSearchEndpoint,
    systems_favorites::SystemsFavoritesEndpoint,
};
use crate::media_types::{MediaTagsUpdateParams, MediaTagsUpdateResult};
use crate::store::{Endpoint, Mutation, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct MediaTagsUpdateMutation;

impl Mutation for MediaTagsUpdateMutation {
    type Args = MediaTagsUpdateParams;
    type Output = MediaTagsUpdateResult;

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.media_tags_update(args).await })
    }

    fn invalidates(_args: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
        vec![
            Tag::any(MediaBrowseEndpoint::NAME),
            Tag::any(MediaFavoritesEndpoint::NAME),
            Tag::any(MediaHistoryEndpoint::NAME),
            Tag::any(MediaSearchEndpoint::NAME),
            Tag::any(SystemsFavoritesEndpoint::NAME),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoints::media_browse::BrowseArgs;
    use crate::remote_resource::ResourceStatus;
    use crate::store::Store;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn tag_updates_invalidate_history_as_well_as_browse_and_favorites() {
        let tags = MediaTagsUpdateMutation::invalidates(
            &MediaTagsUpdateParams::default(),
            &MediaTagsUpdateResult::default(),
        );
        for kind in [
            MediaBrowseEndpoint::NAME,
            MediaFavoritesEndpoint::NAME,
            MediaHistoryEndpoint::NAME,
            MediaSearchEndpoint::NAME,
            SystemsFavoritesEndpoint::NAME,
        ] {
            assert!(tags.contains(&Tag::any(kind)));
        }
    }

    #[tokio::test]
    #[allow(
        clippy::unwrap_used,
        clippy::too_many_lines,
        reason = "bounded WebSocket hide/unhide regression"
    )]
    async fn hide_unhide_refetches_both_visibility_modes_without_stale_first_pages() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("ws://{}/api/v0.1", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let mut hidden = false;
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    let request: Value = serde_json::from_str(&text).unwrap();
                    let params = &request["params"];
                    let tags = if hidden { json!([{"type": "user", "tag": "hidden"}]) } else { json!([]) };
                    let result = match request["method"].as_str().unwrap() {
                        "media.browse" => {
                            let include_hidden = params["includeHidden"].as_bool().unwrap();
                            let entries = if !hidden || include_hidden {
                                json!([{"mediaId": 42, "name": "Game", "path": "/g/Game.nes", "type": "media", "systemId": "NES", "tags": tags}])
                            } else { json!([]) };
                            json!({"totalFiles": entries.as_array().unwrap().len(), "entries": entries})
                        }
                        "media.tags.update" => {
                            assert_eq!(params["mediaId"], 42);
                            assert!(params.get("system").is_none());
                            assert!(params.get("path").is_none());
                            if params.get("add").is_some() {
                                assert_eq!(params["add"], json!(["user:hidden"]));
                                hidden = true;
                            } else {
                                assert_eq!(params["remove"], json!(["user:hidden"]));
                                hidden = false;
                            }
                            json!({"tags": if hidden { json!([{"type": "user", "tag": "hidden"}]) } else { json!([]) }})
                        }
                        _ => json!({}),
                    };
                    socket.send(Message::Text(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}).to_string().into())).await.unwrap();
                }
            });
            let client = Client::new(endpoint, &tokio::runtime::Handle::current());
            let store = Store::new(client, tokio::runtime::Handle::current());
            let args = BrowseArgs::new(String::new(), vec!["NES".into()], 100, Vec::new());
            let visible = store.subscribe::<MediaBrowseEndpoint>(args.clone());
            let all = store.subscribe::<MediaBrowseEndpoint>(args.with_hidden(true));
            let mut visible_rx = visible.subscribe();
            let mut all_rx = all.subscribe();
            assert!(!visible_rx.same_channel(&all_rx));
            for rx in [&mut visible_rx, &mut all_rx] {
                rx.wait_for(|s| matches!(s, ResourceStatus::Ready(r) if r.entries.len() == 1)).await.unwrap();
            }
            store.run_mutation::<MediaTagsUpdateMutation>(MediaTagsUpdateParams {
                media_id: Some(42), add: vec!["user:hidden".into()], ..MediaTagsUpdateParams::default()
            }).await.unwrap();
            assert!(matches!(*visible.subscribe().borrow(), ResourceStatus::Loading));
            visible_rx.wait_for(|s| matches!(s, ResourceStatus::Ready(r) if r.entries.is_empty())).await.unwrap();
            all_rx.wait_for(|s| matches!(s, ResourceStatus::Ready(r) if r.entries.first().is_some_and(|e| e.tags.iter().any(|t| t.tag_type == "user" && t.tag == "hidden")))).await.unwrap();
            store.run_mutation::<MediaTagsUpdateMutation>(MediaTagsUpdateParams {
                media_id: Some(42), remove: vec!["user:hidden".into()], ..MediaTagsUpdateParams::default()
            }).await.unwrap();
            visible_rx.wait_for(|s| matches!(s, ResourceStatus::Ready(r) if r.entries.len() == 1)).await.unwrap();
            all_rx.wait_for(|s| matches!(s, ResourceStatus::Ready(r) if r.entries.first().is_some_and(|e| e.tags.is_empty()))).await.unwrap();
            drop(visible_rx);
            drop(all_rx);
            drop(visible);
            drop(all);
            // Games releases its initial-page lease after filling the view.
            // A later hide must also invalidate that inactive retained payload.
            store.run_mutation::<MediaTagsUpdateMutation>(MediaTagsUpdateParams {
                media_id: Some(42), add: vec!["user:hidden".into()], ..MediaTagsUpdateParams::default()
            }).await.unwrap();
            let reopened = store.subscribe::<MediaBrowseEndpoint>(BrowseArgs::new(String::new(), vec!["NES".into()], 100, Vec::new()));
            let mut reopened_rx = reopened.subscribe();
            assert!(matches!(*reopened_rx.borrow(), ResourceStatus::Loading));
            reopened_rx.wait_for(|s| matches!(s, ResourceStatus::Ready(r) if r.entries.is_empty())).await.unwrap();
            server.abort();
        }).await.unwrap();
    }
}
