//! Debug-only native parking experiment. Nothing in the product calls this adapter.

use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tauri::Manager;

#[cfg(target_os = "macos")]
const FIXTURE_LABEL: &str = "parking-proof";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    HideOnly,
    HideAndResign,
    Detach,
}

impl std::str::FromStr for Variant {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "HideOnly" => Ok(Self::HideOnly),
            "HideAndResign" => Ok(Self::HideAndResign),
            "Detach" => Ok(Self::Detach),
            _ => Err("variant must be HideOnly, HideAndResign or Detach".into()),
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    label: String,
    native_identity: String,
    parked: bool,
    hidden: bool,
    attached: bool,
}

#[cfg(any(target_os = "macos", test))]
fn fixture_url(value: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(value).map_err(|e| e.to_string())?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        || !matches!(url.port(), Some(5173 | 3000))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("fixtureUrl must be an http loopback URL on port 5173 or 3000".into());
    }
    Ok(url)
}

/// Supplying fixtureUrl selects a separate child that product modal teardown does
/// not own. Repeated calls never navigate an existing child. Close it explicitly
/// before choosing another fixture; this keeps document replacement observable.
#[tauri::command]
pub async fn browser_probe_park(
    app: AppHandle,
    caller: tauri::Webview,
    tab_idx: usize,
    variant: String,
    park: bool,
    fixture_url: Option<String>,
    close: Option<bool>,
) -> Result<Observation, String> {
    if caller.label() != "main" {
        return Err("parking probe is available only to the main shell".into());
    }
    let variant: Variant = variant.parse()?;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, tab_idx, variant, park, fixture_url, close);
        Err("unsupported".into())
    }
    #[cfg(target_os = "macos")]
    {
        let url = fixture_url.as_deref().map(self::fixture_url).transpose()?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let app_on_main = app.clone();
        app.run_on_main_thread(move || {
            let _ = tx.send(prepare(&app_on_main, tab_idx, url, close.unwrap_or(false)));
        })
        .map_err(|e| e.to_string())?;
        let Some(view) = rx.await.map_err(|e| e.to_string())?? else {
            return Ok(Observation {
                label: FIXTURE_LABEL.into(),
                native_identity: String::new(),
                parked: false,
                hidden: true,
                attached: false,
            });
        };
        if park {
            self::park(&view, variant).await
        } else {
            unpark(&view, variant).await
        }
    }
}

/// Park one existing child without closing or navigating it.
pub async fn park(view: &tauri::Webview, variant: Variant) -> Result<Observation, String> {
    change(view, variant, true).await
}

/// Restore the saved native frame, visibility and first responder.
pub async fn unpark(view: &tauri::Webview, variant: Variant) -> Result<Observation, String> {
    change(view, variant, false).await
}

async fn change(
    view: &tauri::Webview,
    variant: Variant,
    park: bool,
) -> Result<Observation, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (view, variant, park);
        Err("unsupported".into())
    }
    #[cfg(target_os = "macos")]
    {
        let label = view.label().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Tauri guarantees this callback runs on the main thread. Completion
        // reports native errors rather than treating dispatch success as proof.
        view.with_webview(move |platform| {
            let _ = tx.send(mac::apply(platform, &label, variant, park));
        })
        .map_err(|e| e.to_string())?;
        rx.await
            .map_err(|e| format!("native callback did not complete: {e}"))?
    }
}

