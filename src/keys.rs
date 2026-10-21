//! Keyboard simulation (SendInput) and global hotkey listening (low-level hook).
//!
//! Two simulation backends keep the original "Keyboard Module" choice:
//! - `pynput`: virtual-key events (what pynput sends on Windows)
//! - `keyboard`: hardware scan-code events (what the `keyboard` package sends)

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::console;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Shift,
    Ctrl,
    Alt,
    Space,
}

impl Key {
    pub fn name(&self) -> String {
        match self {
            Key::Char(c) => format!("'{c}'"),
            Key::Shift => "shift".into(),
            Key::Ctrl => "ctrl".into(),
            Key::Alt => "alt".into(),
            Key::Space => "space".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Pynput,
    Keyboard,
}

impl Backend {
    pub fn from_name(name: &str) -> Backend {
        if name == "keyboard" {
            Backend::Keyboard
        } else {
            Backend::Pynput
        }
    }
}

/// Presses/releases keys and tracks which keys are held so they can be released later.
pub struct KeySender {
    held: Mutex<HashSet<Key>>,
    pressed_names: Mutex<Vec<String>>,
    log_keys: bool,
}

impl KeySender {
    pub fn new(log_keys: bool) -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::new(HashSet::new()),
            pressed_names: Mutex::new(Vec::new()),
            log_keys,
        })
    }

    fn log_key(&self, action: &str, key: Key) {
        if !self.log_keys || !console::enabled() {
            return;
        }
        let name = key.name();
        let mut names = self.pressed_names.lock();
        if action == "press" {
            if !names.contains(&name) {
                names.push(name.clone());
            }
        } else if let Some(i) = names.iter().position(|n| *n == name) {
            names.remove(i);
        }
        if names.is_empty() {
            console::log(format!("{action}: {name}"));
        } else {
            let mut sorted = names.clone();
            sorted.sort();
            console::log(format!("{action}: {}", sorted.join("+")));
        }
    }

    pub fn press(&self, key: Key, backend: Backend) {
        platform::send(key, true, backend);
        self.held.lock().insert(key);
        self.log_key("press", key);
    }

    pub fn release(&self, key: Key, backend: Backend) {
        platform::send(key, false, backend);
        self.held.lock().remove(&key);
        self.log_key("release", key);
    }

    pub fn release_all(&self, backend: Backend) {
        let keys: Vec<Key> = self.held.lock().drain().collect();
        for key in keys {
            platform::send(key, false, backend);
            self.log_key("release", key);
        }
    }
}

// ---------------------------------------------------------------------------
// Hotkey names (compatible with the names stored by the original app)
// ---------------------------------------------------------------------------

/// Set on a key code passed to the global hook callback when Alt was held.
pub const ALT_FLAG: u32 = 1 << 16;

/// Alt+1 … Alt+9 (top row or numpad) as a 0-based song slot.
pub fn alt_digit(code: u32) -> Option<usize> {
    if code & ALT_FLAG == 0 {
        return None;
    }
    match code & !ALT_FLAG {
        vk @ 0x31..=0x39 => Some((vk - 0x31) as usize),
        vk @ 0x61..=0x69 => Some((vk - 0x61) as usize),
        _ => None,
    }
}

/// Converts a virtual-key code into the lower-case name used in config.json.
pub fn vk_to_name(vk: u32) -> Option<String> {
    let name = match vk {
        0x70..=0x87 => format!("f{}", vk - 0x6F),
        0x30..=0x39 => ((b'0' + (vk - 0x30) as u8) as char).to_string(),
        0x41..=0x5A => ((b'a' + (vk - 0x41) as u8) as char).to_string(),
        0x60..=0x69 => format!("num {}", vk - 0x60),
        0x08 => "backspace".into(),
        0x09 => "tab".into(),
        0x0D => "enter".into(),
        0x10 | 0xA0 => "shift".into(),
        0xA1 => "right shift".into(),
        0x11 | 0xA2 => "ctrl".into(),
        0xA3 => "right ctrl".into(),
        0x12 | 0xA4 => "alt".into(),
        0xA5 => "right alt".into(),
        0x13 => "pause".into(),
        0x14 => "caps lock".into(),
        0x1B => "esc".into(),
        0x20 => "space".into(),
        0x21 => "page up".into(),
        0x22 => "page down".into(),
        0x23 => "end".into(),
        0x24 => "home".into(),
        0x25 => "left".into(),
        0x26 => "up".into(),
        0x27 => "right".into(),
        0x28 => "down".into(),
        0x2C => "print screen".into(),
        0x2D => "insert".into(),
        0x2E => "delete".into(),
        0x5B => "left windows".into(),
        0x5C => "right windows".into(),
        0x6A => "num *".into(),
        0x6B => "num +".into(),
        0x6D => "num -".into(),
        0x6E => "num .".into(),
        0x6F => "num /".into(),
        0x90 => "num lock".into(),
        0x91 => "scroll lock".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        _ => return None,
    };
    Some(name)
}

