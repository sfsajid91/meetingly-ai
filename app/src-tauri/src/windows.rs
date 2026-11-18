//! Multi-window lifecycle and layout: the floating toolbar (`main`) and attached panel (`chat`).

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const PANEL_OVERLAP: f64 = 4.0;
const MAIN_TOP_MARGIN: f64 = 40.0;
const MOVE_STEP: i32 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowKind {
    Main,
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelTab {
    Answers,
    Transcript,
    Settings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsState {
    pub chat: bool,
    pub panel_tab: PanelTab,
}

struct DragOrigin {
    x: i32,
    y: i32,
}

struct Inner {
    stealth: bool,
    quitting: bool,
    panel_tab: PanelTab,
    panel_extra: f64,
    restore_set: Option<Vec<WindowKind>>,
    drag_origin: Option<DragOrigin>,
}

pub struct WindowManager {
    inner: Mutex<Inner>,
}

impl WindowManager {
    pub fn new(stealth: bool) -> Self {
        Self {
            inner: Mutex::new(Inner {
                stealth,
                quitting: false,
                panel_tab: PanelTab::Answers,
                panel_extra: 0.0,
                restore_set: None,
                drag_origin: None,
            }),
        }
    }

    pub fn is_quitting(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).quitting
    }

    pub fn mark_quitting(&self) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).quitting = true;
    }

    pub fn set_stealth(&self, app: &AppHandle, enabled: bool) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).stealth = enabled;
        for label in ["main", "chat"] {
            if let Some(win) = app.get_webview_window(label) {
                let _ = win.set_content_protected(enabled);
            }
        }
    }

    pub fn state(&self, app: &AppHandle) -> WindowsState {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let chat_visible = app
            .get_webview_window("chat")
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        WindowsState {
            chat: chat_visible,
            panel_tab: g.panel_tab,
        }
    }

    fn emit_state(&self, app: &AppHandle) {
        let _ = app.emit("windows:state", self.state(app));
    }

    pub fn init_main(&self, app: &AppHandle) {
        if let Some(main) = app.get_webview_window("main") {
            let stealth = self.inner.lock().unwrap_or_else(|e| e.into_inner()).stealth;
            let _ = main.set_content_protected(stealth);
            self.center_main(app);
        }
    }

    pub fn ensure_chat(&self, app: &AppHandle) -> Option<WebviewWindow> {
        if let Some(win) = app.get_webview_window("chat") {
            return Some(win);
        }
        let (stealth, extra) = {
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (g.stealth, g.panel_extra)
        };
        let win = WebviewWindowBuilder::new(app, "chat", WebviewUrl::App("index.html#/chat".into()))
            .title("Meetingly")
            .inner_size(920.0 + extra, 640.0)
            .min_inner_size(640.0 + extra, 440.0)
            .max_inner_size(1440.0 + extra, 1000.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .resizable(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
            .ok()?;
        let _ = win.set_content_protected(stealth);
        self.layout_chat(app);
        Some(win)
    }

    pub fn show(&self, app: &AppHandle, kind: WindowKind, background: bool) {
        let should_restore = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut set) = g.restore_set
                && kind != WindowKind::Main
            {
                if background {
                    if !set.contains(&kind) {
                        set.push(kind);
                    }
                    return;
                }
                true
            } else {
                false
            }
        };
        if should_restore {
            self.show_all(app);
        }

        match kind {
            WindowKind::Main => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                }
            }
            WindowKind::Chat => {
                if let Some(win) = self.ensure_chat(app) {
                    self.layout_chat(app);
                    let _ = win.show();
                    if !background {
                        let _ = win.set_focus();
                    }
                }
            }
        }
        self.emit_state(app);
    }

    pub fn hide(&self, app: &AppHandle, kind: WindowKind) {
        let label = match kind {
            WindowKind::Main => "main",
            WindowKind::Chat => "chat",
        };
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.hide();
        }
        self.emit_state(app);
    }

    pub fn toggle(&self, app: &AppHandle, kind: WindowKind) -> bool {
        let label = match kind {
            WindowKind::Main => "main",
            WindowKind::Chat => "chat",
        };
        let visible = app.get_webview_window(label).and_then(|w| w.is_visible().ok()).unwrap_or(false);
        if visible {
            self.hide(app, kind);
            false
        } else {
            self.show(app, kind, false);
            true
        }
    }

    pub fn show_panel(&self, app: &AppHandle, tab: PanelTab, background: bool) {
        {
            self.inner.lock().unwrap_or_else(|e| e.into_inner()).panel_tab = tab;
        }
        let _ = app.emit("panel:tab", tab);
        self.show(app, WindowKind::Chat, background);
    }

    pub fn toggle_panel(&self, app: &AppHandle, tab: PanelTab) {
        let (hidden_all, current_tab) = {
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (g.restore_set.is_some(), g.panel_tab)
        };
        let chat_visible = app
            .get_webview_window("chat")
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        if !hidden_all && chat_visible && current_tab == tab {
            self.hide(app, WindowKind::Chat);
        } else {
            self.show_panel(app, tab, false);
        }
    }

    pub fn set_panel_tab(&self, app: &AppHandle, tab: PanelTab) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).panel_tab = tab;
        self.emit_state(app);
    }

    pub fn set_suggestions_rail(&self, _app: &AppHandle, _visible: bool) {
        // Suggestions now render inline inside the spacious workspace pane; no window resize needed.
    }

    pub fn hide_all(&self, app: &AppHandle) {
        let mut to_restore = Vec::new();
        for (kind, label) in [(WindowKind::Main, "main"), (WindowKind::Chat, "chat")] {
            if let Some(win) = app.get_webview_window(label) {
                if win.is_visible().unwrap_or(false) {
                    to_restore.push(kind);
                }
                let _ = win.hide();
            }
        }
        if to_restore.is_empty() {
            to_restore.push(WindowKind::Main);
        }
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).restore_set = Some(to_restore);
        self.emit_state(app);
    }

    pub fn show_all(&self, app: &AppHandle) {
        let restore = self.inner.lock().unwrap_or_else(|e| e.into_inner()).restore_set.take();
        let kinds = restore.unwrap_or_else(|| vec![WindowKind::Main]);
        for kind in kinds {
            match kind {
                WindowKind::Main => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                    }
                }
                WindowKind::Chat => {
                    if let Some(w) = self.ensure_chat(app) {
                        self.layout_chat(app);
                        let _ = w.show();
                    }
                }
            }
        }
        self.emit_state(app);
    }

    pub fn toggle_all(&self, app: &AppHandle) {
        let has_restore = self.inner.lock().unwrap_or_else(|e| e.into_inner()).restore_set.is_some();
        if has_restore {
            self.show_all(app);
        } else {
            self.hide_all(app);
        }
    }

    pub fn center_main(&self, app: &AppHandle) {
        {
            self.inner.lock().unwrap_or_else(|e| e.into_inner()).restore_set = None;
        }
        let Some(main) = app.get_webview_window("main") else { return };
        if let Ok(Some(monitor)) = main.primary_monitor()
            && let Ok(size) = main.outer_size()
        {
            let m_pos = monitor.position();
            let m_size = monitor.size();
            let scale = monitor.scale_factor();
            let x = m_pos.x + ((m_size.width as i32 - size.width as i32) / 2);
            let y = m_pos.y + (MAIN_TOP_MARGIN * scale).round() as i32;
            let _ = main.set_position(PhysicalPosition::new(x, y));
        }
        let _ = main.show();
        self.layout_chat(app);
    }

    pub fn move_main(&self, app: &AppHandle, dx_steps: i32, dy_steps: i32) {
        let Some(main) = app.get_webview_window("main") else { return };
        if let Ok(pos) = main.outer_position() {
            let _ = main.set_position(PhysicalPosition::new(pos.x + dx_steps * MOVE_STEP, pos.y + dy_steps * MOVE_STEP));
            self.layout_chat(app);
        }
    }

    pub fn fit(&self, app: &AppHandle, label: &str, width: f64, height: f64) {
        let Some(win) = app.get_webview_window(label) else { return };
        let (w, h) = if label == "main" {
            (width.clamp(260.0, 960.0), height.clamp(40.0, 150.0))
        } else {
            let extra = self.inner.lock().unwrap_or_else(|e| e.into_inner()).panel_extra;
            (width.clamp(640.0 + extra, 1440.0 + extra), height.clamp(440.0, 1000.0))
        };
        let _ = win.set_size(LogicalSize::new(w, h));
        if label == "main" {
            self.layout_chat(app);
        }
    }

    pub fn drag_main(&self, app: &AppHandle, phase: &str, dx: f64, dy: f64) {
        let Some(main) = app.get_webview_window("main") else { return };
        let scale = main.scale_factor().unwrap_or(1.0);
        match phase {
            "start" => {
                if let Ok(pos) = main.outer_position() {
                    self.inner.lock().unwrap_or_else(|e| e.into_inner()).drag_origin = Some(DragOrigin { x: pos.x, y: pos.y });
                }
            }
            "move" => {
                let origin = self.inner.lock().unwrap_or_else(|e| e.into_inner()).drag_origin.as_ref().map(|o| (o.x, o.y));
                if let Some((ox, oy)) = origin {
                    let nx = ox + (dx * scale).round() as i32;
                    let ny = oy + (dy * scale).round() as i32;
                    let _ = main.set_position(PhysicalPosition::new(nx, ny));
                    self.layout_chat(app);
                }
            }
            _ => {
                self.inner.lock().unwrap_or_else(|e| e.into_inner()).drag_origin = None;
            }
        }
    }

    pub fn hide_for_capture(&self, app: &AppHandle) -> Box<dyn FnOnce() + Send> {
        let (stealth, main_vis, chat_vis) = {
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let main_vis = app.get_webview_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
            let chat_vis = app.get_webview_window("chat").and_then(|w| w.is_visible().ok()).unwrap_or(false);
            (g.stealth, main_vis, chat_vis)
        };
        if stealth && !cfg!(target_os = "linux") {
            return Box::new(|| {});
        }
        if main_vis && let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
        if chat_vis && let Some(w) = app.get_webview_window("chat") {
            let _ = w.hide();
        }
        let app = app.clone();
        Box::new(move || {
            if main_vis && let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
            if chat_vis && let Some(w) = app.get_webview_window("chat") {
                let _ = w.show();
            }
        })
    }

    pub fn layout_chat(&self, app: &AppHandle) {
        let Some(main) = app.get_webview_window("main") else { return };
        let Some(chat) = app.get_webview_window("chat") else { return };
        let (Ok(m_pos), Ok(m_size), Ok(c_size)) = (main.outer_position(), main.outer_size(), chat.outer_size()) else {
            return;
        };
        let scale = main.scale_factor().unwrap_or(1.0);
        let overlap = (PANEL_OVERLAP * scale).round() as i32;
        let x = m_pos.x + ((m_size.width as i32 - c_size.width as i32) / 2);
        let y = m_pos.y + m_size.height as i32 - overlap;
        let _ = chat.set_position(LogicalPosition::new(x as f64 / scale, y as f64 / scale));
    }
}
