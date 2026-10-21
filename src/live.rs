//! MIDI To QWERTY: live MIDI input device → key presses, plus the on-screen piano state.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::console;
use crate::keys::KeySender;
use crate::midi_file::MidiEvent;
use crate::simulate::{PianoSim, PianoSource};

/// Velocities of the keys currently shown as held on the piano widget.
pub struct PianoDisplay {
    keys: [AtomicU8; 128],
    version: AtomicU64,
}

impl PianoDisplay {
    const fn new() -> Self {
        Self {
            keys: [const { AtomicU8::new(0) }; 128],
            version: AtomicU64::new(0),
        }
    }

    pub fn down(&self, note: i64, velocity: u8) {
        if (0..128).contains(&note) {
            self.keys[note as usize].store(velocity.max(1), Ordering::Relaxed);
            self.version.fetch_add(1, Ordering::Release);
        }
    }

    pub fn up(&self, note: i64) {
        if (0..128).contains(&note) {
            self.keys[note as usize].store(0, Ordering::Relaxed);
            self.version.fetch_add(1, Ordering::Release);
        }
    }

    pub fn is_down(&self, note: u8) -> bool {
        self.keys[note as usize].load(Ordering::Relaxed) > 0
    }

    pub fn clear(&self) {
        for k in &self.keys {
            k.store(0, Ordering::Relaxed);
        }
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }
}

pub static PIANO: PianoDisplay = PianoDisplay::new();

pub fn output_devices() -> Vec<String> {
    midir::MidiOutput::new("nanoMIDIPlayer-list")
        .map(|out| out.ports().iter().filter_map(|p| out.port_name(p).ok()).collect())
        .unwrap_or_default()
}

pub fn input_devices() -> Vec<String> {
    midir::MidiInput::new("nanoMIDIPlayer-list")
        .map(|inp| inp.ports().iter().filter_map(|p| inp.port_name(p).ok()).collect())
        .unwrap_or_default()
}

fn parse_message(bytes: &[u8]) -> Option<MidiEvent> {
    let status = *bytes.first()?;
    let channel = status & 0x0F;
    let d1 = bytes.get(1).copied().unwrap_or(0) & 0x7F;
    let d2 = bytes.get(2).copied().unwrap_or(0) & 0x7F;
    Some(match status & 0xF0 {
        0x90 => MidiEvent::NoteOn { channel, note: d1, velocity: d2 },
        0x80 => MidiEvent::NoteOff { channel, note: d1, velocity: d2 },
        0xB0 => MidiEvent::Control { channel, control: d1, value: d2 },
        _ => return None,
    })
}

pub struct LiveInput {
    conn: Mutex<Option<midir::MidiInputConnection<()>>>,
    sim: Arc<Mutex<PianoSim>>,
    version: AtomicU64,
}

impl LiveInput {
    pub fn new(group: u32) -> Arc<Self> {
        Arc::new(Self {
            conn: Mutex::new(None),
            sim: Arc::new(Mutex::new(PianoSim::new(PianoSource::Live, KeySender::new(true), group))),
            version: AtomicU64::new(0),
        })
    }

    pub fn is_running(&self) -> bool {
        self.conn.lock().is_some()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn start(&self, port_name: &str) -> anyhow::Result<()> {
        if self.is_running() {
            return Ok(());
        }
        console::log("nanoMIDI Mid2VK Translator v1.0 (Live Input)");
        let mut input = midir::MidiInput::new("nanoMIDIPlayer")?;
        input.ignore(midir::Ignore::All);
        let port = input
            .ports()
            .into_iter()
            .find(|p| input.port_name(p).map(|n| n == port_name).unwrap_or(false))
            .ok_or_else(|| anyhow::anyhow!("Could not open MIDI input: {port_name} not found"))?;
        let sim = self.sim.clone();
        sim.lock().refresh();
        let conn = input
            .connect(
                &port,
                "nanoMIDIPlayer-in",
                move |_, bytes, _| {
                    let Some(event) = parse_message(bytes) else { return };
                    let mut sim = sim.lock();
                    sim.refresh();
                    if let MidiEvent::NoteOn { note, .. } | MidiEvent::NoteOff { note, .. } = event {
                        if !(-15..=88).contains(&(note as i64 - 36)) {
                            console::log(format!("out of range: {note}"));
                            return;
                        }
                    }
                    match sim.handle(event) {
                        Some((true, n, vel)) => PIANO.down(n, vel),
                        Some((false, n, _)) => PIANO.up(n),
                        None => {}
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("Could not open MIDI input: {e}"))?;
        *self.conn.lock() = Some(conn);
        self.version.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub fn stop(&self) {
        let Some(conn) = self.conn.lock().take() else { return };
        conn.close();
        self.sim.lock().release_all();
        PIANO.clear();
        console::log("MIDI input stopped.");
        self.version.fetch_add(1, Ordering::Release);
    }
}
