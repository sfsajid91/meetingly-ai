use super::{ActionCapability, ActionId};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut};
use ashpd::desktop::{CreateSessionOptions, Session};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;

const MANAGED_MARKER: &str = "# Managed-by: meetingly";

pub(super) enum PortalCmd {
    ConfigureOrBind {
        preferred_hints: Vec<(ActionId, String)>,
    },
}

pub(super) enum PortalEvent {
    Bound {
        triggers: HashMap<ActionId, String>,
        can_configure: bool,
    },
    NeedsApproval {
        can_configure: bool,
    },
    Unavailable {
        reason: String,
    },
    Activated(ActionId),
}

pub fn ensure_desktop_entry(app_id: &str, exe: &Path) -> Result<(), String> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::data_local_dir)
        .ok_or_else(|| "Could not determine XDG_DATA_HOME.".to_string())?;
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|s| !s.is_empty())
        .map(|s| std::env::split_paths(&s).collect::<Vec<_>>())
        .unwrap_or_else(|| vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
    ensure_desktop_entry_in(&data_home, &data_dirs, app_id, exe)
}

fn ensure_desktop_entry_in(
    data_home: &Path,
    data_dirs: &[PathBuf],
    app_id: &str,
    exe: &Path,
) -> Result<(), String> {
    let target_apps = data_home.join("applications");
    let target = target_apps.join(format!("{app_id}.desktop"));

    match fs::read_to_string(&target) {
        Ok(existing) => {
            if !existing.lines().any(|line| line.trim() == MANAGED_MARKER) {
                return Ok(());
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(format!("Could not read {}: {err}", target.display())),
    }

    let mut search_dirs = Vec::with_capacity(1 + data_dirs.len());
    search_dirs.push(target_apps.clone());
    for dir in data_dirs {
        search_dirs.push(dir.join("applications"));
    }
    let has_visible_entry = search_dirs.iter().any(|dir| {
        let candidate = dir.join("Meetingly.desktop");
        candidate != target && candidate.exists()
    });

    let exec_line = format_desktop_exec(exe);
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Meetingly\n\
         Exec={exec_line}\n\
         StartupWMClass=meetingly-byok\n\
         Terminal=false\n\
         NoDisplay={has_visible_entry}\n\
         {MANAGED_MARKER}\n"
    );

    if fs::read_to_string(&target).ok().as_deref() == Some(content.as_str()) {
        return Ok(());
    }

    fs::create_dir_all(&target_apps)
        .map_err(|e| format!("Could not create {}: {e}", target_apps.display()))?;
    let tmp = target_apps.join(format!(".{app_id}.desktop.{}.tmp", std::process::id()));
    let write_res = (|| {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| format!("Could not create {}: {e}", tmp.display()))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
        file.sync_all()
            .map_err(|e| format!("Could not flush {}: {e}", tmp.display()))?;
        drop(file);
        fs::rename(&tmp, &target)
            .map_err(|e| format!("Could not install {}: {e}", target.display()))?;
        Ok(())
    })();
    if write_res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write_res
}

