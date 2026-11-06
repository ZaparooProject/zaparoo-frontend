// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Core's background slot, simulated for the music player: an Audio index
// to browse, folder and inline playlists started through `run`, the transport
// through `media.control` and `playlists.update`, tracks that end on
// their own and advance, and the notifications real Core sends around
// them (`media.started`, `media.stopped` and `playlists.changed`, all on
// the background slot). Tracks are short so a dev run sees one end.

use crate::media_state::Notifier;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SLOT: &str = "background";
const CONTROLS: [&str; 6] = [
    "toggle_pause",
    "pause",
    "resume",
    "stop",
    "fast_forward",
    "rewind",
];

/// The mock's Audio index: two routes, an artist with an album and a
/// single, and a loose folder of tracks.
const ROUTES: [&str; 2] = ["/media/fat/Music", "/media/fat/Audio"];

fn folder(path: &str) -> Option<(Vec<&'static str>, Vec<&'static str>)> {
    match path {
        "/media/fat/Music" => Some((vec!["Neon Harbor", "The Lanterns"], vec![])),
        "/media/fat/Music/Neon Harbor" => {
            Some((vec!["Night Drive"], vec!["Harbor Lights (single).mp3"]))
        }
        "/media/fat/Music/Neon Harbor/Night Drive" => Some((
            vec![],
            vec![
                "01 Departure.mp3",
                "02 Coastline.flac",
                "03 Tunnel Vision.mp3",
                "04 Rearview.ogg",
                "05 Arrival.mp3",
            ],
        )),
        "/media/fat/Music/The Lanterns" => Some((
            vec![],
            vec!["01 Kindling.mp3", "02 Ember.mp3", "03 Ash.mp3"],
        )),
        "/media/fat/Audio" => Some((vec![], vec!["Menu Theme.ogg", "Rain Loop.wav"])),
        _ => None,
    }
}

fn stem(file: &str) -> &str {
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

/// The methods the background slot answers: the Audio index, its active
/// track and cover, the transport, and a background playlist through `run`.
/// `None` leaves the request to the rest of the mock.
pub fn route(method: &str, params: &Value, notifier: &Notifier) -> Option<Result<Value, String>> {
    match method {
        "media.browse" if is_audio_browse(params) => Some(Ok(browse_response(params))),
        "media.active" => media_active(params).map(Ok),
        "media.active.image" => Some(active_image(params)),
        "media.control" => Some(control(params, notifier)),
        "playlists.update" => Some(update(params, notifier)),
        "run" => {
            let script = params.get("text").and_then(Value::as_str).unwrap_or("");
            run(script, notifier)
        }
        _ => None,
    }
}

/// Whether a `media.browse` asks for the Audio index.
pub fn is_audio_browse(params: &Value) -> bool {
    params
        .get("systems")
        .and_then(Value::as_array)
        .is_some_and(|systems| systems.iter().any(|s| s.as_str() == Some("Audio")))
}

/// `media.browse` over the Audio index: the routes, or one folder.
pub fn browse_response(params: &Value) -> Value {
    let path = params.get("path").and_then(Value::as_str).unwrap_or("");
    let entries: Vec<Value> = if path.is_empty() {
        ROUTES
            .iter()
            .map(|route| json!({ "name": route, "path": route, "type": "root", "systemId": "Audio" }))
            .collect()
    } else if let Some((folders, files)) = folder(path) {
        let mut entries: Vec<Value> = folders
            .iter()
            .map(|name| {
                json!({ "name": name, "path": format!("{path}/{name}"), "type": "directory", "systemId": "Audio" })
            })
            .collect();
        entries.extend(files.iter().map(|file| {
            json!({
                "name": stem(file),
                "path": format!("{path}/{file}"),
                "type": "media",
                "systemId": "Audio",
            })
        }));
        entries
    } else {
        Vec::new()
    };
    json!({ "path": path, "entries": entries, "totalFiles": entries.len() })
}

#[derive(Clone)]
struct Item {
    name: String,
    path: String,
}

#[derive(Clone)]
struct Playlist {
    id: String,
    name: String,
    items: Vec<Item>,
    /// Load order, while shuffled.
    original: Vec<Item>,
    index: usize,
    repeat: String,
    shuffle: bool,
    playing: bool,
}

struct Player {
    playlist: Option<Playlist>,
    /// The track on the slot.
    track: Option<Item>,
    /// Where the track was when it last started or resumed, and when.
    offset_ms: u64,
    since: Option<Instant>,
    generation: u64,
    volume: i32,
    autoplay: bool,
    pause_games: bool,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            playlist: None,
            track: None,
            offset_ms: 0,
            since: None,
            generation: 0,
            volume: 100,
            autoplay: false,
            pause_games: true,
        }
    }
}

