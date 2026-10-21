//! Playback engine for the MIDI Player (QWERTY or MIDI output) and the Drums Player.
//!
//! Each playback runs on one dedicated high-priority thread. Event times are
//! accumulated from absolute targets (no drift) and the thread sleeps until shortly
//! before an event, then spins for sub-millisecond accuracy.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::Rng;

use crate::config;
use crate::console;
use crate::keys::{Key, KeySender};
use crate::midi_file::{self, MidiEvent, MidiSong};
use crate::simulate::{DrumSim, PianoSim, PianoSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Qwerty,
    MidiOut,
    Drums,
}

#[derive(Clone, Copy, Debug, Default)]
struct Clock {
    song_pos: f64,
    wall: Option<Instant>,
    speed: f64,
    paused: bool,
    length: f64,
}

pub struct Player {
    pub mode_drums: bool,
    running: AtomicBool,
    paused: AtomicBool,
    stop: AtomicBool,
    speed_bits: AtomicU64,
    version: AtomicU64,
    clock: Mutex<Clock>,
    thread: Mutex<Option<JoinHandle<()>>>,
    sender: Arc<KeySender>,
    group: u32,
}

impl Player {
    pub fn new(mode_drums: bool, group: u32) -> Arc<Self> {
        Arc::new(Self {
            mode_drums,
            running: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            speed_bits: AtomicU64::new(1.0f64.to_bits()),
            version: AtomicU64::new(0),
            clock: Mutex::new(Clock::default()),
            thread: Mutex::new(None),
            sender: KeySender::new(true),
            group,
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    /// Incremented whenever the play state changes (UI uses it to re-render).
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    fn bump(&self) {
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    pub fn speed(&self) -> f64 {
        f64::from_bits(self.speed_bits.load(Ordering::Acquire))
    }

    /// Sets the speed from a percentage (slider / entry), clamped to 1%..500%.
    pub fn set_speed_percent(&self, percent: f64) -> f64 {
        let speed = (percent / 100.0).clamp(0.01, 5.0);
        self.store_speed(speed);
        speed
    }

    /// Hotkey speed step, clamped to 10%..500% like the original.
    pub fn change_speed(&self, amount: f64) -> f64 {
        let speed = (self.speed() + amount).clamp(0.1, 5.0);
        self.store_speed(speed);
        console::log(format!("Speed: {:.0}%", speed * 100.0));
        speed
    }

    fn store_speed(&self, speed: f64) {
        self.speed_bits.store(speed.to_bits(), Ordering::Release);
        let mut clock = self.clock.lock();
        if let Some(wall) = clock.wall {
            if !clock.paused {
                let now = Instant::now();
                clock.song_pos += now.duration_since(wall).as_secs_f64() * clock.speed;
                clock.wall = Some(now);
            }
        }
        clock.speed = speed;
        drop(clock);
        self.bump();
    }

    /// Current position and length in seconds (for the timeline label).
    pub fn position(&self) -> (f64, f64) {
        let clock = *self.clock.lock();
        let mut pos = clock.song_pos;
        if let (Some(wall), false) = (clock.wall, clock.paused) {
            pos += wall.elapsed().as_secs_f64() * clock.speed;
        }
        (pos.min(clock.length).max(0.0), clock.length)
    }

    pub fn toggle_pause(&self) -> bool {
        let paused = !self.paused.load(Ordering::Acquire);
        self.paused.store(paused, Ordering::Release);
        {
            let mut clock = self.clock.lock();
            let now = Instant::now();
            if paused {
                if let Some(wall) = clock.wall {
                    clock.song_pos += now.duration_since(wall).as_secs_f64() * clock.speed;
                }
            }
            clock.wall = Some(now);
            clock.paused = paused;
        }
        console::log(if paused { "Playback paused." } else { "Playback resumed." });
        self.bump();
        paused
    }

    pub fn start(self: &Arc<Self>, path: PathBuf, mode: Mode, output_device: Option<String>) -> anyhow::Result<()> {
        if self.is_running() {
            return Ok(());
        }
        if let Some(handle) = self.thread.lock().take() {
            let _ = handle.join();
        }
        let song = midi_file::load(&path)?;
        let out = match (mode, output_device) {
            (Mode::MidiOut, Some(name)) => Some(open_output(&name)?),
            (Mode::MidiOut, None) => anyhow::bail!("No MIDI output device selected."),
            _ => None,
        };

        self.stop.store(false, Ordering::Release);
        self.paused.store(false, Ordering::Release);
        self.running.store(true, Ordering::Release);
        *self.clock.lock() = Clock {
            song_pos: 0.0,
            wall: Some(Instant::now()),
            speed: self.speed(),
            paused: false,
            length: song.length,
        };
        self.bump();

        let this = self.clone();
        let handle = std::thread::Builder::new()
            .name("playback".into())
            .spawn(move || {
                platform::boost_thread();
                match mode {
                    Mode::Qwerty => {
                        console::log("nanoMIDI Mid2VK Translator v3.0");
                        console::log(format!("Playing MIDI file: {}", path.display()));
                    }
                    Mode::MidiOut => {
                        console::log("nanoMIDI Direct MIDI Out v1.0");
                        console::log(format!("Playing MIDI file: {}", path.display()));
                    }
                    Mode::Drums => {}
                }
                let mut sink = match mode {
                    Mode::Qwerty => Sink::Piano(PianoSim::new(PianoSource::Player, this.sender.clone(), this.group), HashMap::new()),
                    Mode::Drums => Sink::Drums(DrumSim::new(this.sender.clone(), this.group)),
                    Mode::MidiOut => Sink::Out(OutSink::new(out.expect("output opened"))),
                };
                this.run(&song, &mut sink);
                sink.release_all();
                if let Sink::Out(out) = sink {
                    out.close();
                }
                this.running.store(false, Ordering::Release);
                this.paused.store(false, Ordering::Release);
                {
                    let mut clock = this.clock.lock();
                    clock.wall = None;
                    clock.song_pos = 0.0;
                }
                if !this.mode_drums {
                    console::log("Playback fully stopped.");
                }
                this.bump();
                platform::end_boost();
            })?;
        *self.thread.lock() = Some(handle);
        Ok(())
    }

    pub fn stop(&self) {
        if !self.is_running() {
            return;
        }
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.lock().take() {
            let _ = handle.join();
        }
    }

    fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn loop_song(&self) -> bool {
        let cfg = config::get().read();
        if self.mode_drums { cfg.drums.loop_song } else { cfg.midi_player.loop_song }
    }

    fn random_fail(&self) -> (bool, f64, f64) {
        let cfg = config::get().read();
        let rf = if self.mode_drums { &cfg.drums.random_fail } else { &cfg.midi_player.random_fail };
        (rf.enabled, rf.speed / 100.0, rf.transpose / 100.0)
    }

    fn release_on_pause(&self) -> bool {
        let cfg = config::get().read();
        if self.mode_drums { cfg.drums.release_on_pause } else { cfg.midi_player.release_on_pause }
    }

    fn run(&self, song: &MidiSong, sink: &mut Sink) {
        let mut rng = rand::rng();
        loop {
            if !self.play_once(song, sink, &mut rng) {
                return;
            }
            if !self.loop_song() || self.should_stop() {
                return;
            }
            sink.release_all();
            let mut clock = self.clock.lock();
            clock.song_pos = 0.0;
            clock.wall = Some(Instant::now());
        }
    }

    /// Plays the song once. Returns false if stopped.
    fn play_once(&self, song: &MidiSong, sink: &mut Sink, rng: &mut impl Rng) -> bool {
        let mut target = Instant::now();
        let mut song_pos = 0.0;
        let mut was_paused = false;
        for ev in &song.events {
            if self.should_stop() {
                return false;
            }
            let (fail_enabled, speed_fail, transpose_fail) = self.random_fail();
            let mut delay = ev.delta / self.speed();
            if fail_enabled && !ev.event.is_meta() && rng.random::<f64>() < speed_fail {
                delay *= rng.random_range(0.5..1.5);
            }
            target += Duration::from_secs_f64(delay.max(0.0));
            song_pos += ev.delta;

            // Wait for the event, handling pause / stop.
            loop {
                if self.should_stop() {
                    return false;
                }
                if self.is_paused() {
                    if !was_paused {
                        was_paused = true;
                        if self.release_on_pause() {
                            sink.release_on_pause();
                        }
                    }
                    let start = Instant::now();
                    std::thread::sleep(Duration::from_millis(5));
                    target += start.elapsed();
                    continue;
                }
                was_paused = false;
                let now = Instant::now();
                if now >= target {
                    break;
                }
                let remaining = target - now;
                if remaining > Duration::from_micros(1500) {
                    std::thread::sleep((remaining - Duration::from_millis(1)).min(Duration::from_millis(8)));
                } else {
                    std::hint::spin_loop();
                }
            }

            if ev.event.is_meta() {
                continue;
            }
            {
                let mut clock = self.clock.lock();
                clock.song_pos = song_pos;
                clock.wall = Some(Instant::now());
            }
            sink.handle(ev.event, fail_enabled, transpose_fail, rng);
        }
        true
    }
}

enum Sink {
    Piano(PianoSim, HashMap<u8, Vec<u8>>),
    Drums(DrumSim),
    Out(OutSink),
}

impl Sink {
    fn handle(&mut self, event: MidiEvent, fail: bool, transpose_fail: f64, rng: &mut impl Rng) {
        match self {
            Sink::Piano(sim, transposed) => {
                sim.refresh();
                if let Some(ev) = random_transpose(event, transposed, fail, transpose_fail, rng, |_| true) {
                    sim.handle(ev);
                }
            }
            Sink::Drums(sim) => {
                sim.refresh();
                sim.handle(event);
            }
            Sink::Out(out) => out.handle(event, fail, transpose_fail, rng),
        }
    }

    fn release_on_pause(&mut self) {
        match self {
            Sink::Piano(sim, _) => {
                let backend = sim.backend();
                sim.sender.release_all(backend);
                if sim.sustain_active {
                    sim.sender.release(Key::Space, backend);
                    sim.sustain_active = false;
                }
            }
            Sink::Drums(sim) => {
                let backend = sim.backend();
                sim.sender.release_all(backend);
            }
            Sink::Out(out) => out.release_notes(),
        }
    }

    fn release_all(&mut self) {
        match self {
            Sink::Piano(sim, transposed) => {
                transposed.clear();
                sim.release_all();
            }
            Sink::Drums(sim) => sim.release_all(),
            Sink::Out(out) => out.release_notes(),
        }
    }
}

/// "Random Fail → Transpose": shifts a note-on by ±12 semitones and remembers the
/// shift so the matching note-off releases the right key.
fn random_transpose(
    event: MidiEvent,
    transposed: &mut HashMap<u8, Vec<u8>>,
    fail: bool,
    chance: f64,
    rng: &mut impl Rng,
    allowed: impl Fn(u8) -> bool,
) -> Option<MidiEvent> {
    match event {
        MidiEvent::NoteOn { channel, note, velocity } if velocity > 0 => {
            if fail && rng.random::<f64>() < chance {
                let new = note as i32 + rng.random_range(-12..=12);
                let new = new.clamp(0, 127) as u8;
                if !allowed(new) {
                    console::log(format!("out of range: {new}"));
                    return None;
                }
                transposed.entry(note).or_default().push(new);
                return Some(MidiEvent::NoteOn { channel, note: new, velocity });
            }
            Some(event)
        }
        MidiEvent::NoteOn { channel, note, velocity } | MidiEvent::NoteOff { channel, note, velocity } => {
            if let Some(list) = transposed.get_mut(&note) {
                if !list.is_empty() {
                    let new = list.remove(0);
                    if list.is_empty() {
                        transposed.remove(&note);
                    }
                    return Some(MidiEvent::NoteOff { channel, note: new, velocity });
                }
            }
            Some(event)
        }
        other => Some(other),
    }
}

// ---------------------------------------------------------------------------
// Direct MIDI output
// ---------------------------------------------------------------------------

fn open_output(name: &str) -> anyhow::Result<midir::MidiOutputConnection> {
    let out = midir::MidiOutput::new("nanoMIDIPlayer")?;
    let port = out
        .ports()
        .into_iter()
        .find(|p| out.port_name(p).map(|n| n == name).unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("MIDI output device not found: {name}"))?;
    out.connect(&port, "nanoMIDIPlayer-out")
        .map_err(|e| anyhow::anyhow!("could not open {name}: {e}"))
}

struct OutSink {
    conn: midir::MidiOutputConnection,
    active: HashSet<(u8, u8)>,
    transposed: HashMap<u8, Vec<u8>>,
    sustain_active: bool,
    generation: u64,
    settings: OutSettings,
}

struct OutSettings {
    sustain: bool,
    sustain_cutoff: f64,
    velocity: bool,
    no_doubles: bool,
    offset: i64,
    allowed: [bool; 128],
}

impl OutSettings {
    fn load() -> (Self, u64) {
        let store = config::get();
        let generation = store.generation();
        let cfg = store.read();
        let c = &cfg.midi_player;
        let mut allowed = [false; 128];
        let mut mark = |map: &std::collections::BTreeMap<String, String>| {
            for k in map.keys() {
                if let Ok(n) = k.trim().parse::<usize>() {
                    if n < 128 {
                        allowed[n] = true;
                    }
                }
            }
        };
        mark(&c.piano_map.key_map_61);
        if c.keys_88 {
            mark(&c.piano_map.key_map_88.low_notes);
            mark(&c.piano_map.key_map_88.high_notes);
        }
        (
            Self {
                sustain: c.sustain,
                sustain_cutoff: c.sustain_cutoff,
                velocity: c.velocity,
                no_doubles: c.no_doubles,
                offset: c.pitch_offset + c.transpose_offset,
                allowed,
            },
            generation,
        )
    }
}

impl OutSink {
    fn new(conn: midir::MidiOutputConnection) -> Self {
        let (settings, generation) = OutSettings::load();
        Self {
            conn,
            active: HashSet::new(),
            transposed: HashMap::new(),
            sustain_active: false,
            generation,
            settings,
        }
    }

    fn handle(&mut self, event: MidiEvent, fail: bool, transpose_fail: f64, rng: &mut impl Rng) {
        if config::get().generation() != self.generation {
            let (settings, generation) = OutSettings::load();
            self.settings = settings;
            self.generation = generation;
        }
        let allowed = self.settings.allowed;
        let Some(event) = random_transpose(event, &mut self.transposed, fail, transpose_fail, rng, |n| {
            allowed[n as usize]
        }) else {
            return;
        };
        match event {
            MidiEvent::Control { channel, control: 64, value } => {
                if !self.settings.sustain {
                    return;
                }
                console::log(format!("control_change channel={channel} control=64 value={value}"));
                self.sustain_active = value as f64 > self.settings.sustain_cutoff;
                let _ = self.conn.send(&[0xB0 | channel, 64, value]);
            }
            MidiEvent::NoteOn { channel, note, velocity } | MidiEvent::NoteOff { channel, note, velocity } => {
                let on = matches!(event, MidiEvent::NoteOn { .. }) && velocity > 0;
                let n = note as i64 + self.settings.offset;
                if !(0..128).contains(&n) || !self.settings.allowed[n as usize] {
                    console::log(format!("out of range: {n}"));
                    return;
                }
                let n = n as u8;
                let key = (n, channel);
                if on {
                    if self.settings.no_doubles && self.active.contains(&key) {
                        console::log(format!("skipped double: note {n} ch {channel}"));
                        return;
                    }
                    let vel = if self.settings.velocity { velocity } else { 78 };
                    console::log(format!("note_on channel={channel} note={n} velocity={vel}"));
                    self.active.insert(key);
                    let _ = self.conn.send(&[0x90 | channel, n, vel]);
                } else {
                    console::log(format!("note_off channel={channel} note={n} velocity=0"));
                    self.active.remove(&key);
                    let _ = self.conn.send(&[0x80 | channel, n, 0]);
                }
            }
            _ => {}
        }
    }

    fn release_notes(&mut self) {
        for (note, channel) in self.active.drain() {
            let _ = self.conn.send(&[0x80 | channel, note, 0]);
        }
        if self.sustain_active {
            let _ = self.conn.send(&[0xB0, 64, 0]);
            self.sustain_active = false;
        }
    }

    fn close(mut self) {
        self.release_notes();
        self.conn.close();
    }
}

// ---------------------------------------------------------------------------
// Timer resolution / thread priority
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};

    pub fn boost_thread() {
        unsafe {
            timeBeginPeriod(1);
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        }
    }

    pub fn end_boost() {
        unsafe {
            timeEndPeriod(1);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn boost_thread() {}
    pub fn end_boost() {}
}
