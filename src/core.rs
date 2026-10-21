//! Application state and actions shared by the UI, global hotkeys and worker threads.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::config::{self, HotkeySlot};
use crate::console;
use crate::keys;
use crate::live::{self, LiveInput};
use crate::midi_file;
use crate::slots;
use crate::player::{Mode, Player};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Midi,
    Drums,
    Hub,
    ToQwerty,
    Settings,
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Midi,
    Drums,
}

#[derive(Default, Clone)]
pub struct FileList {
    pub files: Vec<String>,
    pub current: Option<String>,
    pub length: f64,
}

#[derive(Default)]
pub struct CoreState {
    pub tab: Option<Tab>,
    pub midi: FileList,
    pub drums: FileList,
    pub outputs: Vec<String>,
    pub output: Option<String>,
    pub inputs: Vec<String>,
    pub input: Option<String>,
    pub capturing: Option<HotkeySlot>,
}

pub struct Core {
    pub piano: Arc<Player>,
    pub drums: Arc<Player>,
    pub live: Arc<LiveInput>,
    pub state: Mutex<CoreState>,
    version: AtomicU64,
}

fn alert(title: &'static str, message: String) {
    std::thread::spawn(move || {
        rfd::MessageDialog::new()
            .set_title(title)
            .set_description(message)
            .set_level(rfd::MessageLevel::Warning)
            .show();
    });
}

impl Core {
    pub fn new() -> Arc<Self> {
        let core = Arc::new(Self {
            piano: Player::new(false, 1),
            drums: Player::new(true, 2),
            live: LiveInput::new(3),
            state: Mutex::new(CoreState::default()),
            version: AtomicU64::new(0),
        });
        core.load_saved_files(Target::Midi);
        core.load_saved_files(Target::Drums);
        core
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
            + self.piano.version()
            + self.drums.version()
            + self.live.version()
    }

