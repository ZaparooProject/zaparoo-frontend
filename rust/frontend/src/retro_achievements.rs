// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Settings > Library > RetroAchievements on a MiSTer: odelot's
// RetroAchievements Main (`MiSTer_RA`) and the file it reads at the root
// of the SD card. This is the frontend's own stopgap until Core handles
// RetroAchievements itself: it reads and edits `retroachievements.cfg`,
// and checks (never edits) that MiSTer.ini sends the RetroAchievements
// cores to `MiSTer_RA`. Installing is Update All's job.
//
// The cfg is always written in place, never through a temporary file and a
// rename: Core can bind-mount a per-profile copy over the path, and a
// rename would replace the mount point instead of writing the profile's
// file. Rules live in `zaparoo_app::retro_achievements`.

use std::io::{Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};

use slint::ComponentHandle as _;
use zaparoo_app::retro_achievements::{self as rules, Edit, RaConfig};

use crate::router::{lock, Ctx};
use crate::App;

/// The SD card root `MiSTer_RA` reads its cfg from.
const MISTER_ROOT: &str = "/media/fat";
const CFG_FILE: &str = "retroachievements.cfg";
const RA_MAIN: &str = "MiSTer_RA";
const MISTER_INI: &str = "MiSTer.ini";

/// What the page shows, as last read from the card.
#[derive(Clone, Default)]
pub struct Model {
    pub cfg: RaConfig,
    pub ra_main: bool,
    pub ini_ra: bool,
    /// Bumped by every change, so a failed write only rolls back the
    /// newest one.
    seq: u64,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("username", &self.cfg.username)
            .field("logged_in", &self.cfg.logged_in())
            .field("ra_main", &self.ra_main)
            .field("ini_ra", &self.ini_ra)
            .finish_non_exhaustive()
    }
}

/// The frontend runs on the `MiSTer` whose card it would edit, and can reach
/// `RetroAchievements` to check a login.
pub(crate) fn available() -> bool {
    cfg!(not(feature = "hosted")) && zaparoo_core::runtime::current().is_mister()
}

fn root() -> PathBuf {
    PathBuf::from(MISTER_ROOT)
}

/// Read the card: the cfg, whether `MiSTer_RA` is there, and whether
/// MiSTer.ini switches to it.
fn read(root: &Path) -> (RaConfig, bool, bool) {
    let cfg = std::fs::read(root.join(CFG_FILE))
        .map(|bytes| rules::parse(&bytes))
        .unwrap_or_default();
    let ra_main = root.join(RA_MAIN).is_file();
    let ini_ra = std::fs::read(root.join(MISTER_INI))
        .is_ok_and(|bytes| rules::mister_ini_has_ra(&String::from_utf8_lossy(&bytes)));
    (cfg, ra_main, ini_ra)
}

/// Apply `edits` to the cfg where it is, creating it when it is missing.
/// Returns what the file holds afterwards.
fn write_in_place(path: &Path, edits: &[Edit]) -> std::io::Result<RaConfig> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let mut original = Vec::new();
    file.read_to_end(&mut original)?;
    let updated = rules::rewrite(&original, edits);
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(&updated)?;
    file.sync_all()?;
    Ok(rules::parse(&updated))
}

/// Read the card again and repaint the page. Settings asks when the
/// Library page opens.
pub(crate) fn refresh(ctx: &Ctx, app: &App) {
    if !available() {
        return;
    }
    let shared = ctx.shared.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let Ok((cfg, ra_main, ini_ra)) = tokio::task::spawn_blocking(|| read(&root())).await else {
            return;
        };
        {
            let mut shared = lock(&shared);
            shared.ra.cfg = cfg;
            shared.ra.ra_main = ra_main;
            shared.ra.ini_ra = ini_ra;
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::settings::refresh(&ctx2, &app);
            crate::settings::reseat(&ctx2, &app);
        });
    });
}

/// Every write runs on one blocking thread, one after another, and each
/// re-reads the file first, so two quick changes never undo each other.
static WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Write `edits`, then call `done` on the UI thread with the outcome.
fn write(
    ctx: &Ctx,
    app: &App,
    edits: Vec<Edit>,
    done: impl FnOnce(&Ctx, &App, std::io::Result<RaConfig>) + Send + 'static,
) {
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let _turn = WRITER
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            write_in_place(&root().join(CFG_FILE), &edits)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
        let _ = weak.upgrade_in_event_loop(move |app| done(&ctx2, &app, result));
    });
}

