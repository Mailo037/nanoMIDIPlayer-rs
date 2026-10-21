//! MIDI file loading. All tracks are merged into one time-ordered event list with
//! delta times already converted to seconds (tempo map applied), mirroring how
//! `mido.MidiFile` iterates a file. Parsing happens once per file, so playback
//! itself does no tick/tempo math.

use std::path::Path;
use std::sync::Arc;

use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiEvent {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    Control { channel: u8, control: u8, value: u8 },
    /// Meta / sysex / other messages only contribute timing.
    Other,
}

impl MidiEvent {
    pub fn is_meta(&self) -> bool {
        matches!(self, MidiEvent::Other)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TimedEvent {
    /// Seconds since the previous event (at 100% speed).
    pub delta: f64,
    pub event: MidiEvent,
}

#[derive(Debug)]
pub struct MidiSong {
    pub events: Vec<TimedEvent>,
    /// Total length in seconds.
    pub length: f64,
}

pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Arc<MidiSong>> {
    let bytes = std::fs::read(path.as_ref())?;
    parse(&bytes).map(Arc::new)
}

pub fn parse(bytes: &[u8]) -> anyhow::Result<MidiSong> {
    let smf = Smf::parse(bytes).map_err(|e| anyhow::anyhow!("invalid MIDI file: {e}"))?;

    // (absolute tick, track index, index in track, kind)
    let mut merged: Vec<(u64, usize, usize, TrackEventKind)> = Vec::new();
    for (track_idx, track) in smf.tracks.iter().enumerate() {
        let mut tick: u64 = 0;
        for (i, ev) in track.iter().enumerate() {
            tick += ev.delta.as_int() as u64;
            merged.push((tick, track_idx, i, ev.kind));
        }
    }
    // Stable sort keeps file order for simultaneous events (same as mido's merge).
    merged.sort_by_key(|&(tick, ..)| tick);

    let mut tempo: f64 = 500_000.0; // microseconds per beat
    let seconds_per_tick = |tempo: f64| -> f64 {
        match smf.header.timing {
            Timing::Metrical(tpb) => tempo / 1_000_000.0 / (tpb.as_int().max(1) as f64),
            Timing::Timecode(fps, sub) => 1.0 / (fps.as_f32() as f64 * sub.max(1) as f64),
        }
    };

    let mut events = Vec::with_capacity(merged.len());
    let mut last_tick = 0u64;
    let mut length = 0.0;
    for (tick, _, _, kind) in merged {
        let delta = (tick - last_tick) as f64 * seconds_per_tick(tempo);
        last_tick = tick;
        length += delta;
        let event = match kind {
            TrackEventKind::Midi { channel, message } => {
                let channel = channel.as_int();
                match message {
                    MidiMessage::NoteOn { key, vel } => MidiEvent::NoteOn {
                        channel,
                        note: key.as_int(),
                        velocity: vel.as_int(),
                    },
                    MidiMessage::NoteOff { key, vel } => MidiEvent::NoteOff {
                        channel,
                        note: key.as_int(),
                        velocity: vel.as_int(),
                    },
                    MidiMessage::Controller { controller, value } => MidiEvent::Control {
                        channel,
                        control: controller.as_int(),
                        value: value.as_int(),
                    },
                    _ => MidiEvent::Other,
                }
            }
            TrackEventKind::Meta(MetaMessage::Tempo(t)) => {
                tempo = t.as_int() as f64;
                MidiEvent::Other
            }
            _ => MidiEvent::Other,
        };
        events.push(TimedEvent { delta, event });
    }

    Ok(MidiSong { events, length })
}

/// Formats seconds like Python's `str(timedelta(seconds=int(x)))`: `H:MM:SS`.
pub fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}