fn player() -> &'static Mutex<Player> {
    static PLAYER: OnceLock<Mutex<Player>> = OnceLock::new();
    PLAYER.get_or_init(|| Mutex::new(Player::default()))
}

fn with<R>(f: impl FnOnce(&mut Player) -> R) -> R {
    let mut guard = player()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// Every track lasts this long, plus a few seconds per track number.
fn duration_ms(item: &Item) -> u64 {
    let n = item.path.len() as u64 % 20;
    40_000 + n * 1000
}

impl Player {
    fn position_ms(&self) -> u64 {
        let moved = self.since.map_or(0, |since| {
            u64::try_from(since.elapsed().as_millis()).unwrap_or(0)
        });
        let position = self.offset_ms + moved;
        self.track
            .as_ref()
            .map_or(position, |t| position.min(duration_ms(t)))
    }

    fn playing(&self) -> bool {
        self.since.is_some()
    }

    fn active(&self) -> Option<Value> {
        let track = self.track.as_ref()?;
        // Odd-numbered paths carry tags and art, so the player shows both
        // a tagged track and one with only its file name.
        let tagged = track.path.len() % 2 == 1;
        let mut active = json!({
            "started": "2026-01-01T00:00:00Z",
            "launcherId": "native-audio",
            "systemId": "Audio",
            "systemName": "Audio",
            "mediaPath": track.path,
            "mediaName": track.name,
            "slot": SLOT,
            "zapScript": track.path,
            "launcherControls": CONTROLS,
            "positionMs": self.position_ms(),
            "durationMs": duration_ms(track),
            "playbackState": if self.playing() { "playing" } else { "paused" },
        });
        if tagged {
            active["track"] = json!({
                "title": track.name.trim_start_matches(|c: char| c.is_ascii_digit() || c == ' '),
                "artist": "Neon Harbor",
                "album": "Night Drive",
                "hasArt": true,
            });
        }
        Some(active)
    }

    fn playlist_state(&self) -> Option<Value> {
        let list = self.playlist.as_ref()?;
        Some(json!({
            "id": list.id,
            "name": list.name,
            "slot": SLOT,
            "repeat": list.repeat,
            "shuffle": list.shuffle,
            "index": list.index,
            "total": list.items.len(),
            "playing": list.playing,
            "unsafe": false,
            "items": list.items.iter().map(|i| json!({ "name": i.name, "zapScript": i.path })).collect::<Vec<_>>(),
        }))
    }
}

/// The background slot's entry for `media`'s `active`.
pub fn active_entry() -> Option<Value> {
    with(|p| p.active())
}

/// The open playlists for `media` and `playlists.changed`.
pub fn playlists() -> Vec<Value> {
    with(|p| p.playlist_state()).into_iter().collect()
}

/// `media.active`: the background slot's track, or `null`.
pub fn media_active(params: &Value) -> Option<Value> {
    let slot = params.get("slot").and_then(Value::as_str).unwrap_or("");
    if slot == SLOT || slot == "bg" {
        Some(active_entry().unwrap_or(Value::Null))
    } else {
        None
    }
}

/// Core's `media.playback`: the background track as `media.active` has it,
/// after a start, a pause, a resume or a seek.
fn notify_playback(notifier: &Notifier) {
    if let Some(active) = active_entry() {
        notifier.send("media.playback", &active);
    }
}

fn notify_playlists(notifier: &Notifier) {
    notifier.send("playlists.changed", &json!({ "playlists": playlists() }));
}

fn started(notifier: &Notifier) {
    if let Some(active) = active_entry() {
        notifier.send(
            "media.started",
            &json!({
                "systemId": "Audio",
                "systemName": "Audio",
                "mediaPath": active["mediaPath"],
                "mediaName": active["mediaName"],
                "slot": SLOT,
            }),
        );
    }
}

fn stopped(notifier: &Notifier, item: &Item, reason: &str) {
    notifier.send(
        "media.stopped",
        &json!({
            "systemId": "Audio",
            "systemName": "Audio",
            "mediaPath": item.path,
            "mediaName": item.name,
            "launcherId": "native-audio",
            "slot": SLOT,
            "reason": reason,
        }),
    );
}

/// Play the playlist's current item from the start, and arm its end.
fn play_current(notifier: &Notifier) {
    let previous = with(|p| {
        let previous = p.track.take();
        let item = p
            .playlist
            .as_ref()
            .and_then(|l| l.items.get(l.index).cloned());
        p.track = item;
        p.offset_ms = 0;
        p.since = p.track.as_ref().map(|_| Instant::now());
        if let Some(list) = p.playlist.as_mut() {
            list.playing = true;
        }
        p.generation += 1;
        previous
    });
    if let Some(previous) = previous {
        stopped(notifier, &previous, "replaced");
    }
    started(notifier);
    notify_playback(notifier);
    notify_playlists(notifier);
    arm_end(notifier);
}

/// When the track on the slot ends on its own, advance as Core does.
fn arm_end(notifier: &Notifier) {
    let Some((generation, left)) = with(|p| {
        let track = p.track.as_ref()?;
        p.since?;
        Some((
            p.generation,
            duration_ms(track).saturating_sub(p.position_ms()),
        ))
    }) else {
        return;
    };
    let notifier = notifier.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(left)).await;
        let next = with(|p| {
            if p.generation != generation {
                return None;
            }
            let list = p.playlist.as_mut()?;
            let next = match list.repeat.as_str() {
                "one" => Some(list.index),
                _ if list.index + 1 < list.items.len() => Some(list.index + 1),
                "all" => Some(0),
                _ => None,
            };
            Some(next.map(|index| list.index = index))
        });
        match next {
            Some(Some(())) => play_current(&notifier),
            Some(None) => {
                let ended = with(|p| {
                    if let Some(list) = p.playlist.as_mut() {
                        list.playing = false;
                    }
                    p.since = None;
                    p.track.take()
                });
                if let Some(item) = ended {
                    stopped(&notifier, &item, "finished");
                }
                notify_playlists(&notifier);
            }
            None => {}
        }
    });
}

