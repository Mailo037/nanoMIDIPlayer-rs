//! Keyboard simulation and global hotkey listening: SendInput + a low-level hook on
//! Windows, XTest + key state polling on Linux (X11 / XWayland), Quartz events + an
//! event tap on macOS.
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

/// Virtual-key code of a GPUI key name ("f1", "a", "pageup", …), the inverse of [`vk_to_name`].
pub fn name_to_vk(key: &str) -> Option<u32> {
    let name = normalize_hotkey(key);
    (0x08..=0xDE).find(|vk| vk_to_name(*vk).is_some_and(|n| n == name))
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

    pub fn problem() -> Option<&'static str> {
        None
    }
}

/// The unshifted key (US layout) that types `c`; the caller presses Shift itself.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn base_char(c: char) -> char {
    match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        c => c.to_ascii_lowercase(),
    }
}

/// Virtual-key code (as used by [`vk_to_name`]) of a US-layout punctuation key.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn punctuation_vk(c: char) -> Option<u32> {
    Some(match c {
        ';' => 0xBA,
        '=' => 0xBB,
        ',' => 0xBC,
        '-' => 0xBD,
        '.' => 0xBE,
        '/' => 0xBF,
        '`' => 0xC0,
        '[' => 0xDB,
        '\\' => 0xDC,
        ']' => 0xDD,
        '\'' => 0xDE,
        _ => return None,
    })
}

// Linux: XTest for key presses; hotkeys from /dev/input, or X11 key state polling.
#[cfg(target_os = "linux")]
mod platform {
    use super::{Backend, Key};
    use crate::x11::{self, X11};
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::time::{Duration, Instant};

    /// When each keycode was last pressed or released by the app, so the hotkey poll
    /// never mistakes simulated notes for the user's own key presses.
    static INJECTED: Mutex<[Option<Instant>; 256]> = parking_lot::const_mutex([None; 256]);
    static HELD: Mutex<[u8; 32]> = parking_lot::const_mutex([0; 32]);

    fn bit(map: &[u8; 32], code: u8) -> bool {
        map[code as usize / 8] & (1 << (code % 8)) != 0
    }

    fn keycode(x: &X11, key: Key) -> Option<u8> {
        let keysym = match key {
            Key::Shift => 0xffe1,
            Key::Ctrl => 0xffe3,
            Key::Alt => 0xffe9,
            Key::Space => 0x20,
            Key::Char(c) => {
                let c = super::base_char(c);
                if !(' '..='~').contains(&c) {
                    return None;
                }
                c as u32
            }
        };
        x.keycode_for(keysym)
    }

    pub fn send(key: Key, down: bool, _backend: Backend) {
        let Some(x) = x11::get() else { return };
        let Some(code) = keycode(x, key) else { return };
        INJECTED.lock()[code as usize] = Some(Instant::now());
        {
            let mut held = HELD.lock();
            let (i, mask) = (code as usize / 8, 1u8 << (code % 8));
            if down {
                held[i] |= mask;
            } else {
                held[i] &= !mask;
            }
        }
        x.fake_key(code, down);
    }

    fn injected(code: u8) -> bool {
        bit(&HELD.lock(), code) || INJECTED.lock()[code as usize].is_some_and(|t| t.elapsed() < Duration::from_millis(80))
    }

    /// Windows virtual-key code of a physical key, from its X keysym.
    fn vk_for(x: &X11, code: u8) -> Option<u32> {
        let sym = x.keysym(code, 0);
        // Keypad digits sit on the second level unless Num Lock swaps them.
        for s in [sym, x.keysym(code, 1)] {
            if (0xffb0..=0xffb9).contains(&s) {
                return Some(0x60 + (s - 0xffb0));
            }
        }
        Some(match sym {
            0x61..=0x7a => sym - 0x20,
            0x41..=0x5a | 0x30..=0x39 | 0x20 => sym,
            0xffbe..=0xffd5 => 0x70 + (sym - 0xffbe),
            0xff08 => 0x08,
            0xff09 => 0x09,
            0xff0d | 0xff8d => 0x0D,
            0xff13 => 0x13,
            0xff1b => 0x1B,
            0xff50 => 0x24,
            0xff51 => 0x25,
            0xff52 => 0x26,
            0xff53 => 0x27,
            0xff54 => 0x28,
            0xff55 => 0x21,
            0xff56 => 0x22,
            0xff57 => 0x23,
            0xff63 => 0x2D,
            0xffff => 0x2E,
            0xffe1 => 0xA0,
            0xffe2 => 0xA1,
            0xffe3 => 0xA2,
            0xffe4 => 0xA3,
            0xffe9 => 0xA4,
            0xffea => 0xA5,
            0xffe5 => 0x14,
            0xff7f => 0x90,
            0xff14 => 0x91,
            0xffaa => 0x6A,
            0xffab => 0x6B,
            0xffad => 0x6D,
            0xffae => 0x6E,
            0xffaf => 0x6F,
            0x21..=0x7e => return super::punctuation_vk(char::from_u32(sym)?),
            _ => return None,
        })
    }

