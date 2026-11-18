mod accelerator;
#[cfg(target_os = "linux")]
mod portal;

use self::accelerator::Accelerator;
use crate::chat::ChatService;
use crate::session::{SessionManager, SessionStatus};
use crate::settings::{SettingsStore, Shortcuts};
use crate::windows::{PanelTab, WindowManager};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
#[cfg(target_os = "linux")]
use tokio::sync::mpsc;

const DEDUP_WINDOW: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionId {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionCapability {
    Always,
    RequiresPositioning,
}

impl ActionId {
    pub const ALL: [Self; 9] = [
        Self::ShowToolbar,
        Self::ToggleAll,
        Self::Listen,
        Self::Chat,
        Self::Screenshot,
        Self::MoveLeft,
        Self::MoveRight,
        Self::MoveUp,
        Self::MoveDown,
    ];

    pub(super) fn capability(self) -> ActionCapability {
        match self {
            Self::ShowToolbar | Self::ToggleAll | Self::Listen | Self::Chat | Self::Screenshot => {
                ActionCapability::Always
            }
            Self::MoveLeft | Self::MoveRight | Self::MoveUp | Self::MoveDown => {
                ActionCapability::RequiresPositioning
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn portal_id(self) -> &'static str {
        match self {
            Self::ShowToolbar => "show-toolbar",
            Self::ToggleAll => "toggle-all",
            Self::Listen => "listen",
            Self::Chat => "chat",
            Self::Screenshot => "screenshot",
            Self::MoveLeft => "move-left",
            Self::MoveRight => "move-right",
            Self::MoveUp => "move-up",
            Self::MoveDown => "move-down",
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn from_portal_id(id: &str) -> Option<Self> {
        match id {
            "show-toolbar" => Some(Self::ShowToolbar),
            "toggle-all" => Some(Self::ToggleAll),
            "listen" => Some(Self::Listen),
            "chat" => Some(Self::Chat),
            "screenshot" => Some(Self::Screenshot),
            "move-left" => Some(Self::MoveLeft),
            "move-right" => Some(Self::MoveRight),
            "move-up" => Some(Self::MoveUp),
            "move-down" => Some(Self::MoveDown),
            _ => None,
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn description(self) -> &'static str {
        match self {
            Self::ShowToolbar => "Show and center the toolbar",
            Self::ToggleAll => "Hide / show all windows",
            Self::Listen => "Start / pause / resume listening",
            Self::Chat => "Toggle chat",
            Self::Screenshot => "Analyze the screen in chat",
            Self::MoveLeft => "Move toolbar left",
            Self::MoveRight => "Move toolbar right",
            Self::MoveUp => "Move toolbar up",
            Self::MoveDown => "Move toolbar down",
        }
    }

    fn configured_accelerator(self, shortcuts: &Shortcuts) -> &str {
        match self {
            Self::ShowToolbar => &shortcuts.show_toolbar,
            Self::ToggleAll => &shortcuts.toggle_all,
            Self::Listen => &shortcuts.listen,
            Self::Chat => &shortcuts.chat,
            Self::Screenshot => &shortcuts.screenshot,
            Self::MoveLeft => &shortcuts.move_left,
            Self::MoveRight => &shortcuts.move_right,
            Self::MoveUp => &shortcuts.move_up,
            Self::MoveDown => &shortcuts.move_down,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyChord {
    pub ctrl_or_meta: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
    pub code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PortalSessionView {
    Connecting,
    Bound,
    NeedsApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BindingAuthorityView {
    AppManaged,
    DesktopPortal {
        session: PortalSessionView,
        can_configure: bool,
    },
    InAppOnly {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FallbackReason {
    GlobalConflict,
    InvalidAccelerator,
    PortalNotBound,
    PortalUnavailable,
    NoPositioningOnWayland,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ActionBindingView {
    Global {
        display_trigger: String,
    },
    InAppFallback {
        chord: KeyChord,
        display_trigger: String,
        reason: FallbackReason,
    },
    Unsupported {
        reason: FallbackReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionStatusView {
    pub action: ActionId,
    pub configured: String,
    pub binding: ActionBindingView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatusView {
    pub authority: BindingAuthorityView,
    pub actions: Vec<ActionStatusView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    #[cfg(any(target_os = "linux", test))]
    Wayland,
    #[cfg(any(target_os = "linux", test))]
    X11,
    #[cfg(target_os = "macos")]
    MacOs,
    #[cfg(target_os = "windows")]
    Windows,
    #[cfg(any(
        target_os = "linux",
        test,
        not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
    ))]
    Headless,
}

impl DisplayServer {
    pub fn is_wayland(self) -> bool {
        #[cfg(any(target_os = "linux", test))]
        {
            matches!(self, Self::Wayland)
        }
        #[cfg(not(any(target_os = "linux", test)))]
        {
            false
        }
    }

    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "linux")]
        {
            Self::detect_linux(
                std::env::var("GDK_BACKEND").ok().as_deref(),
                std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
                std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
                std::env::var("DISPLAY").ok().as_deref(),
            )
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Self::Headless
        }
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn detect_linux(
        gdk_backend: Option<&str>,
        wayland_display: Option<&str>,
        xdg_session_type: Option<&str>,
        display: Option<&str>,
    ) -> Self {
        if gdk_backend.is_some_and(|v| v.trim().eq_ignore_ascii_case("x11")) {
            return Self::X11;
        }
        if wayland_display.is_some_and(|v| !v.trim().is_empty())
            || xdg_session_type.is_some_and(|v| v.trim().eq_ignore_ascii_case("wayland"))
        {
            return Self::Wayland;
        }
        if display.is_some_and(|v| !v.trim().is_empty()) {
            return Self::X11;
        }
        Self::Headless
    }
}

pub fn validate_shortcuts(shortcuts: &Shortcuts) -> Result<(), String> {
    let mut seen: HashMap<Accelerator, ActionId> = HashMap::new();
    for action in ActionId::ALL {
        let raw = action.configured_accelerator(shortcuts);
        let parsed = Accelerator::parse(raw)?;
        if seen.insert(parsed, action).is_some() {
            return Err(format!("Duplicate shortcut \"{raw}\"."));
        }
    }
    Ok(())
}

struct StatusState {
    status: ShortcutStatusView,
    #[cfg(target_os = "linux")]
    portal_triggers: HashMap<ActionId, String>,
}

pub struct ShortcutService {
    settings: Arc<SettingsStore>,
    windows: Arc<WindowManager>,
    session: Arc<SessionManager>,
    chat: Arc<ChatService>,
    display_server: DisplayServer,
    legacy_bindings: Mutex<HashMap<u32, ActionId>>,
    last_fired: Mutex<HashMap<ActionId, Instant>>,
    state: Mutex<StatusState>,
    #[cfg(target_os = "linux")]
    portal_tx: Mutex<Option<mpsc::UnboundedSender<portal::PortalCmd>>>,
}

impl ShortcutService {
    pub fn uses_legacy_plugin() -> bool {
        match DisplayServer::detect() {
            #[cfg(any(target_os = "linux", test))]
            DisplayServer::X11 => true,
            #[cfg(target_os = "macos")]
            DisplayServer::MacOs => true,
            #[cfg(target_os = "windows")]
            DisplayServer::Windows => true,
            _ => false,
        }
    }

    pub fn new(
        settings: Arc<SettingsStore>,
        windows: Arc<WindowManager>,
        session: Arc<SessionManager>,
        chat: Arc<ChatService>,
    ) -> Arc<Self> {
        let display_server = DisplayServer::detect();
        let initial_authority = match display_server {
            #[cfg(any(target_os = "linux", test))]
            DisplayServer::X11 => BindingAuthorityView::AppManaged,
            #[cfg(target_os = "macos")]
            DisplayServer::MacOs => BindingAuthorityView::AppManaged,
            #[cfg(target_os = "windows")]
            DisplayServer::Windows => BindingAuthorityView::AppManaged,
            #[cfg(any(target_os = "linux", test))]
            DisplayServer::Wayland => BindingAuthorityView::DesktopPortal {
                session: PortalSessionView::Connecting,
                can_configure: false,
            },
            #[cfg(any(
                target_os = "linux",
                test,
                not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
            ))]
            DisplayServer::Headless => BindingAuthorityView::InAppOnly {
                reason: "No display server detected.".into(),
            },
        };
        let initial_actions = compute_fallback_actions(
            &settings.get().shortcuts,
            &initial_authority,
            &HashMap::new(),
        );
        Arc::new(Self {
            settings,
            windows,
            session,
            chat,
            display_server,
            legacy_bindings: Mutex::new(HashMap::new()),
            last_fired: Mutex::new(HashMap::new()),
            state: Mutex::new(StatusState {
                status: ShortcutStatusView {
                    authority: initial_authority,
                    actions: initial_actions,
                },
                #[cfg(target_os = "linux")]
                portal_triggers: HashMap::new(),
            }),
            #[cfg(target_os = "linux")]
            portal_tx: Mutex::new(None),
        })
    }

    pub fn start(self: &Arc<Self>, app: &AppHandle) {
        self.sync_from_settings(app);

        #[cfg(target_os = "linux")]
        if self.display_server == DisplayServer::Wayland {
            let (tx, rx) = mpsc::unbounded_channel();
            *self.portal_tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);

            let app_id = app.config().identifier.clone();
            let exe = std::env::current_exe().ok();
            let already_prompted = self.settings.get().portal_shortcuts_prompted;
            let initial_hints = self.preferred_hints();

            let settings = self.settings.clone();
            let on_prompted: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = settings.mark_portal_shortcuts_prompted();
            });

            let svc = Arc::clone(self);
            let app_handle = app.clone();
            let on_event: Arc<dyn Fn(portal::PortalEvent) + Send + Sync> =
                Arc::new(move |ev| svc.handle_portal_event(&app_handle, ev));

            tauri::async_runtime::spawn(portal::run_actor(
                app_id,
                exe,
                already_prompted,
                initial_hints,
                rx,
                on_prompted,
                on_event,
            ));
        }
    }

    pub fn status(&self) -> ShortcutStatusView {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).status.clone()
    }

    pub fn configure_system(&self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        if self.display_server == DisplayServer::Wayland {
            let hints = self.preferred_hints();
            let guard = self.portal_tx.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(tx) = guard.as_ref() {
                return tx
                    .send(portal::PortalCmd::ConfigureOrBind {
                        preferred_hints: hints,
                    })
                    .map_err(|_| "Desktop shortcuts portal is not running.".to_string());
            }
            return Err("Desktop shortcuts portal is not initialized.".into());
        }
        Err("System shortcut configuration is only available on Wayland.".into())
    }

    pub fn sync_from_settings(&self, app: &AppHandle) {
        let shortcuts = self.settings.get().shortcuts;
        match self.display_server {
            #[cfg(any(target_os = "linux", test))]
            DisplayServer::X11 => self.sync_app_managed(app, &shortcuts),
            #[cfg(target_os = "macos")]
            DisplayServer::MacOs => self.sync_app_managed(app, &shortcuts),
            #[cfg(target_os = "windows")]
            DisplayServer::Windows => self.sync_app_managed(app, &shortcuts),
            #[cfg(any(target_os = "linux", test))]
            DisplayServer::Wayland => self.sync_in_app_or_portal(app, &shortcuts),
            #[cfg(any(
                target_os = "linux",
                test,
                not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
            ))]
            DisplayServer::Headless => self.sync_in_app_or_portal(app, &shortcuts),
        }
    }

    fn sync_app_managed(&self, app: &AppHandle, shortcuts: &Shortcuts) {
        let gs = app.global_shortcut();
        let _ = gs.unregister_all();
        let mut map = self.legacy_bindings.lock().unwrap_or_else(|e| e.into_inner());
        map.clear();

        let mut actions = Vec::with_capacity(ActionId::ALL.len());
        for action in ActionId::ALL {
            let configured = action.configured_accelerator(shortcuts).trim().to_string();
            let binding = match Accelerator::parse(&configured) {
                Ok(accel) => {
                    let hotkey = accel.to_global_hotkey();
                    if gs.register(hotkey).is_ok() {
                        map.insert(hotkey.id(), action);
                        ActionBindingView::Global {
                            display_trigger: configured.clone(),
                        }
                    } else {
                        ActionBindingView::InAppFallback {
                            chord: accel.to_web_chord(),
                            display_trigger: configured.clone(),
                            reason: FallbackReason::GlobalConflict,
                        }
                    }
                }
                Err(_) => ActionBindingView::Unsupported {
                    reason: FallbackReason::InvalidAccelerator,
                },
            };
            actions.push(ActionStatusView {
                action,
                configured,
                binding,
            });
        }

        let next = ShortcutStatusView {
            authority: BindingAuthorityView::AppManaged,
            actions,
        };
        self.state.lock().unwrap_or_else(|e| e.into_inner()).status = next.clone();
        let _ = app.emit("shortcuts:status", &next);
    }

    fn sync_in_app_or_portal(&self, app: &AppHandle, shortcuts: &Shortcuts) {
        let next = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            #[cfg(target_os = "linux")]
            let triggers = &st.portal_triggers;
            #[cfg(not(target_os = "linux"))]
            let empty = HashMap::new();
            #[cfg(not(target_os = "linux"))]
            let triggers = &empty;
            st.status.actions = compute_fallback_actions(shortcuts, &st.status.authority, triggers);
            st.status.clone()
        };
        let _ = app.emit("shortcuts:status", &next);
    }

    pub fn handle_legacy_event(&self, app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
        if state != ShortcutState::Pressed {
            return;
        }
        let action = self
            .legacy_bindings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&shortcut.id())
            .copied();
        if let Some(action) = action {
            self.try_fire(app, action);
        }
    }

    pub fn trigger_in_app(&self, app: &AppHandle, action: ActionId) {
        let allowed = {
            let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.status.actions.iter().any(|item| {
                item.action == action && matches!(item.binding, ActionBindingView::InAppFallback { .. })
            })
        };
        if allowed {
            self.try_fire(app, action);
        }
    }

    #[cfg(target_os = "linux")]
    fn preferred_hints(&self) -> Vec<(ActionId, String)> {
        let shortcuts = self.settings.get().shortcuts;
        ActionId::ALL
            .into_iter()
            .filter(|a| a.capability() == ActionCapability::Always)
            .filter_map(|action| {
                let raw = action.configured_accelerator(&shortcuts);
                Accelerator::parse(raw)
                    .ok()
                    .map(|accel| (action, accel.to_xdg_trigger()))
            })
            .collect()
    }

    #[cfg(target_os = "linux")]
    fn handle_portal_event(&self, app: &AppHandle, event: portal::PortalEvent) {
        match event {
            portal::PortalEvent::Activated(action) => {
                self.try_fire(app, action);
            }
            portal::PortalEvent::Bound {
                triggers,
                can_configure,
            } => self.apply_portal_state(
                app,
                BindingAuthorityView::DesktopPortal {
                    session: PortalSessionView::Bound,
                    can_configure,
                },
                triggers,
            ),
            portal::PortalEvent::NeedsApproval { can_configure } => self.apply_portal_state(
                app,
                BindingAuthorityView::DesktopPortal {
                    session: PortalSessionView::NeedsApproval,
                    can_configure,
                },
                HashMap::new(),
            ),
            portal::PortalEvent::Unavailable { reason } => self.apply_portal_state(
                app,
                BindingAuthorityView::InAppOnly { reason },
                HashMap::new(),
            ),
        }
    }

    #[cfg(target_os = "linux")]
    fn apply_portal_state(
        &self,
        app: &AppHandle,
        authority: BindingAuthorityView,
        triggers: HashMap<ActionId, String>,
    ) {
        let shortcuts = self.settings.get().shortcuts;
        let next = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.portal_triggers = triggers;
            st.status.authority = authority;
            st.status.actions =
                compute_fallback_actions(&shortcuts, &st.status.authority, &st.portal_triggers);
            st.status.clone()
        };
        let _ = app.emit("shortcuts:status", &next);
    }

    fn try_fire(&self, app: &AppHandle, action: ActionId) {
        {
            let mut last = self.last_fired.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if last
                .get(&action)
                .is_some_and(|prev| now.duration_since(*prev) < DEDUP_WINDOW)
            {
                return;
            }
            last.insert(action, now);
        }
        self.execute_action(app, action);
    }

    fn execute_action(&self, app: &AppHandle, action: ActionId) {
        match action {
            ActionId::ShowToolbar => self.windows.center_main(app),
            ActionId::ToggleAll => self.windows.toggle_all(app),
            ActionId::Chat => self.windows.toggle_panel(app, PanelTab::Answers),
            ActionId::Screenshot => {
                self.windows.show_panel(app, PanelTab::Answers, false);
                let chat = self.chat.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = chat.screenshot(None).await;
                });
            }
            ActionId::Listen => match self.session.state().status {
                SessionStatus::Idle => {
                    self.windows.show_panel(app, PanelTab::Transcript, false);
                    let session = self.session.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = session.start().await {
                            session.report_error(err);
                        }
                    });
                }
                SessionStatus::Recording => {
                    self.session.pause();
                }
                SessionStatus::Paused => {
                    self.session.resume();
                }
                SessionStatus::Connecting => {}
            },
            ActionId::MoveLeft => self.windows.move_main(app, -1, 0),
            ActionId::MoveRight => self.windows.move_main(app, 1, 0),
            ActionId::MoveUp => self.windows.move_main(app, 0, -1),
            ActionId::MoveDown => self.windows.move_main(app, 0, 1),
        }
    }
}

