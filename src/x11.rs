//! Shared X11 connection (Linux): key presses through XTest, key state polling for the
//! global hotkeys, the active window for "pause when the game loses focus" and
//! "always on top". Works in X11 sessions and for XWayland windows under Wayland.

use std::sync::OnceLock;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, Window};
use x11rb::protocol::xtest::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

pub struct X11 {
    conn: RustConnection,
    root: Window,
    min_keycode: u8,
    per_keycode: usize,
    keysyms: Vec<u32>,
    net_active_window: u32,
    net_wm_pid: u32,
    net_wm_state: u32,
    net_wm_state_above: u32,
}

static X: OnceLock<Option<X11>> = OnceLock::new();

/// The connection, or `None` when there is no X server (pure Wayland without XWayland).
pub fn get() -> Option<&'static X11> {
    X.get_or_init(|| match open() {
        Ok(x) => Some(x),
        Err(e) => {
            log::warn!("X11 is not available: {e}");
            None
        }
    })
    .as_ref()
}

fn open() -> anyhow::Result<X11> {
    let (conn, screen) = x11rb::connect(None)?;
    let (root, min, max) = {
        let setup = conn.setup();
        let root = setup.roots.get(screen).ok_or_else(|| anyhow::anyhow!("no X screen"))?.root;
        (root, setup.min_keycode, setup.max_keycode)
    };
    if conn.extension_information(xtest::X11_EXTENSION_NAME)?.is_none() {
        anyhow::bail!("the XTest extension is missing");
    }
    let map = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
    let atom = |name: &[u8]| -> anyhow::Result<u32> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
    let net_active_window = atom(b"_NET_ACTIVE_WINDOW")?;
    let net_wm_pid = atom(b"_NET_WM_PID")?;
    let net_wm_state = atom(b"_NET_WM_STATE")?;
    let net_wm_state_above = atom(b"_NET_WM_STATE_ABOVE")?;
    Ok(X11 {
        conn,
        root,
        min_keycode: min,
        per_keycode: (map.keysyms_per_keycode as usize).max(1),
        keysyms: map.keysyms,
        net_active_window,
        net_wm_pid,
        net_wm_state,
        net_wm_state_above,
    })
}

impl X11 {
    /// Keysym of a key at a shift level (0 = plain, 1 = with Shift); 0 if there is none.
    pub fn keysym(&self, keycode: u8, level: usize) -> u32 {
        if keycode < self.min_keycode || level >= self.per_keycode {
            return 0;
        }
        let index = (keycode - self.min_keycode) as usize * self.per_keycode + level;
        self.keysyms.get(index).copied().unwrap_or(0)
    }

    /// The key that produces `keysym` without modifiers.
    pub fn keycode_for(&self, keysym: u32) -> Option<u8> {
        let count = self.keysyms.len() / self.per_keycode;
        (0..count)
            .find(|i| self.keysyms[i * self.per_keycode] == keysym)
            .map(|i| self.min_keycode + i as u8)
    }

    pub fn fake_key(&self, keycode: u8, down: bool) {
        let kind = if down { KEY_PRESS_EVENT } else { KEY_RELEASE_EVENT };
        let _ = self.conn.xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, self.root, 0, 0, 0);
        let _ = self.conn.flush();
    }

    /// One bit per keycode: which keys are down right now.
    pub fn keymap(&self) -> Option<[u8; 32]> {
        Some(self.conn.query_keymap().ok()?.reply().ok()?.keys)
    }

    /// The active top-level window and whether it belongs to this process.
    pub fn active_window(&self) -> Option<(u32, bool)> {
        let window = self
            .conn
            .get_property(false, self.root, self.net_active_window, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()
            .filter(|w| *w != 0)?;
        let pid = self
            .conn
            .get_property(false, window, self.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()));
        Some((window, pid == Some(std::process::id())))
    }

    /// Asks the window manager to keep `window` above the others (or stop doing so).
    pub fn set_above(&self, window: u32, on: bool) {
        let event = ClientMessageEvent::new(32, window, self.net_wm_state, [on as u32, self.net_wm_state_above, 0, 1, 0]);
        let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event);
        let _ = self.conn.flush();
    }
}
