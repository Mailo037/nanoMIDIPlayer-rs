//! MIDI Player, Drums Player and MIDI To QWERTY tabs.
//!
//! Every tab shows the essentials first (song, playback, a few toggles) and folds
//! the less common options into an "Advanced" section. Colors come from the theme.

use gpui::prelude::*;
use gpui::{Context, Div, ElementId, Hsla, Stateful, Window, div, px, relative};

use super::widgets::*;
use super::{Control, DropId, EntryId, RootView};
use crate::config::{self, HotkeySlot};
use crate::core::Target;
use crate::live::PIANO;

const PITCHES: [(&str, i64); 5] = [("Lowest", -24), ("Low", -12), ("Middle", 0), ("High", 12), ("Highest", 24)];
const KEYS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const RADIUS: f32 = 16.0;

type Setter = fn(&mut crate::config::Config, bool);

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Slightly lighter (dark themes) or darker (light themes) shade for card backgrounds.
pub fn shade(c: Hsla, amount: f32) -> Hsla {
    let l = if c.l < 0.5 { (c.l + amount).min(1.0) } else { (c.l - amount).max(0.0) };
    Hsla { l, ..c }
}

impl RootView {
    /// Scrollable page body (the window can be resized smaller than the content).
    pub(super) fn page(&self, id: &'static str, section: &str) -> Stateful<Div> {
        div()
            .id(id)
            .size_full()
            .overflow_y_scroll()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .bg(self.color(section, "BackgroundColor"))
    }

