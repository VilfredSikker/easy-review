//! Cancellable agent subprocesses.
//!
//! Every AI spawn path (background reviews, card AI, tab-local prompts) needs
//! the same three things: a child process that can be killed on demand, a
//! cancel flag the worker can poll so it never writes results for a run the
//! user stopped, and a process *group* so killing the CLI also takes down the
//! grandchildren it spawned (MCP servers, ripgrep, git).
//!
//! The arena (`arena/registry.rs`, `arena/adapter.rs`) already does the child
//! tracking and flag-polling half of this. Killing the *process group* is new
//! here — nothing else in the crate calls `process_group`. The arena keeps its
//! own copy of the rest; folding it into this module is a follow-up, not part
//! of this change.
//!
//! ## Ordering
//!
//! The sequence below is what a **caller** does; this module supplies the
//! pieces, not the pipes. It mirrors the arena's, and exists because `Child`
//! is not `Clone` and the registry lock must not be held across a blocking
//! `wait`:
//!
//! ```ignore
//! let mut child = handle.spawn(&mut cmd)?;   // own process group + record pid
//! let id = child.id();
//! let stdout = child.stdout.take();          // pipes first…
//! handle.register(child);                    // …then hand ownership over
//! let (out, err) = read_pipes_concurrently(stdout, stderr);   // caller's own
//! let child = handle.take_child(id).expect("registered");
//! let status = handle.wait_for(child)?;      // waits, then marks finished
//! if handle.is_cancelled() { return Err(cancelled()); }   // ← the verdict
//! ```
//!
//! [`AgentRunHandle::wait_for`] exists so marking the run finished cannot be
//! forgotten alongside the wait: skipping it leaves `kill` free to signal a
//! pid the OS may since have recycled.
//!
//! Not to be confused with [`crate::agent_runtime`], the sibling module that
//! resolves which provider, model and argv an agent runs with. This module
//! never decides *what* to run — it owns the process once something else has.
//!
//! ## The single-writer rule
//!
//! [`AgentRunHandle::kill`] only sets the flag and signals the process. It
//! never records an outcome. The worker that owns the child decides the
//! outcome exactly once, after `wait()` returns, by consulting
//! [`AgentRunHandle::is_cancelled`]. That split is what makes "never both
//! write a reply and report cancelled" provable rather than a race: whoever
//! reads the flag last still sees a single consistent verdict.

use std::process::{Child, Command};
// Only the signalling paths and the unix tests reach for `Stdio`; importing it
// unconditionally breaks `clippy -D warnings` on a target that has neither.
#[cfg(any(unix, windows))]
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Marker error returned by a worker whose run was stopped.
///
/// A typed sentinel rather than the arena's `bail!("cancelled")` string match:
/// this error travels back through `anyhow` context wrapping and must stay
/// distinguishable from a genuine failure, or a stopped run would surface as
/// "review failed".
#[derive(Debug)]
pub struct RunCancelled;

impl std::fmt::Display for RunCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "run cancelled")
    }
}

impl std::error::Error for RunCancelled {}

/// Build the [`RunCancelled`] error to bail out with.
pub fn cancelled() -> anyhow::Error {
    anyhow::Error::new(RunCancelled)
}

/// Does this error mean the run was stopped (as opposed to failing)?
pub fn is_run_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<RunCancelled>().is_some()
}

/// A cancellable run: the flag, the live children, and the process group.
///
/// Held as `Arc<AgentRunHandle>`; the owning registry keeps one clone and the
/// worker thread keeps another.
#[derive(Debug)]
pub struct AgentRunHandle {
    cancel: AtomicBool,
    /// Set by the worker once it has reaped the child. After this, `kill` must
    /// not signal the recorded pid: reaping frees the pid for reuse, so a
    /// signal sent now could land on an unrelated process group.
    finished: AtomicBool,
    /// The process group `spawn` created, when it created one. Only ever set
    /// by `spawn`; a child handed over by `register` leads no group, and
    /// signalling `-pid` for it would target whatever group owns that number —
    /// which on a bad day is er's own.
    pgid: Mutex<Option<i32>>,
    /// Live children: pushed by `register`, taken back out by the worker just
    /// before it waits. `kill` walks this to reach children the worker has not
    /// taken yet — the window between `register` and `take_child`.
    children: Mutex<Vec<Child>>,
    /// Pid of the leader, recorded at spawn.
    pid: Mutex<Option<i32>>,
}

