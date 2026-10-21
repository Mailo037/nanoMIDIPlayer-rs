//! Slider/entry pairs ("control sets" in the original app): ranges, defaults and the
//! values they read and write.

use crate::config::{self, Config};
use crate::core::{Core, Target};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    MidiSpeed,
    DrumsSpeed,
    MidiFingers,
    DrumsFingers,
    QwSustainCutoff,
    QwNoteLength,
    MidiNoteLength,
    MidiSpeedFail,
    MidiTransposeFail,
    MidiDecreaseSize,
    DrumsNoteLength,
    DrumsSpeedFail,
    DrumsDecreaseSize,
}

pub struct Spec {
    /// Slider range.
    pub from: f64,
    pub to: f64,
    /// Valid value range (entries outside are ignored like the original).
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub float: bool,
    pub steps: Option<u32>,
    pub name: &'static str,
}

impl Control {
    pub fn spec(self) -> Spec {
        use Control::*;
        let s = |from, to, min, max, default, float, name| Spec { from, to, min, max, default, float, steps: None, name };
        match self {
            MidiSpeed => s(1.0, 500.0, 1.0, 500.0, 100.0, false, "midiSpeedController"),
            DrumsSpeed => s(1.0, 500.0, 1.0, 500.0, 100.0, false, "drumsSpeedController"),
            MidiFingers | DrumsFingers => Spec {
                steps: Some(10),
                ..s(1.0, 11.0, 1.0, 11.0, 11.0, false, "")
            },
            QwSustainCutoff => s(0.0, 127.0, 0.0, 127.0, 63.0, false, "midiToQWERTYSustainCutoff"),
            QwNoteLength => s(0.0, 10.0, 0.1, 10.0, 0.1, true, "midiToQWERTYNoteLength"),
            MidiNoteLength => s(0.0, 10.0, 0.1, 10.0, 0.1, true, "midiNoteLength"),
            MidiSpeedFail => s(0.0, 100.0, 0.0, 100.0, 5.0, false, "midiSpeedFail"),
            MidiTransposeFail => s(0.0, 100.0, 0.0, 100.0, 5.0, false, "midiTransposeFail"),
            MidiDecreaseSize => s(0.0, 100.0, 1.0, 100.0, 5.0, false, "midiDecreaseSize"),
            DrumsNoteLength => s(0.0, 10.0, 0.1, 10.0, 0.1, true, "drumsNoteLength"),
            DrumsSpeedFail => s(0.0, 100.0, 0.0, 100.0, 5.0, false, "drumsSpeedFail"),
            DrumsDecreaseSize => s(0.0, 100.0, 1.0, 100.0, 5.0, false, "drumsDecreaseSize"),
        }
    }

    pub fn get(self, core: &Core) -> f64 {
        use Control::*;
        let c = config::get().read();
        match self {
            MidiSpeed => (core.piano.speed() * 100.0).round(),
            DrumsSpeed => (core.drums.speed() * 100.0).round(),
            MidiFingers => c.midi_player.finger_limit as f64,
            DrumsFingers => c.drums.finger_limit as f64,
            QwSustainCutoff => c.midi_to_qwerty.sustain_cutoff,
            QwNoteLength => c.midi_to_qwerty.custom_hold_length.note_length,
            MidiNoteLength => c.midi_player.custom_hold_length.note_length,
            MidiSpeedFail => c.midi_player.random_fail.speed,
            MidiTransposeFail => c.midi_player.random_fail.transpose,
            MidiDecreaseSize => c.midi_player.decrease_size,
            DrumsNoteLength => c.drums.custom_hold_length.note_length,
            DrumsSpeedFail => c.drums.random_fail.speed,
            DrumsDecreaseSize => c.drums.decrease_size,
        }
    }

    fn write(self, core: &Core, v: f64) {
        use Control::*;
        let set = |f: fn(&mut Config, f64)| config::get().update(|c| f(c, v));
        match self {
            MidiSpeed => core.set_speed(Target::Midi, v),
            DrumsSpeed => core.set_speed(Target::Drums, v),
            MidiFingers => set(|c, v| c.midi_player.finger_limit = v.round() as i64),
            DrumsFingers => set(|c, v| c.drums.finger_limit = v.round() as i64),
            QwSustainCutoff => set(|c, v| c.midi_to_qwerty.sustain_cutoff = v),
            QwNoteLength => set(|c, v| c.midi_to_qwerty.custom_hold_length.note_length = v),
            MidiNoteLength => set(|c, v| c.midi_player.custom_hold_length.note_length = v),
            MidiSpeedFail => set(|c, v| c.midi_player.random_fail.speed = v),
            MidiTransposeFail => set(|c, v| c.midi_player.random_fail.transpose = v),
            MidiDecreaseSize => set(|c, v| c.midi_player.decrease_size = v),
            DrumsNoteLength => set(|c, v| c.drums.custom_hold_length.note_length = v),
            DrumsSpeedFail => set(|c, v| c.drums.random_fail.speed = v),
            DrumsDecreaseSize => set(|c, v| c.drums.decrease_size = v),
        }
        core.bump();
    }

    pub fn format(self, v: f64) -> String {
        if self.spec().float {
            format!("{v:.2}")
        } else {
            format!("{}", v.round() as i64)
        }
    }

    fn normalize(self, v: f64) -> f64 {
        if self.spec().float { (v * 100.0).round() / 100.0 } else { v.trunc() }
    }

    fn log(self, v: f64) {
        let name = self.spec().name;
        if !name.is_empty() {
            crate::console::log(format!("{name} set to {}", self.format(v)));
        }
    }

    /// Value from a slider fraction (0..1), clamped to the valid range.
    pub fn set_from_fraction(self, core: &Core, fraction: f32) {
        let spec = self.spec();
        let mut v = spec.from + (spec.to - spec.from) * fraction.clamp(0.0, 1.0) as f64;
        if let Some(steps) = spec.steps {
            let step = (spec.to - spec.from) / steps as f64;
            v = spec.from + ((v - spec.from) / step).round() * step;
        }
        let v = self.normalize(v).clamp(spec.min, spec.max);
        if (v - self.get(core)).abs() > f64::EPSILON {
            self.write(core, v);
            self.log(v);
        }
    }

    /// Value typed into the entry; ignored unless valid (original `updateFromEntry`).
    pub fn set_from_text(self, core: &Core, text: &str) {
        let spec = self.spec();
        let parsed = if spec.float {
            text.trim().parse::<f64>().ok()
        } else {
            text.trim().parse::<i64>().ok().map(|v| v as f64)
        };
        if let Some(v) = parsed.map(|v| self.normalize(v)) {
            if (spec.min..=spec.max).contains(&v) {
                self.write(core, v);
                self.log(v);
            }
        }
    }

    pub fn reset(self, core: &Core) {
        let v = self.spec().default;
        self.write(core, v);
        self.log(v);
    }

    pub fn fraction(self, core: &Core) -> f32 {
        let spec = self.spec();
        ((self.get(core) - spec.from) / (spec.to - spec.from)).clamp(0.0, 1.0) as f32
    }
}
