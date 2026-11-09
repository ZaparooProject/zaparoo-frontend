// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The screensaver's runtime: gathers the art a run can use before the
//! idle timeout fires, picks the style when the saver arms (Random
//! included), steps it on a timer, and takes everything down again when
//! it disarms. The picture itself is `screensaver_scene`.
//!
//! Art stays in memory only: covers come from `MediaCache` (bounded LRU)
//! and system logos from the logo cache; the pool here is just the keys
//! to ask them for, kept for a few minutes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use zaparoo_app::screensaver::{self as rules, Bouncing, Style};
use zaparoo_core::media_types::{MediaHistoryParams, MediaHistoryTopParams, MediaSearchParams};

use crate::media_cache::MediaKey;
use crate::router::{lock, Ctx};
use crate::screensaver_scene::{self as scene, Art, Card, Face, Line, Scene, View};
use crate::App;

/// How long a gathered pool is reused before it is asked for again.
const POOL_FRESH: Duration = Duration::from_mins(10);
/// Seconds before the timeout to start gathering art.
pub(crate) const PREFETCH_LEAD: u64 = 20;
/// Recent games asked for.
const RECENTS: u32 = 100;
/// Top played games asked for, merged with the recent ones.
const TOP_PLAYED: u32 = 100;
/// Below this many played games, a page of the library tops the pool up.
const TOP_UP_BELOW: usize = 16;
/// Covers held by one run, at most, drawn at random from the pool.
const MAX_COVERS: usize = 48;
/// System logos one run draws from, at most.
const MAX_SYSTEMS: usize = 16;
/// Logos prepared on the spot at arm time when the prefetch missed them.
const PREPARE_NOW: usize = 4;
/// The longest step a late timer tick advances by.
const MAX_STEP: f32 = 0.25;

#[derive(Clone)]
struct Recent {
    system: String,
    name: String,
    key: MediaKey,
}

#[derive(Clone, Default)]
struct Pool {
    recents: Vec<Recent>,
    /// Covers from the library when there were few recent games.
    extra: Vec<MediaKey>,
    /// The cover size the keys were made for; a scene change asks again.
    px: u32,
}

static POOL: Mutex<Option<(Instant, Pool)>> = Mutex::new(None);

struct Run {
    timer: slint::Timer,
    heavy: bool,
}

thread_local! {
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
    static LAST: Cell<Option<Style>> = const { Cell::new(None) };
}

fn pool() -> std::sync::MutexGuard<'static, Option<(Instant, Pool)>> {
    POOL.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The wall clock as (hour, minute), local time where the zone is known.
pub(crate) fn local_time() -> (u8, u8) {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    (now.hour(), now.minute())
}

/// The ids of systems that have games, in the catalog's order.
fn systems_with_games(ctx: &Ctx) -> Vec<String> {
    lock(&ctx.shared)
        .systems
        .iter()
        .filter(|s| s.media_count.unwrap_or(0) > 0)
        .map(|s| s.id.clone())
        .collect()
}

fn logo_bounds(view: View) -> zaparoo_app::logo_cache::Bounds {
    let h = view.face_px();
    zaparoo_app::logo_cache::Bounds::new(h * 2, h)
}

/// A cover sized for the largest box the saver paints one in, a Cover
/// wall tile, at Core's tier for that size.
fn cover_key(ctx: &Ctx, px: u32, system: &str, path: &str, relative: &str) -> MediaKey {
    let side = i32::try_from(px).unwrap_or(i32::MAX);
    MediaKey {
        media_id: None,
        system: system.to_string(),
        path: path.to_string(),
        max_size: zaparoo_app::sizing::snap_cover_tier(f64::from(px)) as u32,
        fit: zaparoo_app::covers::Fit::new(side, side),
        folder: false,
        image_type: None,
        prefer: ctx.media.artwork_for(system, relative),
    }
}

/// The saved style and Bouncing choice.
fn chosen(ctx: &Ctx) -> (Style, Bouncing) {
    let shared = lock(&ctx.shared);
    let s = &shared.persist.settings;
    (
        Style::parse(&s.screensaver_style),
        Bouncing::parse(&s.screensaver_bouncing),
    )
}

/// Whether a run of `style` can draw covers or system logos. Random can
/// resolve to any style, so it does.
fn uses_art(style: Style, bouncing: Bouncing) -> bool {
    style != Style::Bouncing || bouncing == Bouncing::RecentGame
}