impl AgentRunHandle {
    /// Handles are always shared, so there is no plain-`Self` constructor and
    /// deliberately no `Default`: a lone handle could never be `kill`ed from
    /// another thread, and `Arc::new(AgentRunHandle::default())` would hand
    /// out exactly that.
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            cancel: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            pgid: Mutex::new(None),
            children: Mutex::new(Vec::new()),
            pid: Mutex::new(None),
        })
    }

    /// Has this run been stopped? Workers consult this before every
    /// observable side effect.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// The cancel flag itself, for APIs that take `&AtomicBool` rather than a
    /// handle — notably [`crate::agent_slots::acquire`], which aborts a run
    /// still waiting for a concurrency slot.
    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    /// Record that the child has been reaped and nothing is left to kill.
    ///
    /// The worker calls this immediately after `wait()` returns, before it
    /// decides the outcome. Without it, a `kill` arriving after reaping would
    /// signal a pid the OS is free to have reused.
    pub fn mark_finished(&self) {
        self.finished.store(true, Ordering::SeqCst);
    }

    /// Has the child been reaped?
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    /// Spawn a child into its own process group, so `kill` reaches the whole
    /// tree rather than just the CLI.
    ///
    /// The caller still owns the returned child and must `register` it once
    /// its pipes have been taken.
    #[cfg(unix)]
    pub fn spawn(&self, cmd: &mut Command) -> anyhow::Result<Child> {
        use std::os::unix::process::CommandExt;
        // New process group; the child's pid becomes the group's id.
        cmd.process_group(0);
        let child = cmd.spawn()?;
        // This pid is also a group id: the child now leads that group.
        *self.pgid.lock().unwrap_or_else(|e| e.into_inner()) = Some(child.id() as i32);
        self.record_pid(&child);
        Ok(child)
    }

    /// Non-unix fallback. std exposes no process groups, so none is created
    /// and `pgid` stays clear; `kill` signals the pid instead, which on Windows
    /// goes through `taskkill /T` and so still reaches the CLI's children.
    #[cfg(not(unix))]
    pub fn spawn(&self, cmd: &mut Command) -> anyhow::Result<Child> {
        let child = cmd.spawn()?;
        self.record_pid(&child);
        Ok(child)
    }

    /// Wait for a child and record that the run is over, in one step.
    ///
    /// Always prefer this to a bare `child.wait()`: the two must happen
    /// together, and a wait that forgets [`AgentRunHandle::mark_finished`]
    /// leaves `kill` free to signal a pid the OS may since have handed to
    /// another process.
    pub fn wait_for(&self, mut child: Child) -> std::io::Result<std::process::ExitStatus> {
        let status = child.wait();
        self.mark_finished();
        status
    }

    /// Record the leader's pid. Not "only the first": a handle describes one
    /// run, and later spawns on the same handle are a caller bug rather than
    /// something worth silently supporting.
    fn record_pid(&self, child: &Child) {
        let mut pid = self.pid.lock().unwrap_or_else(|e| e.into_inner());
        *pid = Some(child.id() as i32);
    }

    /// Hand ownership of a spawned child to the registry so `kill` can reach
    /// it. Take the pipes off it first — `wait_with_output` needs them.
    ///
    /// Normally this is the child `spawn` just returned. Registering any other
    /// child clears the recorded group, because that child does not lead it.
    pub fn register(&self, child: Child) {
        let id = child.id() as i32;
        let mut pgid = self.pgid.lock().unwrap_or_else(|e| e.into_inner());
        if *pgid != Some(id) {
            *pgid = None;
        }
        drop(pgid);
        self.record_pid(&child);
        let mut kids = self.children.lock().unwrap_or_else(|e| e.into_inner());
        kids.push(child);
    }

    /// Take a registered child back out, so the caller can `wait()` on it
    /// without holding the registry lock. Holding the mutex across a
    /// long-running agent would block unrelated spawns.
    pub fn take_child(&self, id: u32) -> Option<Child> {
        let mut kids = self.children.lock().unwrap_or_else(|e| e.into_inner());
        let idx = kids.iter().position(|c| c.id() == id)?;
        Some(kids.remove(idx))
    }

    /// Stop the run: set the flag, then signal the process.
    ///
    /// The flag is set *first*, and unconditionally, so any worker about to
    /// record an outcome already sees it even if every signal below fails.
    /// Idempotent, and harmless after the run finished.
    ///
    /// Takes no lock the caller owns and never re-enters caller state, so it
    /// is safe to call from any thread. It does fork `kill`, though, so
    /// AGENTS.md's "never hold the app mutex during network or subprocess
    /// work" applies the usual way: a caller holding a long-lived lock should
    /// clone the handle, release the lock, then call this.
    pub fn kill(&self) {
        self.cancel.store(true, Ordering::SeqCst);

        // Reaping frees the pid for reuse, so once the worker has finished,
        // signalling is unsafe as well as pointless.
        if self.is_finished() {
            return;
        }

        let pid = *self.pid.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pid) = pid {
            // SIGTERM so a well-behaved CLI can clean up, then SIGKILL
            // immediately: these agents are read-only, so there is no state
            // worth waiting for, and a grace period would need a timer thread
            // plus a pid-reuse guard for no benefit.
            //
            // Signal the group only when this handle created one. A child that
            // arrived via `register` leads no group, so `-pid` names someone
            // else's, and the pid is signalled directly instead.
            let pgid = *self.pgid.lock().unwrap_or_else(|e| e.into_inner());
            match pgid {
                Some(pgid) => {
                    signal_group(pgid, "TERM");
                    signal_group(pgid, "KILL");
                }
                None => {
                    signal_pid(pid, "TERM");
                    signal_pid(pid, "KILL");
                }
            }
        }

        // Reaches children the worker has not taken out yet — between
        // `register` and `take_child`, and on a non-unix target, where the
        // signal above is a no-op and this loop is the only kill that lands.
        // Once the worker holds the child for `wait`, the pid signal above is
        // what reaches it.
        let mut kids = self.children.lock().unwrap_or_else(|e| e.into_inner());
        for child in kids.iter_mut() {
            let _ = child.kill();
        }
    }
}

