//! "Pause when the game loses focus": watches the foreground window while a
//! keyboard playback runs, pauses as soon as another window (including this app)
//! is active, and resumes when the game window comes back.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::config;
use crate::console;
use crate::core::{Core, Target};

/// Per player: the window the song is played into, once known.
#[derive(Default, Clone, Copy)]
struct Watch {
    target: Option<isize>,
}

pub fn start(core: Arc<Core>) {
    std::thread::Builder::new()
        .name("focus-watch".into())
        .spawn(move || {
            let mut watches = [Watch::default(); 2];
            loop {
                std::thread::sleep(Duration::from_millis(60));
                let enabled = config::get().read().app_ui.pause_on_focus_loss;
                for (i, target) in [Target::Midi, Target::Drums].into_iter().enumerate() {
                    check(&core, target, enabled, &mut watches[i]);
                }
            }
        })
        .ok();
}

fn check(core: &Core, target: Target, enabled: bool, watch: &mut Watch) {
    let player = match target {
        Target::Midi => &core.piano,
        Target::Drums => &core.drums,
    };
    let flag = &core.auto_paused[target as usize];
    // MIDI device output presses no keys, so focus doesn't matter there.
    let midi_out = target == Target::Midi && config::get().read().midi_player.use_midi_output;
    if !enabled || midi_out || !player.is_running() {
        *watch = Watch::default();
        flag.store(false, Ordering::Release);
        return;
    }
    let Some((fg, own)) = platform::foreground() else { return };

    match watch.target {
        // The first non-app window after starting is the game (covers starting with
        // the Play button and then clicking into the game).
        None => {
            if !own {
                watch.target = Some(fg);
            }
        }
        Some(game) if fg != game => {
            if !player.is_paused() {
                player.toggle_pause();
                flag.store(true, Ordering::Release);
                console::insert("Paused: game window lost focus");
                core.bump();
            }
        }
        Some(_) => {
            if flag.load(Ordering::Acquire) {
                flag.store(false, Ordering::Release);
                if player.is_paused() {
                    player.toggle_pause();
                    core.bump();
                }
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    /// Foreground window handle and whether it belongs to this app.
    pub fn foreground() -> Option<(isize, bool)> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            Some((hwnd.0 as isize, pid == GetCurrentProcessId()))
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn foreground() -> Option<(isize, bool)> {
        None
    }
}