    /// Linux input key code (`KEY_*` from linux/input-event-codes.h) to virtual-key code.
    fn evdev_vk(code: u16) -> Option<u32> {
        const LETTERS: [(u16, u8); 26] = [
            (30, b'A'), (48, b'B'), (46, b'C'), (32, b'D'), (18, b'E'), (33, b'F'), (34, b'G'), (35, b'H'), (23, b'I'),
            (36, b'J'), (37, b'K'), (38, b'L'), (50, b'M'), (49, b'N'), (24, b'O'), (25, b'P'), (16, b'Q'), (19, b'R'),
            (31, b'S'), (20, b'T'), (22, b'U'), (47, b'V'), (17, b'W'), (45, b'X'), (21, b'Y'), (44, b'Z'),
        ];
        if let Some((_, c)) = LETTERS.iter().find(|(k, _)| *k == code) {
            return Some(*c as u32);
        }
        Some(match code {
            2..=10 => 0x31 + (code as u32 - 2),
            11 => 0x30,
            59..=68 => 0x70 + (code as u32 - 59),
            87 => 0x7A,
            88 => 0x7B,
            183..=194 => 0x7C + (code as u32 - 183),
            1 => 0x1B,
            12 => 0xBD,
            13 => 0xBB,
            14 => 0x08,
            15 => 0x09,
            26 => 0xDB,
            27 => 0xDD,
            28 | 96 => 0x0D,
            29 => 0xA2,
            97 => 0xA3,
            39 => 0xBA,
            40 => 0xDE,
            41 => 0xC0,
            42 => 0xA0,
            54 => 0xA1,
            43 => 0xDC,
            51 => 0xBC,
            52 => 0xBE,
            53 => 0xBF,
            55 => 0x6A,
            56 => 0xA4,
            100 => 0xA5,
            57 => 0x20,
            58 => 0x14,
            69 => 0x90,
            70 => 0x91,
            71 => 0x67,
            72 => 0x68,
            73 => 0x69,
            74 => 0x6D,
            75 => 0x64,
            76 => 0x65,
            77 => 0x66,
            78 => 0x6B,
            79 => 0x61,
            80 => 0x62,
            81 => 0x63,
            82 => 0x60,
            83 => 0x6E,
            98 => 0x6F,
            99 => 0x2C,
            102 => 0x24,
            103 => 0x26,
            104 => 0x21,
            105 => 0x25,
            106 => 0x27,
            107 => 0x23,
            108 => 0x28,
            109 => 0x22,
            110 => 0x2D,
            111 => 0x2E,
            119 => 0x13,
            125 => 0x5B,
            126 => 0x5C,
            _ => return None,
        })
    }

    /// `/dev/input/eventN` paths of the keyboards listed in /proc/bus/input/devices
    /// (devices with a `kbd` handler and key repeat).
    fn keyboard_devices() -> Vec<String> {
        let Ok(text) = std::fs::read_to_string("/proc/bus/input/devices") else { return Vec::new() };
        let mut found = Vec::new();
        for block in text.split("\n\n") {
            let mut event = None;
            let mut kbd = false;
            let mut repeat = false;
            for line in block.lines() {
                if let Some(h) = line.strip_prefix("H: Handlers=") {
                    kbd = h.split_whitespace().any(|w| w == "kbd");
                    event = h.split_whitespace().find(|w| w.starts_with("event")).map(str::to_owned);
                } else if let Some(ev) = line.strip_prefix("B: EV=") {
                    repeat = u64::from_str_radix(ev.trim(), 16).is_ok_and(|bits| bits & (1 << 20) != 0);
                }
            }
            if let (Some(event), true, true) = (event, kbd, repeat) {
                found.push(format!("/dev/input/{event}"));
            }
        }
        found
    }

    /// 0 = not started, 1 = reading keyboards directly, 2 = X11 polling only.
    static MODE: AtomicU8 = AtomicU8::new(0);

