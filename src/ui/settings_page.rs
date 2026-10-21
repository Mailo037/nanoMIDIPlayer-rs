//! Settings tab: one card per area, every setting is a row with a label on the
//! left and its control on the right. Sub-settings only appear when their switch
//! is on. Sustain / No Doubles / Velocity / 88 Keys live on the player tabs.

use gpui::prelude::*;
use gpui::{Context, Div, ElementId, Stateful, Window, div, px};

use super::widgets::*;
use super::{Control, DropId, EntryId, RootView};
use crate::config::{self, Config};
use crate::console;
use crate::core::Target;
use crate::theme;

const S: &str = "Settings";

type Setter = fn(&mut Config, bool);

impl RootView {
    /// A settings row: label (+ optional description) left, control right.
    fn row(&self, label: &str, description: &str, control: impl IntoElement) -> Div {
        let mut left = div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(self.label(label, S));
        if !description.is_empty() {
            left = left.child(self.hint(S, description));
        }
        div().flex().items_center().gap(px(12.)).min_h(px(32.)).child(left).child(control)
    }

    fn divider(&self) -> Div {
        div().h(px(1.)).w_full().bg(super::pages::shade(self.color(S, "BackgroundColor"), 0.08))
    }

    fn set_switch(&self, id: &'static str, checked: bool, cx: &mut Context<Self>, set: Setter) -> Stateful<Div> {
        let h = Self::handler(cx, move |this, on: bool, _, cx| {
            config::get().update(|c| set(c, on));
            this.core.bump();
            cx.notify();
        });
        switch(&self.theme, id, "", checked, true, self.switch_colors(S), h)
    }

    /// Slider + value box + reset, used as the control of a row.
    fn value_control(&mut self, control: Control, entry_w: f32, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let colors = EntryColors {
            bg: self.color(S, "ValueBoxBackColor"),
            border: self.color(S, "ValueBoxBorderColor"),
            text: self.color(S, "TextColor"),
            placeholder: self.color(S, "TextColorDisabled"),
        };
        let slider = self.control_slider(control, 130.0, true, self.slider_colors(S, "Slider"), cx);
        let entry = self.entry(EntryId::Control(control), entry_w, "", true, colors, window, cx);
        let reset = self.reset_button(
            &format!("reset-set-{control:?}"),
            S,
            true,
            "Reset to default",
            Self::click(cx, move |this, _, cx| {
                control.reset(&this.core);
                cx.notify();
            }),
        );
        div().flex().items_center().gap(px(8.)).child(slider).child(entry).child(reset)
    }

    fn module_select(&self, id: DropId, value: String, cx: &mut Context<Self>, set: fn(&mut Config, String)) -> Div {
        let modules = vec!["pynput".to_string(), "keyboard".to_string()];
        let value = if cfg!(windows) { value } else { "Unavailable".into() };
        self.dropdown(id, value, modules.clone(), 130.0, cfg!(windows), self.dropdown_colors(S), cx, move |_, i, _, cx| {
            let m = modules[i].clone();
            config::get().update(|c| set(c, m));
            cx.notify();
        })
    }