fn format_desktop_exec(exe: &Path) -> String {
    let raw = exe.to_string_lossy();
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            '%' => out.push_str("%%"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn session_path(session: &Session<GlobalShortcuts>) -> Option<String> {
    serde_json::to_value(session)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
}

fn extract_triggers(shortcuts: &[Shortcut]) -> HashMap<ActionId, String> {
    let mut map = HashMap::new();
    for shortcut in shortcuts {
        if let Some(action) = ActionId::from_portal_id(shortcut.id()) {
            let trigger = shortcut.trigger_description().trim();
            if !trigger.is_empty() {
                map.insert(action, trigger.to_string());
            }
        }
    }
    map
}

fn build_new_shortcuts(hints: &[(ActionId, String)]) -> Vec<NewShortcut> {
    let hint_map: HashMap<ActionId, &str> = hints.iter().map(|(a, h)| (*a, h.as_str())).collect();
    ActionId::ALL
        .into_iter()
        .filter(|action| action.capability() == ActionCapability::Always)
        .map(|action| {
            let mut item = NewShortcut::new(action.portal_id(), action.description());
            if let Some(trigger) = hint_map.get(&action) {
                item = item.preferred_trigger(*trigger);
            }
            item
        })
        .collect()
}

fn spawn_bind(
    gs: Arc<GlobalShortcuts>,
    session: Arc<Session<GlobalShortcuts>>,
    session_id: Option<String>,
    hints: Vec<(ActionId, String)>,
    bind_tx: mpsc::UnboundedSender<(Option<String>, Result<Vec<Shortcut>, String>)>,
) {
    tokio::spawn(async move {
        let new_shortcuts = build_new_shortcuts(&hints);
        let res = match gs
            .bind_shortcuts(&session, &new_shortcuts, None, Default::default())
            .await
        {
            Ok(req) => req
                .response()
                .map(|r| r.shortcuts().to_vec())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        let _ = bind_tx.send((session_id, res));
    });
}

pub(super) async fn run_actor(
    app_id: String,
    exe: Option<PathBuf>,
    already_prompted: bool,
    initial_hints: Vec<(ActionId, String)>,
    mut cmd_rx: mpsc::UnboundedReceiver<PortalCmd>,
    on_prompted: Arc<dyn Fn() + Send + Sync>,
    on_event: Arc<dyn Fn(PortalEvent) + Send + Sync>,
) {
    if !ashpd::is_sandboxed()
        && let Some(exe_path) = exe.as_deref()
        && let Err(err) = ensure_desktop_entry(&app_id, exe_path)
    {
        eprintln!("[shortcuts] desktop entry skipped: {err}");
    }

    let conn = match ashpd::zbus::Connection::session().await {
        Ok(c) => c,
        Err(err) => {
            on_event(PortalEvent::Unavailable {
                reason: format!("D-Bus session unavailable: {err}"),
            });
            return;
        }
    };

    if !ashpd::is_sandboxed()
        && let Ok(parsed_id) = ashpd::AppID::try_from(app_id.as_str())
        && let Err(err) = ashpd::register_host_app_with_connection(conn.clone(), parsed_id).await
    {
        eprintln!("[shortcuts] register_host_app failed: {err}");
    }

    let gs = match GlobalShortcuts::with_connection(conn).await {
        Ok(g) => Arc::new(g),
        Err(err) => {
            on_event(PortalEvent::Unavailable {
                reason: format!("GlobalShortcuts portal unavailable: {err}"),
            });
            return;
        }
    };
    let can_configure = gs.version() >= 2;

    // Subscribe to portal signals before creating the session so early events are never missed.
    let mut activated = match gs.receive_activated().await {
        Ok(s) => s,
        Err(err) => {
            on_event(PortalEvent::Unavailable {
                reason: format!("Could not subscribe to portal Activated signal: {err}"),
            });
            return;
        }
    };
    let mut changed = match gs.receive_shortcuts_changed().await {
        Ok(s) => s,
        Err(err) => {
            on_event(PortalEvent::Unavailable {
                reason: format!("Could not subscribe to portal ShortcutsChanged signal: {err}"),
            });
            return;
        }
    };

    let mut session = match gs.create_session(CreateSessionOptions::default()).await {
        Ok(s) => Arc::new(s),
        Err(err) => {
            on_event(PortalEvent::Unavailable {
                reason: format!("Could not create shortcuts portal session: {err}"),
            });
            return;
        }
    };
    let mut session_id = session_path(&session);

    let list_res = match gs.list_shortcuts(&session, Default::default()).await {
        Ok(req) => req.response().map_err(|e| e.to_string()),
        Err(err) => Err(err.to_string()),
    };

    let (bind_tx, mut bind_rx) = mpsc::unbounded_channel();
    let mut is_bound = false;
    let mut bind_in_flight = false;

    match list_res {
        Ok(list) if !list.shortcuts().is_empty() => {
            is_bound = true;
            on_event(PortalEvent::Bound {
                triggers: extract_triggers(list.shortcuts()),
                can_configure,
            });
        }
        _ if !already_prompted => {
            on_prompted();
            bind_in_flight = true;
            spawn_bind(
                gs.clone(),
                session.clone(),
                session_id.clone(),
                initial_hints,
                bind_tx.clone(),
            );
        }
        _ => {
            on_event(PortalEvent::NeedsApproval { can_configure });
        }
    }

    loop {
        tokio::select! {
            maybe_ev = activated.next() => {
                let Some(ev) = maybe_ev else { break };
                if session_id.as_deref() == Some(ev.session_handle().as_str())
                    && let Some(action) = ActionId::from_portal_id(ev.shortcut_id())
                {
                    on_event(PortalEvent::Activated(action));
                }
            }
            maybe_changed = changed.next() => {
                let Some(ev) = maybe_changed else { break };
                if session_id.as_deref() == Some(ev.session_handle().as_str()) {
                    let triggers = extract_triggers(ev.shortcuts());
                    if triggers.is_empty() {
                        is_bound = false;
                        on_event(PortalEvent::NeedsApproval { can_configure });
                    } else {
                        is_bound = true;
                        on_event(PortalEvent::Bound { triggers, can_configure });
                    }
                }
            }
            Some((done_id, res)) = bind_rx.recv() => {
                bind_in_flight = false;
                if done_id == session_id {
                    match res {
                        Ok(shortcuts) if !shortcuts.is_empty() => {
                            is_bound = true;
                            on_event(PortalEvent::Bound {
                                triggers: extract_triggers(&shortcuts),
                                can_configure,
                            });
                        }
                        _ => {
                            is_bound = false;
                            on_event(PortalEvent::NeedsApproval { can_configure });
                        }
                    }
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break };
                match cmd {
                    PortalCmd::ConfigureOrBind { preferred_hints } => {
                        if bind_in_flight {
                            continue;
                        }
                        if is_bound && can_configure {
                            let gs_cfg = gs.clone();
                            let sess_cfg = session.clone();
                            tokio::spawn(async move {
                                if let Err(err) = gs_cfg
                                    .configure_shortcuts(&sess_cfg, None, Default::default())
                                    .await
                                {
                                    eprintln!("[shortcuts] configure_shortcuts failed: {err}");
                                }
                            });
                        } else {
                            let _ = session.close().await;
                            match gs.create_session(CreateSessionOptions::default()).await {
                                Ok(new_sess) => {
                                    session = Arc::new(new_sess);
                                    session_id = session_path(&session);
                                    is_bound = false;
                                    bind_in_flight = true;
                                    spawn_bind(
                                        gs.clone(),
                                        session.clone(),
                                        session_id.clone(),
                                        preferred_hints,
                                        bind_tx.clone(),
                                    );
                                }
                                Err(err) => {
                                    on_event(PortalEvent::Unavailable {
                                        reason: format!("Could not create shortcuts portal session: {err}"),
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::TempDir;

    #[test]
    fn ensure_desktop_entry_is_idempotent_and_respects_unmanaged_files() {
        let dir = TempDir::new();
        let data_home = dir.path().join("data_home");
        let sys_share = dir.path().join("usr_share");
        let exe = Path::new("/opt/Meetingly/meetingly-byok");
        let app_id = "com.meetingly.byok";

        ensure_desktop_entry_in(&data_home, std::slice::from_ref(&sys_share), app_id, exe).unwrap();
        let target = data_home.join("applications").join(format!("{app_id}.desktop"));
        let first = fs::read_to_string(&target).unwrap();
        assert!(first.contains("NoDisplay=false"));
        assert!(first.contains("Exec=\"/opt/Meetingly/meetingly-byok\""));
        assert!(first.contains(MANAGED_MARKER));

        let mtime_before = fs::metadata(&target).unwrap().modified().unwrap();
        ensure_desktop_entry_in(&data_home, std::slice::from_ref(&sys_share), app_id, exe).unwrap();
        let mtime_after = fs::metadata(&target).unwrap().modified().unwrap();
        assert_eq!(first, fs::read_to_string(&target).unwrap());
        assert_eq!(mtime_before, mtime_after);

        let sys_apps = sys_share.join("applications");
        fs::create_dir_all(&sys_apps).unwrap();
        fs::write(sys_apps.join("Meetingly.desktop"), "[Desktop Entry]\nName=Meetingly\n").unwrap();
        ensure_desktop_entry_in(&data_home, std::slice::from_ref(&sys_share), app_id, exe).unwrap();
        let with_visible = fs::read_to_string(&target).unwrap();
        assert!(with_visible.contains("NoDisplay=true"));

        let custom = "[Desktop Entry]\nType=Application\nName=Custom Meetingly\n";
        fs::write(&target, custom).unwrap();
        ensure_desktop_entry_in(&data_home, std::slice::from_ref(&sys_share), app_id, exe).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), custom);
    }

    #[test]
    fn desktop_exec_quotes_special_characters() {
        let formatted = format_desktop_exec(Path::new("/home/user/My Apps/meetingly \"100%\""));
        assert_eq!(formatted, "\"/home/user/My Apps/meetingly \\\"100%%\\\"\"");
    }
}