    /// Reads key presses straight from the keyboards (works under Wayland and X11, and
    /// never sees the app's own XTest presses). Needs read access to /dev/input, which
    /// members of the `input` group have. Returns false when no keyboard could be opened.
    fn start_evdev(callback: Arc<dyn Fn(u32) -> bool + Send + Sync>) -> bool {
        use std::io::Read;
        static ALT: AtomicU8 = AtomicU8::new(0);
        // struct input_event: struct timeval, u16 type, u16 code, i32 value.
        const TIMEVAL: usize = 2 * std::mem::size_of::<usize>();
        const SIZE: usize = TIMEVAL + 8;
        let mut opened = 0;
        for path in keyboard_devices() {
            let mut file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    log::info!("cannot read {path}: {e}");
                    continue;
                }
            };
            opened += 1;
            let callback = callback.clone();
            let _ = std::thread::Builder::new().name("keyboard-evdev".into()).spawn(move || {
                let mut buf = [0u8; SIZE * 32];
                loop {
                    let n = match file.read(&mut buf) {
                        Ok(0) | Err(_) => return, // unplugged
                        Ok(n) => n,
                    };
                    for ev in buf[..n].chunks_exact(SIZE) {
                        let kind = u16::from_ne_bytes([ev[TIMEVAL], ev[TIMEVAL + 1]]);
                        let code = u16::from_ne_bytes([ev[TIMEVAL + 2], ev[TIMEVAL + 3]]);
                        let value = i32::from_ne_bytes([ev[TIMEVAL + 4], ev[TIMEVAL + 5], ev[TIMEVAL + 6], ev[TIMEVAL + 7]]);
                        if kind != 1 {
                            continue; // not EV_KEY
                        }
                        if code == 56 || code == 100 {
                            let bit = if code == 56 { 1 } else { 2 };
                            if value == 0 {
                                ALT.fetch_and(!bit, Ordering::AcqRel);
                            } else {
                                ALT.fetch_or(bit, Ordering::AcqRel);
                            }
                        }
                        if value != 1 {
                            continue; // release or auto-repeat
                        }
                        if let Some(vk) = evdev_vk(code) {
                            let alt = if ALT.load(Ordering::Acquire) != 0 { super::ALT_FLAG } else { 0 };
                            callback(vk | alt);
                        }
                    }
                }
            });
        }
        if opened > 0 {
            log::info!("hotkeys: reading {opened} keyboard device(s) directly");
        }
        opened > 0
    }

    /// Global hotkeys: read the keyboards directly when allowed, otherwise poll the X11
    /// key state (which only sees keys while an X11 / XWayland window has focus).
    pub fn start_hook(callback: impl Fn(u32) -> bool + Send + Sync + 'static) {
        let callback: Arc<dyn Fn(u32) -> bool + Send + Sync> = Arc::new(callback);
        if start_evdev(callback.clone()) {
            MODE.store(1, Ordering::Release);
            return;
        }
        MODE.store(2, Ordering::Release);
        log::warn!("hotkeys: no readable keyboard in /dev/input (join the input group); falling back to X11 polling");
        std::thread::Builder::new()
            .name("keyboard-poll".into())
            .spawn(move || {
                let Some(x) = x11::get() else {
                    log::warn!("global hotkeys need X11, XWayland or access to /dev/input");
                    return;
                };
                let alt_keys: Vec<u8> = [0xffe9, 0xffea, 0xffe7, 0xffe8].into_iter().filter_map(|s| x.keycode_for(s)).collect();
                let mut prev = x.keymap().unwrap_or([0; 32]);
                loop {
                    std::thread::sleep(Duration::from_millis(8));
                    let Some(now) = x.keymap() else {
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    };
                    if now == prev {
                        continue;
                    }
                    let alt = if alt_keys.iter().any(|k| bit(&now, *k)) { super::ALT_FLAG } else { 0 };
                    for code in 8..=255u8 {
                        if bit(&now, code) && !bit(&prev, code) && !injected(code) {
                            if let Some(vk) = vk_for(x, code) {
                                callback(vk | alt);
                            }
                        }
                    }
                    prev = now;
                }
            })
            .ok();
    }

    fn wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
    }

    pub fn problem() -> Option<&'static str> {
        if x11::get().is_none() {
            return Some("No X11 display found. Key presses need X11 or XWayland.");
        }
        (MODE.load(Ordering::Acquire) == 2 && wayland()).then_some(
            "Hotkeys only work while this window or an X11 game window is focused. For hotkeys everywhere, run \"sudo usermod -aG input $USER\", then log out and back in.",
        )
    }
}

