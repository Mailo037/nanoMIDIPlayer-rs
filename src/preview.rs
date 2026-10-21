//! Song preview for the MIDI Hub: plays the start of a MIDI through a MIDI output
//! device (on Windows the built-in "Microsoft GS Wavetable Synth") without
//! pressing any keys, so a song can be heard before using it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::hub;
use crate::midi_file::{self, MidiEvent};

/// How much of the song is played.
pub const PREVIEW_SECS: f64 = 30.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewState {
    Loading,
    Playing,
}

#[derive(Default)]
pub struct Preview {
    /// Key of the hub song currently previewed.
    current: Mutex<Option<(String, PreviewState)>>,
    /// Bumped on every start/stop; a playback thread stops once it no longer matches.
    generation: AtomicU64,
}

impl Preview {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn state_of(&self, key: &str) -> Option<PreviewState> {
        self.current.lock().as_ref().filter(|(cur, _)| cur == key).map(|(_, s)| *s)
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        *self.current.lock() = None;
    }

    /// Starts previewing `filename` (stops any running preview). `changed` is bumped
    /// whenever the state changes so the UI can redraw.
    pub fn start(self: &Arc<Self>, key: String, filename: String, changed: Arc<AtomicU64>) {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self.current.lock() = Some((key.clone(), PreviewState::Loading));
        let this = self.clone();
        std::thread::spawn(move || {
            let alive = || this.generation.load(Ordering::Acquire) == generation;
            if let Err(e) = this.play(&filename, &alive, || {
                *this.current.lock() = Some((key.clone(), PreviewState::Playing));
                changed.fetch_add(1, Ordering::Release);
            }) {
                log::warn!("Preview failed: {e}");
                crate::console::insert(format!("Preview failed: {e}"));
            }
            if alive() {
                *this.current.lock() = None;
            }
            changed.fetch_add(1, Ordering::Release);
        });
    }

    fn play(&self, filename: &str, alive: &dyn Fn() -> bool, on_start: impl FnOnce()) -> anyhow::Result<()> {
        // Separate preview cache, so previews don't fill up the song list.
        let path = hub::preview_midi(filename)?;
        let song = midi_file::load(&path)?;
        if !alive() {
            return Ok(());
        }
        let mut conn = open_preview_output()?;
        on_start();

        let begin = Instant::now();
        let mut at = 0.0;
        for ev in &song.events {
            at += ev.delta;
            if at > PREVIEW_SECS {
                break;
            }
            // Sleep in small steps so stopping reacts quickly.
            loop {
                if !alive() {
                    all_notes_off(&mut conn);
                    return Ok(());
                }
                let wait = at - begin.elapsed().as_secs_f64();
                if wait <= 0.0 {
                    break;
                }
                std::thread::sleep(Duration::from_secs_f64(wait.min(0.05)));
            }
            let msg = match ev.event {
                MidiEvent::NoteOn { channel, note, velocity } => [0x90 | channel, note, velocity],
                MidiEvent::NoteOff { channel, note, velocity } => [0x80 | channel, note, velocity],
                MidiEvent::Control { channel, control, value } => [0xB0 | channel, control, value],
                MidiEvent::Other => continue,
            };
            let _ = conn.send(&msg);
        }
        all_notes_off(&mut conn);
        Ok(())
    }
}

/// Prefers the system software synth, falls back to the first output device.
fn open_preview_output() -> anyhow::Result<midir::MidiOutputConnection> {
    let out = midir::MidiOutput::new("nanoMIDIPlayer-preview")?;
    let ports = out.ports();
    let port = ports
        .iter()
        .find(|p| out.port_name(p).is_ok_and(|n| n.to_lowercase().contains("wavetable")))
        .or_else(|| ports.first())
        .ok_or_else(|| anyhow::anyhow!("no MIDI output device found"))?
        .clone();
    out.connect(&port, "nanoMIDIPlayer-preview")
        .map_err(|e| anyhow::anyhow!("could not open MIDI output: {e}"))
}

fn all_notes_off(conn: &mut midir::MidiOutputConnection) {
    for channel in 0..16u8 {
        let _ = conn.send(&[0xB0 | channel, 64, 0]); // sustain off
        let _ = conn.send(&[0xB0 | channel, 123, 0]); // all notes off
    }
}
