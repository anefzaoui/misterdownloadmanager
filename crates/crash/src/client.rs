//! The app's side: start the monitor, connect to it, tell it what it needs to know, ask it for
//! dumps. (Catching native crashes is the app's platform layer, win32.rs / unix.rs: it hands
//! [`Reporter::dump`] to crash-handler.)

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::protocol::{AppState, Freeze, Panic, Session, kind};

/// How long the connection is tried. The window never waits for the monitor, so a slow one (a
/// cold disk at log-on) is given a minute; past it MDM runs on without crash reports, as when
/// the monitor couldn't start at all.
const CONNECT_FOR: Duration = Duration::from_secs(60);
/// Between two tries to connect.
const RETRY: Duration = Duration::from_millis(25);

/// The app's connection to its monitor.
pub struct Reporter {
    /// The connection, made in the background: `None` until it is there, and again after a
    /// give-up. What is sent meanwhile is kept and goes over when it is made.
    client: Mutex<Option<minidumper::Client>>,
    monitor: Mutex<Option<Child>>,
    /// The session as sent (sent again when the connection comes).
    session: Mutex<Option<Session>>,
    /// The last state sent (a change is sent, not every frame).
    last_state: Mutex<Option<AppState>>,
}

impl Reporter {
    /// Start `exe --crash-monitor <socket> --data <dir> --parent <pid>` and connect to it from a
    /// thread of its own. None when it can't start: MDM runs on without crash reports.
    pub fn start(exe: &Path, data: &Path) -> Option<Arc<Reporter>> {
        let socket = crate::monitor::socket_path(std::process::id());
        let mut cmd = Command::new(exe);
        cmd.arg(crate::MONITOR_ARG)
            .arg(&socket)
            .arg("--data")
            .arg(data)
            .arg("--parent")
            .arg(std::process::id().to_string());
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("crash monitor didn't start: {e}");
                return None;
            }
        };
        let reporter = Arc::new(Reporter {
            client: Mutex::new(None),
            monitor: Mutex::new(Some(child)),
            session: Mutex::new(None),
            last_state: Mutex::new(None),
        });
        Self::connect_in_background(&reporter, socket);
        Some(reporter)
    }

    /// Connect to the monitor from a thread of its own. It listens a moment after it starts,
    /// and a slow start (or a socket the system refuses for a while: a Windows Server was seen
    /// refusing MDM's for five seconds with os error 10022, #476) must neither hold the window
    /// back nor take the monitor away: it may answer later, and what was sent meanwhile goes
    /// over then.
    fn connect_in_background(reporter: &Arc<Self>, socket: PathBuf) {
        let reporter = Arc::clone(reporter);
        let spawned = std::thread::Builder::new()
            .name("monitor connector".into())
            .spawn(move || reporter.connect_until(socket, Instant::now() + CONNECT_FOR));
        if let Err(e) = spawned {
            tracing::warn!("crash monitor: no thread to connect with ({e})");
        }
    }

    /// Try to connect until it answers, the monitor goes on its own, or the time is up.
    fn connect_until(self: Arc<Self>, socket: PathBuf, deadline: Instant) {
        let started = Instant::now();
        loop {
            match minidumper::Client::with_name(minidumper::SocketName::Path(&socket)) {
                Ok(client) => {
                    *self.lock_client() = Some(client);
                    tracing::info!(
                        "crash monitor {} listening, {} ms after start",
                        self.monitor_pid().unwrap_or_default(),
                        started.elapsed().as_millis()
                    );
                    // What was sent before the connection goes again: the monitor missed it.
                    let kept = self.lock_session().clone();
                    if let Some(s) = kept {
                        self.send(kind::SESSION, &s);
                    }
                    let kept = self.lock_state().clone();
                    if let Some(s) = kept {
                        self.send(kind::STATE, &s);
                    }
                    return;
                }
                Err(_) if Instant::now() < deadline => {
                    // A monitor that couldn't open its socket has already said why in its own
                    // log and gone: don't try for a process that is already gone.
                    if self.monitor_exited() {
                        tracing::warn!("crash monitor exited before it listened");
                        return;
                    }
                    std::thread::sleep(RETRY);
                }
                Err(e) => {
                    tracing::warn!("crash monitor didn't answer: {e}");
                    // It can never answer now: don't leave a second MDM process behind.
                    if let Some(mut child) = self.lock_monitor().take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    return;
                }
            }
        }
    }

    /// The monitor's process id (Linux lets only it read this process's memory: ptrace).
    pub fn monitor_pid(&self) -> Option<u32> {
        self.lock_monitor().as_ref().map(Child::id)
    }

    /// Connected to the monitor? (The connection is made in the background.)
    pub fn connected(&self) -> bool {
        self.lock_client().is_some()
    }

    fn send(&self, kind: u32, body: &impl serde::Serialize) {
        if let Ok(b) = serde_json::to_vec(body)
            && let Some(client) = self.lock_client().as_ref()
        {
            let _ = client.send_message(kind, b);
        }
    }

    pub fn session(&self, s: &Session) {
        *self.lock_session() = Some(s.clone());
        self.send(kind::SESSION, s);
    }

    /// Send the state when it changed since the last time.
    pub fn state(&self, s: &AppState) {
        let changed = {
            let mut last = self.lock_state();
            let changed = last.as_ref() != Some(s);
            if changed {
                *last = Some(s.clone());
            }
            changed
        };
        if changed {
            self.send(kind::STATE, s);
        }
    }

    pub fn panic(&self, p: &Panic) {
        self.send(kind::PANIC, p);
        // The messages are in before the dump request (on macOS they travel separately).
        self.ping();
    }

    pub fn freeze(&self, f: &Freeze) {
        self.send(kind::FREEZE, f);
        self.ping();
    }

    pub fn recovered(&self) {
        self.send(kind::RECOVERED, &());
    }

    /// The app is closing normally: no report.
    pub fn clean(&self) {
        self.send(kind::CLEAN, &());
        self.ping();
    }

    fn ping(&self) {
        if let Some(client) = self.lock_client().as_ref() {
            let _ = client.ping();
        }
    }

    /// Ask the monitor for a dump of this process, `context` being the thread that asks (from
    /// crash-handler). True when it was written.
    pub fn dump(&self, context: &crash_handler::CrashContext) -> bool {
        self.lock_client()
            .as_ref()
            .is_some_and(|client| client.request_dump(context).is_ok())
    }

    fn lock_client(&self) -> MutexGuard<'_, Option<minidumper::Client>> {
        self.client.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_monitor(&self) -> MutexGuard<'_, Option<Child>> {
        self.monitor.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_session(&self) -> MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_state(&self) -> MutexGuard<'_, Option<AppState>> {
        self.last_state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The monitor is gone on its own (it couldn't open its socket, or the computer took it).
    fn monitor_exited(&self) -> bool {
        self.lock_monitor()
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(Some(_))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A monitor that couldn't open its socket exits at once: `start` answers at once (the
    /// window never waits for the monitor), and nothing ever connects.
    #[test]
    fn a_monitor_that_exits_before_listening_is_not_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        // A system program that starts and exits at once; the monitor's arguments mean nothing
        // to it.
        let exe = if cfg!(windows) {
            "whoami.exe"
        } else {
            // macOS has no /bin/true: found on the PATH.
            "true"
        };
        let started = Instant::now();
        let reporter =
            Reporter::start(Path::new(exe), dir.path()).expect("the monitor program starts");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "waited {:?} to start the monitor",
            started.elapsed()
        );
        // The connector sees the exit on its own, and no connection comes of it.
        let limit = Instant::now() + Duration::from_secs(5);
        while Instant::now() < limit && !reporter.monitor_exited() {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reporter.monitor_exited(), "the monitor's exit went unseen");
        assert!(!reporter.connected());
    }

    /// What is sent before the connection is made is kept for it: a monitor that listens late
    /// (a slow start at log-on, or a socket the system refuses for a while) misses nothing.
    #[test]
    fn what_is_sent_before_the_connection_is_kept_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("late.sock");
        /// What the monitor's side has received, as (kind, body).
        type Received = Vec<(u32, Vec<u8>)>;
        // The monitor's side, beside the test: it records what arrives.
        struct Record(Arc<std::sync::Mutex<Received>>);
        impl minidumper::ServerHandler for Record {
            fn create_minidump_file(
                &self,
            ) -> Result<(std::fs::File, std::path::PathBuf), std::io::Error> {
                Err(std::io::Error::other("no dumps are asked for here"))
            }
            fn on_minidump_created(
                &self,
                _: Result<minidumper::MinidumpBinary, minidumper::Error>,
            ) -> minidumper::LoopAction {
                minidumper::LoopAction::Continue
            }
            fn on_message(&self, kind: u32, buffer: Vec<u8>) {
                self.0.lock().unwrap().push((kind, buffer));
            }
        }
        let seen: Arc<std::sync::Mutex<Received>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut server =
            minidumper::Server::with_name(minidumper::SocketName::Path(&socket)).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        {
            let seen = Arc::clone(&seen);
            let shutdown = Arc::clone(&shutdown);
            std::thread::spawn(move || {
                let _ = server.run(Box::new(Record(seen)), &shutdown, None);
            });
        }
        // A reporter with no monitor process of its own: only its connection is tested. The
        // session and the state are sent before it is connected, the way they are when the
        // monitor is slow.
        let reporter = Arc::new(Reporter {
            client: Mutex::new(None),
            monitor: Mutex::new(None),
            session: Mutex::new(None),
            last_state: Mutex::new(None),
        });
        let session = Session {
            version: "test".into(),
            ..Default::default()
        };
        reporter.session(&session);
        let state = AppState {
            downloads: 2,
            running: 1,
            ..Default::default()
        };
        reporter.state(&state);
        assert!(!reporter.connected());
        Reporter::connect_in_background(&reporter, socket);
        let limit = Instant::now() + Duration::from_secs(5);
        while !reporter.connected() && Instant::now() < limit {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reporter.connected(), "the connection never came");
        let arrived = |wanted: u32| seen.lock().unwrap().iter().any(|(k, _)| *k == wanted);
        // The messages reach the monitor once it has read them (a moment after the connect).
        let limit = Instant::now() + Duration::from_secs(5);
        while !arrived(kind::SESSION) && Instant::now() < limit {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            arrived(kind::SESSION),
            "the session didn't reach the monitor"
        );
        let limit = Instant::now() + Duration::from_secs(5);
        while !arrived(kind::STATE) && Instant::now() < limit {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(arrived(kind::STATE), "the state didn't reach the monitor");
        // And what is sent once connected goes once, not again with the kept ones.
        reporter.clean();
        let count = |wanted: u32| {
            seen.lock()
                .unwrap()
                .iter()
                .filter(|(k, _)| *k == wanted)
                .count()
        };
        assert_eq!(count(kind::SESSION), 1);
        assert_eq!(count(kind::STATE), 1);
        shutdown.store(true, Ordering::Relaxed);
    }
}