/// A handle dropped before its run finished means nothing else is going to
/// stop it: the worker panicked, or the app is shutting down. Kill, so the CLI
/// tree is not orphaned.
///
/// This fires only when the *last* clone drops, and never after
/// `mark_finished`, so the normal completion path is unaffected.
///
/// Two consequences worth knowing at a call site:
///
/// - **It cancels the run.** The flag is set, so a worker that is mid-flight
///   will find `is_cancelled()` true after its `wait` and report the run
///   stopped, discarding whatever it produced. That is the right reading of
///   "nobody holds this handle any more", but it is a decision, not a no-op.
/// - **It forks `kill`.** Avoid dropping a live handle while holding a lock
///   you would mind holding across a subprocess; dropping a *finished* one
///   signals nothing and is free.
impl Drop for AgentRunHandle {
    fn drop(&mut self) {
        if !self.is_finished() {
            self.kill();
        }
        // Reap whatever we still own. `kill` leaves children as zombies and no
        // worker is left to collect them once the handle is gone. Draining
        // *after* the waits keeps this clear of the arena's mistake — that one
        // cleared unreaped children, which is a zombie leak.
        let mut kids = self.children.lock().unwrap_or_else(|e| e.into_inner());
        for child in kids.iter_mut() {
            let _ = child.wait();
        }
        kids.clear();
    }
}