fn pause(notifier: &Notifier) {
    with(|p| {
        p.offset_ms = p.position_ms();
        p.since = None;
        p.generation += 1;
        if let Some(list) = p.playlist.as_mut() {
            list.playing = false;
        }
    });
    notify_playback(notifier);
    notify_playlists(notifier);
}

fn resume(notifier: &Notifier) {
    with(|p| {
        if p.track.is_some() && p.since.is_none() {
            p.since = Some(Instant::now());
        }
        p.generation += 1;
        if let Some(list) = p.playlist.as_mut() {
            list.playing = true;
        }
    });
    notify_playback(notifier);
    notify_playlists(notifier);
    arm_end(notifier);
}

fn stop(notifier: &Notifier) {
    let ended = with(|p| {
        p.generation += 1;
        p.playlist = None;
        p.since = None;
        p.track.take()
    });
    if let Some(item) = ended {
        stopped(notifier, &item, "stopped");
    }
    notify_playlists(notifier);
}

fn unescape(script: &str) -> String {
    let mut out = String::with_capacity(script.len());
    let mut chars = script.chars();
    while let Some(ch) = chars.next() {
        if ch == '^' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// A deterministic reorder that keeps `first` in front, standing in for
/// Core's shuffle.
fn shuffled(items: &[Item], first: usize) -> Vec<Item> {
    let mut rest: Vec<Item> = items
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != first)
        .map(|(_, item)| item.clone())
        .collect();
    rest.reverse();
    let mut out: Vec<Item> = items.get(first).cloned().into_iter().collect();
    out.extend(rest);
    out
}

fn adv_arg<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    args.split('&')
        .find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='))
}

/// `run` with a background `**playlist.*` script; `None` for anything
/// else, which the primary-slot launch simulation takes.
pub fn run(script: &str, notifier: &Notifier) -> Option<Result<Value, String>> {
    let rest = script.strip_prefix("**playlist.")?;
    let (command, rest) = rest
        .split_once(['?', ':'])
        .map_or((rest, ""), |(c, r)| (c, r));
    let (arg, args) = if script[("**playlist.".len() + command.len())..].starts_with(':') {
        if rest.starts_with('{') {
            let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
            let value = stream.next()?.ok()?;
            let end = stream.byte_offset();
            (Some(value), rest[end..].trim_start_matches('?'))
        } else {
            let (arg, args) = rest.split_once('?').unwrap_or((rest, ""));
            (Some(Value::String(arg.to_string())), args)
        }
    } else {
        (None, rest)
    };
    if !args
        .split('&')
        .any(|pair| pair == "slot=background" || pair == "slot=bg")
    {
        return None;
    }
    match command {
        "play" => return play(arg.as_ref(), args, notifier),
        "pause" => pause(notifier),
        "stop" => stop(notifier),
        "next" | "previous" => {
            let moved = with(|p| {
                let restart = command == "previous" && p.position_ms() > 3000;
                let list = p.playlist.as_mut()?;
                let len = list.items.len();
                if !restart {
                    list.index = if command == "next" {
                        (list.index + 1) % len
                    } else {
                        (list.index + len - 1) % len
                    };
                }
                Some(())
            });
            if moved.is_none() {
                return Some(Err("no playlist is open".into()));
            }
            play_current(notifier);
        }
        "goto" => {
            let n = arg
                .as_ref()
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1);
            let moved = with(|p| {
                let list = p.playlist.as_mut()?;
                (n >= 1 && n <= list.items.len()).then(|| list.index = n - 1)
            });
            if moved.is_none() {
                return Some(Err("no such playlist item".into()));
            }
            play_current(notifier);
        }
        _ => return Some(Err(format!("unknown playlist command {command}"))),
    }
    Some(Ok(Value::Null))
}

