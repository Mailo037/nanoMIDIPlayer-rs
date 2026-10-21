//! Persistent configuration, stored in `Documents/nanoMIDIPlayer/config.json`.
//!
//! The file format is identical to the original Python nanoMIDIPlayer so existing
//! configs keep working. Missing keys are filled from the embedded default config.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

pub const DEFAULT_CONFIG: &str = include_str!("../assets/defaultConfig.json");

pub fn base_dir() -> PathBuf {
    let docs = dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = docs.join("nanoMIDIPlayer");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn midis_dir() -> PathBuf {
    let dir = base_dir().join("Midis");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn assets_dir() -> PathBuf {
    let dir = base_dir().join("assets");
    let _ = std::fs::create_dir_all(dir.join("customThemes"));
    dir
}

fn de_i64<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_i64()
        .or_else(|| v.as_f64().map(|f| f.round() as i64))
        .unwrap_or(0))
}

fn de_f64<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HoldLength {
    pub enabled: bool,
    #[serde(rename = "noteLength", deserialize_with = "de_f64")]
    pub note_length: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RandomFail {
    pub enabled: bool,
    #[serde(deserialize_with = "de_f64")]
    pub speed: f64,
    #[serde(default, deserialize_with = "de_f64", skip_serializing_if = "is_neg")]
    pub transpose: f64,
}

fn is_neg(v: &f64) -> bool {
    *v < 0.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyMap88 {
    #[serde(rename = "lowNotes")]
    pub low_notes: BTreeMap<String, String>,
    #[serde(rename = "highNotes")]
    pub high_notes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PianoMap {
    #[serde(rename = "88keyMap")]
    pub key_map_88: KeyMap88,
    #[serde(rename = "61keyMap")]
    pub key_map_61: BTreeMap<String, String>,
    #[serde(rename = "velocityMap")]
    pub velocity_map: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MidiPlayerConfig {
    #[serde(rename = "currentFile")]
    pub current_file: String,
    #[serde(rename = "midiList")]
    pub midi_list: Vec<String>,
    #[serde(rename = "inputModule")]
    pub input_module: String,
    #[serde(rename = "loopSong")]
    pub loop_song: bool,
    #[serde(rename = "releaseOnPause")]
    pub release_on_pause: bool,
    #[serde(rename = "useMIDIOutput")]
    pub use_midi_output: bool,
    pub velocity: bool,
    pub sustain: bool,
    #[serde(rename = "noDoubles")]
    pub no_doubles: bool,
    #[serde(rename = "88Keys")]
    pub keys_88: bool,
    #[serde(rename = "sustainCutoff", deserialize_with = "de_f64")]
    pub sustain_cutoff: f64,
    #[serde(rename = "decreaseSize", deserialize_with = "de_f64")]
    pub decrease_size: f64,
    #[serde(rename = "customHoldLength")]
    pub custom_hold_length: HoldLength,
    #[serde(rename = "fingerLimit", deserialize_with = "de_i64")]
    pub finger_limit: i64,
    #[serde(rename = "randomFail")]
    pub random_fail: RandomFail,
    #[serde(rename = "pianoMap")]
    pub piano_map: PianoMap,
    #[serde(rename = "pitchOffset", default, deserialize_with = "de_i64")]
    pub pitch_offset: i64,
    #[serde(rename = "transposeOffset", default, deserialize_with = "de_i64")]
    pub transpose_offset: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrumsConfig {
    #[serde(rename = "currentFile")]
    pub current_file: String,
    #[serde(rename = "midiList")]
    pub midi_list: Vec<String>,
    #[serde(rename = "inputModule")]
    pub input_module: String,
    #[serde(rename = "loopSong")]
    pub loop_song: bool,
    #[serde(rename = "decreaseSize", deserialize_with = "de_f64")]
    pub decrease_size: f64,
    #[serde(rename = "releaseOnPause")]
    pub release_on_pause: bool,
    #[serde(rename = "customHoldLength")]
    pub custom_hold_length: HoldLength,
    #[serde(rename = "fingerLimit", deserialize_with = "de_i64")]
    pub finger_limit: i64,
    #[serde(rename = "randomFail")]
    pub random_fail: RandomFail,
    #[serde(rename = "drumsMap")]
    pub drums_map: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MidiToQwertyConfig {
    #[serde(rename = "inputModule")]
    pub input_module: String,
    pub velocity: bool,
    pub sustain: bool,
    #[serde(rename = "noDoubles")]
    pub no_doubles: bool,
    #[serde(rename = "88Keys")]
    pub keys_88: bool,
    #[serde(rename = "sustainCutoff", deserialize_with = "de_f64")]
    pub sustain_cutoff: f64,
    #[serde(rename = "customHoldLength")]
    pub custom_hold_length: HoldLength,
    #[serde(rename = "pianoMap")]
    pub piano_map: PianoMap,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hotkeys {
    pub play: String,
    pub pause: String,
    pub stop: String,
    /// Named "speedup" for config compatibility; in the UI this key is labelled "Slow Down"
    /// and decreases the speed, exactly like the original app.
    pub speedup: String,
    /// Labelled "Speed Up" in the UI (increases the speed).
    pub slowdown: String,
}

impl Hotkeys {
    pub fn get(&self, slot: HotkeySlot) -> &str {
        match slot {
            HotkeySlot::Play => &self.play,
            HotkeySlot::Pause => &self.pause,
            HotkeySlot::Stop => &self.stop,
            HotkeySlot::SpeedUp => &self.speedup,
            HotkeySlot::SlowDown => &self.slowdown,
        }
    }

    pub fn set(&mut self, slot: HotkeySlot, value: String) {
        match slot {
            HotkeySlot::Play => self.play = value,
            HotkeySlot::Pause => self.pause = value,
            HotkeySlot::Stop => self.stop = value,
            HotkeySlot::SpeedUp => self.speedup = value,
            HotkeySlot::SlowDown => self.slowdown = value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HotkeySlot {
    Play,
    Pause,
    Stop,
    SpeedUp,
    SlowDown,
}

impl HotkeySlot {
    pub const ALL: [HotkeySlot; 5] = [
        HotkeySlot::Play,
        HotkeySlot::Pause,
        HotkeySlot::Stop,
        HotkeySlot::SpeedUp,
        HotkeySlot::SlowDown,
    ];

    pub fn config_name(self) -> &'static str {
        match self {
            HotkeySlot::Play => "play",
            HotkeySlot::Pause => "pause",
            HotkeySlot::Stop => "stop",
            HotkeySlot::SpeedUp => "speedup",
            HotkeySlot::SlowDown => "slowdown",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppUi {
    pub topmost: bool,
    pub console: bool,
    #[serde(rename = "forceTheme")]
    pub force_theme: bool,
    pub tooltip: bool,
    pub timestamp: bool,
    #[serde(rename = "checkForUpdates")]
    pub check_for_updates: bool,
    /// Download and install a found update right away (see `update.rs`).
    #[serde(rename = "autoUpdate", default)]
    pub auto_update: bool,
    #[serde(rename = "sendTelemetry")]
    pub send_telemetry: bool,
    /// Keep every song downloaded from the MIDI Hub in `Midis`. When off, only the
    /// most recently played downloads are kept (see `hub::RECENT_LIMIT`).
    #[serde(rename = "keepDownloads", default)]
    pub keep_downloads: bool,
    /// Pause keyboard playback while another window than the game is focused.
    #[serde(rename = "pauseOnFocusLoss", default = "default_true")]
    pub pause_on_focus_loss: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(rename = "midiPlayer")]
    pub midi_player: MidiPlayerConfig,
    #[serde(rename = "drumsMacro")]
    pub drums: DrumsConfig,
    #[serde(rename = "midiToQwerty")]
    pub midi_to_qwerty: MidiToQwertyConfig,
    pub hotkeys: Hotkeys,
    #[serde(rename = "appUI")]
    pub app_ui: AppUi,
    /// Unknown keys are preserved so the file stays compatible with other versions.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Adds every key from `defaults` that is missing in `target` (recursively).
fn deep_merge(target: &mut Value, defaults: &Value) {
    if let (Value::Object(t), Value::Object(d)) = (target, defaults) {
        for (k, dv) in d {
            match t.get_mut(k) {
                None => {
                    t.insert(k.clone(), dv.clone());
                }
                Some(tv) => {
                    if tv.is_object() && dv.is_object() {
                        deep_merge(tv, dv);
                    }
                }
            }
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        base_dir().join("config.json")
    }

    pub fn defaults() -> Config {
        serde_json::from_str(DEFAULT_CONFIG).expect("embedded default config is valid")
    }

    pub fn load() -> Config {
        let defaults: Value = serde_json::from_str(DEFAULT_CONFIG).expect("valid default config");
        let path = Self::path();
        let mut value = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Value>(&text).unwrap_or_else(|e| {
                log::warn!("config.json is corrupted ({e}), restoring defaults");
                defaults.clone()
            }),
            Err(_) => defaults.clone(),
        };
        deep_merge(&mut value, &defaults);
        let config = serde_json::from_value::<Config>(value).unwrap_or_else(|e| {
            log::warn!("config.json has invalid values ({e}), restoring defaults");
            Self::defaults()
        });
        config.save_now();
        config
    }

    pub fn save_now(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(Self::path(), text) {
                    log::error!("failed to save config: {e}");
                }
            }
            Err(e) => log::error!("failed to serialize config: {e}"),
        }
    }
}

/// Shared, thread-safe configuration with debounced saving.
pub struct ConfigStore {
    data: RwLock<Config>,
    dirty: AtomicBool,
    generation: AtomicU64,
}

impl ConfigStore {
    pub fn new(config: Config) -> Arc<Self> {
        let store = Arc::new(Self {
            data: RwLock::new(config),
            dirty: AtomicBool::new(false),
            generation: AtomicU64::new(1),
        });
        let weak = Arc::downgrade(&store);
        std::thread::Builder::new()
            .name("config-saver".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(400));
                let Some(store) = weak.upgrade() else { break };
                if store.dirty.swap(false, Ordering::AcqRel) {
                    let snapshot = store.data.read().clone();
                    snapshot.save_now();
                }
            })
            .ok();
        store
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, Config> {
        self.data.read()
    }

    /// Mutates the config and schedules a save.
    pub fn update<R>(&self, f: impl FnOnce(&mut Config) -> R) -> R {
        let r = f(&mut self.data.write());
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Release);
        r
    }

    /// Increments on every change; lets hot paths cache derived settings cheaply.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn flush(&self) {
        if self.dirty.swap(false, Ordering::AcqRel) {
            self.data.read().save_now();
        }
    }
}

static STORE: OnceLock<Arc<ConfigStore>> = OnceLock::new();

pub fn init() -> Arc<ConfigStore> {
    STORE.get_or_init(|| ConfigStore::new(Config::load())).clone()
}

pub fn get() -> &'static ConfigStore {
    STORE.get().expect("config initialized")
}