/// Signal a whole process group, named by its leader's pid.
#[cfg(unix)]
fn signal_group(pgid: i32, sig: &str) {
    signal(&format!("-{pgid}"), sig);
}

/// Signal a single process by pid.
#[cfg(unix)]
fn signal_pid(pid: i32, sig: &str) {
    signal(&pid.to_string(), sig);
}

/// `kill -s <sig> -- <target>`.
///
/// `--` stops a negative target being parsed as an option, which some `kill`
/// builds require.
///
/// Both candidates are absolute so a GUI launch with a thin `PATH` still finds
/// one, and so a shadowing `kill` earlier on `PATH` can never be picked up.
/// Shelling out is the only option: std exposes no signalling API and this
/// crate deliberately carries no `libc`/`nix` dependency. If neither binary
/// can be launched, the caller's registered-children loop is the only kill
/// that lands.
#[cfg(unix)]
fn signal(target: &str, sig: &str) {
    for bin in ["/bin/kill", "/usr/bin/kill"] {
        let ran = Command::new(bin)
            .args(["-s", sig, "--", target])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok();
        // Either it ran (whatever it reported about the pid), or the binary
        // is missing and the next candidate is worth trying.
        if ran {
            return;
        }
    }
}

/// Windows has no process groups, but `taskkill /T` walks the child tree,
/// which is the same thing the group signal buys on unix. So the group call is
/// a no-op and the pid call carries the weight.
///
/// Untested: this repo is macOS-primary and CI does not build for Windows.
#[cfg(windows)]
fn signal_group(_pid: i32, _sig: &str) {}