fn compute_fallback_actions(
    shortcuts: &Shortcuts,
    authority: &BindingAuthorityView,
    portal_triggers: &HashMap<ActionId, String>,
) -> Vec<ActionStatusView> {
    let wayland_mode = matches!(
        authority,
        BindingAuthorityView::DesktopPortal { .. } | BindingAuthorityView::InAppOnly { .. }
    );
    let portal_bound = matches!(
        authority,
        BindingAuthorityView::DesktopPortal {
            session: PortalSessionView::Bound,
            ..
        }
    );
    let default_reason = match authority {
        BindingAuthorityView::InAppOnly { .. } => FallbackReason::PortalUnavailable,
        BindingAuthorityView::DesktopPortal { .. } => FallbackReason::PortalNotBound,
        BindingAuthorityView::AppManaged => FallbackReason::GlobalConflict,
    };

    ActionId::ALL
        .into_iter()
        .map(|action| {
            let configured = action.configured_accelerator(shortcuts).trim().to_string();
            let binding = if wayland_mode && action.capability() == ActionCapability::RequiresPositioning {
                ActionBindingView::Unsupported {
                    reason: FallbackReason::NoPositioningOnWayland,
                }
            } else if portal_bound && let Some(trigger) = portal_triggers.get(&action) {
                ActionBindingView::Global {
                    display_trigger: trigger.clone(),
                }
            } else {
                match Accelerator::parse(&configured) {
                    Ok(accel) => ActionBindingView::InAppFallback {
                        chord: accel.to_web_chord(),
                        display_trigger: configured.clone(),
                        reason: default_reason,
                    },
                    Err(_) => ActionBindingView::Unsupported {
                        reason: FallbackReason::InvalidAccelerator,
                    },
                }
            };
            ActionStatusView {
                action,
                configured,
                binding,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_linux_display_servers() {
        assert_eq!(
            DisplayServer::detect_linux(Some("x11"), Some("wayland-0"), Some("wayland"), Some(":0")),
            DisplayServer::X11
        );
        assert_eq!(
            DisplayServer::detect_linux(Some("X11"), Some("wayland-0"), None, Some(":0")),
            DisplayServer::X11
        );
        assert_eq!(
            DisplayServer::detect_linux(None, Some("wayland-0"), Some("wayland"), Some(":0")),
            DisplayServer::Wayland
        );
        assert_eq!(
            DisplayServer::detect_linux(None, None, Some("wayland"), None),
            DisplayServer::Wayland
        );
        assert_eq!(
            DisplayServer::detect_linux(None, None, Some("x11"), Some(":0")),
            DisplayServer::X11
        );
        assert_eq!(
            DisplayServer::detect_linux(None, Some(""), None, None),
            DisplayServer::Headless
        );
    }

    #[test]
    fn validates_shortcuts_and_rejects_duplicates() {
        assert!(validate_shortcuts(&Shortcuts::default()).is_ok());
        let dup = Shortcuts {
            listen: "CmdOrCtrl+B".into(),
            ..Shortcuts::default()
        };
        assert!(validate_shortcuts(&dup).is_err());

        let invalid = Shortcuts {
            listen: "AltGr+L".into(),
            ..Shortcuts::default()
        };
        assert!(validate_shortcuts(&invalid).is_err());
    }

    #[test]
    fn computes_wayland_fallback_and_global_bindings() {
        let shortcuts = Shortcuts::default();
        let authority = BindingAuthorityView::DesktopPortal {
            session: PortalSessionView::Bound,
            can_configure: true,
        };
        let mut triggers = HashMap::new();
        triggers.insert(ActionId::Listen, "Ctrl+L".to_string());

        let actions = compute_fallback_actions(&shortcuts, &authority, &triggers);
        let listen = actions.iter().find(|a| a.action == ActionId::Listen).unwrap();
        assert_eq!(
            listen.binding,
            ActionBindingView::Global {
                display_trigger: "Ctrl+L".into(),
            }
        );

        let chat = actions.iter().find(|a| a.action == ActionId::Chat).unwrap();
        assert!(matches!(
            chat.binding,
            ActionBindingView::InAppFallback {
                reason: FallbackReason::PortalNotBound,
                ..
            }
        ));

        let move_left = actions.iter().find(|a| a.action == ActionId::MoveLeft).unwrap();
        assert_eq!(
            move_left.binding,
            ActionBindingView::Unsupported {
                reason: FallbackReason::NoPositioningOnWayland,
            }
        );
    }
}
