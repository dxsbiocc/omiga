#[cfg(target_os = "macos")]
use std::time::Duration;

use tauri::{image::Image, AppHandle, Manager};

use crate::commands::CommandResult;
use crate::errors::AppError;

const CLASSIC_CAPYBARA_ICON: &[u8] = include_bytes!("../../icons/icon.png");
const WARM_CAPYBARA_ICON: &[u8] = include_bytes!("../../icons/omiga-logo-warm.png");

fn icon_bytes_for_skin(skin: &str) -> Option<&'static [u8]> {
    match skin {
        "classic-capybara" => Some(CLASSIC_CAPYBARA_ICON),
        "warm-capybara" => Some(WARM_CAPYBARA_ICON),
        _ => None,
    }
}

fn config_error(message: impl Into<String>) -> AppError {
    AppError::Config(message.into())
}

#[cfg(target_os = "macos")]
fn set_macos_dock_icon_on_main_thread(icon_bytes: &'static [u8]) -> Result<(), String> {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let app = NSApplication::sharedApplication(mtm);
    let data = NSData::with_bytes(icon_bytes);
    let app_icon = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or_else(|| "failed to decode icon PNG for macOS Dock".to_string())?;
    unsafe { app.setApplicationIconImage(Some(&app_icon)) };
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_platform_dock_icon(app: &AppHandle, icon_bytes: &'static [u8]) -> CommandResult<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(set_macos_dock_icon_on_main_thread(icon_bytes));
    })
    .map_err(|err| config_error(format!("failed to schedule macOS Dock icon update: {err}")))?;

    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|err| config_error(format!("macOS Dock icon update did not finish: {err}")))?
        .map_err(|err| config_error(format!("failed to update macOS Dock icon: {err}")))?;

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn set_platform_dock_icon(_app: &AppHandle, _icon_bytes: &'static [u8]) -> CommandResult<()> {
    Ok(())
}

#[tauri::command]
pub fn set_app_icon_skin(app: AppHandle, skin: String) -> CommandResult<()> {
    let icon_bytes = icon_bytes_for_skin(&skin)
        .ok_or_else(|| config_error(format!("unknown app icon skin: {skin}")))?;
    let icon = Image::from_bytes(icon_bytes)
        .map_err(|err| config_error(format!("failed to decode app icon skin: {err}")))?;

    for window in app.webview_windows().values() {
        if let Err(err) = window.set_icon(icon.clone()) {
            tracing::warn!(?err, "Failed to update window icon for app skin");
        }
    }

    set_platform_dock_icon(&app, icon_bytes)
}

#[cfg(test)]
mod tests {
    use super::icon_bytes_for_skin;

    #[test]
    fn resolves_known_icon_skins() {
        assert!(icon_bytes_for_skin("classic-capybara").is_some());
        assert!(icon_bytes_for_skin("warm-capybara").is_some());
        assert!(icon_bytes_for_skin("missing").is_none());
    }
}