/// Normalizes a configured hotkey name so it can be compared with [`vk_to_name`].
pub fn normalize_hotkey(name: &str) -> String {
    let n = name.trim().to_lowercase().replace('_', " ");
    match n.as_str() {
        "left shift" => "shift".into(),
        "left ctrl" | "control" => "ctrl".into(),
        "left alt" => "alt".into(),
        "escape" => "esc".into(),
        "return" => "enter".into(),
        "pageup" => "page up".into(),
        "pagedown" => "page down".into(),
        "capslock" => "caps lock".into(),
        "numlock" => "num lock".into(),
        "scrolllock" => "scroll lock".into(),
        _ => n,
    }
}

// ---------------------------------------------------------------------------
// Platform implementation
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use super::{Backend, Key};
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    fn vk_for(key: Key) -> Option<u16> {
        Some(match key {
            Key::Shift => VK_SHIFT.0,
            Key::Ctrl => VK_CONTROL.0,
            Key::Alt => VK_MENU.0,
            Key::Space => VK_SPACE.0,
            Key::Char(c) => {
                let r = unsafe { VkKeyScanW(c as u16) };
                if r == -1 {
                    return None;
                }
                (r as u16) & 0xFF
            }
        })
    }

    /// Keys that the original pynput backend refused to press (F1-F12, Tab, Backspace, Esc).
    fn blocked(vk: u16) -> bool {
        (0x70..=0x7B).contains(&vk) || vk == VK_TAB.0 || vk == VK_BACK.0 || vk == VK_ESCAPE.0
    }

    pub fn send(key: Key, down: bool, backend: Backend) {
        let Some(vk) = vk_for(key) else {
            return;
        };
        if blocked(vk) {
            return;
        }
        let up_flag = if down { KEYBD_EVENT_FLAGS(0) } else { KEYEVENTF_KEYUP };
        let ki = match backend {
            Backend::Pynput => KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16,
                dwFlags: up_flag,
                time: 0,
                dwExtraInfo: 0,
            },
            Backend::Keyboard => KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16,
                dwFlags: up_flag | KEYEVENTF_SCANCODE,
                time: 0,
                dwExtraInfo: 0,
            },
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    type Callback = Box<dyn Fn(u32) -> bool + Send + Sync>;
    static CALLBACK: OnceLock<Callback> = OnceLock::new();

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let msg = wparam.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
                // Ignore events we injected ourselves so simulated notes never trigger hotkeys.
                if (info.flags.0 & LLKHF_INJECTED.0) == 0 {
                    if let Some(cb) = CALLBACK.get() {
                        let alt = if (info.flags.0 & LLKHF_ALTDOWN.0) != 0 { super::ALT_FLAG } else { 0 };
                        cb(info.vkCode | alt);
                    }
                }
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Installs a global low-level keyboard hook on its own thread. The callback runs
    /// on the hook thread and must return quickly.
    pub fn start_hook(callback: impl Fn(u32) -> bool + Send + Sync + 'static) {
        if CALLBACK.set(Box::new(callback)).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("keyboard-hook".into())
            .spawn(|| unsafe {
                let module = GetModuleHandleW(None).ok();
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module.map(|m| m.into()), 0);
                if hook.is_err() {
                    log::error!("failed to install keyboard hook");
                    return;
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .ok();
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{Backend, Key};

    pub fn send(_key: Key, _down: bool, _backend: Backend) {
        // Key simulation is currently implemented for Windows only.
    }

    pub fn start_hook(_callback: impl Fn(u32) -> bool + Send + Sync + 'static) {
        log::warn!("global hotkeys are only implemented on Windows");
    }
}

pub use platform::start_hook;
