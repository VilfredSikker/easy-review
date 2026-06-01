//! Keep the main window fully usable when restoring saved geometry.
//!
//! `tauri-plugin-window-state` restores position if any window corner lies on a
//! monitor, which can still leave most of the window off-screen (e.g. after
//! display layout changes). We clamp the position to fit on the best monitor.

use tauri::{Monitor, PhysicalPosition, WebviewWindow};

/// Below this visible fraction, the window is recentered instead of clamped.
const RECENTER_FRACTION: f64 = 0.30;

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

/// Clamp `window` so all four sides fit inside `monitor`. If the window is
/// larger than the monitor in some dimension, prefer the top-left edge.
fn clamp_to_monitor(window: RectI32, monitor: RectI32) -> RectI32 {
    let max_x = (monitor.x + monitor.w - window.w).max(monitor.x);
    let max_y = (monitor.y + monitor.h - window.h).max(monitor.y);
    RectI32 {
        x: window.x.clamp(monitor.x, max_x),
        y: window.y.clamp(monitor.y, max_y),
        w: window.w,
        h: window.h,
    }
}

/// Clamp the window to fit fully on the best monitor; recenter if hopelessly off-screen.
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
    if monitors.is_empty() {
        return Ok(());
    }

    // If hopelessly off-screen, recenter on primary monitor.
    if visible_fraction(window_rect, &monitors) < RECENTER_FRACTION {
        window.center()?;
        return Ok(());
    }

    // Pick the monitor with most overlap, clamp the window into its bounds.
    let best_monitor = monitors
        .iter()
        .max_by_key(|m| intersect_area(window_rect, monitor_rect(m)));

    if let Some(monitor) = best_monitor {
        let m_rect = monitor_rect(monitor);
        let clamped = clamp_to_monitor(window_rect, m_rect);
        if clamped.x != window_rect.x || clamped.y != window_rect.y {
            window.set_position(PhysicalPosition::new(clamped.x, clamped.y))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_pushes_window_left_when_right_edge_off_screen() {
        let monitor = RectI32 {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let window = RectI32 {
            x: 1500,
            y: 100,
            w: 1400,
            h: 900,
        };
        let clamped = clamp_to_monitor(window, monitor);
        assert_eq!(clamped.x, 1920 - 1400);
        assert_eq!(clamped.y, 100);
        assert_eq!(clamped.w, 1400);
        assert_eq!(clamped.h, 900);
    }

    #[test]
    fn clamp_pulls_window_into_negative_origin_monitor() {
        let monitor = RectI32 {
            x: -1920,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let window = RectI32 {
            x: 100,
            y: 100,
            w: 800,
            h: 600,
        };
        let clamped = clamp_to_monitor(window, monitor);
        assert_eq!(clamped.x, -1920 + 1920 - 800);
        assert!(clamped.x + clamped.w <= monitor.x + monitor.w);
    }

    #[test]
    fn clamp_noop_when_fully_inside() {
        let monitor = RectI32 {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let window = RectI32 {
            x: 100,
            y: 100,
            w: 800,
            h: 600,
        };
        let clamped = clamp_to_monitor(window, monitor);
        assert_eq!(clamped, window);
    }
}