/// Start gathering art for the next run: system logos in color, and the
/// covers of recently and most played games (topped up from the library when
/// there are few). Called ahead of the timeout so a run starts with its
/// art in memory; a fresh pool is only asked for its covers again.
pub(crate) fn prefetch(ctx: &Ctx, app: &App) {
    let (style, bouncing) = chosen(ctx);
    if !uses_art(style, bouncing) {
        return;
    }
    let view = View::of(app);
    let bounds = logo_bounds(view);
    let prefs = crate::systems::LogoPrefs::of(&lock(&ctx.shared));
    let ids = systems_with_games(ctx);
    ctx.logos.request(
        ids.iter()
            .filter_map(|id| prefs.color_key(ctx, id, bounds))
            .collect::<Vec<_>>(),
    );
    let px = view.cover_px();
    let fresh = pool()
        .as_ref()
        .filter(|(at, p)| at.elapsed() < POOL_FRESH && p.px == px)
        .map(|(_, p)| p.clone());
    if let Some(pool) = fresh {
        enqueue(ctx, &pool);
        return;
    }
    let ctx = ctx.clone();
    let handle = ctx.handle.clone();
    handle.spawn(async move {
        let client = ctx.store.client();
        let mut recents = Vec::new();
        if let Ok(history) = client
            .media_history(MediaHistoryParams {
                limit: Some(RECENTS),
                cursor: None,
                systems: Vec::new(),
                distinct_media: Some(true),
            })
            .await
        {
            for e in history.entries {
                if e.media_path.is_empty() {
                    continue;
                }
                recents.push(Recent {
                    key: cover_key(
                        &ctx,
                        px,
                        &e.system_id,
                        &e.media_path,
                        e.relative_path.as_deref().unwrap_or_default(),
                    ),
                    system: e.system_id,
                    name: e.media_name,
                });
            }
        }
        if let Ok(top) = client
            .media_history_top(MediaHistoryTopParams {
                limit: Some(TOP_PLAYED),
                systems: Vec::new(),
            })
            .await
        {
            for e in top.entries {
                if e.media_path.is_empty() || recents.iter().any(|r| r.key.path == e.media_path) {
                    continue;
                }
                recents.push(Recent {
                    key: cover_key(
                        &ctx,
                        px,
                        &e.system_id,
                        &e.media_path,
                        e.relative_path.as_deref().unwrap_or_default(),
                    ),
                    system: e.system_id,
                    name: e.media_name,
                });
            }
        }
        let extra = if recents.len() < TOP_UP_BELOW && !ids.is_empty() {
            top_up(&ctx, &client, &ids, px, TOP_UP_BELOW - recents.len()).await
        } else {
            Vec::new()
        };
        let pool_now = Pool { recents, extra, px };
        enqueue(&ctx, &pool_now);
        *pool() = Some((Instant::now(), pool_now));
    });
}

/// Up to `want` covers from a page of one random system's games, for a
/// player with few played games.
async fn top_up(
    ctx: &Ctx,
    client: &zaparoo_core::client::Client,
    ids: &[String],
    px: u32,
    want: usize,
) -> Vec<MediaKey> {
    let mut rng = scene::rng_now();
    let system = ids[rng.below(ids.len())].clone();
    let Ok(page) = client
        .media_search(MediaSearchParams {
            systems: vec![system],
            max_results: Some(40),
            ..MediaSearchParams::default()
        })
        .await
    else {
        return Vec::new();
    };
    let mut items = page.results;
    rng.shuffle(&mut items);
    items
        .into_iter()
        .take(want)
        .map(|item| {
            cover_key(
                ctx,
                px,
                &item.system.id,
                &item.path,
                item.relative_path.as_deref().unwrap_or_default(),
            )
        })
        .collect()
}

/// Ask for a random `MAX_COVERS` of the pool's covers, so each run
/// draws a different handful from a long play history.
fn enqueue(ctx: &Ctx, pool: &Pool) {
    let mut keys: Vec<&MediaKey> = pool
        .recents
        .iter()
        .map(|r| &r.key)
        .chain(&pool.extra)
        .collect();
    scene::rng_now().shuffle(&mut keys);
    for key in keys.into_iter().take(MAX_COVERS) {
        ctx.media.enqueue(key.clone());
    }
}