    fn small_button(&self, id: &str, label: &str, cx: &mut Context<Self>, f: impl Fn(&mut Self) + 'static) -> Stateful<Div> {
        button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(id.to_string().into()),
                label: label.to_string().into(),
                width: 130.0,
                height: ROW_H,
                enabled: true,
                colors: self.button_colors(S, "ButtonColor", "ButtonHoverColor"),
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                f(this);
                cx.notify();
            }),
        )
    }

    /// Settings shared by the MIDI and drums players.
    fn player_settings(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let cfg = config::get().read().clone();
        let (title, loop_song, release, hold, fail, module) = match target {
            Target::Midi => {
                let m = &cfg.midi_player;
                ("MIDI Player", m.loop_song, m.release_on_pause, m.custom_hold_length.enabled, m.random_fail.enabled, m.input_module.clone())
            }
            Target::Drums => {
                let d = &cfg.drums;
                ("Drums Player", d.loop_song, d.release_on_pause, d.custom_hold_length.enabled, d.random_fail.enabled, d.input_module.clone())
            }
        };
        let midi = target == Target::Midi;
        let (note_len, speed_fail, step) = if midi {
            (Control::MidiNoteLength, Control::MidiSpeedFail, Control::MidiDecreaseSize)
        } else {
            (Control::DrumsNoteLength, Control::DrumsSpeedFail, Control::DrumsDecreaseSize)
        };

        let loop_sw = self.set_switch(if midi { "s-loop" } else { "d-loop" }, loop_song, cx, if midi { |c, v| c.midi_player.loop_song = v } else { |c, v| c.drums.loop_song = v });
        let release_sw = self.set_switch(if midi { "s-release" } else { "d-release" }, release, cx, if midi { |c, v| c.midi_player.release_on_pause = v } else { |c, v| c.drums.release_on_pause = v });
        let hold_sw = self.set_switch(if midi { "s-hold" } else { "d-hold" }, hold, cx, if midi { |c, v| c.midi_player.custom_hold_length.enabled = v } else { |c, v| c.drums.custom_hold_length.enabled = v });
        let fail_sw = self.set_switch(if midi { "s-fail" } else { "d-fail" }, fail, cx, if midi { |c, v| c.midi_player.random_fail.enabled = v } else { |c, v| c.drums.random_fail.enabled = v });

        let mut card = self
            .card(S, title)
            .child(self.row("Loop song", "Start again when the song ends", loop_sw))
            .child(self.row("Release keys on pause", "Let go of all held keys while paused", release_sw))
            .child(self.divider())
            .child(self.row("Fixed note length", "Release every key after a fixed time", hold_sw));
        if hold {
            let c = self.value_control(note_len, 56.0, window, cx);
            card = card.child(self.row("    Length (seconds)", "", c));
        }
        card = card.child(self.row("Random mistakes", "Play slightly off, like a human", fail_sw));
        if fail {
            let c = self.value_control(speed_fail, 48.0, window, cx);
            card = card.child(self.row("    Timing mistakes (%)", "", c));
            if midi {
                let c = self.value_control(Control::MidiTransposeFail, 48.0, window, cx);
                card = card.child(self.row("    Wrong notes (%)", "", c));
            }
        }
        let step_c = self.value_control(step, 48.0, window, cx);
        let module_dd = self.module_select(
            if midi { DropId::MidiModule } else { DropId::DrumsModule },
            module,
            cx,
            if midi { |c, m| c.midi_player.input_module = m } else { |c, m| c.drums.input_module = m },
        );
        let clear = self.small_button(&format!("clear-{target:?}"), "Clear list", cx, move |this| this.core.clear_list(target));
        card.child(self.divider())
            .child(self.row("Speed hotkey step (%)", "How much Slower / Faster change the speed", step_c))
            .child(self.row("Input method", "Try \"keyboard\" if a game ignores the keys", module_dd))
            .child(self.row("Song list", "Remove all songs from the dropdown", clear))
    }

    pub fn settings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = config::get().read().app_ui.clone();

        // ---------------- Appearance
        let mut theme_names: Vec<String> = self.themes.lock().remote.iter().map(|(n, _)| n.clone()).collect();
        let active = self.theme.name.clone();
        if !theme_names.contains(&active) {
            theme_names.push(active.clone());
        }
        theme_names.extend(theme::custom_theme_names());
        let names = theme_names.clone();
        let theme_dd = self.dropdown(DropId::Theme, active, theme_names, 200.0, true, self.dropdown_colors(S), cx, move |this, i, _, cx| {
            let Some(name) = names.get(i).cloned() else { return };
            config::get().update(|c| c.app_ui.force_theme = false);
            let remote = this.themes.lock().remote.clone();
            let pending = this.pending_theme.clone();
            std::thread::spawn(move || match theme::install_theme(&name, &remote) {
                Ok(t) => *pending.lock() = Some(t),
                Err(e) => console::insert(format!("Theme error: {e}")),
            });
            cx.notify();
        });
        let appearance = self
            .card(S, "Appearance")
            .child(self.row("Theme", "", theme_dd))
            .child(self.row("Always use the default theme", "Reset to the online default theme on every start", self.set_switch("a-force", ui.force_theme, cx, |c, v| c.app_ui.force_theme = v)))
            .child(self.divider())
            .child(self.row("Always on top", "Keep the window above other windows", self.set_switch("a-top", ui.topmost, cx, |c, v| c.app_ui.topmost = v)))
            .child(self.row("Show log", "Log box on the player tabs", self.set_switch("a-console", ui.console, cx, |c, v| {
                c.app_ui.console = v;
                console::set_enabled(v);
            })))
            .child(self.row("Tooltips", "Help text when hovering controls", self.set_switch("a-tooltip", ui.tooltip, cx, |c, v| c.app_ui.tooltip = v)))
            .child(self.row("Show playback time", "Time and progress bar while playing", self.set_switch("a-timestamp", ui.timestamp, cx, |c, v| c.app_ui.timestamp = v)));

        // ---------------- Hotkeys
        let hotkeys = self.card(S, "Hotkeys").child(self.hotkey_row(S, cx));

        // ---------------- Advanced
        let console_buttons = div()
            .flex()
            .gap(px(8.))
            .child(self.small_button("open-console", "Open", cx, |_| crate::logging::open_console()))
            .child(self.small_button("close-console", "Close", cx, |_| crate::logging::close_console()));
        let advanced = self
            .card(S, "Advanced")
            .child(self.row("Check for updates", "Look for a new version on startup", self.set_switch("a-updates", ui.check_for_updates, cx, |c, v| c.app_ui.check_for_updates = v)))
            .child(self.row("Debug console", "Shows the detailed log in a terminal", console_buttons));

        let midi = self.player_settings(Target::Midi, window, cx);
        let drums = self.player_settings(Target::Drums, window, cx);

        self.page("settings-page", S)
            .track_scroll(&self.settings_scroll)
            .child(midi)
            .child(drums)
            .child(appearance)
            .child(hotkeys)
            .child(advanced)
    }
}
