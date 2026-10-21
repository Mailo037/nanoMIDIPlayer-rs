//! MIDI → QWERTY translation (piano and drums), ported 1:1 from the original
//! `simulateKey` / `parseMidi` logic but with all lookups precomputed into arrays.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use crate::config::{self, DrumsConfig, MidiPlayerConfig, MidiToQwertyConfig, PianoMap};
use crate::console;
use crate::keys::{Backend, Key, KeySender};
use crate::midi_file::MidiEvent;
use crate::scheduler;

const SHIFTED_SYMBOLS: &[char] = &['!', '@', '$', '%', '^', '*', '('];

fn note_table(map: &std::collections::BTreeMap<String, String>) -> [Option<char>; 128] {
    let mut table = [None; 128];
    for (k, v) in map {
        if let (Ok(n), Some(c)) = (k.trim().parse::<usize>(), v.chars().next()) {
            if n < 128 {
                table[n] = Some(c);
            }
        }
    }
    table
}

pub struct PianoSettings {
    pub backend: Backend,
    pub velocity: bool,
    pub sustain: bool,
    pub no_doubles: bool,
    /// When false only the 61-key map is used (MIDI Player "88 Keys" toggle).
    pub use_88: bool,
    pub sustain_cutoff: f64,
    pub hold: Option<Duration>,
    pub finger_limit: Option<i64>,
    pub offset: i64,
    pub map61: [Option<char>; 128],
    pub low: [Option<char>; 128],
    pub high: [Option<char>; 128],
    pub velocity_thresholds: Vec<(i64, char)>,
}

impl PianoSettings {
    fn from_map(map: &PianoMap) -> ([Option<char>; 128], [Option<char>; 128], [Option<char>; 128], Vec<(i64, char)>) {
        let mut thresholds: Vec<(i64, char)> = map
            .velocity_map
            .iter()
            .filter_map(|(k, v)| Some((k.trim().parse().ok()?, v.chars().next()?)))
            .collect();
        thresholds.sort_by_key(|t| t.0);
        (
            note_table(&map.key_map_61),
            note_table(&map.key_map_88.low_notes),
            note_table(&map.key_map_88.high_notes),
            thresholds,
        )
    }

    pub fn for_player(c: &MidiPlayerConfig) -> Self {
        let (map61, low, high, velocity_thresholds) = Self::from_map(&c.piano_map);
        Self {
            backend: Backend::from_name(&c.input_module),
            velocity: c.velocity,
            sustain: c.sustain,
            no_doubles: c.no_doubles,
            use_88: c.keys_88,
            sustain_cutoff: c.sustain_cutoff,
            hold: c
                .custom_hold_length
                .enabled
                .then(|| Duration::from_secs_f64(c.custom_hold_length.note_length.max(0.0))),
            finger_limit: (c.finger_limit <= 10).then_some(c.finger_limit),
            offset: c.pitch_offset + c.transpose_offset,
            map61,
            low,
            high,
            velocity_thresholds,
        }
    }

    pub fn for_live(c: &MidiToQwertyConfig) -> Self {
        let (map61, low, high, velocity_thresholds) = Self::from_map(&c.piano_map);
        Self {
            backend: Backend::from_name(&c.input_module),
            velocity: c.velocity,
            sustain: c.sustain,
            no_doubles: c.no_doubles,
            // The original live translator always used the full 88-key maps.
            use_88: true,
            sustain_cutoff: c.sustain_cutoff,
            hold: c
                .custom_hold_length
                .enabled
                .then(|| Duration::from_secs_f64(c.custom_hold_length.note_length.max(0.0))),
            finger_limit: None,
            offset: 0,
            map61,
            low,
            high,
            velocity_thresholds,
        }
    }

    /// Same (slightly unusual) binary search as the original `findVelocityKey`.
    fn velocity_key(&self, velocity: u8) -> Option<char> {
        let t = &self.velocity_thresholds;
        if t.is_empty() {
            return None;
        }
        let (mut min, mut max) = (0i64, t.len() as i64 - 1);
        let mut index = 0i64;
        while min <= max {
            index = (min + max) / 2;
            if index == 0 || index == t.len() as i64 - 1 {
                break;
            }
            if t[index as usize].0 < velocity as i64 {
                min = index + 1;
            } else {
                max = index - 1;
            }
        }
        Some(t[index as usize].1)
    }
}

/// Where the settings come from; they are re-derived whenever the config changes.
#[derive(Clone, Copy)]
pub enum PianoSource {
    Player,
    Live,
}

pub struct PianoSim {
    pub sender: Arc<KeySender>,
    source: PianoSource,
    settings: PianoSettings,
    generation: u64,
    held_count: Arc<AtomicI64>,
    pub sustain_active: bool,
    group: u32,
}