    /// A rounded card with an optional small caption.
    pub fn card(&self, section: &str, title: &str) -> Div {
        let bg = shade(self.color(section, "BackgroundColor"), 0.04);
        let mut d = div()
            .rounded(px(RADIUS))
            .bg(bg)
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.));
        if !title.is_empty() {
            d = d.child(text(&self.theme, title.to_uppercase(), 11.0, self.color(section, "TextColorDisabled")));
        }
        d
    }

    pub(super) fn hint(&self, section: &str, s: &str) -> Div {
        lines(&self.theme, s, 11.0, self.color(section, "TextColorDisabled"))
    }

    fn entry_colors(&self, section: &str, prefix: &str) -> EntryColors {
        EntryColors {
            bg: self.color(section, &format!("{prefix}BackColor")),
            border: self.color(section, &format!("{prefix}BorderColor")),
            text: self.color(section, "TextColor"),
            placeholder: self.color(section, "TextColorDisabled"),
        }
    }

    fn file_dropdown(&self, target: Target, id: DropId, section: &str, cx: &mut Context<Self>) -> Div {
        let files = {
            let state = self.core.state.lock();
            match target {
                Target::Midi => state.midi.clone(),
                Target::Drums => state.drums.clone(),
            }
        };
        let value = files.current.as_deref().map(file_name).unwrap_or_else(|| "No file selected".into());
        let names = files.files.iter().map(|f| file_name(f)).collect();
        let paths = files.files.clone();
        self.dropdown(id, value, names, 0.0, true, self.dropdown_colors(section), cx, move |this, i, _, cx| {
            if let Some(path) = paths.get(i) {
                this.core.switch_file(target, path.clone());
            }
            cx.notify();
        })
    }

    fn song_card(&self, target: Target, id: DropId, section: &str, hint: Option<&str>, cx: &mut Context<Self>) -> Div {
        let browse = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(format!("select-{target:?}").into()),
                label: "Browse…".into(),
                width: 96.0,
                height: ROW_H,
                enabled: true,
                colors: self.button_colors(section, "ButtonColor", "ButtonHoverColor"),
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                this.core.select_file(target);
                cx.notify();
            }),
        );
        let mut card = self.card(section, "Song").child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().child(self.file_dropdown(target, id, section, cx)))
                .child(with_tooltip(browse, self.tooltips(), "Open a MIDI file (.mid | .midi)", &self.theme)),
        );
        if let Some(h) = hint {
            card = card.child(self.hint(section, h));
        }
        card
    }

    /// Status, progress bar, Play/Pause + Stop and speed.
    fn player_card(&mut self, target: Target, section: &str, speed: Control, mode_label: &str, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let player = match target {
            Target::Midi => self.core.piano.clone(),
            Target::Drums => self.core.drums.clone(),
        };
        let running = player.is_running();
        let paused = player.is_paused();
        let has_file = {
            let st = self.core.state.lock();
            match target {
                Target::Midi => st.midi.current.is_some(),
                Target::Drums => st.drums.current.is_some(),
            }
        };
        let hotkeys = config::get().read().hotkeys.clone();

        let (status, status_color) = if !running {
            ("Stopped", self.color(section, "TextColorDisabled"))
        } else if paused && self.core.auto_paused[target as usize].load(std::sync::atomic::Ordering::Acquire) {
            ("Paused · click back into your game", self.color(section, "PausedColor"))
        } else if paused {
            ("Paused", self.color(section, "PausedColor"))
        } else {
            ("Playing", self.color(section, "PlayingColor"))
        };
        let (label, bg, hover) = if !running {
            (format!("▶  Play  ({})", hotkeys.play.to_uppercase()), "PlayColor", "PlayColorHover")
        } else if paused {
            (format!("▶  Resume  ({})", hotkeys.pause.to_uppercase()), "PausedColor", "PausedColorHover")
        } else {
            (format!("❚❚  Pause  ({})", hotkeys.pause.to_uppercase()), "PlayingColor", "PlayingColorHover")
        };
        let play = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(format!("play-{target:?}").into()),
                label: label.into(),
                width: 170.0,
                height: 38.0,
                enabled: running || has_file,
                colors: self.button_colors(section, bg, hover),
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                this.core.play_button(target);
                cx.notify();
            }),
        )
        .rounded_full();
        let stop_colors = ButtonColors {
            bg: self.color(section, if running { "StopColor" } else { "StopColorDisabled" }),
            ..self.button_colors(section, "StopColor", "StopColorHover")
        };
        let stop = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(format!("stop-{target:?}").into()),
                label: format!("■  Stop  ({})", hotkeys.stop.to_uppercase()).into(),
                width: 130.0,
                height: 38.0,
                enabled: running,
                colors: stop_colors,
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                this.core.stop(target);
                cx.notify();
            }),
        )
        .rounded_full();

        let (pos, len) = if running { player.position() } else { (0.0, 0.0) };
        let show_time = config::get().read().app_ui.timestamp;
        let frac = if len > 0.0 && show_time { (pos / len).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let progress = div()
            .h(px(6.))
            .w_full()
            .rounded_full()
            .bg(self.color(section, "SpeedSliderBackColor"))
            .child(div().h_full().w(relative(frac)).rounded_full().bg(self.color(section, "PlayColor")));

        let slider = self.control_slider(speed, 170.0, true, self.slider_colors(section, "SpeedSlider"), cx);
        let entry = self.entry(EntryId::Control(speed), 52.0, "100", true, self.entry_colors(section, "SpeedValueBox"), window, cx);
        let reset = self.reset_button(
            &format!("reset-{speed:?}"),
            section,
            true,
            "Reset speed to 100%",
            Self::click(cx, move |this, _, cx| {
                speed.reset(&this.core);
                cx.notify();
            }),
        );

        self.card(section, "")
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(10.)).h(px(10.)).rounded_full().bg(status_color))
                            .child(text(&self.theme, status, 16.0, self.color(section, "TextColor")))
                            .child(self.hint(section, mode_label)),
                    )
                    .child(self.label(&self.timeline_text(target), section)),
            )
            .child(progress)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(with_tooltip(play, self.tooltips(), "Start / pause playback.\nClick into your game window after starting.", &self.theme))
                    .child(stop),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.label("Speed", section))
                    .child(with_tooltip(slider, self.tooltips(), "Playback speed (1-500%)", &self.theme))
                    .child(with_tooltip(entry, self.tooltips(), "Type an exact speed in %", &self.theme))
                    .child(reset),
            )
    }

    fn toggle(
        &self,
        id: &'static str,
        label: &str,
        checked: bool,
        enabled: bool,
        section: &str,
        tooltip: &'static str,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, bool) + 'static,
    ) -> Stateful<Div> {
        let h = Self::handler(cx, move |this, on: bool, _, cx| {
            f(this, on);
            this.core.bump();
            cx.notify();
        });
        let el = switch(&self.theme, id, label, checked, enabled, self.switch_colors(section), h);
        with_tooltip(el, self.tooltips(), tooltip, &self.theme)
    }

    /// Two-column grid of switches.
    fn toggle_grid(&self, section: &str, items: [(&'static str, &str, bool, &'static str, Setter); 4], cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap_y(px(6.));
        for (id, label, checked, tip, set) in items {
            let sw = self.toggle(id, label, checked, true, section, tip, cx, move |_, on| config::get().update(|c| set(c, on)));
            grid = grid.child(div().w(relative(0.5)).child(sw));
        }
        grid
    }

    fn finger_row(&self, control: Control, section: &str, cx: &mut Context<Self>) -> Div {
        let limit = control.get(&self.core);
        let display = if limit > 10.0 { "No limit".to_string() } else { format!("{} keys", limit as i64) };
        let slider = self.control_slider(control, 150.0, true, self.slider_colors(section, "SpeedSlider"), cx);
        let reset = self.reset_button(
            &format!("reset-{control:?}"),
            section,
            true,
            "Reset to no limit",
            Self::click(cx, move |this, _, cx| {
                control.reset(&this.core);
                cx.notify();
            }),
        );
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(130.)).child(self.label("Max. keys at once", section)))
            .child(with_tooltip(slider, self.tooltips(), "Max simultaneous keys (1-10)\nSlide past 10 for No Limit (∞)", &self.theme))
            .child(div().w(px(70.)).child(self.label(&display, section)))
            .child(reset)
    }

    pub(super) fn hotkey_row(&self, section: &str, cx: &mut Context<Self>) -> Div {
        let capturing = self.core.state.lock().capturing.is_some();
        let mut row = div().flex().flex_wrap().gap(px(8.));
        for (label, slot) in [
            ("Play", HotkeySlot::Play),
            ("Pause", HotkeySlot::Pause),
            ("Stop", HotkeySlot::Stop),
            ("Slower", HotkeySlot::SpeedUp),
            ("Faster", HotkeySlot::SlowDown),
        ] {
            row = row.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .child(lines(&self.theme, label, 12.0, self.color(section, "TextColor")))
                    .child(self.hotkey_button(slot, section, cx)),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(self.label(if capturing { "Hotkeys — press any key…" } else { "Hotkeys (global, click to change)" }, section))
            .child(row)
    }

    fn log_box(&self, section: &str, max: usize) -> Div {
        let enabled = config::get().read().app_ui.console;
        let body = if enabled {
            self.console_box(section, 0.0, 0.0, max).w_full().h(px(130.)).rounded(px(10.)).p(px(6.))
        } else {
            self.hint(section, "Console is disabled in Settings.")
        };
        div().flex().flex_col().gap(px(4.)).child(self.label("Log", section)).child(body)
    }

    /// Two-option segmented control.
    fn segmented(
        &self,
        id: &'static str,
        section: &str,
        options: [(&'static str, bool); 2],
        selected: usize,
        tooltip: &'static str,
        cx: &mut Context<Self>,
        on_select: impl Fn(&mut Self, usize) + 'static + Clone,
    ) -> Stateful<Div> {
        let on = self.color(section, "SwitchEnabled");
        let off = self.color(section, "ButtonColor");
        let hover = self.color(section, "ButtonHoverColor");
        let text_c = self.color(section, "TextColor");
        let dis = self.color(section, "TextColorDisabled");
        let mut row = div().id(id).flex().rounded_full().overflow_hidden().bg(off).p(px(3.)).gap(px(3.));
        for (i, (label, enabled)) in options.into_iter().enumerate() {
            let is_sel = i == selected;
            let select = on_select.clone();
            let mut seg = div()
                .id(ElementId::NamedInteger(id.into(), i as u64))
                .h(px(ROW_H - 4.0))
                .px(px(14.))
                .rounded_full()
                .flex()
                .items_center()
                .when(is_sel, |d| d.bg(on))
                .child(text(&self.theme, label, 13.0, if enabled { text_c } else { dis }));
            if enabled && !is_sel {
                seg = seg
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        select(this, i);
                        cx.notify();
                    }));
            }
            row = row.child(seg);
        }
        with_tooltip(row, self.tooltips(), tooltip, &self.theme)
    }

    /// Clickable "Advanced" header; returns the header and whether it is open.
    fn advanced_header(&self, section: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.advanced;
        let hover = shade(self.color(section, "BackgroundColor"), 0.07);
        div()
            .id("advanced-toggle")
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .rounded_full()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(text(&self.theme, if open { "▾" } else { "▸" }, 14.0, self.color(section, "TextColor")))
            .child(self.label("Advanced", section))
            .child(self.hint(section, if open { "" } else { "output, pitch, finger limit, hotkeys, log" }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.advanced = !this.advanced;
                cx.notify();
            }))
    }

    // ------------------------------------------------------------------ MIDI

    pub fn midi_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        const S: &str = "MidiPlayer";
        let cfg = config::get().read().midi_player.clone();
        let (outputs, output) = {
            let st = self.core.state.lock();
            (st.outputs.clone(), st.output.clone())
        };
        let has_outputs = !outputs.is_empty();
        let use_out = cfg.use_midi_output && has_outputs;

        let quick = self.card(S, "Options").child(self.toggle_grid(
            S,
            [
                ("mp-sustain", "Sustain", cfg.sustain, "Holds Space while the sustain pedal is down.\nOnly works on supported games.", |c, v| c.midi_player.sustain = v),
                ("mp-nodoubles", "No Doubles", cfg.no_doubles, "Prevents double-triggering of keys", |c, v| c.midi_player.no_doubles = v),
                ("mp-velocity", "Velocity", cfg.velocity, "Simulates how hard a key is pressed (Alt + key).\nOnly works on supported games.", |c, v| c.midi_player.velocity = v),
                ("mp-88", "88 Keys", cfg.keys_88, "Plays notes outside the 61-key range with Ctrl.\nOnly works on supported games.", |c, v| c.midi_player.keys_88 = v),
            ],
            cx,
        ));

        let mode_label = if use_out { "· MIDI device" } else { "· Keyboard" };
        let mut page = self
            .page("midi-page", S)
            .child(self.song_card(Target::Midi, DropId::MidiFile, S, None, cx))
            .child(self.player_card(Target::Midi, S, Control::MidiSpeed, mode_label, window, cx))
            .child(quick)
            .child(self.advanced_header(S, cx));

        if self.advanced {
            let mode = self.segmented(
                "mode",
                S,
                [("Keyboard", true), ("MIDI device", has_outputs)],
                use_out as usize,
                "Keyboard: presses keys in your game (macro).\nMIDI device: sends notes to a MIDI output\n(no keys are pressed).",
                cx,
                |this, i| this.core.set_use_midi_output(i == 1),
            );
            let mut output_row = div().flex().flex_wrap().items_center().gap(px(8.)).child(div().w(px(80.)).child(self.label("Output", S))).child(mode);
            if use_out {
                let out_items = outputs.clone();
                let out_dd = self.dropdown(DropId::MidiOutput, output.clone().unwrap_or_default(), out_items.clone(), 220.0, true,
                    self.dropdown_colors(S), cx, move |this, i, _, cx| {
                        if let Some(name) = out_items.get(i) {
                            this.core.state.lock().output = Some(name.clone());
                            this.core.bump();
                        }
                        cx.notify();
                    });
                let refresh = self.reset_button("refresh-out", S, true, "Refresh MIDI output devices",
                    Self::click(cx, |this, _, cx| {
                        this.core.refresh_outputs();
                        cx.notify();
                    }));
                output_row = output_row.child(out_dd).child(refresh);
            }

            let pitch_label = PITCHES.iter().find(|p| p.1 == cfg.pitch_offset).map(|p| p.0).unwrap_or("Middle");
            let key_label = if (0..12).contains(&cfg.transpose_offset) { KEYS[cfg.transpose_offset as usize] } else { "C" };
            let pitch = self.dropdown(DropId::Pitch, pitch_label.into(), PITCHES.iter().map(|p| p.0.to_string()).collect(), 120.0, true,
                self.dropdown_colors(S), cx, |_, i, _, cx| {
                    config::get().update(|c| c.midi_player.pitch_offset = PITCHES[i].1);
                    cx.notify();
                });
            let key = self.dropdown(DropId::Key, key_label.into(), KEYS.iter().map(|k| k.to_string()).collect(), 80.0, true,
                self.dropdown_colors(S), cx, |_, i, _, cx| {
                    config::get().update(|c| c.midi_player.transpose_offset = i as i64);
                    cx.notify();
                });
            let pitch_row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(80.)).child(self.label("Octave", S)))
                .child(with_tooltip(div().id("pitch-tt").child(pitch), self.tooltips(), "Shift playback by octaves", &self.theme))
                .child(div().w(px(16.)))
                .child(self.label("Transpose", S))
                .child(with_tooltip(div().id("key-tt").child(key), self.tooltips(), "Transpose by semitones (C = none)", &self.theme));

            page = page.child(
                self.card(S, "")
                    .gap(px(12.))
                    .child(output_row)
                    .child(pitch_row)
                    .child(self.finger_row(Control::MidiFingers, S, cx))
                    .child(self.hotkey_row(S, cx))
                    .child(self.log_box(S, 8)),
            );
        }
        page
    }

    // ----------------------------------------------------------------- Drums

    pub fn drums_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        const S: &str = "DrumsMacro";
        let mut page = self
            .page("drums-page", S)
            .child(self.song_card(Target::Drums, DropId::DrumsFile, S, Some("Needs a MIDI file with a drum track (GM drums)."), cx))
            .child(self.player_card(Target::Drums, S, Control::DrumsSpeed, "· Drums", window, cx))
            .child(self.advanced_header(S, cx));
        if self.advanced {
            page = page.child(
                self.card(S, "")
                    .gap(px(12.))
                    .child(self.finger_row(Control::DrumsFingers, S, cx))
                    .child(self.hotkey_row(S, cx))
                    .child(self.log_box(S, 11)),
            );
        }
        page.child(
            div().flex().justify_center().pt(px(6.)).child(
                lines(&self.theme, "♡ Drums Macro created by fearsomeorc1406", 12.0, self.color("Navigation", "WatermarkColor")),
            ),
        )
    }

    // ---------------------------------------------------------- MIDI To QWERTY

    pub fn qwerty_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        const S: &str = "MidiToQWERTY";
        const SET: &str = "Settings";
        let cfg = config::get().read().midi_to_qwerty.clone();
        let running = self.core.live.is_running();
        let (inputs, input) = {
            let st = self.core.state.lock();
            (st.inputs.clone(), st.input.clone())
        };
        let has_inputs = !inputs.is_empty();

        let items = if has_inputs { inputs.clone() } else { vec!["No input devices".into()] };
        let value = input.unwrap_or_else(|| "No MIDI keyboard found".into());
        let dd = self.dropdown(DropId::QwInput, value, items.clone(), 0.0, has_inputs && !running, self.dropdown_colors(S), cx,
            move |this, i, _, cx| {
                if let Some(name) = items.get(i) {
                    this.core.state.lock().input = Some(name.clone());
                    this.core.bump();
                }
                cx.notify();
            });
        let refresh = self.reset_button("refresh-in", "MidiPlayer", !running, "Refresh MIDI input devices",
            Self::click(cx, |this, _, cx| {
                this.core.refresh_inputs();
                cx.notify();
            }));
        let (label, bg, hover) = if running {
            ("■  Stop listening", "StopColor", "StopColorHover")
        } else {
            ("▶  Start listening", "PlayColor", "PlayColorHover")
        };
        let enable = button(
            &self.theme,
            ButtonSpec {
                id: "qw-enable".into(),
                label: label.into(),
                width: 190.0,
                height: 38.0,
                enabled: running || has_inputs,
                colors: self.button_colors(S, bg, hover),
                icon: None,
            },
            Self::click(cx, |this, _, cx| {
                this.core.toggle_live();
                cx.notify();
            }),
        )
        .rounded_full();
        let status = if running { "Listening — play your MIDI keyboard" } else { "Turns your MIDI keyboard into key presses" };
        let device = self
            .card(S, "MIDI keyboard")
            .child(div().flex().items_center().gap(px(8.)).child(div().flex_1().min_w_0().child(dd)).child(refresh))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(with_tooltip(enable, self.tooltips(), "Start Listening to MIDI Input Device", &self.theme))
                    .child(self.hint(S, status)),
            );

        let quick = self.card(S, "Options").child(self.toggle_grid(
            S,
            [
                ("qw-sustain", "Sustain", cfg.sustain, "Holds Space while the sustain pedal is down.\nOnly works on supported games.", |c, v| c.midi_to_qwerty.sustain = v),
                ("qw-nodoubles", "No Doubles", cfg.no_doubles, "Prevents double-triggering of keys", |c, v| c.midi_to_qwerty.no_doubles = v),
                ("qw-velocity", "Velocity", cfg.velocity, "Simulates how hard a key is pressed (Alt + key).\nOnly works on supported games.", |c, v| c.midi_to_qwerty.velocity = v),
                ("qw-88", "88 Keys", cfg.keys_88, "Plays notes outside the 61-key range with Ctrl.\nOnly works on supported games.", |c, v| c.midi_to_qwerty.keys_88 = v),
            ],
            cx,
        ));

        let mut page = self
            .page("qwerty-page", S)
            .child(device)
            .child(self.card(S, "").child(self.piano_widget(S)))
            .child(quick)
            .child(self.advanced_header(S, cx));

        if self.advanced {
            let entry_colors = self.entry_colors(SET, "ValueBox");
            let cutoff_entry = self.entry(EntryId::Control(Control::QwSustainCutoff), 44.0, "63", true, entry_colors, window, cx);
            let cutoff_slider = self.control_slider(Control::QwSustainCutoff, 140.0, true, self.slider_colors(SET, "Slider"), cx);
            let cutoff_reset = self.reset_button("reset-cutoff", SET, true, "Reset Sustain Cutoff (63)",
                Self::click(cx, |this, _, cx| {
                    Control::QwSustainCutoff.reset(&this.core);
                    cx.notify();
                }));

            let hold_on = cfg.custom_hold_length.enabled;
            let hold_sw = self.toggle("qw-hold", "Fixed note length (seconds)", hold_on, true, S, "Release every key after a fixed time\ninstead of on note-off", cx,
                |_, on| config::get().update(|c| c.midi_to_qwerty.custom_hold_length.enabled = on));
            let hold_slider = self.control_slider(Control::QwNoteLength, 140.0, hold_on, self.slider_colors(SET, "Slider"), cx);
            let hold_entry = self.entry(EntryId::Control(Control::QwNoteLength), 52.0, "0.10", hold_on, entry_colors, window, cx);
            let hold_reset = self.reset_button("reset-qwhold", SET, hold_on, "Reset Note Hold Length",
                Self::click(cx, |this, _, cx| {
                    Control::QwNoteLength.reset(&this.core);
                    cx.notify();
                }));

            let modules = vec!["pynput".to_string(), "keyboard".to_string()];
            let module = self.dropdown(DropId::QwModule, cfg.input_module.clone(), modules.clone(), 130.0, cfg!(windows), self.dropdown_colors(SET), cx,
                move |_, i, _, cx| {
                    let m = modules[i].clone();
                    config::get().update(|c| c.midi_to_qwerty.input_module = m);
                    cx.notify();
                });

            page = page.child(
                self.card(S, "")
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(150.)).child(self.label("Pedal threshold", S)))
                            .child(with_tooltip(cutoff_slider, self.tooltips(), "Pedal values above this count as pressed\nDefault: 63", &self.theme))
                            .child(cutoff_entry)
                            .child(cutoff_reset),
                    )
                    .child(hold_sw)
                    .child(div().flex().items_center().gap(px(8.)).pl(px(42.)).child(hold_slider).child(hold_entry).child(hold_reset))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(150.)).child(self.label("Input method", S)))
                            .child(with_tooltip(div().id("qw-module").child(module), self.tooltips(), "pynput: virtual keys\nkeyboard: scan codes (try if a game ignores keys)", &self.theme)),
                    )
                    .child(self.log_box(S, 13)),
            );
        }
        page
    }

    /// 88-key piano (21..=108) showing held notes; scales with the window width.
    fn piano_widget(&self, section: &str) -> Div {
        let white = self.color(section, "WhiteNoteColor");
        let white_held = self.color(section, "WhiteNoteColorHeld");
        let black = self.color(section, "BlackNoteColor");
        let black_held = self.color(section, "BlackNoteColorHeld");
        let key_w = 1.0 / 52.0;
        let black_w = key_w * 0.69;
        let has_black = |i: i32| !((i - 1).rem_euclid(7) == 0 || (i - 1).rem_euclid(7) == 3) && i != 51;

        let mut piano = div().relative().w_full().h(px(64.)).bg(gpui::rgb(0x333333)).rounded(px(10.)).overflow_hidden();
        let mut note = 21u8;
        for i in 0..52 {
            piano = piano.child(
                div()
                    .absolute()
                    .left(relative(i as f32 * key_w))
                    .top_0()
                    .w(relative(key_w))
                    .h_full()
                    .border_1()
                    .border_color(gpui::black())
                    .bg(if PIANO.is_down(note) { white_held } else { white }),
            );
            note += 1;
            if has_black(i) {
                note += 1;
            }
        }
        let mut note = 22u8;
        for i in 0..52 {
            if has_black(i) {
                let x = (i + 1) as f32 * key_w - black_w / 2.0;
                piano = piano.child(
                    div()
                        .absolute()
                        .left(relative(x))
                        .top_0()
                        .w(relative(black_w))
                        .h(relative(0.62))
                        .border_1()
                        .border_color(gpui::black())
                        .rounded_b(px(2.))
                        .bg(if PIANO.is_down(note) { black_held } else { black }),
                );
                note += 2;
            } else {
                note += 1;
            }
        }
        piano
    }
}
