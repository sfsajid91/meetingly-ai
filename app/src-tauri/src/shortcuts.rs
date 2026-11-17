//! Global keyboard shortcuts backed by `tauri-plugin-global-shortcut`.

use crate::chat::ChatService;
use crate::session::{SessionManager, SessionStatus};
use crate::settings::SettingsStore;
use crate::windows::{PanelTab, WindowManager};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    ShowToolbar,
    ToggleAll,
    Listen,
    Chat,
    Screenshot,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
}

impl ShortcutAction {
    fn key_name(self) -> &'static str {
        match self {
            Self::ShowToolbar => "showToolbar",
            Self::ToggleAll => "toggleAll",
            Self::Listen => "listen",
            Self::Chat => "chat",
            Self::Screenshot => "screenshot",
            Self::MoveLeft => "moveLeft",
            Self::MoveRight => "moveRight",
            Self::MoveUp => "moveUp",
            Self::MoveDown => "moveDown",
        }
    }
}

pub struct ShortcutManager {
    settings: Arc<SettingsStore>,
    windows: Arc<WindowManager>,
    session: Arc<SessionManager>,
    chat: Arc<ChatService>,
    bindings: Mutex<HashMap<u32, ShortcutAction>>,
}

impl ShortcutManager {
    pub fn new(
        settings: Arc<SettingsStore>,
        windows: Arc<WindowManager>,
        session: Arc<SessionManager>,
        chat: Arc<ChatService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            windows,
            session,
            chat,
            bindings: Mutex::new(HashMap::new()),
        })
    }

    pub fn register_all(&self, app: &AppHandle) -> Vec<String> {
        let gs = app.global_shortcut();
        let _ = gs.unregister_all();
        let mut map = self.bindings.lock().unwrap_or_else(|e| e.into_inner());
        map.clear();

        let s = self.settings.get().shortcuts;
        let entries = [
            (ShortcutAction::ShowToolbar, s.show_toolbar),
            (ShortcutAction::ToggleAll, s.toggle_all),
            (ShortcutAction::Listen, s.listen),
            (ShortcutAction::Chat, s.chat),
            (ShortcutAction::Screenshot, s.screenshot),
            (ShortcutAction::MoveLeft, s.move_left),
            (ShortcutAction::MoveRight, s.move_right),
            (ShortcutAction::MoveUp, s.move_up),
            (ShortcutAction::MoveDown, s.move_down),
        ];

        let mut conflicts = Vec::new();
        for (action, accel) in entries {
            let trimmed = accel.trim();
            if trimmed.is_empty() {
                continue;
            }
            let normalized = normalize_accelerator(trimmed);
            match Shortcut::from_str(&normalized) {
                Ok(shortcut) => {
                    if gs.register(shortcut).is_ok() {
                        map.insert(shortcut.id(), action);
                    } else {
                        conflicts.push(action.key_name().to_string());
                    }
                }
                Err(_) => {
                    conflicts.push(action.key_name().to_string());
                }
            }
        }
        self.settings.set_shortcut_conflicts(conflicts.clone());
        conflicts
    }

    pub fn handle_event(&self, app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
        if state != ShortcutState::Pressed {
            return;
        }
        let action = self.bindings.lock().unwrap_or_else(|e| e.into_inner()).get(&shortcut.id()).copied();
        let Some(action) = action else { return };
        match action {
            ShortcutAction::ShowToolbar => self.windows.center_main(app),
            ShortcutAction::ToggleAll => self.windows.toggle_all(app),
            ShortcutAction::Chat => self.windows.toggle_panel(app, PanelTab::Answers),
            ShortcutAction::Screenshot => {
                self.windows.show_panel(app, PanelTab::Answers, false);
                let chat = self.chat.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = chat.screenshot(None).await;
                });
            }
            ShortcutAction::Listen => {
                let status = self.session.state().status;
                match status {
                    SessionStatus::Idle => {
                        self.windows.show_panel(app, PanelTab::Transcript, false);
                        let session = self.session.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = session.start().await;
                        });
                    }
                    SessionStatus::Recording => {
                        self.session.pause();
                    }
                    SessionStatus::Paused => {
                        self.session.resume();
                    }
                    SessionStatus::Connecting => {}
                }
            }
            ShortcutAction::MoveLeft => self.windows.move_main(app, -1, 0),
            ShortcutAction::MoveRight => self.windows.move_main(app, 1, 0),
            ShortcutAction::MoveUp => self.windows.move_main(app, 0, -1),
            ShortcutAction::MoveDown => self.windows.move_main(app, 0, 1),
        }
    }
}

pub fn normalize_accelerator(value: &str) -> String {
    value
        .split('+')
        .map(str::trim)
        .map(|part| match part {
            "CommandOrControl" | "CmdOrCtrl" => "CmdOrCtrl",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_electron_accelerators_for_global_shortcut_parser() {
        let s = normalize_accelerator("CommandOrControl+Shift+C");
        assert_eq!(s, "CmdOrCtrl+Shift+C");
        assert!(Shortcut::from_str(&s).is_ok());
        assert!(Shortcut::from_str(&normalize_accelerator("CommandOrControl+Space")).is_ok());
        assert!(Shortcut::from_str(&normalize_accelerator("CommandOrControl+Enter")).is_ok());
    }
}
