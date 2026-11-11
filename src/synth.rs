//! Small built-in software synth for song previews. Used when the system has no
//! software MIDI synth (Linux and macOS have none by default; on Linux the first MIDI
//! port is usually the silent "Midi Through"). Plays a soft piano-like tone and simple
//! drums through the default audio output.

use std::f32::consts::TAU;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use parking_lot::Mutex;

const MAX_VOICES: usize = 48;
const TABLE_LEN: usize = 2048;
const DRUM_CHANNEL: u8 = 9;

struct Voice {
    channel: u8,
    note: u8,
    drum: bool,
    phase: f32,
    step: f32,
    /// Per-sample pitch multiplier (drum kicks and toms drop in pitch).
    glide: f32,
    amp: f32,
    noise: f32,
    env: f32,
    attack: f32,
    decay: f32,
    release: f32,
    released: bool,
    /// Note-off arrived while the sustain pedal was down.
    sustained: bool,
    age: u64,
}

struct State {
    rate: f32,
    table: Vec<f32>,
    voices: Vec<Voice>,
    sustain: [bool; 16],
    counter: u64,
    seed: u32,
}

impl State {
    fn new(rate: f32) -> Self {
        // A few decaying harmonics: soft, piano-ish.
        let table = (0..TABLE_LEN)
            .map(|i| {
                let p = i as f32 / TABLE_LEN as f32 * TAU;
                (p.sin() + 0.45 * (2.0 * p).sin() + 0.22 * (3.0 * p).sin() + 0.1 * (4.0 * p).sin()) / 1.77
            })
            .collect();
        Self { rate, table, voices: Vec::with_capacity(MAX_VOICES), sustain: [false; 16], counter: 0, seed: 0x1234_5678 }
    }

    fn coef(&self, seconds: f32) -> f32 {
        (-1.0 / (seconds * self.rate)).exp()
    }

    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(channel, note);
        }
        // Retrigger: release the same note first.
        for v in self.voices.iter_mut().filter(|v| v.channel == channel && v.note == note && !v.released) {
            v.released = true;
        }
        if self.voices.len() >= MAX_VOICES {
            // Steal the quietest (released first), then the oldest.
            if let Some(i) = (0..self.voices.len()).min_by(|&a, &b| {
                let (va, vb) = (&self.voices[a], &self.voices[b]);
                (!va.released, va.env, va.age).partial_cmp(&(!vb.released, vb.env, vb.age)).unwrap_or(std::cmp::Ordering::Equal)
            }) {
                self.voices.swap_remove(i);
            }
        }
        let vel = velocity as f32 / 127.0;
        let drum = channel == DRUM_CHANNEL;
        let (freq, glide, decay) = if drum {
            match note {
                35 | 36 => (150.0, self.coef(0.12), 0.25), // kick
                41 | 43 | 45 | 47 | 48 | 50 => (110.0 + (note as f32 - 41.0) * 15.0, self.coef(0.6), 0.3), // toms
                _ => (0.0, 1.0, if matches!(note, 46 | 49 | 51 | 52 | 55 | 57 | 59) { 0.45 } else { 0.07 }),
            }
        } else {
            let freq = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
            // Higher notes ring shorter, like a piano.
            let decay = (3.0 * 2f32.powf(-(note as f32 - 60.0) / 24.0)).clamp(0.35, 4.0);
            (freq, 1.0, decay)
        };
        self.counter += 1;
        self.voices.push(Voice {
            channel,
            note,
            drum,
            phase: 0.0,
            step: freq / self.rate,
            glide,
            amp: 0.22 * vel * vel.sqrt(),
            noise: if drum && freq == 0.0 { 1.0 } else { 0.0 },
            env: 0.0,
            attack: 1.0 / (0.004 * self.rate),
            decay: self.coef(decay),
            release: self.coef(if drum { 0.08 } else { 0.12 }),
            released: false,
            sustained: false,
            age: self.counter,
        });
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        let pedal = self.sustain[channel as usize & 15];
        for v in self.voices.iter_mut().filter(|v| v.channel == channel && v.note == note && !v.released && !v.drum) {
            if pedal {
                v.sustained = true;
            } else {
                v.released = true;
            }
        }
    }

    fn control(&mut self, channel: u8, control: u8, value: u8) {
        let ch = channel as usize & 15;
        match control {
            64 => {
                self.sustain[ch] = value >= 64;
                if !self.sustain[ch] {
                    for v in self.voices.iter_mut().filter(|v| v.channel == channel && v.sustained) {
                        v.sustained = false;
                        v.released = true;
                    }
                }
            }
            120 | 123 => {
                for v in self.voices.iter_mut().filter(|v| v.channel == channel) {
                    v.released = true;
                }
            }
            _ => {}
        }
    }

    fn handle(&mut self, msg: &[u8]) {
        let [status, a, b] = match msg {
            [s, a, b, ..] => [*s, *a, *b],
            _ => return,
        };
        let channel = status & 0x0F;
        match status & 0xF0 {
            0x90 => self.note_on(channel, a, b),
            0x80 => self.note_off(channel, a),
            0xB0 => self.control(channel, a, b),
            _ => {}
        }
    }

    fn next_sample(&mut self) -> f32 {
        let mut out = 0.0;
        let len = TABLE_LEN as f32;
        let mut seed = self.seed;
        for v in self.voices.iter_mut() {
            if v.env < 1.0 && !v.released && v.attack > 0.0 {
                v.env = (v.env + v.attack).min(1.0);
                if v.env >= 1.0 {
                    v.attack = 0.0;
                }
            } else {
                v.env *= if v.released { v.release } else { v.decay };
            }
            let tone = if v.noise > 0.0 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
            } else if v.drum {
                (v.phase * TAU).sin()
            } else {
                let pos = v.phase * len;
                let i = pos as usize;
                let frac = pos - i as f32;
                let a = self.table[i % TABLE_LEN];
                let b = self.table[(i + 1) % TABLE_LEN];
                a + (b - a) * frac
            };
            out += tone * v.env * v.amp;
            v.phase += v.step;
            v.phase -= v.phase.floor();
            v.step *= v.glide;
        }
        self.seed = seed;
        self.voices.retain(|v| (v.attack > 0.0 && !v.released) || v.env > 0.0005);
        // Soft clip so dense chords don't crackle.
        out.tanh()
    }
}

/// A running synth; audio stops when this is dropped.
pub struct Synth {
    state: Arc<Mutex<State>>,
    _stream: cpal::Stream,
}

impl Synth {
    pub fn open() -> anyhow::Result<Synth> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| anyhow::anyhow!("no audio output device found"))?;
        let supported = device.default_output_config()?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let state = Arc::new(Mutex::new(State::new(config.sample_rate.0 as f32)));
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, state.clone())?,
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, state.clone())?,
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, state.clone())?,
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, state.clone())?,
            other => anyhow::bail!("unsupported audio sample format {other:?}"),
        };
        stream.play()?;
        Ok(Synth { state, _stream: stream })
    }

    /// Takes a raw MIDI channel message (note on/off, control change).
    pub fn send(&self, msg: &[u8]) {
        self.state.lock().handle(msg);
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<Mutex<State>>,
) -> anyhow::Result<cpal::Stream> {
    let channels = config.channels as usize;
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let mut s = state.lock();
            for frame in data.chunks_mut(channels) {
                let sample = T::from_sample(s.next_sample());
                frame.fill(sample);
            }
        },
        |e| log::warn!("preview audio error: {e}"),
        None,
    )?;
    Ok(stream)
}
