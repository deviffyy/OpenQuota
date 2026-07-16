use std::{thread, time::Duration};

use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Window, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::{desktop_integration::DesktopIntegration, popup::PopupDismissGuard};

pub const MAIN_WINDOW: &str = "main";

/// Brings the already-running application forward when a later launch is redirected to it by the
/// single-instance plugin. During an extremely tight simultaneous-launch race the callback can arrive
/// before setup has installed the popup state; the fallback still reveals and focuses the window, while
/// the normal path preserves tray positioning and cancels any pending focus-loss dismissal.
pub fn activate_existing_instance(app: &AppHandle) {
    if let Some(guard) = app.try_state::<PopupDismissGuard>() {
        guard.cancel_pending();
    }

    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    if app.try_state::<DesktopIntegration>().is_some() {
        show_popup(&window);
        return;
    }

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn show_popup(window: &WebviewWindow) {
    let standalone = window
        .app_handle()
        .state::<DesktopIntegration>()
        .standalone_window;
    if standalone || cfg!(target_os = "linux") {
        let _ = window.center();
        let _ = window.unminimize();
    } else {
        let _ = window
            .as_ref()
            .window()
            .move_window_constrained(Position::TrayCenter);
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn hide_popup(window: &WebviewWindow) {
    let _ = window.hide();
    let _ = window.app_handle().emit("popup-hidden", ());
}

pub fn toggle_popup(app: &AppHandle) {
    app.state::<PopupDismissGuard>().cancel_pending();

    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        if app.state::<DesktopIntegration>().standalone_window {
            let _ = window.minimize();
        } else {
            hide_popup(&window);
        }
    } else {
        show_popup(&window);
    }
}

pub fn open_screen(app: &AppHandle, screen: &str) {
    app.state::<PopupDismissGuard>().cancel_pending();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        show_popup(&window);
        let _ = app.emit("open-screen", screen);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerticalFrame {
    top: i32,
    height: u32,
}

fn anchored_vertical_frame(
    current: VerticalFrame,
    work_area: VerticalFrame,
    new_height: u32,
) -> VerticalFrame {
    let current_bottom = i64::from(current.top) + i64::from(current.height);
    let work_bottom = i64::from(work_area.top) + i64::from(work_area.height);
    let top_gap = (i64::from(current.top) - i64::from(work_area.top)).abs();
    let bottom_gap = (work_bottom - current_bottom).abs();
    let top = if bottom_gap <= top_gap {
        current_bottom.saturating_sub(i64::from(new_height))
    } else {
        i64::from(current.top)
    };
    VerticalFrame {
        top: top.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        height: new_height,
    }
}

#[cfg(target_os = "windows")]
pub fn resize_popup_anchored(window: &WebviewWindow, height: u32) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };

    let outer_position = window
        .outer_position()
        .map_err(|_| "OpenQuota window position is unavailable.")?;
    let outer_size = window
        .outer_size()
        .map_err(|_| "OpenQuota window size is unavailable.")?;
    let inner_size = window
        .inner_size()
        .map_err(|_| "OpenQuota content size is unavailable.")?;
    let scale = window
        .scale_factor()
        .map_err(|_| "OpenQuota display scale is unavailable.")?;
    let monitor = window
        .current_monitor()
        .map_err(|_| "OpenQuota display is unavailable.")?
        .ok_or("OpenQuota display is unavailable.")?;
    let work_area = monitor.work_area();
    let frame_overhead = outer_size.height.saturating_sub(inner_size.height);
    let target_inner_height = (f64::from(height) * scale)
        .round()
        .clamp(1.0, f64::from(u32::MAX));
    let target_outer_height = (target_inner_height as u32).saturating_add(frame_overhead);
    let anchored = anchored_vertical_frame(
        VerticalFrame {
            top: outer_position.y,
            height: outer_size.height,
        },
        VerticalFrame {
            top: work_area.position.y,
            height: work_area.size.height,
        },
        target_outer_height,
    );
    let result = unsafe {
        SetWindowPos(
            window
                .hwnd()
                .map_err(|_| "OpenQuota native window is unavailable.")?
                .0 as _,
            std::ptr::null_mut(),
            outer_position.x,
            anchored.top,
            i32::try_from(outer_size.width).unwrap_or(i32::MAX),
            i32::try_from(anchored.height).unwrap_or(i32::MAX),
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER,
        )
    };
    if result == 0 {
        return Err("OpenQuota window could not be resized.".into());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn resize_popup_anchored(window: &WebviewWindow, height: u32) -> Result<(), String> {
    let outer_position = window
        .outer_position()
        .map_err(|_| "OpenQuota window position is unavailable.")?;
    let outer_size = window
        .outer_size()
        .map_err(|_| "OpenQuota window size is unavailable.")?;
    let monitor = window
        .current_monitor()
        .map_err(|_| "OpenQuota display is unavailable.")?
        .ok_or("OpenQuota display is unavailable.")?;
    let work_area = monitor.work_area();
    let scale = window
        .scale_factor()
        .map_err(|_| "OpenQuota display scale is unavailable.")?;
    let target_outer_height = (f64::from(height) * scale)
        .round()
        .clamp(1.0, f64::from(u32::MAX)) as u32;
    let anchored = anchored_vertical_frame(
        VerticalFrame {
            top: outer_position.y,
            height: outer_size.height,
        },
        VerticalFrame {
            top: work_area.position.y,
            height: work_area.size.height,
        },
        target_outer_height,
    );
    window
        .set_size(tauri::LogicalSize::new(320.0, f64::from(height)))
        .and_then(|_| {
            window.set_position(tauri::PhysicalPosition::new(outer_position.x, anchored.top))
        })
        .map_err(|_| "OpenQuota window could not be resized.".into())
}

fn schedule_outside_click_dismiss(window: Window) {
    let app = window.app_handle().clone();
    let token = app.state::<PopupDismissGuard>().token();

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        let app_for_dismiss = app.clone();
        let _ = app.run_on_main_thread(move || {
            let guard = app_for_dismiss.state::<PopupDismissGuard>();
            let still_unfocused = window.is_focused().is_ok_and(|focused| !focused);

            if guard.is_current(token) && still_unfocused {
                let _ = window.hide();
                let _ = app_for_dismiss.emit("popup-hidden", ());
            }
        });
    });
}

pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }

    match event {
        WindowEvent::Focused(false)
            if !window
                .app_handle()
                .state::<DesktopIntegration>()
                .standalone_window =>
        {
            schedule_outside_click_dismiss(window.clone())
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if window
                .app_handle()
                .state::<DesktopIntegration>()
                .standalone_window
            {
                window.app_handle().exit(0);
                return;
            }
            window
                .app_handle()
                .state::<PopupDismissGuard>()
                .cancel_pending();
            let _ = window.hide();
            let _ = window.app_handle().emit("popup-hidden", ());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{anchored_vertical_frame, VerticalFrame};

    #[test]
    fn shrinking_bottom_anchored_popup_preserves_its_bottom_edge() {
        let resized = anchored_vertical_frame(
            VerticalFrame {
                top: 496,
                height: 300,
            },
            VerticalFrame {
                top: 100,
                height: 700,
            },
            200,
        );
        assert_eq!(
            resized,
            VerticalFrame {
                top: 596,
                height: 200
            }
        );
    }

    #[test]
    fn shrinking_top_anchored_popup_preserves_its_top_edge() {
        let resized = anchored_vertical_frame(
            VerticalFrame {
                top: 104,
                height: 300,
            },
            VerticalFrame {
                top: 100,
                height: 700,
            },
            200,
        );
        assert_eq!(
            resized,
            VerticalFrame {
                top: 104,
                height: 200
            }
        );
    }
}