#[cfg(target_os = "macos")]
fn prepare(
    app: &AppHandle,
    tab_idx: usize,
    url: Option<tauri::Url>,
    close: bool,
) -> Result<Option<tauri::Webview>, String> {
    if close {
        if let Some(view) = app.get_webview(FIXTURE_LABEL) {
            view.close().map_err(|e| e.to_string())?;
        }
        mac::forget(FIXTURE_LABEL);
        return Ok(None);
    }
    let Some(url) = url else {
        let label = crate::browser_webview::webview_label(tab_idx);
        return app.get_webview(&label).map(Some).ok_or_else(|| {
            mac::forget(&label);
            "browser child not found; parked metadata discarded".into()
        });
    };
    if let Some(view) = app.get_webview(FIXTURE_LABEL) {
        return Ok(Some(view));
    }
    mac::forget(FIXTURE_LABEL);
    let window = app.get_window("main").ok_or("main window not found")?;
    let builder =
        tauri::webview::WebviewBuilder::new(FIXTURE_LABEL, tauri::WebviewUrl::External(url))
            .devtools(true);
    window
        .add_child(
            builder,
            tauri::LogicalPosition::new(240.0, 120.0),
            tauri::LogicalSize::new(600.0, 400.0),
        )
        .map(Some)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
pub fn clear() {
    mac::clear();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;

    use objc2::{rc::Retained, MainThreadMarker, Message};
    use objc2_app_kit::{NSResponder, NSView, NSWindow, NSWindowOrderingMode};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use objc2_web_kit::WKWebView;

    use super::{Observation, Variant};

    struct Parked {
        label: String,
        view: Retained<WKWebView>,
        parent: Retained<NSView>,
        window: Retained<NSWindow>,
        above: Option<Retained<NSView>>,
        responder: Option<Retained<NSResponder>>,
        frame: NSRect,
        variant: Variant,
    }

    thread_local! {
        // One outstanding experiment bounds retention. Native objects never
        // cross threads; unpark, explicit close and main-window destruction drop them.
        static PARKED: RefCell<Option<Parked>> = const { RefCell::new(None) };
    }

    pub(super) fn forget(label: &str) {
        if MainThreadMarker::new().is_some() {
            PARKED.with_borrow_mut(|state| {
                if state.as_ref().is_some_and(|state| state.label == label) {
                    *state = None;
                }
            });
        }
    }

    pub(super) fn clear() {
        if MainThreadMarker::new().is_some() {
            PARKED.with_borrow_mut(|state| *state = None);
        }
    }

    pub(super) fn apply(
        platform: tauri::webview::PlatformWebview,
        label: &str,
        variant: Variant,
        park: bool,
    ) -> Result<Observation, String> {
        let _main = MainThreadMarker::new().ok_or("native callback is not on main thread")?;
        let ptr = platform.inner().cast::<WKWebView>();
        // SAFETY: Tauri's macOS PlatformWebview::inner is the live WKWebView
        // pointer, borrowed only inside its main-thread with_webview callback.
        let view = unsafe { ptr.as_ref() }.ok_or("null WKWebView")?;
        PARKED.with_borrow_mut(|state| {
            if let Some(saved) = state.as_ref() {
                if saved.label != label {
                    return Err("unpark the outstanding probe before parking another child".into());
                }
                if Retained::as_ptr(&saved.view) != ptr {
                    *state = None;
                    return Err("native identity changed; stale metadata discarded".into());
                }
                if saved.variant != variant {
                    return Err("unpark with the variant used to park".into());
                }
                if park {
                    return Err("already parked; unpark before another park".into());
                }
                let focused = restore(saved)?;
                *state = None;
                if !focused {
                    return Err("window refused saved focus responder; record failed proof".into());
                }
            } else if park {
                *state = Some(hide(view, label, variant)?);
            }
            // SAFETY: view is live in this callback and hierarchy reads are on
            // the main thread, with no asynchronous work between reads and retain.
            let attached = unsafe { view.superview() }.is_some();
            Ok(Observation {
                label: label.into(),
                native_identity: format!("{ptr:p}"),
                parked: state.is_some(),
                hidden: view.isHidden(),
                attached,
            })
        })
    }

    fn hide(view: &WKWebView, label: &str, variant: Variant) -> Result<Parked, String> {
        // SAFETY: the callback retains a live view; this synchronous main-thread
        // hierarchy read retains its parent before any detachment occurs.
        let parent = unsafe { view.superview() }.ok_or("child has no superview")?;
        let window = view.window().ok_or("child has no window")?;
        if view.isHidden() {
            return Err("show the child before starting the probe".into());
        }
        let siblings = parent.subviews().to_vec();
        let above = siblings
            .iter()
            .position(|sibling| std::ptr::eq::<NSView>(&**sibling, view))
            .and_then(|index| siblings.get(index + 1))
            .cloned();
        let saved = Parked {
            label: label.into(),
            view: view.retain(),
            parent,
            window,
            above,
            responder: view.window().and_then(|window| window.firstResponder()),
            frame: view.frame(),
            variant,
        };
        if variant != Variant::HideOnly {
            let content = saved
                .window
                .contentView()
                .ok_or("window has no content view")?;
            if !saved.window.makeFirstResponder(Some(&content)) {
                return Err("window refused focus resignation".into());
            }
        }
        view.setHidden(true);
        if variant != Variant::HideOnly {
            let bounds = saved.parent.bounds();
            view.setFrame(NSRect::new(
                NSPoint::new(bounds.origin.x + bounds.size.width - 1.0, bounds.origin.y),
                NSSize::new(1.0, 1.0),
            ));
        }
        if variant == Variant::Detach {
            view.removeFromSuperview();
        }
        Ok(saved)
    }

    fn restore(saved: &Parked) -> Result<bool, String> {
        if !saved.view.isHidden() {
            return Err("parked child was shown externally; record failed proof".into());
        }
        // SAFETY: saved retains view, parent and window throughout this
        // synchronous main-thread restore. No borrowed hierarchy pointer escapes.
        let parent = unsafe { saved.view.superview() };
        if saved.variant == Variant::Detach {
            if parent.is_some() {
                return Err("detached child was reattached externally; record failed proof".into());
            }
            if saved.parent.window().as_deref() != Some(&*saved.window) {
                return Err("saved parent left its window; close the probe".into());
            }
            if let Some(above) = saved.above.as_deref() {
                // SAFETY: the saved sibling is retained and inspected on the
                // main thread immediately before using it as a stacking anchor.
                if unsafe { above.superview() }.as_deref() != Some(&*saved.parent) {
                    return Err("saved stacking sibling moved; close the probe".into());
                }
            }
            // Restore the saved native frame rather than converting shell
            // top-left coordinates through a potentially resized/flipped parent.
            saved.view.setFrame(saved.frame);
            saved.parent.addSubview_positioned_relativeTo(
                &saved.view,
                if saved.above.is_some() {
                    NSWindowOrderingMode::Below
                } else {
                    NSWindowOrderingMode::Above
                },
                saved.above.as_deref(),
            );
        } else {
            if parent.as_deref() != Some(&*saved.parent) {
                return Err("child superview changed; close the probe".into());
            }
            saved.view.setFrame(saved.frame);
        }
        saved.view.setHidden(false);
        Ok(saved.window.makeFirstResponder(saved.responder.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_explicit() {
        assert_eq!("Detach".parse::<Variant>(), Ok(Variant::Detach));
        assert!("detach".parse::<Variant>().is_err());
        assert!("".parse::<Variant>().is_err());
    }

    #[test]
    fn fixture_urls_are_limited_to_the_fixture_server() {
        for url in [
            "http://localhost:5173/form.html",
            "http://127.0.0.1:3000/dialog.html",
        ] {
            assert!(fixture_url(url).is_ok(), "{url}");
        }
        for url in [
            "https://localhost:5173/",
            "http://localhost:5174/",
            "http://evil.test:3000/",
            "http://user@localhost:5173/",
        ] {
            assert!(fixture_url(url).is_err(), "{url}");
        }
    }
}