/// A folder played as a playlist: its tracks as the folder's browse lists
/// them, as Core does.
fn folder_items(path: &str) -> Vec<Item> {
    folder(path)
        .map(|(_, files)| {
            files
                .iter()
                .map(|file| Item {
                    name: stem(file).to_string(),
                    path: format!("{path}/{file}"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `**playlist.play`: an inline playlist or a folder starts; with no
/// argument the open playlist resumes, or skips when it is already playing.
fn play(arg: Option<&Value>, args: &str, notifier: &Notifier) -> Option<Result<Value, String>> {
    let folder_list = arg.and_then(Value::as_str).map(|path| {
        let path = unescape(path);
        let name = path.rsplit('/').next().unwrap_or_default().to_string();
        json!({ "id": path, "name": name, "folder": true })
    });
    if let Some(Value::Object(list)) = arg.filter(|a| a.is_object()).or(folder_list.as_ref()) {
        let items: Vec<Item> = if list.contains_key("folder") {
            folder_items(list["id"].as_str().unwrap_or_default())
        } else {
            list.get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| Item {
                            name: item["name"].as_str().unwrap_or_default().to_string(),
                            path: unescape(item["zapscript"].as_str().unwrap_or_default()),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        if items.is_empty() {
            return Some(Err("playlist is empty".into()));
        }
        let start = adv_arg(args, "start")
            .and_then(|s| s.parse::<usize>().ok())
            .map_or(0, |s| s.saturating_sub(1).min(items.len() - 1));
        let shuffle = adv_arg(args, "mode") == Some("shuffle");
        let repeat = match adv_arg(args, "repeat") {
            Some("all") => "all",
            Some("one") => "one",
            _ => "none",
        };
        let order = if shuffle {
            shuffled(&items, start)
        } else {
            items.clone()
        };
        with(|p| {
            p.playlist = Some(Playlist {
                id: list["id"].as_str().unwrap_or_default().to_string(),
                name: list["name"].as_str().unwrap_or_default().to_string(),
                index: if shuffle { 0 } else { start },
                items: order,
                original: if shuffle { items } else { Vec::new() },
                repeat: repeat.to_string(),
                shuffle,
                playing: true,
            });
        });
        play_current(notifier);
    } else {
        let open = with(|p| p.playlist.as_ref().map(|l| l.playing));
        match open {
            Some(false) => resume(notifier),
            Some(true) => return run("**playlist.next?slot=background", notifier),
            None => return Some(Err("no playlist is open".into())),
        }
    }
    Some(Ok(Value::Null))
}

/// `media.control` on the background slot.
pub fn control(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let action = params.get("action").and_then(Value::as_str).unwrap_or("");
    let has_track = with(|p| p.track.is_some());
    if !has_track {
        return Err("no active media".into());
    }
    match action {
        "toggle_pause" => {
            if with(|p| p.playing()) {
                pause(notifier);
            } else {
                resume(notifier);
            }
        }
        "pause" => pause(notifier),
        "resume" => resume(notifier),
        "stop" => stop(notifier),
        "fast_forward" | "rewind" => {
            let seconds = params
                .get("args")
                .and_then(|a| a.get("seconds"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(10);
            with(|p| {
                let at = p.position_ms();
                p.offset_ms = if action == "rewind" {
                    at.saturating_sub(seconds * 1000)
                } else {
                    at + seconds * 1000
                };
                if p.since.is_some() {
                    p.since = Some(Instant::now());
                }
                p.generation += 1;
            });
            notify_playback(notifier);
            arm_end(notifier);
        }
        other => return Err(format!("unsupported control {other}")),
    }
    Ok(Value::Null)
}

/// `playlists.update`: shuffle or repeat the open playlist in place.
pub fn update(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let shuffle = params.get("shuffle").and_then(Value::as_bool);
    let repeat = params.get("repeat").and_then(Value::as_str);
    if shuffle.is_none() && repeat.is_none() {
        return Err("shuffle or repeat is required".into());
    }
    with(|p| {
        let list = p.playlist.as_mut().ok_or("no playlist is open")?;
        if let Some(repeat) = repeat {
            list.repeat = match repeat {
                "all" | "one" => repeat.to_string(),
                _ => "none".to_string(),
            };
        }
        if let Some(on) = shuffle {
            if on && !list.shuffle {
                list.original = list.items.clone();
                list.items = shuffled(&list.items, list.index);
                list.index = 0;
            } else if !on && list.shuffle {
                let current = list.items.get(list.index).map(|i| i.path.clone());
                list.items = std::mem::take(&mut list.original);
                list.index = current
                    .and_then(|c| list.items.iter().position(|i| i.path == c))
                    .unwrap_or(0);
            }
            list.shuffle = on;
        }
        Ok::<(), String>(())
    })?;
    notify_playlists(notifier);
    Ok(Value::Null)
}

/// `media.active.image`: placeholder art for a tagged track.
pub fn active_image(params: &Value) -> Result<Value, String> {
    let active = active_entry().ok_or("no active audio track")?;
    if !active["track"]["hasArt"].as_bool().unwrap_or(false) {
        return Err("no cover art".into());
    }
    let mut image = crate::fixtures::media_image_response(&json!({
        "system": "Audio",
        "path": active["mediaPath"],
        "maxSize": params.get("maxSize").cloned().unwrap_or(json!(256)),
    }))?;
    image["typeTag"] = json!("audio:embedded");
    Ok(image)
}

/// The music keys of `settings`.
pub fn settings() -> (i32, bool, bool) {
    with(|p| (p.volume, p.autoplay, p.pause_games))
}

/// Apply the music keys of a `settings.update`.
pub fn update_settings(params: &Value) -> Result<(), String> {
    if let Some(volume) = params.get("audioMusicVolume").and_then(Value::as_i64) {
        if !(0..=200).contains(&volume) {
            return Err("audioMusicVolume must be 0 to 200".into());
        }
        with(|p| p.volume = i32::try_from(volume).unwrap_or(100));
    }
    if let Some(on) = params.get("audioMusicAutoplay").and_then(Value::as_bool) {
        with(|p| p.autoplay = on);
    }
    if let Some(on) = params
        .get("audioMusicPauseOnLaunch")
        .and_then(Value::as_bool)
    {
        with(|p| p.pause_games = on);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audio_index_lists_routes_then_folders_then_tracks() {
        let routes = browse_response(&json!({ "systems": ["Audio"] }));
        assert_eq!(routes["entries"].as_array().map(Vec::len), Some(2));
        let artist = browse_response(&json!({
            "systems": ["Audio"],
            "path": "/media/fat/Music/Neon Harbor",
        }));
        let entries = artist["entries"].as_array().cloned().unwrap_or_default();
        assert_eq!(entries[0]["type"], "directory");
        assert_eq!(entries[1]["type"], "media");
        assert_eq!(entries[1]["name"], "Harbor Lights (single)");
    }

    #[test]
    fn escapes_are_undone_and_shuffle_keeps_the_first_track() {
        assert_eq!(unescape("/m/A^, B^?.mp3"), "/m/A, B?.mp3");
        let items: Vec<Item> = (0..4)
            .map(|i| Item {
                name: i.to_string(),
                path: format!("/m/{i}"),
            })
            .collect();
        let order = shuffled(&items, 2);
        assert_eq!(order[0].path, "/m/2");
        assert_eq!(order.len(), 4);
    }

    #[test]
    fn a_folder_plays_its_tracks_as_browse_lists_them() {
        let items = folder_items("/media/fat/Music/Neon Harbor/Night Drive");
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "01 Departure",
                "02 Coastline",
                "03 Tunnel Vision",
                "04 Rearview",
                "05 Arrival"
            ]
        );
        let single: Vec<String> = folder_items("/media/fat/Music/Neon Harbor")
            .into_iter()
            .map(|i| i.path)
            .collect();
        assert_eq!(
            single,
            ["/media/fat/Music/Neon Harbor/Harbor Lights (single).mp3"],
            "folders are not items"
        );
    }

    #[test]
    fn adv_args_read_by_key() {
        assert_eq!(adv_arg("slot=background&start=3", "start"), Some("3"));
        assert_eq!(adv_arg("slot=background", "mode"), None);
    }
}
