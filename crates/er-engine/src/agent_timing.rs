//! Phase 0 measurement for the agent runner: where does agent wall-clock go?
//!
//! Answers three questions the optimisation backlog turns on:
//!
//! 1. How much of an agent run is slot wait, versus the child process running,
//!    versus our own post-processing?
//! 2. How long do spawners park waiting for a slot — and is work that is
//!    parked waiting reported to the user as "running"?
//! 3. Is arena wall-clock set by the round count, or by the concurrency cap?
//!
//! Off by default. Enable with `ER_AGENT_TIMING=1`; lines go to stderr with an
//! `er-agent` prefix so they can be separated from `er-desktop` profile lines.
//! Run the app with `ER_AGENT_TIMING=1` to collect a session's numbers.
//!
//! Timestamps are always recorded — three `Instant::now()` calls per agent run
//! round to nothing beside spawning a subprocess, and keeping the marks
//! unconditional means the numbers cannot silently differ between an
//! instrumented and an uninstrumented build. Only the reporting is gated.
//!
//! No new dependencies: `std::time::Instant` only. Child CPU time is measured
//! externally (`/usr/bin/time -l <provider>`), because Rust's std does not
//! expose `getrusage` for a reaped child.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static ENABLED: AtomicBool = AtomicBool::new(false);
static ENV_READ: OnceLock<()> = OnceLock::new();

/// Whether agent timing is on for this process.
///
/// The env var is read once; the flag itself is an `AtomicBool` so a test can
/// turn reporting on without depending on which test happened to call this
/// first. Nothing a caller does can turn it back off.
pub fn enabled() -> bool {
    ENV_READ.get_or_init(|| {
        if std::env::var("ER_AGENT_TIMING").as_deref() == Ok("1") {
            ENABLED.store(true, Ordering::Relaxed);
        }
    });
    ENABLED.load(Ordering::Relaxed)
}

/// Turn reporting on or off, ignoring the env var. Tests only.
#[cfg(test)]
pub fn set_enabled(on: bool) {
    let _ = ENV_READ.set(());
    ENABLED.store(on, Ordering::Relaxed);
}

// --------------------------------------------------------------- slot waits

static SLOT_ACQUIRES: AtomicU64 = AtomicU64::new(0);
static SLOT_BLOCKED: AtomicU64 = AtomicU64::new(0);
static SLOT_WAIT_TOTAL_MS: AtomicU64 = AtomicU64::new(0);
static SLOT_WAIT_MAX_MS: AtomicU64 = AtomicU64::new(0);

/// Process-wide slot-acquisition totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotWaitStats {
    pub acquires: u64,
    /// Acquisitions that had to wait at least 1ms — i.e. the cap was reached.
    pub blocked: u64,
    pub total_wait_ms: u64,
    pub max_wait_ms: u64,
}

/// Record one slot acquisition and how long it waited. Called from `SlotPool`
/// rather than from each spawn site, so every path is covered — including paths
/// added later.
pub fn record_slot_wait(waited: Duration) {
    if !enabled() {
        return;
    }
    let ms = waited.as_millis().min(u64::MAX as u128) as u64;
    SLOT_ACQUIRES.fetch_add(1, Ordering::Relaxed);
    SLOT_WAIT_TOTAL_MS.fetch_add(ms, Ordering::Relaxed);
    SLOT_WAIT_MAX_MS.fetch_max(ms, Ordering::Relaxed);
    if ms > 0 {
        SLOT_BLOCKED.fetch_add(1, Ordering::Relaxed);
    }
}

/// Current slot-wait totals for this process.
pub fn slot_wait_stats() -> SlotWaitStats {
    SlotWaitStats {
        acquires: SLOT_ACQUIRES.load(Ordering::Relaxed),
        blocked: SLOT_BLOCKED.load(Ordering::Relaxed),
        total_wait_ms: SLOT_WAIT_TOTAL_MS.load(Ordering::Relaxed),
        max_wait_ms: SLOT_WAIT_MAX_MS.load(Ordering::Relaxed),
    }
}

// ------------------------------------------------------- prepared-diff passes