    pub fn bump(&self) {
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    fn player(&self, target: Target) -> &Arc<Player> {
        match target {
            Target::Midi => &self.piano,
            Target::Drums => &self.drums,
        }
    }

    // ------------------------------------------------------------------ files

    fn files(state: &mut CoreState, target: Target) -> &mut FileList {
        match target {
            Target::Midi => &mut state.midi,
            Target::Drums => &mut state.drums,
        }
    }

    fn song_length(path: &str) -> f64 {
        midi_file::load(path).map(|s| s.length).unwrap_or(0.0)
    }

    /// Restores the dropdown list from config (+ downloaded MIDIs for the piano player).
    pub fn load_saved_files(&self, target: Target) {
        let (list, current) = config::get().update(|c| {
            let (list, current) = match target {
                Target::Midi => (&mut c.midi_player.midi_list, &mut c.midi_player.current_file),
                Target::Drums => (&mut c.drums.midi_list, &mut c.drums.current_file),
            };
            list.retain(|f| Path::new(f).exists());
            if !current.is_empty() && !Path::new(current.as_str()).exists() {
                current.clear();
            }
            if current.is_empty() {
                if let Some(first) = list.first() {
                    *current = first.clone();
                }
            }
            (list.clone(), current.clone())
        });

        let mut entries: Vec<String> = Vec::new();
        if target == Target::Midi {
            if let Ok(rd) = std::fs::read_dir(config::midis_dir()) {
                let mut found: Vec<String> = rd
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"))
                            .unwrap_or(false)
                    })
                    .map(|p| p.to_string_lossy().into_owned())
                    .filter(|p| !list.contains(p))
                    .collect();
                found.sort();
                entries.extend(found);
            }
        }
        for p in &list {
            if !entries.contains(p) {
                entries.push(p.clone());
            }
        }
        let current = (!current.is_empty()).then_some(current);
        if let Some(c) = &current {
            if !entries.contains(c) {
                entries.push(c.clone());
            }
        }
        let length = current.as_deref().map(Self::song_length).unwrap_or(0.0);
        {
            let mut state = self.state.lock();
            *Self::files(&mut state, target) = FileList { files: entries, current, length };
        }
        self.bump();
    }

    /// Adds `path` to the list and makes it the current file.
    pub fn use_file(&self, target: Target, path: &Path) {
        let path = path.to_string_lossy().into_owned();
        let length = Self::song_length(&path);
        config::get().update(|c| {
            let (list, current) = match target {
                Target::Midi => (&mut c.midi_player.midi_list, &mut c.midi_player.current_file),
                Target::Drums => (&mut c.drums.midi_list, &mut c.drums.current_file),
            };
            if !list.contains(&path) {
                list.push(path.clone());
            }
            *current = path.clone();
        });
        {
            let mut state = self.state.lock();
            let files = Self::files(&mut state, target);
            if !files.files.contains(&path) {
                files.files.push(path.clone());
            }
            files.current = Some(path);
            files.length = length;
        }
        self.bump();
    }

    /// Switches to another entry of the dropdown.
    pub fn switch_file(&self, target: Target, path: String) {
        let length = Self::song_length(&path);
        config::get().update(|c| match target {
            Target::Midi => c.midi_player.current_file = path.clone(),
            Target::Drums => c.drums.current_file = path.clone(),
        });
        {
            let mut state = self.state.lock();
            let files = Self::files(&mut state, target);
            files.current = Some(path);
            files.length = length;
        }
        self.bump();
    }

    /// "Select File" button: stops playback, then opens a file dialog (off the UI thread).
    pub fn select_file(self: &Arc<Self>, target: Target) {
        self.stop(target);
        let core = self.clone();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .add_filter("MIDI files", &["mid", "midi"])
                .set_title("Select MIDI File")
                .pick_file();
            if let Some(path) = picked {
                core.use_file(target, &path);
            }
        });
    }

    pub fn clear_list(&self, target: Target) {
        config::get().update(|c| match target {
            Target::Midi => {
                c.midi_player.midi_list.clear();
                c.midi_player.current_file.clear();
            }
            Target::Drums => {
                c.drums.midi_list.clear();
                c.drums.current_file.clear();
            }
        });
        {
            let mut state = self.state.lock();
            *Self::files(&mut state, target) = FileList::default();
        }
        self.load_saved_files(target);
        console::log("MIDI list fully cleared");
    }

    // --------------------------------------------------------------- devices

    pub fn refresh_outputs(&self) {
        let devices = live::output_devices();
        let default = devices
            .iter()
            .find(|d| d.contains("LoopBe"))
            .or(devices.first())
            .cloned();
        if devices.is_empty() {
            config::get().update(|c| c.midi_player.use_midi_output = false);
        }
        {
            let mut state = self.state.lock();
            state.outputs = devices;
            state.output = default;
        }
        self.bump();
    }

    pub fn refresh_inputs(&self) {
        let devices = live::input_devices();
        let mut state = self.state.lock();
        let previous = state.input.clone();
        state.input = previous
            .filter(|p| devices.contains(p))
            .or_else(|| devices.iter().find(|d| d.contains("LoopBe")).cloned())
            .or_else(|| devices.first().cloned());
        state.inputs = devices;
        drop(state);
        self.bump();
    }

    pub fn set_use_midi_output(&self, on: bool) {
        config::get().update(|c| c.midi_player.use_midi_output = on);
        console::clear();
        self.refresh_outputs();
        if on {
            console::insert("-------< WARNING >-------   This will not press keys for you!");
        } else {
            console::insert("Macro Mode.");
        }
    }

    // -------------------------------------------------------------- playback

    pub fn play_button(&self, target: Target) {
        if self.player(target).is_running() {
            self.pause(target);
        } else {
            self.start(target);
        }
    }

    pub fn start(&self, target: Target) {
        let player = self.player(target);
        if player.is_running() {
            return;
        }
        let (current, output) = {
            let state = self.state.lock();
            let files = match target {
                Target::Midi => &state.midi,
                Target::Drums => &state.drums,
            };
            (files.current.clone(), state.output.clone())
        };
        let Some(path) = current.filter(|p| Path::new(p).exists()) else {
            console::insert("MIDI File does not exist.");
            return;
        };
        let mode = match target {
            Target::Drums => Mode::Drums,
            Target::Midi if config::get().read().midi_player.use_midi_output => Mode::MidiOut,
            Target::Midi => Mode::Qwerty,
        };
        if let Err(e) = player.start(PathBuf::from(path), mode, output) {
            console::insert(e.to_string());
        }
        self.bump();
    }

    pub fn pause(&self, target: Target) {
        let player = self.player(target);
        if player.is_running() {
            player.toggle_pause();
        }
    }

    pub fn stop(&self, target: Target) {
        self.player(target).stop();
        self.bump();
    }

    pub fn set_speed(&self, target: Target, percent: f64) {
        self.player(target).set_speed_percent(percent);
    }

    /// `faster = false` is the "Slow Down" hotkey (config key `speedup`).
    pub fn step_speed(&self, target: Target, faster: bool) {
        let size = {
            let cfg = config::get().read();
            match target {
                Target::Midi => cfg.midi_player.decrease_size,
                Target::Drums => cfg.drums.decrease_size,
            }
        } / 100.0;
        self.player(target).change_speed(if faster { size } else { -size });
    }

    // ------------------------------------------------------------- MIDI→QWERTY

    pub fn toggle_live(&self) {
        if self.live.is_running() {
            self.live.stop();
        } else {
            let input = self.state.lock().input.clone();
            match input {
                None => console::insert("No MIDI output device selected."),
                Some(name) => {
                    if let Err(e) = self.live.start(&name) {
                        console::log(e.to_string());
                    }
                }
            }
        }
        self.bump();
    }

    // --------------------------------------------------------------- hotkeys

    pub fn set_tab(&self, tab: Tab) {
        let mut state = self.state.lock();
        if state.tab == Some(tab) {
            return;
        }
        state.tab = Some(tab);
        drop(state);
        console::clear();
        if tab == Tab::Drums {
            console::insert("Note: This will only work if your MIDI file specifically uses a drum instrument.");
        }
        self.bump();
    }

    pub fn begin_capture(&self, slot: HotkeySlot) {
        self.state.lock().capturing = Some(slot);
        self.bump();
    }

    fn finish_capture(&self, slot: HotkeySlot, name: String) {
        let conflict = {
            let cfg = config::get().read();
            HotkeySlot::ALL
                .iter()
                .copied()
                .filter(|s| *s != slot)
                .find(|s| keys::normalize_hotkey(cfg.hotkeys.get(*s)) == name)
        };
        match conflict {
            Some(other) => alert(
                "Keybind Conflict",
                format!("This keybind is in use!\n{}: {}", other.config_name(), name),
            ),
            None => config::get().update(|c| c.hotkeys.set(slot, name)),
        }
        self.state.lock().capturing = None;
        self.bump();
    }

    /// Called on a worker thread for every physical key press.
    pub fn on_global_key(&self, vk: u32) {
        let Some(name) = keys::vk_to_name(vk) else { return };
        let (capturing, tab) = {
            let state = self.state.lock();
            (state.capturing, state.tab)
        };
        if let Some(slot) = capturing {
            self.finish_capture(slot, name);
            return;
        }
        let target = match tab {
            Some(Tab::Midi) => Target::Midi,
            Some(Tab::Drums) => Target::Drums,
            _ => return,
        };
        let slot = {
            let cfg = config::get().read();
            HotkeySlot::ALL
                .iter()
                .copied()
                .find(|s| keys::normalize_hotkey(cfg.hotkeys.get(*s)) == name)
        };
        match slot {
            Some(HotkeySlot::Play) => self.start(target),
            Some(HotkeySlot::Pause) => self.pause(target),
            Some(HotkeySlot::Stop) => self.stop(target),
            Some(HotkeySlot::SpeedUp) => self.step_speed(target, false),
            Some(HotkeySlot::SlowDown) => self.step_speed(target, true),
            None => {}
        }
        self.bump();
    }

    pub fn shutdown(&self) {
        self.piano.stop();
        self.drums.stop();
        self.live.stop();
        config::get().flush();
    }
}

/// Starts the global keyboard hook; key presses are handled on a worker thread so the
/// hook itself returns immediately.
pub fn start_hotkeys(core: Arc<Core>) {
    let (tx, rx) = crossbeam_channel::unbounded::<u32>();
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || {
            for code in rx {
                // Alt+1 … Alt+9 load song slots (unless a hotkey is being recorded).
                if let Some(slot) = keys::alt_digit(code) {
                    if core.state.lock().capturing.is_none() {
                        slots::activate(&core, slot);
                        continue;
                    }
                }
                core.on_global_key(code & !keys::ALT_FLAG);
            }
        })
        .ok();
    keys::start_hook(move |vk| {
        let _ = tx.send(vk);
        false
    });
}