impl PianoSim {
    pub fn new(source: PianoSource, sender: Arc<KeySender>, group: u32) -> Self {
        let (settings, generation) = Self::load(source);
        Self {
            sender,
            source,
            settings,
            generation,
            held_count: Arc::new(AtomicI64::new(0)),
            sustain_active: false,
            group,
        }
    }

    fn load(source: PianoSource) -> (PianoSettings, u64) {
        let store = config::get();
        let generation = store.generation();
        let cfg = store.read();
        let settings = match source {
            PianoSource::Player => PianoSettings::for_player(&cfg.midi_player),
            PianoSource::Live => PianoSettings::for_live(&cfg.midi_to_qwerty),
        };
        (settings, generation)
    }

    /// Picks up config changes made while playing (toggles, sliders, transposition).
    pub fn refresh(&mut self) {
        if config::get().generation() != self.generation {
            let (settings, generation) = Self::load(self.source);
            self.settings = settings;
            self.generation = generation;
        }
    }

    pub fn backend(&self) -> Backend {
        self.settings.backend
    }

    fn press(&self, key: Key) {
        self.sender.press(key, self.settings.backend);
    }

    fn release(&self, key: Key) {
        self.sender.release(key, self.settings.backend);
    }

    fn press_and_maybe_release(&self, c: char) {
        if let Some(limit) = self.settings.finger_limit {
            if self.held_count.load(Ordering::Relaxed) >= limit {
                console::log(format!("Finger limit ({limit}) reached, skipping note"));
                return;
            }
        }
        self.held_count.fetch_add(1, Ordering::Relaxed);
        let key = Key::Char(c);
        self.press(key);
        if let Some(hold) = self.settings.hold {
            let sender = self.sender.clone();
            let backend = self.settings.backend;
            let count = self.held_count.clone();
            scheduler::schedule(self.group, hold, move || {
                sender.release(key, backend);
                let _ = count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some((v - 1).max(0)));
            });
        }
    }

    fn lookup(&self, note: i64) -> Option<char> {
        if !(0..128).contains(&note) {
            return None;
        }
        let n = note as usize;
        let s = &self.settings;
        s.map61[n].or_else(|| if s.use_88 { s.low[n].or(s.high[n]) } else { None })
    }

    /// Returns the (possibly transposed) note that was simulated, if any.
    pub fn simulate_key(&self, on: bool, note: u8, velocity: u8) -> Option<i64> {
        let note = note as i64 + self.settings.offset;
        let Some(key) = self.lookup(note) else {
            console::log(format!("out of range: {note}"));
            return None;
        };
        let lower = key.to_ascii_lowercase();
        let in_61 = (36..=96).contains(&note);
        let symbol = SHIFTED_SYMBOLS.contains(&key);
        let base_of_symbol = || self.settings.map61[(note - 1).clamp(0, 127) as usize];

        if on {
            if self.settings.velocity {
                if let Some(vk) = self.settings.velocity_key(velocity) {
                    self.press(Key::Alt);
                    self.press(Key::Char(vk));
                    self.release(Key::Char(vk));
                    self.release(Key::Alt);
                }
            }
            if in_61 {
                if self.settings.no_doubles {
                    if symbol {
                        if let Some(b) = base_of_symbol() {
                            self.release(Key::Char(b));
                        }
                    } else {
                        self.release(Key::Char(lower));
                    }
                }
                if symbol {
                    if let Some(b) = base_of_symbol() {
                        self.press(Key::Shift);
                        self.press_and_maybe_release(b);
                        self.release(Key::Shift);
                    }
                } else if key.is_uppercase() {
                    self.press(Key::Shift);
                    self.press_and_maybe_release(lower);
                    self.release(Key::Shift);
                } else {
                    self.press_and_maybe_release(key);
                }
            } else {
                self.release(Key::Char(lower));
                self.press(Key::Ctrl);
                self.press_and_maybe_release(lower);
                self.release(Key::Ctrl);
            }
        } else {
            if in_61 && symbol {
                if let Some(b) = base_of_symbol() {
                    self.release(Key::Char(b));
                }
            } else {
                self.release(Key::Char(lower));
            }
            let _ = self
                .held_count
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some((v - 1).max(0)));
        }
        Some(note)
    }

    /// Handles one MIDI message (notes + sustain pedal).
    pub fn handle(&mut self, event: MidiEvent) -> Option<(bool, i64, u8)> {
        match event {
            MidiEvent::Control { control: 64, value, .. } if self.settings.sustain => {
                let value = value as f64;
                if !self.sustain_active && value > self.settings.sustain_cutoff {
                    self.sustain_active = true;
                    self.press(Key::Space);
                } else if self.sustain_active && value < self.settings.sustain_cutoff {
                    self.sustain_active = false;
                    self.release(Key::Space);
                }
                None
            }
            MidiEvent::NoteOn { note, velocity, .. } if velocity > 0 => {
                self.simulate_key(true, note, velocity).map(|n| (true, n, velocity))
            }
            MidiEvent::NoteOn { note, .. } | MidiEvent::NoteOff { note, .. } => {
                self.simulate_key(false, note, 0).map(|n| (false, n, 0))
            }
            _ => None,
        }
    }

    pub fn release_all(&mut self) {
        scheduler::clear(self.group);
        self.sender.release_all(self.settings.backend);
        if self.sustain_active {
            self.sustain_active = false;
        }
        self.held_count.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Drums
// ---------------------------------------------------------------------------

/// General-MIDI drum note → config key name (same table as the original).
const DRUM_NOTES: &[(u8, &str)] = &[
    (42, "closed_Hi-Hat"),
    (44, "closed_Hi-Hat2"),
    (46, "open_Hi-Hat"),
    (48, "tom1"),
    (50, "tom1_2"),
    (60, "tom"),
    (62, "tom2_2"),
    (49, "rightCrash"),
    (55, "leftCrash"),
    (38, "snare"),
    (40, "snare2"),
    (37, "snareSide"),
    (35, "kick"),
    (36, "kick2"),
    (51, "ride"),
    (53, "rideBell"),
    (39, "cowbell"),
    (52, "crashChina"),
    (57, "splashCrash"),
    (45, "lowTom"),
    (47, "lowMidTom"),
];

pub struct DrumSettings {
    pub backend: Backend,
    pub hold: Option<Duration>,
    pub finger_limit: Option<i64>,
    pub map: [Option<char>; 128],
}

impl DrumSettings {
    pub fn from_config(c: &DrumsConfig) -> Self {
        let mut map = [None; 128];
        for (note, name) in DRUM_NOTES {
            if let Some(c) = c.drums_map.get(*name).and_then(|k| k.chars().next()) {
                map[*note as usize] = Some(c.to_ascii_lowercase());
            }
        }
        Self {
            backend: Backend::from_name(&c.input_module),
            hold: c
                .custom_hold_length
                .enabled
                .then(|| Duration::from_secs_f64(c.custom_hold_length.note_length.max(0.0))),
            finger_limit: (c.finger_limit <= 10).then_some(c.finger_limit),
            map,
        }
    }
}

pub struct DrumSim {
    pub sender: Arc<KeySender>,
    settings: DrumSettings,
    generation: u64,
    held_count: Arc<AtomicI64>,
    group: u32,
}

impl DrumSim {
    pub fn new(sender: Arc<KeySender>, group: u32) -> Self {
        let store = config::get();
        Self {
            sender,
            generation: store.generation(),
            settings: DrumSettings::from_config(&store.read().drums),
            held_count: Arc::new(AtomicI64::new(0)),
            group,
        }
    }

    pub fn refresh(&mut self) {
        let store = config::get();
        if store.generation() != self.generation {
            self.generation = store.generation();
            self.settings = DrumSettings::from_config(&store.read().drums);
        }
    }

    pub fn backend(&self) -> Backend {
        self.settings.backend
    }

    pub fn handle(&mut self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn { note, velocity, .. } if velocity > 0 => {
                let Some(c) = self.settings.map[note as usize] else { return };
                if let Some(limit) = self.settings.finger_limit {
                    if self.held_count.load(Ordering::Relaxed) >= limit {
                        console::log(format!("Finger limit ({limit}) reached, skipping drum hit"));
                        return;
                    }
                }
                self.held_count.fetch_add(1, Ordering::Relaxed);
                let key = Key::Char(c);
                self.sender.press(key, self.settings.backend);
                if let Some(hold) = self.settings.hold {
                    let sender = self.sender.clone();
                    let backend = self.settings.backend;
                    let count = self.held_count.clone();
                    scheduler::schedule(self.group, hold, move || {
                        sender.release(key, backend);
                        let _ = count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some((v - 1).max(0)));
                    });
                }
            }
            MidiEvent::NoteOn { note, .. } | MidiEvent::NoteOff { note, .. } => {
                let Some(c) = self.settings.map[note as usize] else { return };
                self.sender.release(Key::Char(c), self.settings.backend);
                let _ = self
                    .held_count
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some((v - 1).max(0)));
            }
            _ => {}
        }
    }

    pub fn release_all(&mut self) {
        scheduler::clear(self.group);
        self.sender.release_all(self.settings.backend);
        self.held_count.store(0, Ordering::Relaxed);
    }
}