// Annotation passes performed on this thread. Test-only, and thread-local so a
// test can assert its own delta while other tests in the binary run
// concurrently against their own counters.
#[cfg(test)]
thread_local! {
    static ANNOTATE_PASSES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Record one full annotation pass over a prepared diff.
///
/// Exists so a test can prove the pass was *skipped*. Counting writes cannot
/// show that: the write was already conditional before the skip was added, so
/// a write-counting test would pass against the unoptimised code too.
pub fn record_annotate_pass() {
    #[cfg(test)]
    ANNOTATE_PASSES.with(|c| c.set(c.get() + 1));
}

/// Annotation passes performed on this thread. Tests only.
#[cfg(test)]
pub fn annotate_passes() -> u64 {
    ANNOTATE_PASSES.with(|c| c.get())
}

// ------------------------------------------------------------------ emitting

/// Write one measurement line to stderr. No-op when timing is off.
pub fn emit(kind: &str, fields: &[(&str, String)]) {
    if !enabled() {
        return;
    }
    let mut parts = vec![format!("kind={kind}")];
    for (k, v) in fields {
        parts.push(format!("{k}={v}"));
    }
    eprintln!("er-agent {}", parts.join(" "));
}

/// Emit the process-wide slot-wait totals, so the cost of queueing behind the
/// cap is legible without a profiler attached.
pub fn emit_slot_summary(context: &str) {
    if !enabled() {
        return;
    }
    let s = slot_wait_stats();
    emit(
        "slot_summary",
        &[
            ("context", context.to_string()),
            ("acquires", s.acquires.to_string()),
            ("blocked", s.blocked.to_string()),
            ("total_wait_ms", s.total_wait_ms.to_string()),
            ("max_wait_ms", s.max_wait_ms.to_string()),
        ],
    );
}

// -------------------------------------------------------------- phase timings

/// Wall-clock split of one agent run, in milliseconds.
///
/// `queue` is acceptance to slot granted, so it covers both genuine slot wait
/// and time the worker thread spent not yet scheduled. `run` is the child's own
/// lifetime; on a healthy run it should dominate everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunPhases {
    pub queue_ms: u64,
    pub spawn_ms: u64,
    pub run_ms: u64,
    pub total_ms: u64,
}

impl RunPhases {
    /// Split a run into phases from its marks. A missing mark collapses the
    /// phase it would have bounded, so a partly-instrumented path reports
    /// unknown time as zero rather than inventing a slot wait.
    pub fn from_marks(
        started: Instant,
        slot_acquired: Option<Instant>,
        spawned: Option<Instant>,
        finished: Option<Instant>,
    ) -> Self {
        // `from` is always known; `to` is the mark that may be missing. Reading
        // them the other way round makes every span saturate to zero.
        let since = |from: Instant, to: Option<Instant>| -> u64 {
            to.map_or(0, |to| {
                to.saturating_duration_since(from).as_millis() as u64
            })
        };
        Self {
            queue_ms: since(started, slot_acquired),
            spawn_ms: since(slot_acquired.unwrap_or(started), spawned),
            run_ms: since(spawned.unwrap_or(started), finished),
            total_ms: since(started, finished),
        }
    }
}

/// Wall-clock marks for one agent run.
#[derive(Debug)]
pub struct AgentRunTimer {
    started: Instant,
    slot_acquired: Option<Instant>,
    spawned: Option<Instant>,
    finished: Option<Instant>,
}

impl AgentRunTimer {
    /// Begin timing, `started` being the moment the run was accepted.
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
            slot_acquired: None,
            spawned: None,
            finished: None,
        }
    }

    /// The slot was granted.
    pub fn mark_slot_acquired(&mut self) {
        self.slot_acquired = Some(Instant::now());
    }

    /// The child process was spawned.
    pub fn mark_spawned(&mut self) {
        self.spawned = Some(Instant::now());
    }

    /// The child was reaped and the run is over.
    pub fn mark_finished(&mut self) {
        self.finished = Some(Instant::now());
    }

    /// Phase split so far.
    pub fn phases(&self) -> RunPhases {
        RunPhases::from_marks(
            self.started,
            self.slot_acquired,
            self.spawned,
            self.finished,
        )
    }

    /// Emit the phase breakdown under `label`. `wait_pct` is slot wait as a
    /// percentage of the child's runtime — the single number that says whether
    /// the cap, rather than the model, is setting wall-clock.
    pub fn emit(self, label: &str, extra: &[(&str, String)]) {
        if !enabled() {
            return;
        }
        let p = self.phases();
        let mut fields: Vec<(&str, String)> = vec![
            // `site`, not `kind` — `emit` already writes `kind=run` in front,
            // and a second `kind=` key would be ambiguous.
            ("site", label.to_string()),
            ("queue_ms", p.queue_ms.to_string()),
            ("spawn_ms", p.spawn_ms.to_string()),
            ("run_ms", p.run_ms.to_string()),
            ("total_ms", p.total_ms.to_string()),
        ];
        fields.push((
            "wait_pct",
            format!("{}", p.queue_ms.saturating_mul(100) / p.run_ms.max(1)),
        ));
        for (k, v) in extra {
            fields.push((k, v.clone()));
        }
        emit("run", &fields);
    }

    /// Emit for a spawn site that has a command name and an outcome.
    ///
    /// Every spawn path reports the same two fields, and a new one should not
    /// have to rediscover their names. The per-phase marks stay at the call
    /// site — they sit at different points in each path's control flow, so
    /// they can't be folded in here.
    pub fn emit_run(self, label: &str, command: &str, ok: bool) {
        self.emit(label, &run_fields(command, ok));
    }
}