// macOS: Quartz events for key presses, an event tap for the hotkeys.
#[cfg(target_os = "macos")]
mod platform {
    use super::{Backend, Key};
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
    use std::time::Duration;

    type CGEventRef = *mut c_void;
    type TapCallback = extern "C" fn(*mut c_void, u32, CGEventRef, *mut c_void) -> CGEventRef;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state_id: i32) -> *mut c_void;
        fn CGEventCreateKeyboardEvent(source: *mut c_void, keycode: u16, key_down: bool) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventGetFlags(event: CGEventRef) -> u64;
        fn CGEventPost(tap: u32, event: CGEventRef);
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
        fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, user_info: *mut c_void) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopCommonModes: *const c_void;
        fn CFRelease(cf: *const c_void);
        fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRun();
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    const FLAG_SHIFT: u64 = 0x2_0000;
    const FLAG_CONTROL: u64 = 0x4_0000;
    const FLAG_OPTION: u64 = 0x8_0000;
    /// kCGKeyboardEventAutorepeat, kCGKeyboardEventKeycode, kCGEventSourceUserData.
    const FIELD_AUTOREPEAT: u32 = 8;
    const FIELD_KEYCODE: u32 = 9;
    const FIELD_USER_DATA: u32 = 42;
    /// Marks the events this app posts, so the hotkey tap can skip them.
    const MAGIC: i64 = 0x6e4d_4944;
    const EVENT_KEY_DOWN: u32 = 10;

    /// Modifier flags of the simulated Shift/Ctrl/Alt keys that are currently held.
    static MODIFIERS: AtomicU64 = AtomicU64::new(0);

    /// ANSI (US layout) key codes: (character, macOS key code).
    const KEYCODES: [(char, u16); 47] = [
        ('a', 0), ('s', 1), ('d', 2), ('f', 3), ('h', 4), ('g', 5), ('z', 6), ('x', 7), ('c', 8), ('v', 9),
        ('b', 11), ('q', 12), ('w', 13), ('e', 14), ('r', 15), ('y', 16), ('t', 17), ('1', 18), ('2', 19),
        ('3', 20), ('4', 21), ('6', 22), ('5', 23), ('=', 24), ('9', 25), ('7', 26), ('-', 27), ('8', 28),
        ('0', 29), (']', 30), ('o', 31), ('u', 32), ('[', 33), ('i', 34), ('p', 35), ('l', 37), ('j', 38),
        ('\'', 39), ('k', 40), (';', 41), ('\\', 42), (',', 43), ('/', 44), ('n', 45), ('m', 46), ('.', 47),
        ('`', 50),
    ];

    fn keycode(key: Key) -> Option<u16> {
        Some(match key {
            Key::Shift => 56,
            Key::Ctrl => 59,
            Key::Alt => 58,
            Key::Space => 49,
            Key::Char(c) => {
                let c = super::base_char(c);
                KEYCODES.iter().find(|(ch, _)| *ch == c)?.1
            }
        })
    }

    fn source() -> *mut c_void {
        static SOURCE: OnceLock<usize> = OnceLock::new();
        // kCGEventSourceStateHIDSystemState
        *SOURCE.get_or_init(|| unsafe { CGEventSourceCreate(1) } as usize) as *mut c_void
    }

    pub fn send(key: Key, down: bool, _backend: Backend) {
        let Some(code) = keycode(key) else { return };
        let flag = match key {
            Key::Shift => FLAG_SHIFT,
            Key::Ctrl => FLAG_CONTROL,
            Key::Alt => FLAG_OPTION,
            _ => 0,
        };
        let modifiers = if flag == 0 {
            MODIFIERS.load(Ordering::Acquire)
        } else if down {
            MODIFIERS.fetch_or(flag, Ordering::AcqRel) | flag
        } else {
            MODIFIERS.fetch_and(!flag, Ordering::AcqRel) & !flag
        };
        unsafe {
            let event = CGEventCreateKeyboardEvent(source(), code, down);
            if event.is_null() {
                return;
            }
            CGEventSetFlags(event, modifiers);
            CGEventSetIntegerValueField(event, FIELD_USER_DATA, MAGIC);
            CGEventPost(0, event); // kCGHIDEventTap
            CFRelease(event as *const c_void);
        }
    }

    /// Windows virtual-key code of a macOS key code.
    fn vk_for(code: u16) -> Option<u32> {
        if let Some((c, _)) = KEYCODES.iter().find(|(_, k)| *k == code) {
            return match c {
                'a'..='z' => Some(*c as u32 - 0x20),
                '0'..='9' => Some(*c as u32),
                _ => super::punctuation_vk(*c),
            };
        }
        Some(match code {
            122 => 0x70,
            120 => 0x71,
            99 => 0x72,
            118 => 0x73,
            96 => 0x74,
            97 => 0x75,
            98 => 0x76,
            100 => 0x77,
            101 => 0x78,
            109 => 0x79,
            103 => 0x7A,
            111 => 0x7B,
            36 | 76 => 0x0D,
            48 => 0x09,
            49 => 0x20,
            51 => 0x08,
            53 => 0x1B,
            56 => 0xA0,
            60 => 0xA1,
            59 => 0xA2,
            62 => 0xA3,
            58 => 0xA4,
            61 => 0xA5,
            57 => 0x14,
            115 => 0x24,
            116 => 0x21,
            117 => 0x2E,
            119 => 0x23,
            121 => 0x22,
            123 => 0x25,
            124 => 0x27,
            125 => 0x28,
            126 => 0x26,
            82..=89 => 0x60 + (code as u32 - 82),
            91 => 0x68,
            92 => 0x69,
            67 => 0x6A,
            69 => 0x6B,
            78 => 0x6D,
            65 => 0x6E,
            75 => 0x6F,
            _ => return None,
        })
    }

    type Callback = Box<dyn Fn(u32) -> bool + Send + Sync>;
    static CALLBACK: OnceLock<Callback> = OnceLock::new();
    static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    extern "C" fn tap_callback(_proxy: *mut c_void, kind: u32, event: CGEventRef, _user_info: *mut c_void) -> CGEventRef {
        unsafe {
            // The system switches a slow tap off (timeout / user input): switch it on again.
            if kind == 0xFFFF_FFFE || kind == 0xFFFF_FFFF {
                let tap = TAP.load(Ordering::Acquire);
                if !tap.is_null() {
                    CGEventTapEnable(tap, true);
                }
                return event;
            }
            if kind == EVENT_KEY_DOWN
                && !event.is_null()
                && CGEventGetIntegerValueField(event, FIELD_USER_DATA) != MAGIC
                && CGEventGetIntegerValueField(event, FIELD_AUTOREPEAT) == 0
            {
                let code = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
                if let (Some(vk), Some(callback)) = (vk_for(code), CALLBACK.get()) {
                    let alt = if CGEventGetFlags(event) & FLAG_OPTION != 0 { super::ALT_FLAG } else { 0 };
                    callback(vk | alt);
                }
            }
        }
        event
    }

    /// Listens for key presses with an event tap on its own thread. macOS only allows
    /// that once the app is enabled under Privacy & Security, so this keeps trying.
    pub fn start_hook(callback: impl Fn(u32) -> bool + Send + Sync + 'static) {
        if CALLBACK.set(Box::new(callback)).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("keyboard-tap".into())
            .spawn(|| unsafe {
                let mut warned = false;
                let tap = loop {
                    // Session tap, head insert, listen only, key-down events.
                    let tap = CGEventTapCreate(1, 0, 1, 1u64 << EVENT_KEY_DOWN, tap_callback, std::ptr::null_mut());
                    if !tap.is_null() {
                        break tap;
                    }
                    if !warned {
                        warned = true;
                        log::warn!("global hotkeys need the Accessibility / Input Monitoring permission");
                    }
                    std::thread::sleep(Duration::from_secs(3));
                };
                TAP.store(tap, Ordering::Release);
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    log::error!("failed to attach the keyboard event tap");
                    return;
                }
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                CFRunLoopRun();
            })
            .ok();
    }

    pub fn problem() -> Option<&'static str> {
        (unsafe { AXIsProcessTrusted() } == 0).then_some(
            "macOS is blocking key presses. Allow nanoMIDIPlayer in System Settings → Privacy & Security → Accessibility and Input Monitoring, then restart the app.",
        )
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::{Backend, Key};

    pub fn send(_key: Key, _down: bool, _backend: Backend) {}

    pub fn start_hook(_callback: impl Fn(u32) -> bool + Send + Sync + 'static) {
        log::warn!("global hotkeys are not implemented on this system");
    }

    pub fn problem() -> Option<&'static str> {
        Some("Key presses and hotkeys are not supported on this system.")
    }
}

pub use platform::start_hook;

/// Why key presses and hotkeys cannot work right now, if they cannot (shown in the UI).
pub fn problem() -> Option<&'static str> {
    platform::problem()
}
