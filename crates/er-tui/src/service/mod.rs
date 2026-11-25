//! The TUI: tabs open on the service, and each frame draws the published tab
//! content beside the TUI's own view state.
//!
//! Keys pick their action from the key table; navigation runs on the
//! `TuiView`, and anything that changes domain state goes to the service as a
//! command, sent without waiting.

mod agent_log;
mod ai_picker;
mod ai_runs;
mod chrome;
mod clears;
mod config_hub;
mod context;
mod copy;
mod export;
mod filter;
mod findings;
mod flows;
mod github;
mod handoff;
mod history;
mod layers;
mod modes;
mod open;
mod open_hub;
mod open_pr;
mod pr_probe;
mod review_hub;
mod reviewed;
mod session;
mod settings;
mod shell;
mod startup;
mod threads;
mod tour;
mod tui_view;
mod watched;
mod watcher;
mod worktree;

pub use shell::run;
pub use startup::Args;

#[cfg(test)]
mod ai_runs_tests;
#[cfg(test)]
mod chrome_tests;
#[cfg(test)]
mod clears_tests;
#[cfg(test)]
mod config_hub_tests;
#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod copy_tests;
#[cfg(test)]
mod duties_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod findings_tests;
#[cfg(test)]
mod flows_tests;
#[cfg(test)]
mod github_tests;
#[cfg(test)]
pub(crate) mod golden_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod loop_tests;
#[cfg(test)]
mod modes_tests;
#[cfg(test)]
mod open_tests;
#[cfg(test)]
mod parity;
#[cfg(test)]
mod pr_stack_tests;
#[cfg(test)]
mod review_hub_tests;
#[cfg(test)]
mod reviewed_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod thread_writes_tests;
#[cfg(test)]
mod threads_tests;
#[cfg(test)]
mod tour_tests;
#[cfg(test)]
mod watched_tests;
#[cfg(test)]
mod worktree_tests;
