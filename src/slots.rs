//! Song slots: up to nine hub songs bound to Alt+1 … Alt+9, stored in
//! `Documents/nanoMIDIPlayer/songSlots.json`. Pressing the hotkey (globally, also
//! while a game is focused) loads that song into the MIDI Player.

use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::console;
use crate::core::{Core, Target};
use crate::{config, hub};

pub const SLOT_COUNT: usize = 9;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotSong {
    /// [`hub::HubMidi::key`] of the song.
    pub key: String,
    pub name: String,
    pub midi_filename: String,
}

type Slots = [Option<SlotSong>; SLOT_COUNT];

fn store() -> &'static Mutex<Slots> {
    static SLOTS: OnceLock<Mutex<Slots>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(load()))
}

fn path() -> std::path::PathBuf {
    config::base_dir().join("songSlots.json")
}

fn load() -> Slots {
    let list: Vec<Option<SlotSong>> = std::fs::read(path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    std::array::from_fn(|i| list.get(i).cloned().flatten())
}

fn save(slots: &Slots) {
    if let Ok(bytes) = serde_json::to_vec_pretty(slots) {
        let _ = std::fs::write(path(), bytes);
    }
}

pub fn get(slot: usize) -> Option<SlotSong> {
    store().lock().get(slot).cloned().flatten()
}

/// Slot index (0-based) the song with `key` is assigned to, if any.
pub fn slot_of(key: &str) -> Option<usize> {
    store().lock().iter().position(|s| s.as_ref().is_some_and(|s| s.key == key))
}

/// Puts `song` into `slot`, removing it from any other slot it was in.
pub fn assign(slot: usize, song: &hub::HubMidi) {
    let Some(file) = song.midi_filename.clone().filter(|f| !f.is_empty()) else { return };
    let key = song.key();
    let mut slots = store().lock();
    for s in slots.iter_mut() {
        if s.as_ref().is_some_and(|s| s.key == key) {
            *s = None;
        }
    }
    slots[slot] = Some(SlotSong { key, name: song.name.clone(), midi_filename: file });
    save(&slots);
}

pub fn clear_song(key: &str) {
    let mut slots = store().lock();
    for s in slots.iter_mut() {
        if s.as_ref().is_some_and(|s| s.key == key) {
            *s = None;
        }
    }
    save(&slots);
}

pub fn clear(slot: usize) {
    let mut slots = store().lock();
    if let Some(s) = slots.get_mut(slot) {
        *s = None;
    }
    save(&slots);
}

/// Alt+<n>: stops the MIDI Player and loads the slot's song (downloaded if needed).
pub fn activate(core: &Arc<Core>, slot: usize) {
    let Some(song) = get(slot) else {
        console::insert(format!("Alt+{}: no song assigned (right-click a song in the MIDI Hub)", slot + 1));
        return;
    };
    core.stop(Target::Midi);
    let core = core.clone();
    std::thread::spawn(move || match hub::download_midi(&song.midi_filename) {
        Ok(path) => {
            core.use_file(Target::Midi, &path);
            console::log(format!("Slot {}:", slot + 1));
            console::insert(song.name);
        }
        Err(e) => console::insert(format!("Slot {} failed: {e}", slot + 1)),
    });
}
