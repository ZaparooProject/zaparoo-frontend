// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Running the game inside our own Steam session, on SteamOS.
//
// Core cannot start an emulator inside a Steam session by itself, so in
// Gaming Mode it borrows one: it asks Steam to run a second non-Steam
// shortcut called Zaparoo Runtime, which dials back over a Unix socket,
// is handed one command, and execs it. That is why launching a game used
// to show a second Steam card, and why Steam then held two apps open for
// one game, which the user had to unwind a Back press at a time.
//
// When Steam started us we are already that session. Registering as a
// standing host on the same socket lets Core hand the command here
// instead, and the game becomes a child of the frontend rather than of a
// second Steam app. One card, one Back press, and the frontend is
// already in place to come back when the game exits.
//
// The shortcut stays the fallback: Core prefers a registered host and
// falls back to launching it whenever there is not one, which covers a
// token scanned with no frontend running, Desktop Mode, and a frontend
// Steam did not launch. Everything else stays Core's: it resolves the
// media, tracks the session, owns active media and history, stops the
// game, and claims the compositor for the window we spawn. This module
// execs and waits.
//
// `MiSTer` has no Steam and no compositor, so none of this compiles
// there.

/// Nothing to host on `MiSTer`: its wrapper owns launching outright.
#[cfg(feature = "mister")]
pub fn start(_endpoint: &str) {}

/// No hosted game can be running, so nothing is ever held open.
#[cfg(feature = "mister")]
pub fn hosting() -> bool {
    false
}

#[cfg(feature = "desktop")]
pub use desktop::{hosting, start};

#[cfg(feature = "desktop")]
mod desktop {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use serde::{Deserialize, Serialize};

    /// Wire version, shared with Core's `steamruntime` package. Both sides
    /// refuse a frame that does not carry this exact value.
    const PROTOCOL_VERSION: u32 = 1;
    /// A standing registration: this connection carries no command, it just
    /// says we are here and waits to be poked.
    const ROLE_HOST: &str = "host";
    /// A connection that carries exactly one command and then finishes, opened
    /// by a standing host. Distinct from the Runtime shortcut's plain "launch"
    /// so Core can tell the two apart even when they arrive out of order: it
    /// decides who gets signalled when a launch is stopped, and signalling a
    /// host means killing the frontend.
    const ROLE_LAUNCH: &str = "host-launch";
    const POKE_LAUNCH: &str = "launch";
    const PHASE_STARTED: &str = "started";
    const PHASE_EXITED: &str = "exited";
    const PHASE_ERROR: &str = "error";

    /// Core's sockets, under `$XDG_RUNTIME_DIR/zaparoo`. Registration has a
    /// path of its own on purpose: a Core without host support binds the
    /// launch socket only for the seconds a launch is in flight, and a
    /// registration retrying against it would be handed that launch's command
    /// and swallow it. An older Core has no host socket, so we never register
    /// and it never notices us.
    const LAUNCH_SOCKET: &str = "steam-runtime.sock";
    const HOST_SOCKET: &str = "steam-launch-host.sock";

    /// How long a launch connection waits for its command before giving up.
    /// Core's own Runtime peer uses the same bound for the same reason.
    const COMMAND_WAIT: Duration = Duration::from_secs(30);

    const RECONNECT_MIN: Duration = Duration::from_secs(1);
    const RECONNECT_MAX: Duration = Duration::from_secs(30);
    /// How long a registration has to hold before it counts as having worked,
    /// so the backoff starts again from the bottom next time.
    const SETTLED_REGISTRATION: Duration = Duration::from_secs(30);

    /// How many games we are currently running. A count rather than a flag
    /// because Core preempts a running launch by starting the next one, so
    /// the two overlap for as long as the old one takes to die.
    static HOSTED: AtomicUsize = AtomicUsize::new(0);

    #[derive(Serialize)]
    struct Hello {
        role: &'static str,
        version: u32,
    }

    #[derive(Deserialize)]
    struct Poke {
        #[serde(default)]
        kind: String,
        #[serde(default)]
        version: u32,
    }