#[cfg(windows)]
fn signal_pid(pid: i32, _sig: &str) {
    // `/F` is already force, so the signal name is not used.
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Any other target: nothing to signal with. `kill` still sets the flag and
/// still `Child::kill`s whatever is registered, so a run the worker has not
/// yet taken out can be stopped, but one it already holds cannot.
#[cfg(not(any(unix, windows)))]
fn signal_group(_pid: i32, _sig: &str) {}

#[cfg(not(any(unix, windows)))]
fn signal_pid(_pid: i32, _sig: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_handle_is_not_cancelled() {
        assert!(!AgentRunHandle::new().is_cancelled());
    }

    /// A live child, so this exercises the real kill path rather than only the
    /// atomic store at the top of it.
    #[cfg(unix)]
    #[test]
    fn kill_sets_the_flag_and_is_idempotent() {
        let handle = AgentRunHandle::new();
        let mut cmd = Command::new("sleep");
        cmd.arg("30").stdout(Stdio::null()).stderr(Stdio::null());
        let child = handle.spawn(&mut cmd).expect("spawn");
        let id = child.id();
        handle.register(child);

        handle.kill();
        handle.kill(); // must not panic on an already-killed child

        assert!(handle.is_cancelled());
        let child = handle.take_child(id).expect("registered");
        assert!(
            handle.wait_for(child).is_ok(),
            "wait_for must not fail on a killed child"
        );
        assert!(handle.is_finished());
    }

    #[test]
    fn kill_sets_the_flag_without_a_child() {
        let handle = AgentRunHandle::new();
        handle.kill();
        handle.kill();
        assert!(handle.is_cancelled());
    }

    #[test]
    fn run_cancelled_round_trips_through_anyhow_context() {
        let err = cancelled().context("while reviewing");
        assert!(is_run_cancelled(&err));
        assert!(!is_run_cancelled(&anyhow::anyhow!("boom")));
    }

    /// After reaping, the pid is free for reuse, so `kill` must stop
    /// signalling — but it must still set the flag, because the worker's
    /// verdict comes from the flag and a stop that raced completion must not
    /// silently evaporate.
    ///
    /// The child is deliberately left unreaped and alive to stand in for
    /// whatever process may since have inherited the pid: it surviving the
    /// kill is what proves no signal was sent.
    #[cfg(unix)]
    #[test]
    fn kill_after_finish_sets_the_flag_without_signalling() {
        let handle = AgentRunHandle::new();
        let mut cmd = Command::new("sleep");
        cmd.arg("30").stdout(Stdio::null()).stderr(Stdio::null());
        let child = handle.spawn(&mut cmd).expect("spawn");
        let id = child.id();
        handle.register(child);

        assert!(!handle.is_finished());
        handle.mark_finished();
        handle.kill();

        assert!(handle.is_cancelled(), "the flag must still be set");

        let mut child = handle.take_child(id).expect("still registered");
        // Long enough for a signal to be delivered and the child to exit, so
        // `try_wait` returning `None` means nothing was sent.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "kill signalled a pid belonging to a finished run"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn finished_defaults_to_false_and_sticks() {
        let h = AgentRunHandle::new();
        assert!(!h.is_finished());
        h.mark_finished();
        assert!(h.is_finished());
    }

    /// The point of the process group: a grandchild the CLI spawned must die
    /// too. Before this module, killing only the direct child left it running.
    ///
    /// The child is taken out before the kill, exactly as a worker does before
    /// `wait`, so the registered-children fallback is empty and the group
    /// signal is the only mechanism that can reach it. That is the real path,
    /// and it is what makes this test meaningful rather than incidental.
    #[cfg(unix)]
    #[test]
    fn kill_takes_down_grandchildren_in_the_group() {
        let dir = std::env::temp_dir().join(format!("er-agentrun-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let pid_file = dir.join("grandchild.pid");
        let _ = std::fs::remove_file(&pid_file);

        let handle = AgentRunHandle::new();
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            // Background a grandchild, record its pid, then stay alive so the
            // leader is there for us to kill.
            .arg(format!("sleep 30 & echo $! > {}; wait", pid_file.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let child = handle.spawn(&mut cmd).expect("spawn");
        let leader = child.id();
        handle.register(child);

        let grandchild = wait_for_pid_file(&pid_file).expect("grandchild pid written");

        let child = handle.take_child(leader).expect("registered");
        handle.kill();
        let status = handle.wait_for(child).expect("wait");

        // The claim under test, asserted first so a failure names the real
        // defect rather than a knock-on symptom.
        assert!(
            wait_until_gone(grandchild),
            "grandchild {grandchild} survived the group kill"
        );
        // Signalled, not exited — the leader had no reason to return on its
        // own within the test's lifetime.
        assert!(
            !status.success(),
            "leader exited cleanly instead of being killed"
        );
    }

    #[cfg(unix)]
    fn wait_for_pid_file(path: &std::path::Path) -> Option<i32> {
        for _ in 0..100 {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(pid) = text.trim().parse::<i32>() {
                    return Some(pid);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        None
    }

    /// Poll until the process is gone; true when it died inside the window.
    #[cfg(unix)]
    fn wait_until_gone(pid: i32) -> bool {
        for _ in 0..100 {
            if !proc_alive(pid) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    /// Signal-0 probe on a single **pid** (positive), not a process group.
    ///
    /// Deliberately positive: the grandchild here is not a group leader, so a
    /// `-pid` probe would test for a group that never existed and report
    /// "dead" no matter what — which is exactly how an earlier version of this
    /// test passed while proving nothing.
    #[cfg(unix)]
    fn proc_alive(pid: i32) -> bool {
        Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// `take_child` hands the child back exactly once, so the worker can wait
    /// on it while holding no lock.
    #[cfg(unix)]
    #[test]
    fn take_child_returns_it_exactly_once() {
        let handle = AgentRunHandle::new();
        let mut cmd = Command::new("sleep");
        cmd.arg("30").stdout(Stdio::null()).stderr(Stdio::null());
        let child = handle.spawn(&mut cmd).expect("spawn");
        let id = child.id();
        handle.register(child);

        let taken = handle.take_child(id).expect("first take");
        assert!(handle.take_child(id).is_none(), "second take must be None");

        handle.kill();
        let _ = handle.wait_for(taken);
    }

    /// A child handed over by `register` alone never got its own process
    /// group, so `kill` must signal it by pid rather than targeting `-pid`
    /// (which would name an unrelated group). It still has to die.
    #[cfg(unix)]
    #[test]
    fn register_makes_a_plain_child_killable_by_pid() {
        let handle = AgentRunHandle::new();
        let mut cmd = Command::new("sleep");
        cmd.arg("30").stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn().expect("spawn");
        let id = child.id();
        assert!(handle.pid.lock().unwrap().is_none());

        handle.register(child);
        assert!(
            handle.pgid.lock().unwrap().is_none(),
            "a registered child must not be treated as a group leader"
        );
        assert_eq!(*handle.pid.lock().unwrap(), Some(id as i32));

        handle.kill();
        let taken = handle.take_child(id).expect("registered");
        let status = handle.wait_for(taken).expect("wait");
        assert!(!status.success(), "kill did not stop the registered child");
    }

    /// Registering a child that is not the one `spawn` produced must clear the
    /// recorded group. Otherwise `kill` aims `-pid` at a group that child does
    /// not lead — which is whatever owns that number, on a bad day er's own.
    #[cfg(unix)]
    #[test]
    fn register_of_a_foreign_child_clears_the_group() {
        let handle = AgentRunHandle::new();

        let mut first = Command::new("sleep");
        first.arg("30").stdout(Stdio::null()).stderr(Stdio::null());
        let leader = handle.spawn(&mut first).expect("spawn");
        let leader_id = leader.id();
        handle.register(leader);
        assert_eq!(*handle.pgid.lock().unwrap(), Some(leader_id as i32));

        // Spawned plainly, so it is in our group, not its own.
        let mut second = Command::new("sleep");
        second.arg("30").stdout(Stdio::null()).stderr(Stdio::null());
        let other = second.spawn().expect("spawn");
        let other_id = other.id();
        handle.register(other);

        assert!(
            handle.pgid.lock().unwrap().is_none(),
            "a foreign child must not inherit the recorded group"
        );
        assert_eq!(*handle.pid.lock().unwrap(), Some(other_id as i32));

        handle.kill();
        // Dropping reaps whatever is still registered.
        drop(handle);
    }

    /// Dropping the last clone of a live handle stops the run, so a worker
    /// that panicked mid-flight cannot orphan the CLI tree.
    ///
    /// The child is 30s from writing a marker file. Observing the *marker*
    /// rather than the process is deliberate: `Drop` reaps its children, so
    /// "the process is gone" is true either way — without the kill it is
    /// merely true 30 seconds later, once the child has finished the job the
    /// drop was supposed to prevent.
    #[cfg(unix)]
    #[test]
    fn drop_of_a_live_handle_kills_the_child() {
        let dir = std::env::temp_dir().join(format!("er-agentrun-drop-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let marker = dir.join("completed");
        let _ = std::fs::remove_file(&marker);

        {
            let handle = AgentRunHandle::new();
            let mut cmd = Command::new("sh");
            cmd.arg("-c")
                .arg(format!("sleep 30; echo done > {}", marker.display()))
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = handle.spawn(&mut cmd).expect("spawn");
            handle.register(child);
            // The handle is the only clone, so it drops here.
        }

        assert!(
            !marker.exists(),
            "dropping a live handle let the run finish instead of stopping it"
        );
    }

    /// The normal path: a finished handle drops without signalling. The child
    /// was already reaped by `wait_for`, so `kill` must see `finished` and
    /// return rather than signalling a pid the OS could have reused.
    #[cfg(unix)]
    #[test]
    fn drop_of_a_finished_handle_signals_nothing() {
        let handle = AgentRunHandle::new();
        let mut cmd = Command::new("true");
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let child = handle.spawn(&mut cmd).expect("spawn");
        assert!(handle.wait_for(child).expect("wait").success());

        handle.kill();
        assert!(handle.is_finished());
        drop(handle);
    }
}
