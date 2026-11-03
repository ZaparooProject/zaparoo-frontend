// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Optional embedding-host image picker for Change icon… on a Hub tile. The
//! host runs its own picker and answers with a readable absolute path,
//! which becomes the tile's `icon` exactly as a pick in the file browser
//! does. Rules live in `zaparoo_app::image_picker`.

use crate::router::Ctx;
use crate::App;
use std::sync::{Arc, Mutex};
use zaparoo_app::image_picker::{Model as Rules, State};
use zaparoo_core::persist::HubTarget;

type Request = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Default)]
struct Data {
    request: Option<Request>,
    rules: Rules,
    /// The tile the open request is for.
    target: Option<HubTarget>,
}

#[derive(Default, Clone)]
pub struct Model(Arc<Mutex<Data>>);

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImagePicker").finish_non_exhaustive()
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Model {
    pub fn available(&self) -> bool {
        lock(&self.0).request.is_some()
    }

    #[cfg(any(feature = "hosted", test))]
    pub fn configure(&self, request: Request) {
        lock(&self.0).request = Some(request);
    }

    #[cfg(any(feature = "hosted", test))]
    pub fn pending(&self) -> bool {
        lock(&self.0).rules.pending()
    }
}

/// Ask the host for an image for `target`. A request already open is
/// left to finish.
pub(crate) fn request(ctx: &Ctx, app: &App, target: HubTarget) {
    let callback = {
        let mut data = lock(&ctx.images.0);
        let Some(callback) = data.request.clone() else {
            return;
        };
        if !data.rules.begin() {
            return;
        }
        data.target = Some(target);
        callback
    };
    crate::input::stop_repeat(ctx);
    if !callback() {
        let mut data = lock(&ctx.images.0);
        data.rules.state = State::Failed;
        data.target = None;
        drop(data);
        crate::router::report_action_error(ctx, app, "image_picker", "");
    }
}

/// The host's answer, ordered by revision. A pick lands on the tile it was
/// asked for, while that tile is still where it was.
#[cfg(feature = "hosted")]
pub(crate) fn answered(ctx: &Ctx, app: &App, revision: u64, state: State) {
    let (state, target) = {
        let mut data = lock(&ctx.images.0);
        if !data.rules.update(revision, state) {
            return;
        }
        (data.rules.state.clone(), data.target.take())
    };
    let Some(target) = target else {
        return;
    };
    match state {
        // The host promised a readable file; one that is not would save an
        // icon that can never show.
        State::Picked(path)
            if std::fs::File::open(&path)
                .is_ok_and(|file| file.metadata().is_ok_and(|meta| meta.is_file())) =>
        {
            crate::hub::set_tile_icon(ctx, app, &target, &path);
            crate::hub::rebuild(ctx, app);
        }
        State::Picked(path) => {
            tracing::warn!("host image picker answered with a file that cannot be read: {path}");
            crate::router::report_action_error(ctx, app, "image_picker", "");
        }
        State::Denied | State::Failed => {
            crate::router::report_action_error(ctx, app, "image_picker", "");
        }
        State::Ready | State::Opening | State::Cancelled => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_picker_is_available_and_tracks_its_open_request() {
        let model = Model::default();
        assert!(!model.available());
        model.configure(Arc::new(|| true));
        assert!(model.available());
        assert!(!model.pending());
        assert!(lock(&model.0).rules.begin());
        assert!(model.pending());
    }
}
