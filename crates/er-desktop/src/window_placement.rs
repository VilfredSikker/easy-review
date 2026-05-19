//! Keep the main window fully usable when restoring saved geometry.
//!
//! `tauri-plugin-window-state` restores position if any window corner lies on a
//! monitor, which can still leave most of the window off-screen (e.g. after
//! display layout changes). We re-center when too little of the window is visible.

use tauri::{Monitor, WebviewWindow};

/// Minimum fraction of the window area that must lie on some monitor.
const MIN_VISIBLE_FRACTION: f64 = 0.55;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RectI32 {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn intersect_area(a: RectI32, b: RectI32) -> i64 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.w).min(b.x + b.w);
    let bottom = (a.y + a.h).min(b.y + b.h);
    let w = right - left;
    let h = bottom - top;
    if w <= 0 || h <= 0 {
        0
    } else {
        i64::from(w) * i64::from(h)
    }
}

fn monitor_rect(m: &Monitor) -> RectI32 {
    let pos = m.position();
    let size = m.size();
    RectI32 {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    }
}

fn visible_fraction(window: RectI32, monitors: &[Monitor]) -> f64 {
    let total = i64::from(window.w) * i64::from(window.h);
    if total <= 0 {
        return 0.0;
    }
    let visible = monitors
        .iter()
        .map(|m| intersect_area(window, monitor_rect(m)))
        .sum::<i64>();
    visible as f64 / total as f64
}

/// If the window is mostly off-screen after restore, center it on the current monitor.
pub fn ensure_window_visible(window: &WebviewWindow) -> tauri::Result<()> {
    let pos = window.outer_position()?;
    let size = window.outer_size()?;
    let window_rect = RectI32 {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    };

    let monitors = window.available_monitors()?;
    if monitors.is_empty() || visible_fraction(window_rect, &monitors) < MIN_VISIBLE_FRACTION {
        window.center()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_on_screen_counts_as_visible() {
        let window = RectI32 {
            x: 100,
            y: 100,
            w: 800,
            h: 600,
        };
        let monitors = [RectI32 {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        }];
        let frac = visible_fraction(window, &monitors);
        assert!(frac >= MIN_VISIBLE_FRACTION);
    }

    #[test]
    fn corner_only_is_not_enough() {
        let window = RectI32 {
            x: 1800,
            y: 1000,
            w: 800,
            h: 600,
        };
        let monitors = [RectI32 {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        }];
        let frac = visible_fraction(window, &monitors);
        assert!(frac < MIN_VISIBLE_FRACTION);
    }

    fn visible_fraction(window: RectI32, monitors: &[RectI32]) -> f64 {
        let total = i64::from(window.w) * i64::from(window.h);
        let visible = monitors
            .iter()
            .map(|m| intersect_area(window, *m))
            .sum::<i64>();
        visible as f64 / total as f64
    }
}