/// The two fields every spawn site reports alongside the phase breakdown.
///
/// Named here so a new spawn path does not have to guess them; see
/// `run_fields_names_the_same_two_fields_every_spawn_reports`.
fn run_fields(command: &str, ok: bool) -> [(&'static str, String); 2] {
    [("command", command.to_string()), ("ok", ok.to_string())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_fields_names_the_same_two_fields_every_spawn_reports() {
        // The point of `emit_run` is that a new spawn path inherits the field
        // names rather than reinventing them. Nothing reads this output at
        // runtime, so without this the names could drift unnoticed.
        let fields = run_fields("review", true);
        assert_eq!(fields[0], ("command", "review".to_string()));
        assert_eq!(fields[1], ("ok", "true".to_string()));

        let failed = run_fields("triage", false);
        assert_eq!(failed[1], ("ok", "false".to_string()));
    }

    #[test]
    fn phases_split_the_run() {
        let t0 = Instant::now();
        let slot = t0 + Duration::from_millis(40);
        let spawned = slot + Duration::from_millis(10);
        let done = spawned + Duration::from_millis(900);
        let p = RunPhases::from_marks(t0, Some(slot), Some(spawned), Some(done));
        assert_eq!(p.queue_ms, 40, "queue is acceptance to slot");
        assert_eq!(p.spawn_ms, 10, "spawn is slot to child live");
        assert_eq!(p.run_ms, 900, "run is the child's own lifetime");
        assert_eq!(p.total_ms, 950, "total spans the whole run");
    }

    #[test]
    fn missing_marks_collapse_instead_of_lying() {
        // A path that only marks start and finish: queue and spawn are unknown
        // and must read 0, with the whole span attributed to `run`, so a
        // partly-instrumented path cannot report a slot wait it never measured.
        let t0 = Instant::now();
        let done = t0 + Duration::from_millis(250);
        let p = RunPhases::from_marks(t0, None, None, Some(done));
        assert_eq!(p.queue_ms, 0);
        assert_eq!(p.spawn_ms, 0);
        assert_eq!(p.run_ms, 250);
        assert_eq!(p.total_ms, 250);
    }

    #[test]
    fn unstarted_run_reports_zero() {
        let p = RunPhases::from_marks(Instant::now(), None, None, None);
        assert_eq!(p, RunPhases::default());
    }

    #[test]
    fn timer_records_every_mark() {
        let mut t = AgentRunTimer::start();
        t.mark_slot_acquired();
        t.mark_spawned();
        t.mark_finished();
        let p = t.phases();
        assert!(p.total_ms >= p.run_ms, "total cannot be shorter than run");
    }

    #[test]
    fn a_real_run_attributes_time_to_the_child() {
        // End-to-end shape: a run that spends ~60ms in a child process must
        // report that time as `run_ms`, not as queue or spawn.
        let mut t = AgentRunTimer::start();
        t.mark_slot_acquired();
        t.mark_spawned();
        std::thread::sleep(Duration::from_millis(60));
        t.mark_finished();
        let p = t.phases();
        assert!(
            p.run_ms >= 50,
            "child time lands in run_ms, got {}",
            p.run_ms
        );
        // The phases tile the run. Each is truncated to whole milliseconds
        // independently, so the parts can sum to up to 2ms below the total.
        let parts = p.queue_ms + p.spawn_ms + p.run_ms;
        assert!(
            parts <= p.total_ms + 2 && parts + 2 >= p.total_ms,
            "phases should tile the run: parts={parts} total={}",
            p.total_ms
        );
    }
}
