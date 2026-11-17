//! System tray icon and menu.

use crate::windows::{PanelTab, WindowManager};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub fn setup_tray(app: &AppHandle, windows: Arc<WindowManager>, transcript_dir: PathBuf, data_dir: PathBuf) -> Result<(), String> {
    let show_toolbar = MenuItemBuilder::with_id("show_toolbar", "Show toolbar").build(app).map_err(|e| e.to_string())?;
    let toggle_all = MenuItemBuilder::with_id("toggle_all", "Show / hide all windows").build(app).map_err(|e| e.to_string())?;
    let transcript = MenuItemBuilder::with_id("transcript", "Live transcript").build(app).map_err(|e| e.to_string())?;
    let settings = MenuItemBuilder::with_id("settings", "Settings & providers").build(app).map_err(|e| e.to_string())?;
    let open_transcripts = MenuItemBuilder::with_id("open_transcripts", "Open saved transcripts").build(app).map_err(|e| e.to_string())?;
    let open_data = MenuItemBuilder::with_id("open_data", "Open app data folder").build(app).map_err(|e| e.to_string())?;
    let sep1 = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let sep2 = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Meetingly").build(app).map_err(|e| e.to_string())?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &show_toolbar,
            &toggle_all,
            &transcript,
            &settings,
            &sep1,
            &open_transcripts,
            &open_data,
            &sep2,
            &quit,
        ])
        .build()
        .map_err(|e| e.to_string())?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png")).map_err(|e| e.to_string())?;
    let menu_windows = windows.clone();
    let click_windows = windows;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("Meetingly")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show_toolbar" => menu_windows.center_main(app),
            "toggle_all" => menu_windows.toggle_all(app),
            "transcript" => menu_windows.show_panel(app, PanelTab::Transcript, false),
            "settings" => menu_windows.show_panel(app, PanelTab::Settings, false),
            "open_transcripts" => {
                let _ = std::fs::create_dir_all(&transcript_dir);
                open_in_file_manager(&transcript_dir);
            }
            "open_data" => {
                open_in_file_manager(&data_dir);
            }
            "quit" => {
                menu_windows.mark_quitting();
                if let Some(session) = app.try_state::<Arc<crate::session::SessionManager>>() {
                    let _ = session.finish();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                click_windows.center_main(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn open_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}