/// What is in memory right now, ready to draw.
fn gather(ctx: &Ctx, view: View) -> Art {
    let pool = pool().as_ref().map(|(_, p)| p.clone()).unwrap_or_default();
    let bounds = logo_bounds(view);
    let prefs = crate::systems::LogoPrefs::of(&lock(&ctx.shared));
    let mut prepared_now = 0;
    let mut logo = |id: &str, prepare: bool| {
        let key = prefs.color_key(ctx, id, bounds)?;
        if let Some(ready) = ctx.logos.get(&key) {
            return Some(ready.images().0);
        }
        if prepare && prepared_now < PREPARE_NOW {
            prepared_now += 1;
            return ctx.logos.prepare_now(key).map(|p| p.images().0);
        }
        ctx.logos.request([key]);
        None
    };

    let mut rng = scene::rng_now();
    let mut ids = systems_with_games(ctx);
    rng.shuffle(&mut ids);
    let systems: Vec<Face> = ids
        .iter()
        .filter_map(|id| logo(id, true).map(Face::of))
        .take(MAX_SYSTEMS)
        .collect();

    let cover = |key: &MediaKey| {
        ctx.media
            .get(key)
            .map(|d| Face::of(slint::Image::from_rgba8(d.buffer)))
    };
    let mut covers = Vec::new();
    let mut cards = Vec::new();
    let mut lines = Vec::new();
    let mut played = pool.recents.clone();
    rng.shuffle(&mut played);
    for recent in &played {
        lines.push(Line {
            logo: logo(&recent.system, false).unwrap_or_default(),
            title: recent.name.clone(),
        });
        if covers.len() < MAX_COVERS {
            if let Some(face) = cover(&recent.key) {
                cards.push(Card {
                    cover: face.image.clone(),
                });
                covers.push(face);
            }
        }
    }
    for key in &pool.extra {
        if covers.len() >= MAX_COVERS {
            break;
        }
        if let Some(face) = cover(key) {
            covers.push(face);
        }
    }
    rng.shuffle(&mut cards);
    Art {
        systems,
        covers,
        cards,
        lines,
    }
}

/// The idle timeout fired: pick the style and start it.
pub(crate) fn arm(ctx: &Ctx, app: &App) {
    let shell = app.global::<crate::Shell>();
    if shell.get_saver_armed() {
        return;
    }
    // A run whose flag was dropped without `disarm` still holds its timer.
    teardown(app);
    let (style, bouncing) = chosen(ctx);
    let view = View::of(app);
    let art = if uses_art(style, bouncing) {
        gather(ctx, view)
    } else {
        Art::default()
    };
    let mut rng = scene::rng_now();
    let resolved = rules::resolve(style, bouncing, art.counts(), LAST.get(), &mut rng);
    LAST.set(Some(resolved.style));
    let twelve = ctx
        .clock_twelve_hour
        .load(std::sync::atomic::Ordering::Relaxed);
    let scene = Scene::new(
        app,
        resolved,
        art,
        view,
        rng.next_u64(),
        local_time(),
        twelve,
    );
    let heavy = scene.heavy();
    let interval = scene.interval();
    let scene = Rc::new(RefCell::new(scene));

    let weak = app.as_weak();
    let on_bounce = scene.clone();
    app.global::<crate::Saver>().on_bounced(move || {
        if let Some(app) = weak.upgrade() {
            on_bounce.borrow_mut().bounced(&app);
        }
    });

    let timer = slint::Timer::default();
    if let Some(interval) = interval {
        let weak = app.as_weak();
        let mut last = Instant::now();
        timer.start(slint::TimerMode::Repeated, interval, move || {
            let Some(app) = weak.upgrade() else { return };
            let dt = last.elapsed().as_secs_f32().min(MAX_STEP);
            last = Instant::now();
            scene.borrow_mut().tick(&app, dt, local_time());
        });
    }
    if heavy {
        crate::drs::heavy_begin();
        crate::drs::set_picture(true);
    }
    tracing::debug!(style = resolved.style.token(), "screensaver armed");
    RUN.with(|run| *run.borrow_mut() = Some(Run { timer, heavy }));
    shell.set_saver_armed(true);
}

/// Take the screensaver down: stop its timer, leave the heavy phase it
/// entered, and drop every picture it held.
pub(crate) fn disarm(app: &App) {
    app.global::<crate::Shell>().set_saver_armed(false);
    teardown(app);
}

fn teardown(app: &App) {
    let run = RUN.with(|run| run.borrow_mut().take());
    if let Some(run) = run {
        run.timer.stop();
        if run.heavy {
            crate::drs::set_picture(false);
            crate::drs::heavy_end();
        }
        app.global::<crate::Saver>().on_bounced(|| {});
        scene::clear(app);
    }
}

/// Whether a run is live (its timer and models held).
#[cfg(all(test, feature = "mister"))]
pub(crate) fn running() -> bool {
    RUN.with(|run| run.borrow().is_some())
}