    #[derive(Deserialize)]
    struct LaunchCommand {
        executable: String,
        #[serde(default)]
        dir: String,
        #[serde(rename = "launchId")]
        launch_id: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<String>,
        version: u32,
    }

    #[derive(Serialize)]
    struct CommandResult<'a> {
        phase: &'a str,
        error: &'a str,
        #[serde(rename = "launchId")]
        launch_id: &'a str,
        pid: i64,
        #[serde(rename = "exitCode")]
        exit_code: i32,
        version: u32,
    }

    /// True while a game we started is still running. The quit dialog is
    /// held shut for this: leaving would close the connection Core is
    /// watching for the game's exit, so Core would call the game over while
    /// it is still on screen.
    pub fn hosting() -> bool {
        HOSTED.load(Ordering::SeqCst) > 0
    }

    /// Holds the hosted count up for as long as one game runs.
    struct HostedGuard;

    impl HostedGuard {
        fn new() -> Self {
            HOSTED.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }

    impl Drop for HostedGuard {
        fn drop(&mut self) {
            HOSTED.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Offer to host Core's launches, if all three things that make it the
    /// right thing to do are true.
    ///
    /// A gamescope session means Gaming Mode, which is the only place the
    /// Runtime shortcut is used at all. A Steam app id means Steam started
    /// us, so we have a session to put the game inside; without one we are
    /// an ordinary process and hosting would give the game less than the
    /// shortcut does. A loopback Core means the games are this machine's,
    /// which is the same gate the dormancy lifecycle already uses.
    pub fn start(endpoint: &str) {
        if !crate::gamescope::in_session() {
            return;
        }
        if !crate::steam::launched_by_steam() {
            tracing::debug!("not a Steam launch; leaving hosting to the Runtime shortcut");
            return;
        }
        if !zaparoo_app::covers::endpoint_is_loopback(endpoint) {
            tracing::debug!("Core is remote; its launches are not ours to host");
            return;
        }
        let Some(path) = host_socket_path() else {
            tracing::debug!("no XDG_RUNTIME_DIR; cannot find Core's host socket");
            return;
        };
        let spawned = thread::Builder::new()
            .name("steam-launch-host".into())
            .spawn(move || register_loop(&path));
        if let Err(err) = spawned {
            tracing::warn!(%err, "failed to start the Steam launch host");
        }
    }

    /// Core's socket, derived the way Core derives it.
    ///
    /// Core falls back to a temp directory when `XDG_RUNTIME_DIR` is unset,
    /// but a Steam session always has one, and guessing wrong would leave us
    /// registered on a socket nobody is listening to. Absent means we do not
    /// host, and the shortcut handles the launch as it always did.
    fn socket_dir() -> Option<PathBuf> {
        let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
        dir.is_absolute().then(|| dir.join("zaparoo"))
    }

    fn host_socket_path() -> Option<PathBuf> {
        Some(socket_dir()?.join(HOST_SOCKET))
    }

    fn launch_socket_path() -> Option<PathBuf> {
        Some(socket_dir()?.join(LAUNCH_SOCKET))
    }

    /// Stay registered for as long as the frontend runs. Core restarting is
    /// ordinary, so a dropped connection is a reconnect, not an error.
    ///
    /// The backoff resets on a registration that actually lasted, not on any
    /// clean close. A Core that hangs up the moment we say hello closes the
    /// connection cleanly too, and that is what a rejected peer looks like:
    /// a protocol version Core no longer accepts, say. Resetting on every
    /// clean close would turn that into a one-second reconnect loop for the
    /// life of the process, spinning and filling the log. Backing off instead
    /// costs a working reconnect nothing, because a real registration is up
    /// for far longer than this.
    fn register_loop(path: &Path) {
        let mut backoff = RECONNECT_MIN;
        loop {
            let started = Instant::now();
            match register(path) {
                Ok(()) => tracing::info!("Core closed the launch host registration"),
                Err(err) => tracing::debug!(%err, "launch host registration unavailable"),
            }
            thread::sleep(backoff);
            backoff = next_backoff(backoff, started.elapsed());
        }
    }

    /// The wait before the next attempt: back to the bottom after a
    /// registration that held, doubling otherwise.
    fn next_backoff(current: Duration, lasted: Duration) -> Duration {
        if lasted >= SETTLED_REGISTRATION {
            return RECONNECT_MIN;
        }
        (current * 2).min(RECONNECT_MAX)
    }

    /// One registration, held until Core hangs up.
    fn register(path: &Path) -> std::io::Result<()> {
        let stream = UnixStream::connect(path)?;
        send(
            &stream,
            &Hello {
                role: ROLE_HOST,
                version: PROTOCOL_VERSION,
            },
        )?;
        tracing::info!("registered as Core's Steam launch host");
        for line in BufReader::new(stream.try_clone()?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            handle_poke(&line);
        }
        Ok(())
    }

    fn handle_poke(line: &str) {
        match serde_json::from_str::<Poke>(line) {
            Ok(poke) if poke.kind == POKE_LAUNCH && poke.version == PROTOCOL_VERSION => {
                let Some(path) = launch_socket_path() else {
                    tracing::warn!("poked with no launch socket to answer on");
                    return;
                };
                // A launch outlives the poke that asked for it, so the
                // registration has to stay free to read the next one.
                let spawned = thread::Builder::new()
                    .name("steam-launch".into())
                    .spawn(move || {
                        if let Err(err) = serve_launch(&path) {
                            tracing::warn!(%err, "hosted launch failed");
                        }
                    });
                if let Err(err) = spawned {
                    tracing::warn!(%err, "failed to start a hosted launch");
                }
            }
            Ok(poke) => {
                tracing::warn!(kind = poke.kind, version = poke.version, "unknown poke");
            }
            Err(err) => tracing::warn!(%err, "unreadable poke from Core"),
        }
    }

    /// Take one command off a fresh connection and run it to completion.
    fn serve_launch(path: &Path) -> std::io::Result<()> {
        let stream = UnixStream::connect(path)?;
        send(
            &stream,
            &Hello {
                role: ROLE_LAUNCH,
                version: PROTOCOL_VERSION,
            },
        )?;
        // Core sends the command as soon as it takes the connection, so a
        // silence here is Core gone or the launch given up on. Without a
        // deadline this thread would wait on it for the life of the process.
        stream.set_read_timeout(Some(COMMAND_WAIT))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Core closed the launch connection before sending a command",
            ));
        }
        let spec: LaunchCommand = serde_json::from_str(&line).map_err(std::io::Error::other)?;
        if spec.version != PROTOCOL_VERSION {
            return Err(std::io::Error::other(format!(
                "unsupported launch protocol version: {}",
                spec.version
            )));
        }
        // The game runs for as long as it runs; only the handshake is timed.
        stream.set_read_timeout(None)?;
        run(&stream, &spec)
    }

    fn build(spec: &LaunchCommand) -> Command {
        let mut command = Command::new(&spec.executable);
        command.args(&spec.args);
        if !spec.dir.is_empty() {
            command.current_dir(&spec.dir);
        }
        // Core sends only the keys it wants changed; everything else is
        // inherited, which is the point of hosting: the child gets the Steam
        // session's environment because it gets ours.
        for entry in &spec.env {
            if let Some((key, value)) = entry.split_once('=') {
                command.env(key, value);
            }
        }
        // Its own process group, so Core can signal the game without the
        // signal reaching the frontend that started it.
        command.process_group(0);
        command
    }

    fn run(stream: &UnixStream, spec: &LaunchCommand) -> std::io::Result<()> {
        let mut child = match build(spec).spawn() {
            Ok(child) => child,
            Err(err) => {
                send(stream, &result(spec, PHASE_ERROR, 0, 0, &err.to_string()))?;
                return Err(err);
            }
        };
        let pid = i64::from(child.id());
        if let Err(err) = send(stream, &result(spec, PHASE_STARTED, pid, 0, "")) {
            // Core never learned the PID, so nobody else can stop this.
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }
        let guard = HostedGuard::new();
        tracing::info!(pid, executable = spec.executable, "hosting a launch");
        let (code, error) = wait(&mut child);
        drop(guard);
        tracing::info!(pid, code, "hosted launch finished");
        send(stream, &result(spec, PHASE_EXITED, 0, code, &error))
    }

    /// Core reads an exit the way Go reports one: zero for a clean run, the
    /// child's code when it has one, and -1 for anything else, which
    /// includes being killed by a signal.
    fn wait(child: &mut Child) -> (i32, String) {
        match child.wait() {
            Ok(status) if status.success() => (0, String::new()),
            Ok(status) => (status.code().unwrap_or(-1), status.to_string()),
            Err(err) => (-1, err.to_string()),
        }
    }

    fn result<'a>(
        spec: &'a LaunchCommand,
        phase: &'a str,
        pid: i64,
        exit_code: i32,
        error: &'a str,
    ) -> CommandResult<'a> {
        CommandResult {
            phase,
            error,
            launch_id: &spec.launch_id,
            pid,
            exit_code,
            version: PROTOCOL_VERSION,
        }
    }

    /// One JSON value per line, which is what Go's `json.Encoder` writes and
    /// what its `json.Decoder` reads back.
    fn send<T: Serialize>(stream: &UnixStream, frame: &T) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(frame).map_err(std::io::Error::other)?;
        line.push(b'\n');
        let mut writer = stream;
        writer.write_all(&line)
    }

    #[cfg(test)]
    #[allow(
        clippy::panic,
        clippy::expect_used,
        reason = "tests should fail fast on a protocol frame that does not arrive"
    )]
    mod tests {
        use std::os::unix::net::UnixListener;

        use super::*;

        /// Reads one line and parses it, the way Core's broker does.
        fn read_frame(reader: &mut impl BufRead) -> serde_json::Value {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).expect("read frame") > 0);
            serde_json::from_str(&line).expect("parse frame")
        }

        fn command(executable: &str) -> serde_json::Value {
            serde_json::json!({
                "executable": executable,
                "launchId": "abc123",
                "args": [],
                "version": PROTOCOL_VERSION,
            })
        }

        /// Stands in for Core: accept the launch connection, check its
        /// hello, hand over one command, and report what comes back.
        fn broker_serves(
            listener: &UnixListener,
            spec: &serde_json::Value,
        ) -> Vec<serde_json::Value> {
            let (stream, _) = listener.accept().expect("accept launch");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let hello = read_frame(&mut reader);
            assert_eq!(hello["role"], ROLE_LAUNCH);
            assert_eq!(hello["version"], PROTOCOL_VERSION);
            let mut line = serde_json::to_vec(spec).expect("encode command");
            line.push(b'\n');
            (&stream).write_all(&line).expect("send command");
            vec![read_frame(&mut reader), read_frame(&mut reader)]
        }

        /// A private runtime directory for one test, pointed at by
        /// `XDG_RUNTIME_DIR` so the module resolves both socket paths inside
        /// it. Each test runs in its own process, so the variable is ours.
        fn temp_runtime(name: &str) -> PathBuf {
            let root = std::env::temp_dir()
                .join(format!("zaparoo-host-test-{}-{name}", std::process::id()));
            std::fs::create_dir_all(root.join("zaparoo")).expect("create test dir");
            std::env::set_var("XDG_RUNTIME_DIR", &root);
            root
        }

        fn temp_socket(name: &str) -> PathBuf {
            temp_runtime(name);
            let path = launch_socket_path().expect("launch socket path");
            let _ = std::fs::remove_file(&path);
            path
        }

        #[test]
        fn a_hosted_command_reports_its_pid_and_exit() {
            let path = temp_socket("run");
            let listener = UnixListener::bind(&path).expect("bind");
            let serving = thread::spawn({
                let spec = command("true");
                move || broker_serves(&listener, &spec)
            });

            serve_launch(&path).expect("serve the launch");

            let frames = serving.join().expect("broker thread");
            assert_eq!(frames[0]["phase"], PHASE_STARTED);
            assert_eq!(frames[0]["launchId"], "abc123");
            assert!(frames[0]["pid"].as_i64().expect("pid") > 0);
            assert_eq!(frames[1]["phase"], PHASE_EXITED);
            assert_eq!(frames[1]["exitCode"], 0);
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn a_nonzero_exit_reaches_core() {
            let path = temp_socket("exit");
            let listener = UnixListener::bind(&path).expect("bind");
            let serving = thread::spawn({
                let mut spec = command("sh");
                spec["args"] = serde_json::json!(["-c", "exit 3"]);
                move || broker_serves(&listener, &spec)
            });

            serve_launch(&path).expect("serve the launch");

            let frames = serving.join().expect("broker thread");
            assert_eq!(frames[1]["phase"], PHASE_EXITED);
            assert_eq!(frames[1]["exitCode"], 3);
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn a_missing_executable_reports_the_error_phase() {
            let path = temp_socket("missing");
            let listener = UnixListener::bind(&path).expect("bind");
            let serving = thread::spawn({
                let spec = command("zaparoo-no-such-emulator");
                move || {
                    let (stream, _) = listener.accept().expect("accept launch");
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let _ = read_frame(&mut reader);
                    let mut line = serde_json::to_vec(&spec).expect("encode command");
                    line.push(b'\n');
                    (&stream).write_all(&line).expect("send command");
                    read_frame(&mut reader)
                }
            });

            // The spawn failure is the error we report, and also the error we
            // return, so Core is told and the log says why.
            assert!(serve_launch(&path).is_err());

            let frame = serving.join().expect("broker thread");
            assert_eq!(frame["phase"], PHASE_ERROR);
            assert!(!frame["error"].as_str().expect("error text").is_empty());
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn a_registration_says_who_it_is_and_then_serves_pokes() {
            // Two sockets, as Core binds them: the registration on one, the
            // launch the poke asks for on the other.
            temp_runtime("register");
            let host_path = host_socket_path().expect("host socket path");
            let launch_path = launch_socket_path().expect("launch socket path");
            let _ = std::fs::remove_file(&host_path);
            let _ = std::fs::remove_file(&launch_path);
            let host = UnixListener::bind(&host_path).expect("bind host");
            let listener = UnixListener::bind(&launch_path).expect("bind launch");
            let registering = thread::spawn({
                let host_path = host_path.clone();
                move || register(&host_path)
            });

            let (stream, _) = host.accept().expect("accept registration");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let hello = read_frame(&mut reader);
            assert_eq!(hello["role"], ROLE_HOST);
            assert_eq!(hello["version"], PROTOCOL_VERSION);

            // A poke brings a second connection, which is the launch.
            let poke = serde_json::json!({"kind": POKE_LAUNCH, "version": PROTOCOL_VERSION});
            let mut line = serde_json::to_vec(&poke).expect("encode poke");
            line.push(b'\n');
            (&stream).write_all(&line).expect("send poke");
            let frames = broker_serves(&listener, &command("true"));
            assert_eq!(frames[0]["phase"], PHASE_STARTED);
            assert_eq!(frames[1]["phase"], PHASE_EXITED);

            // Core hanging up ends the registration rather than erroring.
            // Shutdown rather than drop: the reader above holds a dup of this
            // socket, so dropping one handle leaves the connection open.
            stream
                .shutdown(std::net::Shutdown::Both)
                .expect("close the registration");
            registering
                .join()
                .expect("registration thread")
                .expect("clean close");
            let _ = std::fs::remove_file(&host_path);
            let _ = std::fs::remove_file(&launch_path);
        }

        /// The bytes Core has to be able to read. Nothing builds both sides
        /// together, so the wire format is pinned here and against the same
        /// literals in Core's `steamruntime` tests; a renamed field would
        /// otherwise only surface on a Steam Deck.
        #[test]
        fn the_frames_core_reads_are_exactly_these() {
            let hello = serde_json::to_string(&Hello {
                role: ROLE_HOST,
                version: PROTOCOL_VERSION,
            })
            .expect("encode hello");
            assert_eq!(hello, r#"{"role":"host","version":1}"#);

            let launch = serde_json::to_string(&Hello {
                role: ROLE_LAUNCH,
                version: PROTOCOL_VERSION,
            })
            .expect("encode hello");
            assert_eq!(launch, r#"{"role":"host-launch","version":1}"#);

            let spec = LaunchCommand {
                executable: "true".into(),
                dir: String::new(),
                launch_id: "abc123".into(),
                args: Vec::new(),
                env: Vec::new(),
                version: PROTOCOL_VERSION,
            };
            let started =
                serde_json::to_string(&result(&spec, PHASE_STARTED, 4242, 0, "")).expect("encode");
            assert_eq!(
                started,
                r#"{"phase":"started","error":"","launchId":"abc123","pid":4242,"exitCode":0,"version":1}"#
            );
            let exited =
                serde_json::to_string(&result(&spec, PHASE_EXITED, 0, 3, "")).expect("encode");
            assert_eq!(
                exited,
                r#"{"phase":"exited","error":"","launchId":"abc123","pid":0,"exitCode":3,"version":1}"#
            );
        }

        /// And the frame Core sends, spelled the way Core spells it.
        #[test]
        fn a_command_from_core_parses() {
            let spec: LaunchCommand = serde_json::from_str(
                r#"{"executable":"/usr/bin/flatpak","dir":"/home/deck","launchId":"abc123",
                    "args":["run","net.retrodeck.retrodeck"],"env":["HOME=/home/deck"],"version":1}"#,
            )
            .expect("parse command");
            assert_eq!(spec.executable, "/usr/bin/flatpak");
            assert_eq!(spec.dir, "/home/deck");
            assert_eq!(spec.launch_id, "abc123");
            assert_eq!(spec.args, ["run", "net.retrodeck.retrodeck"]);
            assert_eq!(spec.env, ["HOME=/home/deck"]);
            assert_eq!(spec.version, PROTOCOL_VERSION);
        }

        /// Core omits every empty field, so the optional ones have to default
        /// rather than fail the parse.
        #[test]
        fn a_command_without_optional_fields_parses() {
            let spec: LaunchCommand =
                serde_json::from_str(r#"{"executable":"true","launchId":"a","version":1}"#)
                    .expect("parse command");
            assert!(spec.dir.is_empty());
            assert!(spec.args.is_empty());
            assert!(spec.env.is_empty());
        }

        /// A Core that hangs up the moment we say hello closes cleanly, which
        /// is what a rejected peer looks like. Treating that as a working
        /// registration would spin at one second forever.
        #[test]
        fn a_rejected_registration_backs_off_instead_of_spinning() {
            let instant = Duration::from_millis(20);
            let mut backoff = RECONNECT_MIN;
            for _ in 0..8 {
                backoff = next_backoff(backoff, instant);
            }
            assert_eq!(backoff, RECONNECT_MAX);

            // A registration that actually held starts again from the bottom,
            // so an ordinary Core restart reconnects straight away.
            assert_eq!(next_backoff(backoff, SETTLED_REGISTRATION), RECONNECT_MIN);
            assert_eq!(
                next_backoff(RECONNECT_MIN, SETTLED_REGISTRATION * 4),
                RECONNECT_MIN
            );
        }

        #[test]
        fn the_sockets_come_from_the_runtime_directory() {
            std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
            assert_eq!(
                launch_socket_path(),
                Some(PathBuf::from("/run/user/1000/zaparoo/steam-runtime.sock"))
            );
            // Registration has its own path, so a Core that only binds the
            // launch socket per launch never hears from us at all.
            assert_eq!(
                host_socket_path(),
                Some(PathBuf::from(
                    "/run/user/1000/zaparoo/steam-launch-host.sock"
                ))
            );
            // A relative value is not a runtime directory, and guessing past
            // it would register us on a socket nobody reads.
            std::env::set_var("XDG_RUNTIME_DIR", "run/user/1000");
            assert_eq!(host_socket_path(), None);
            std::env::remove_var("XDG_RUNTIME_DIR");
            assert_eq!(host_socket_path(), None);
        }
    }
}