/// Show `edits` at once, write them, and put the page back if the write
/// fails.
fn change(ctx: &Ctx, app: &App, edits: Vec<Edit>) {
    let (before, seq) = {
        let mut shared = lock(&ctx.shared);
        let before = shared.ra.cfg.clone();
        rules::apply(&mut shared.ra.cfg, &edits);
        shared.ra.seq = shared.ra.seq.wrapping_add(1);
        (before, shared.ra.seq)
    };
    crate::settings::refresh(ctx, app);
    write(ctx, app, edits, move |ctx, app, result| {
        if let Err(e) = result {
            tracing::warn!("could not write {CFG_FILE}: {e}");
            {
                let mut shared = lock(&ctx.shared);
                if shared.ra.seq == seq {
                    shared.ra.cfg = before;
                }
            }
            crate::settings::refresh(ctx, app);
            crate::router::report_action_error(ctx, app, "setting", "");
        }
    });
}

/// Flip one of the cfg's on/off options.
pub(crate) fn toggle_option(ctx: &Ctx, app: &App, id: &str) {
    let on = !rules::option_checked(&lock(&ctx.shared).ra.cfg, id);
    let edits = rules::option_edits(id, on);
    if !edits.is_empty() {
        change(ctx, app, edits);
    }
}

pub(crate) fn set_popup_position(ctx: &Ctx, app: &App, value: &str) {
    let position = rules::PopupPosition::parse(value);
    if lock(&ctx.shared).ra.cfg.popup_position == position {
        return;
    }
    change(
        ctx,
        app,
        vec![Edit::Set(
            rules::POPUP_POSITION,
            position.token().to_string(),
        )],
    );
}

/// The user confirmed logging out: remove the login from the cfg.
pub(crate) fn log_out(ctx: &Ctx, app: &App) {
    change(
        ctx,
        app,
        vec![Edit::Remove(rules::USERNAME), Edit::Remove(rules::PASSWORD)],
    );
    crate::settings::reseat_to(ctx, app, "raLogIn");
}

/// Save a login `RetroAchievements` accepted, then call `done` with whether
/// it was written.
pub(crate) fn save_login(
    ctx: &Ctx,
    app: &App,
    username: String,
    password: String,
    done: impl FnOnce(&Ctx, &App, bool) + Send + 'static,
) {
    let edits = vec![
        Edit::Set(rules::USERNAME, username),
        Edit::Set(rules::PASSWORD, password),
    ];
    write(ctx, app, edits, move |ctx, app, result| match result {
        Ok(cfg) => {
            {
                let mut shared = lock(&ctx.shared);
                shared.ra.cfg = cfg;
                shared.ra.seq = shared.ra.seq.wrapping_add(1);
            }
            done(ctx, app, true);
        }
        Err(e) => {
            tracing::warn!("could not write {CFG_FILE}: {e}");
            done(ctx, app, false);
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a failed temp file is a test failure")]

    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zaparoo-ra-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_cfg_is_created_and_reads_back() {
        let root = temp_root("create");
        let path = root.join(CFG_FILE);
        let cfg = write_in_place(
            &path,
            &[
                Edit::Set(rules::USERNAME, "Me".into()),
                Edit::Set(rules::PASSWORD, "pw".into()),
            ],
        )
        .unwrap();
        assert!(cfg.logged_in());
        assert_eq!(std::fs::read(&path).unwrap(), b"username=Me\npassword=pw\n");
        let (read_back, ra_main, ini_ra) = read(&root);
        assert_eq!(read_back, cfg);
        assert!(!ra_main && !ini_ra);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn writing_keeps_the_same_file_and_every_other_line() {
        use std::os::unix::fs::MetadataExt as _;
        let root = temp_root("in-place");
        let path = root.join(CFG_FILE);
        std::fs::write(&path, "# mine\nusername=old\nsmart_cache=2\n").unwrap();
        let inode = std::fs::metadata(&path).unwrap().ino();
        write_in_place(&path, &[Edit::Set(rules::HARDCORE, "1".into())]).unwrap();
        write_in_place(&path, &[Edit::Remove(rules::USERNAME)]).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# mine\nsmart_cache=2\nhardcore=1\n"
        );
        std::fs::write(root.join(RA_MAIN), b"").unwrap();
        std::fs::write(root.join(MISTER_INI), "[RA_*]\nmain=MiSTer_RA\n").unwrap();
        let (cfg, ra_main, ini_ra) = read(&root);
        assert!(cfg.hardcore && ra_main && ini_ra);
        let _ = std::fs::remove_dir_all(root);
    }
}
