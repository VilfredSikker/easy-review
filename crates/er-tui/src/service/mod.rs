//! The flag-on TUI: tabs open on the service, and each frame draws the
//! published tab content beside the TUI's own view state.
//!
//! Nothing here reads the App. Keys pick their action from the table the App
//! path uses; navigation runs on the `TuiView`, and anything that changes
//! domain state goes to the service as a command, sent without waiting. An
//! action no service route covers yet says so on the status line.

mod session;
mod shell;
mod startup;
mod tui_view;

pub use shell::run;
pub use startup::Args;

/// Whether the flag build starts on the service. A constant, so the App path
/// stays reachable and compiled in this build until the flag is removed.
pub const RUNS_ON_SERVICE: bool = true;

#[cfg(test)]
mod loop_tests;
#[cfg(test)]
mod tests;
