use super::KeyChord;
use tauri_plugin_global_shortcut::{Code, Modifiers as GlobalModifiers, Shortcut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(super) struct Modifiers {
    pub cmd_or_ctrl: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum NamedKey {
    Space,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Minus,
    Equal,
    Comma,
    Period,
    Slash,
    Backslash,
    Semicolon,
    Quote,
    Backquote,
    BracketLeft,
    BracketRight,
    PrintScreen,
    ScrollLock,
    Pause,
    CapsLock,
    NumLock,
    MediaPlayPause,
    MediaStop,
    MediaTrackNext,
    MediaTrackPrevious,
    VolumeUp,
    VolumeDown,
    VolumeMute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum CanonicalKey {
    Letter(char),
    Digit(char),
    Function(u8),
    Named(NamedKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Accelerator {
    pub mods: Modifiers,
    pub key: CanonicalKey,
}

impl Accelerator {
    pub(super) fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("Shortcut cannot be empty.".into());
        }

        let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
        if parts.iter().any(|p| p.is_empty()) {
            return Err("Remove empty '+' segments.".into());
        }
        if parts.len() < 2 {
            return Err("Include at least one modifier (e.g. CommandOrControl+L).".into());
        }

        let mut mods = Modifiers::default();
        for &part in &parts[..parts.len() - 1] {
            let slot = match part.to_ascii_lowercase().as_str() {
                "commandorcontrol" | "cmdorctrl" => &mut mods.cmd_or_ctrl,
                "control" | "ctrl" => &mut mods.ctrl,
                "command" | "cmd" | "super" | "meta" => &mut mods.meta,
                "alt" | "option" => &mut mods.alt,
                "shift" => &mut mods.shift,
                _ => return Err(format!("Unknown modifier \"{part}\".")),
            };
            if *slot {
                return Err(format!("Duplicate modifier \"{part}\"."));
            }
            *slot = true;
        }

        let key = parse_key(parts[parts.len() - 1])?;
        Ok(Self { mods, key })
    }

    pub(super) fn to_global_hotkey(self) -> Shortcut {
        let mut gm = GlobalModifiers::empty();
        if self.mods.cmd_or_ctrl {
            #[cfg(target_os = "macos")]
            {
                gm |= GlobalModifiers::SUPER;
            }
            #[cfg(not(target_os = "macos"))]
            {
                gm |= GlobalModifiers::CONTROL;
            }
        }
        if self.mods.ctrl {
            gm |= GlobalModifiers::CONTROL;
        }
        if self.mods.meta {
            gm |= GlobalModifiers::SUPER;
        }
        if self.mods.alt {
            gm |= GlobalModifiers::ALT;
        }
        if self.mods.shift {
            gm |= GlobalModifiers::SHIFT;
        }

        Shortcut::new(Some(gm), self.key.to_global_code())
    }

    #[cfg(any(target_os = "linux", test))]
    pub(super) fn to_xdg_trigger(self) -> String {
        let mut tokens: Vec<String> = Vec::with_capacity(5);
        let has_ctrl = self.mods.ctrl || (self.mods.cmd_or_ctrl && !cfg!(target_os = "macos"));
        let has_logo = self.mods.meta || (self.mods.cmd_or_ctrl && cfg!(target_os = "macos"));

        if has_ctrl {
            tokens.push("CTRL".into());
        }
        if self.mods.alt {
            tokens.push("ALT".into());
        }
        if self.mods.shift {
            tokens.push("SHIFT".into());
        }
        if has_logo {
            tokens.push("LOGO".into());
        }
        tokens.push(self.key.to_xdg_key());
        tokens.join("+")
    }

    pub(super) fn to_web_chord(self) -> KeyChord {
        KeyChord {
            ctrl_or_meta: self.mods.cmd_or_ctrl,
            ctrl: self.mods.ctrl,
            meta: self.mods.meta,
            alt: self.mods.alt,
            shift: self.mods.shift,
            code: self.key.to_dom_code(),
        }
    }
}

fn parse_key(raw: &str) -> Result<CanonicalKey, String> {
    let bytes = raw.as_bytes();
    if bytes.len() == 1 {
        let c = bytes[0] as char;
        if c.is_ascii_alphabetic() {
            return Ok(CanonicalKey::Letter(c.to_ascii_uppercase()));
        }
        if c.is_ascii_digit() {
            return Ok(CanonicalKey::Digit(c));
        }
    }

    let lower = raw.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("key")
        && rest.len() == 1
        && rest.as_bytes()[0].is_ascii_alphabetic()
    {
        return Ok(CanonicalKey::Letter((rest.as_bytes()[0] as char).to_ascii_uppercase()));
    }
    if let Some(rest) = lower.strip_prefix("digit")
        && rest.len() == 1
        && rest.as_bytes()[0].is_ascii_digit()
    {
        return Ok(CanonicalKey::Digit(rest.as_bytes()[0] as char));
    }
    if let Some(rest) = lower.strip_prefix('f')
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
        && let Ok(n) = rest.parse::<u8>()
        && (1..=24).contains(&n)
    {
        return Ok(CanonicalKey::Function(n));
    }

    let named = match lower.as_str() {
        "space" => NamedKey::Space,
        "enter" | "return" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "backspace" => NamedKey::Backspace,
        "delete" => NamedKey::Delete,
        "insert" => NamedKey::Insert,
        "escape" | "esc" => NamedKey::Escape,
        "up" | "arrowup" => NamedKey::Up,
        "down" | "arrowdown" => NamedKey::Down,
        "left" | "arrowleft" => NamedKey::Left,
        "right" | "arrowright" => NamedKey::Right,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "minus" => NamedKey::Minus,
        "equal" => NamedKey::Equal,
        "comma" => NamedKey::Comma,
        "period" => NamedKey::Period,
        "slash" => NamedKey::Slash,
        "backslash" => NamedKey::Backslash,
        "semicolon" => NamedKey::Semicolon,
        "quote" => NamedKey::Quote,
        "backquote" => NamedKey::Backquote,
        "bracketleft" => NamedKey::BracketLeft,
        "bracketright" => NamedKey::BracketRight,
        "printscreen" => NamedKey::PrintScreen,
        "scrolllock" => NamedKey::ScrollLock,
        "pause" => NamedKey::Pause,
        "capslock" => NamedKey::CapsLock,
        "numlock" => NamedKey::NumLock,
        "mediaplaypause" => NamedKey::MediaPlayPause,
        "mediastop" => NamedKey::MediaStop,
        "mediatracknext" | "medianexttrack" => NamedKey::MediaTrackNext,
        "mediatrackprevious" | "mediaprevioustrack" => NamedKey::MediaTrackPrevious,
        "volumeup" => NamedKey::VolumeUp,
        "volumedown" => NamedKey::VolumeDown,
        "volumemute" => NamedKey::VolumeMute,
        _ => return Err(format!("Unsupported key \"{raw}\".")),
    };
    Ok(CanonicalKey::Named(named))
}

impl CanonicalKey {
    fn to_global_code(self) -> Code {
        match self {
            Self::Letter(c) => match c {
                'A' => Code::KeyA,
                'B' => Code::KeyB,
                'C' => Code::KeyC,
                'D' => Code::KeyD,
                'E' => Code::KeyE,
                'F' => Code::KeyF,
                'G' => Code::KeyG,
                'H' => Code::KeyH,
                'I' => Code::KeyI,
                'J' => Code::KeyJ,
                'K' => Code::KeyK,
                'L' => Code::KeyL,
                'M' => Code::KeyM,
                'N' => Code::KeyN,
                'O' => Code::KeyO,
                'P' => Code::KeyP,
                'Q' => Code::KeyQ,
                'R' => Code::KeyR,
                'S' => Code::KeyS,
                'T' => Code::KeyT,
                'U' => Code::KeyU,
                'V' => Code::KeyV,
                'W' => Code::KeyW,
                'X' => Code::KeyX,
                'Y' => Code::KeyY,
                'Z' => Code::KeyZ,
                _ => unreachable!("letter outside A..=Z"),
            },
            Self::Digit(d) => match d {
                '0' => Code::Digit0,
                '1' => Code::Digit1,
                '2' => Code::Digit2,
                '3' => Code::Digit3,
                '4' => Code::Digit4,
                '5' => Code::Digit5,
                '6' => Code::Digit6,
                '7' => Code::Digit7,
                '8' => Code::Digit8,
                '9' => Code::Digit9,
                _ => unreachable!("digit outside 0..=9"),
            },
            Self::Function(n) => match n {
                1 => Code::F1,
                2 => Code::F2,
                3 => Code::F3,
                4 => Code::F4,
                5 => Code::F5,
                6 => Code::F6,
                7 => Code::F7,
                8 => Code::F8,
                9 => Code::F9,
                10 => Code::F10,
                11 => Code::F11,
                12 => Code::F12,
                13 => Code::F13,
                14 => Code::F14,
                15 => Code::F15,
                16 => Code::F16,
                17 => Code::F17,
                18 => Code::F18,
                19 => Code::F19,
                20 => Code::F20,
                21 => Code::F21,
                22 => Code::F22,
                23 => Code::F23,
                24 => Code::F24,
                _ => unreachable!("function key outside 1..=24"),
            },
            Self::Named(n) => match n {
                NamedKey::Space => Code::Space,
                NamedKey::Enter => Code::Enter,
                NamedKey::Tab => Code::Tab,
                NamedKey::Backspace => Code::Backspace,
                NamedKey::Delete => Code::Delete,
                NamedKey::Insert => Code::Insert,
                NamedKey::Escape => Code::Escape,
                NamedKey::Up => Code::ArrowUp,
                NamedKey::Down => Code::ArrowDown,
                NamedKey::Left => Code::ArrowLeft,
                NamedKey::Right => Code::ArrowRight,
                NamedKey::Home => Code::Home,
                NamedKey::End => Code::End,
                NamedKey::PageUp => Code::PageUp,
                NamedKey::PageDown => Code::PageDown,
                NamedKey::Minus => Code::Minus,
                NamedKey::Equal => Code::Equal,
                NamedKey::Comma => Code::Comma,
                NamedKey::Period => Code::Period,
                NamedKey::Slash => Code::Slash,
                NamedKey::Backslash => Code::Backslash,
                NamedKey::Semicolon => Code::Semicolon,
                NamedKey::Quote => Code::Quote,
                NamedKey::Backquote => Code::Backquote,
                NamedKey::BracketLeft => Code::BracketLeft,
                NamedKey::BracketRight => Code::BracketRight,
                NamedKey::PrintScreen => Code::PrintScreen,
                NamedKey::ScrollLock => Code::ScrollLock,
                NamedKey::Pause => Code::Pause,
                NamedKey::CapsLock => Code::CapsLock,
                NamedKey::NumLock => Code::NumLock,
                NamedKey::MediaPlayPause => Code::MediaPlayPause,
                NamedKey::MediaStop => Code::MediaStop,
                NamedKey::MediaTrackNext => Code::MediaTrackNext,
                NamedKey::MediaTrackPrevious => Code::MediaTrackPrevious,
                NamedKey::VolumeUp => Code::AudioVolumeUp,
                NamedKey::VolumeDown => Code::AudioVolumeDown,
                NamedKey::VolumeMute => Code::AudioVolumeMute,
            },
        }
    }

    #[cfg(any(target_os = "linux", test))]
    fn to_xdg_key(self) -> String {
        match self {
            Self::Letter(c) => c.to_ascii_lowercase().to_string(),
            Self::Digit(d) => d.to_string(),
            Self::Function(n) => format!("F{n}"),
            Self::Named(n) => match n {
                NamedKey::Space => "space",
                NamedKey::Enter => "Return",
                NamedKey::Tab => "Tab",
                NamedKey::Backspace => "BackSpace",
                NamedKey::Delete => "Delete",
                NamedKey::Insert => "Insert",
                NamedKey::Escape => "Escape",
                NamedKey::Up => "Up",
                NamedKey::Down => "Down",
                NamedKey::Left => "Left",
                NamedKey::Right => "Right",
                NamedKey::Home => "Home",
                NamedKey::End => "End",
                NamedKey::PageUp => "Page_Up",
                NamedKey::PageDown => "Page_Down",
                NamedKey::Minus => "minus",
                NamedKey::Equal => "equal",
                NamedKey::Comma => "comma",
                NamedKey::Period => "period",
                NamedKey::Slash => "slash",
                NamedKey::Backslash => "backslash",
                NamedKey::Semicolon => "semicolon",
                NamedKey::Quote => "apostrophe",
                NamedKey::Backquote => "grave",
                NamedKey::BracketLeft => "bracketleft",
                NamedKey::BracketRight => "bracketright",
                NamedKey::PrintScreen => "Print",
                NamedKey::ScrollLock => "Scroll_Lock",
                NamedKey::Pause => "Pause",
                NamedKey::CapsLock => "Caps_Lock",
                NamedKey::NumLock => "Num_Lock",
                NamedKey::MediaPlayPause => "XF86AudioPlay",
                NamedKey::MediaStop => "XF86AudioStop",
                NamedKey::MediaTrackNext => "XF86AudioNext",
                NamedKey::MediaTrackPrevious => "XF86AudioPrev",
                NamedKey::VolumeUp => "XF86AudioRaiseVolume",
                NamedKey::VolumeDown => "XF86AudioLowerVolume",
                NamedKey::VolumeMute => "XF86AudioMute",
            }
            .to_string(),
        }
    }

    fn to_dom_code(self) -> String {
        match self {
            Self::Letter(c) => format!("Key{c}"),
            Self::Digit(d) => format!("Digit{d}"),
            Self::Function(n) => format!("F{n}"),
            Self::Named(n) => match n {
                NamedKey::Space => "Space",
                NamedKey::Enter => "Enter",
                NamedKey::Tab => "Tab",
                NamedKey::Backspace => "Backspace",
                NamedKey::Delete => "Delete",
                NamedKey::Insert => "Insert",
                NamedKey::Escape => "Escape",
                NamedKey::Up => "ArrowUp",
                NamedKey::Down => "ArrowDown",
                NamedKey::Left => "ArrowLeft",
                NamedKey::Right => "ArrowRight",
                NamedKey::Home => "Home",
                NamedKey::End => "End",
                NamedKey::PageUp => "PageUp",
                NamedKey::PageDown => "PageDown",
                NamedKey::Minus => "Minus",
                NamedKey::Equal => "Equal",
                NamedKey::Comma => "Comma",
                NamedKey::Period => "Period",
                NamedKey::Slash => "Slash",
                NamedKey::Backslash => "Backslash",
                NamedKey::Semicolon => "Semicolon",
                NamedKey::Quote => "Quote",
                NamedKey::Backquote => "Backquote",
                NamedKey::BracketLeft => "BracketLeft",
                NamedKey::BracketRight => "BracketRight",
                NamedKey::PrintScreen => "PrintScreen",
                NamedKey::ScrollLock => "ScrollLock",
                NamedKey::Pause => "Pause",
                NamedKey::CapsLock => "CapsLock",
                NamedKey::NumLock => "NumLock",
                NamedKey::MediaPlayPause => "MediaPlayPause",
                NamedKey::MediaStop => "MediaStop",
                NamedKey::MediaTrackNext => "MediaTrackNext",
                NamedKey::MediaTrackPrevious => "MediaTrackPrevious",
                NamedKey::VolumeUp => "AudioVolumeUp",
                NamedKey::VolumeDown => "AudioVolumeDown",
                NamedKey::VolumeMute => "AudioVolumeMute",
            }
            .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn parses_valid_accelerators_and_rejects_invalid_ones() {
        let a = Accelerator::parse("CommandOrControl+Shift+C").unwrap();
        assert_eq!(a, Accelerator::parse("cmdorctrl+shift+KeyC").unwrap());

        assert!(Accelerator::parse("CommandOrControl+MediaTrackNext").is_ok());
        assert!(Accelerator::parse("CommandOrControl+MediaNextTrack").is_ok());
        assert!(Accelerator::parse("CommandOrControl+MediaTrackPrevious").is_ok());
        assert!(Accelerator::parse("CommandOrControl+MediaPreviousTrack").is_ok());
        assert!(Accelerator::parse("Alt+F24").is_ok());
        assert!(Accelerator::parse("Super+Digit5").is_ok());
        assert!(Accelerator::parse("Ctrl+Return").is_ok());

        assert!(Accelerator::parse("").is_err());
        assert!(Accelerator::parse("  ").is_err());
        assert!(Accelerator::parse("L").is_err());
        assert!(Accelerator::parse("Ctrl++L").is_err());
        assert!(Accelerator::parse("AltGr+L").is_err());
        assert!(Accelerator::parse("Ctrl+Control+L").is_err());
        assert!(Accelerator::parse("Alt+Option+L").is_err());
        assert!(Accelerator::parse("Super+Meta+L").is_err());
        assert!(Accelerator::parse("Ctrl+BogusKey").is_err());
        assert!(Accelerator::parse("Ctrl+Plus").is_err());
    }

    #[test]
    fn converts_to_global_hotkey() {
        for raw in [
            "CommandOrControl+Space",
            "CommandOrControl+B",
            "CommandOrControl+L",
            "CommandOrControl+Shift+C",
            "CommandOrControl+Enter",
            "CommandOrControl+Left",
            "CommandOrControl+Right",
            "CommandOrControl+Up",
            "CommandOrControl+Down",
            "Alt+MediaTrackNext",
            "Ctrl+VolumeMute",
        ] {
            let accel = Accelerator::parse(raw).unwrap();
            let hotkey = accel.to_global_hotkey();
            let normalized = raw.replace("CommandOrControl", "CmdOrCtrl");
            let from_plugin = Shortcut::from_str(&normalized).unwrap();
            assert_eq!(hotkey, from_plugin, "mismatch for {raw}");
        }
    }

    #[test]
    fn converts_to_xdg_trigger() {
        let space = Accelerator::parse("CommandOrControl+Space").unwrap().to_xdg_trigger();
        let chat = Accelerator::parse("CommandOrControl+Shift+C").unwrap().to_xdg_trigger();
        let enter = Accelerator::parse("CommandOrControl+Return").unwrap().to_xdg_trigger();
        if cfg!(target_os = "macos") {
            assert_eq!(space, "LOGO+space");
            assert_eq!(chat, "SHIFT+LOGO+c");
            assert_eq!(enter, "LOGO+Return");
        } else {
            assert_eq!(space, "CTRL+space");
            assert_eq!(chat, "CTRL+SHIFT+c");
            assert_eq!(enter, "CTRL+Return");
        }

        assert_eq!(
            Accelerator::parse("Ctrl+Alt+MediaTrackNext").unwrap().to_xdg_trigger(),
            "CTRL+ALT+XF86AudioNext"
        );
        assert_eq!(
            Accelerator::parse("Super+PageUp").unwrap().to_xdg_trigger(),
            "LOGO+Page_Up"
        );
        assert_eq!(
            Accelerator::parse("Shift+Quote").unwrap().to_xdg_trigger(),
            "SHIFT+apostrophe"
        );
    }

    #[test]
    fn converts_to_web_chord() {
        assert_eq!(
            Accelerator::parse("CommandOrControl+Shift+C").unwrap().to_web_chord(),
            KeyChord {
                ctrl_or_meta: true,
                ctrl: false,
                meta: false,
                alt: false,
                shift: true,
                code: "KeyC".into(),
            }
        );
        assert_eq!(
            Accelerator::parse("CommandOrControl+Return").unwrap().to_web_chord(),
            KeyChord {
                ctrl_or_meta: true,
                ctrl: false,
                meta: false,
                alt: false,
                shift: false,
                code: "Enter".into(),
            }
        );
        assert_eq!(
            Accelerator::parse("Alt+Up").unwrap().to_web_chord(),
            KeyChord {
                ctrl_or_meta: false,
                ctrl: false,
                meta: false,
                alt: true,
                shift: false,
                code: "ArrowUp".into(),
            }
        );
        assert_eq!(
            Accelerator::parse("Ctrl+VolumeMute").unwrap().to_web_chord(),
            KeyChord {
                ctrl_or_meta: false,
                ctrl: true,
                meta: false,
                alt: false,
                shift: false,
                code: "AudioVolumeMute".into(),
            }
        );
    }
}
