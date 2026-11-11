//! Song preview for the MIDI Hub: plays the start of a MIDI without pressing any keys,
//! so a song can be heard before using it. Uses a software MIDI synth when the system
//! has one (Windows: "Microsoft GS Wavetable Synth", Linux: FluidSynth / TiMidity),
//! otherwise the app's own small synth through the default audio output.

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
        let mut conn = Output::open()?;
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
            conn.send(&msg);
        }
        all_notes_off(&mut conn);
        Ok(())
    }
}

enum Output {
    Midi(midir::MidiOutputConnection),
    Synth(crate::synth::Synth),
}

impl Output {
    /// A system software synth if there is one, else the built-in synth, else any
    /// real MIDI output device ("Midi Through" ports are skipped: they make no sound).
    fn open() -> anyhow::Result<Output> {
        let out = midir::MidiOutput::new("nanoMIDIPlayer-preview")?;
        let ports: Vec<(midir::MidiOutputPort, String)> = out
            .ports()
            .into_iter()
            .filter_map(|p| {
                let name = out.port_name(&p).ok()?.to_lowercase();
                (!name.contains("through")).then_some((p, name))
            })
            .collect();
        let software = ports
            .iter()
            .find(|(_, n)| ["wavetable", "fluid", "timidity", "synth"].iter().any(|s| n.contains(s)));
        if let Some((port, name)) = software {
            match out.connect(port, "nanoMIDIPlayer-preview") {
                Ok(conn) => return Ok(Output::Midi(conn)),
                Err(e) => log::warn!("could not open {name}: {e}"),
            }
            // `connect` consumed `out`; the built-in synth is the fallback.
            return crate::synth::Synth::open().map(Output::Synth);
        }
        let synth_err = match crate::synth::Synth::open() {
            Ok(s) => return Ok(Output::Synth(s)),
            Err(e) => e,
        };
        log::warn!("built-in preview synth failed: {synth_err}");
        let (port, _) = ports.first().ok_or_else(|| anyhow::anyhow!("no audio or MIDI output found ({synth_err})"))?;
        out.connect(port, "nanoMIDIPlayer-preview")
            .map(Output::Midi)
            .map_err(|e| anyhow::anyhow!("could not open MIDI output: {e}"))
    }

    fn send(&mut self, msg: &[u8]) {
        match self {
            Output::Midi(c) => {
                let _ = c.send(msg);
            }
            Output::Synth(s) => s.send(msg),
        }
    }
}

fn all_notes_off(conn: &mut Output) {
    for channel in 0..16u8 {
        conn.send(&[0xB0 | channel, 64, 0]); // sustain off
        conn.send(&[0xB0 | channel, 123, 0]); // all notes off
    }
    if matches!(conn, Output::Synth(_)) {
        // Let the notes fade out instead of cutting the audio with a click.
        std::thread::sleep(Duration::from_millis(150));
    }
}
