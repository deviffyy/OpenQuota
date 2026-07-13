use tauri::{AppHandle, LogicalSize, Manager};

use crate::{
    desktop_integration::DesktopIntegration,
    window::{hide_popup, resize_popup_anchored, MAIN_WINDOW},
};

#[tauri::command]
pub fn dismiss_main_window(app: AppHandle) {
    if app.state::<DesktopIntegration>().standalone_window {
        app.exit(0);
    } else if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        hide_popup(&window);
    }
}

#[tauri::command]
pub fn resize_main_window(app: AppHandle, height: u32) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return Err("OpenQuota window is unavailable.".into());
    };
    let height = height.max(1);
    if app.state::<DesktopIntegration>().standalone_window {
        return window
            .set_size(LogicalSize::new(320.0, f64::from(height)))
            .map_err(|_| "OpenQuota window could not be resized.".into());
    }

    resize_popup_anchored(&window, height)
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
