//! Settings tab, split into sub-pages (General, Players, Hotkeys, Library, Theme,
//! Updates, Advanced). Every setting is a row with a label + short explanation on the
//! left and its control on the right. Sub-settings only appear when their switch is on.

use gpui::prelude::*;
use gpui::{Context, Div, ElementId, Stateful, Window, div, px};

use super::widgets::*;
use super::{Control, DropId, EntryId, RootView};
use crate::config::{self, Config};
use crate::core::Target;
use crate::update::{self, Status};
use crate::{console, net, slots, theme};

const S: &str = "Settings";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Players,
    Hotkeys,
    Library,
    Theme,
    Updates,
    Advanced,
}

impl SettingsSection {
    const ALL: [(SettingsSection, &'static str); 7] = [
        (SettingsSection::General, "General"),
        (SettingsSection::Players, "Players"),
        (SettingsSection::Hotkeys, "Hotkeys"),
        (SettingsSection::Library, "Library"),
        (SettingsSection::Theme, "Theme"),
        (SettingsSection::Updates, "Updates"),
        (SettingsSection::Advanced, "Advanced"),
    ];

    fn subtitle(self) -> &'static str {
        match self {
            SettingsSection::General => "Window, playback behaviour and what is shown.",
            SettingsSection::Players => "Options for the MIDI and Drums players.",
            SettingsSection::Hotkeys => "Global keys for playback and song slots.",
            SettingsSection::Library => "Downloaded songs and song lists.",
            SettingsSection::Theme => "Colors and look of the app.",
            SettingsSection::Updates => "Keep nanoMIDIPlayer up to date.",
            SettingsSection::Advanced => "Debugging and data.",
        }
    }
}

type Setter = fn(&mut Config, bool);

impl RootView {
    fn row(&self, label: &str, description: &str, control: impl IntoElement) -> Div {
        self.form_row(S, label, description, control)
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
        let colors = self.entry_colors(S, "ValueBox");
        let slider = self.control_slider(control, 120.0, true, self.slider_colors(S, "Slider"), cx);
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
        self.styled_button(id, label, 130.0, true, ("ButtonColor", "ButtonHoverColor"), cx, f)
    }

    fn styled_button(
        &self,
        id: &str,
        label: &str,
        width: f32,
        enabled: bool,
        colors: (&str, &str),
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self) + 'static,
    ) -> Stateful<Div> {
        button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(id.to_string().into()),
                label: label.to_string().into(),
                width,
                height: ROW_H,
                enabled,
                colors: self.button_colors("MidiPlayer", colors.0, colors.1),
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                f(this);
                cx.notify();
            }),
        )
    }

    /// Short status shown on the right of a category row in the overview.
    fn section_status(&self, section: SettingsSection) -> (String, bool) {
        match section {
            SettingsSection::Hotkeys => {
                let h = config::get().read().hotkeys.clone();
                (format!("{} {} {}", h.play, h.pause, h.stop).to_uppercase(), false)
            }
            SettingsSection::Theme => (self.theme.name.clone(), false),
            SettingsSection::Library => (format!("{} saved", crate::hub::kept_download_count()), false),
            SettingsSection::Updates => match update::state().status.clone() {
                Status::Available => ("Update available".into(), true),
                Status::Downloading(_) => ("Downloading…".into(), true),
                Status::Installed => ("Restart to update".into(), true),
                Status::UpToDate => (format!("v{} · up to date", update::current_version()), false),
                _ => (format!("v{}", update::current_version()), false),
            },
            _ => (String::new(), false),
        }
    }

    /// Settings start page: one row per category, like the Windows settings app.
    fn settings_overview(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let text_c = self.color(S, "TextColor");
        let dim = self.color(S, "TextColorDisabled");
        let hover = super::pages::shade(self.color(S, "BackgroundColor"), 0.07);
        let groups: [(&str, &[SettingsSection]); 2] = [
            ("Playing", &[SettingsSection::General, SettingsSection::Players, SettingsSection::Hotkeys]),
            ("App", &[SettingsSection::Library, SettingsSection::Theme, SettingsSection::Updates, SettingsSection::Advanced]),
        ];
        let mut cards = Vec::new();
        for (title, sections) in groups {
            let mut card = self.card(S, title).gap(px(2.)).p(px(6.)).pt(px(10.));
            for (i, &section) in sections.iter().enumerate() {
                let label = SettingsSection::ALL.iter().find(|(s, _)| *s == section).map(|(_, l)| *l).unwrap_or("");
                let (status, highlight) = self.section_status(section);
                let mut right = div().flex().items_center().gap(px(8.)).flex_shrink_0();
                if highlight {
                    right = right.child(div().w(px(8.)).h(px(8.)).rounded_full().bg(self.accent()));
                }
                if !status.is_empty() {
                    right = right.child(text(&self.theme, ellipsize(&status, 11.0, 140.0), 11.0, if highlight { self.accent() } else { dim }).whitespace_nowrap());
                }
                right = right.child(text(&self.theme, "›", 18.0, dim));
                if i > 0 {
                    card = card.child(div().mx(px(8.)).h(px(1.)).bg(super::pages::shade(self.color(S, "BackgroundColor"), 0.06)));
                }
                card = card.child(
                    div()
                        .id(ElementId::Name(format!("settings-cat-{label}").into()))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(8.))
                        .py(px(8.))
                        .rounded(px(10.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(text(&self.theme, label, 14.0, text_c))
                                .child(self.hint(S, section.subtitle())),
                        )
                        .child(right)
                        .on_click(cx.listener(move |this, _, _, cx| this.show_settings(section, cx))),
                );
            }
            cards.push(card);
        }
        cards
    }

    /// "‹ Settings" back link above a sub-page.
    fn settings_back(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let hover = super::pages::shade(self.color(S, "BackgroundColor"), 0.07);
        div()
            .id("settings-back")
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .ml(px(-8.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(text(&self.theme, "‹", 16.0, self.color(S, "TextColorDisabled")))
            .child(text(&self.theme, "Settings", 12.0, self.color(S, "TextColorDisabled")))
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_section = None;
                this.settings_scroll.set_offset(gpui::point(px(0.), px(0.)));
                cx.notify();
            }))
    }

    // ----------------------------------------------------------------- General

    fn general_settings(&mut self, cx: &mut Context<Self>) -> Vec<Div> {
        let ui = config::get().read().app_ui.clone();
        vec![
            self.card(S, "Window")
                .child(self.row("Always on top", "Keep the window above other windows", self.set_switch("a-top", ui.topmost, cx, |c, v| c.app_ui.topmost = v)))
                .child(self.row("Tooltips", "Help text when hovering controls", self.set_switch("a-tooltip", ui.tooltip, cx, |c, v| c.app_ui.tooltip = v))),
            self.card(S, "Playback")
                .child(self.row(
                    "Pause when the game loses focus",
                    "Pauses if you switch to another window and resumes when you go back",
                    self.set_switch("a-focus-pause", ui.pause_on_focus_loss, cx, |c, v| c.app_ui.pause_on_focus_loss = v),
                ))
                .child(self.row("Show playback time", "Time and progress bar while playing", self.set_switch("a-timestamp", ui.timestamp, cx, |c, v| c.app_ui.timestamp = v)))
                .child(self.row("Show log", "Log box under \"More settings\" on the player tabs", self.set_switch("a-console", ui.console, cx, |c, v| {
                    c.app_ui.console = v;
                    console::set_enabled(v);
                }))),
        ]
    }

    // ----------------------------------------------------------------- Players

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
            .child(self.divider(S))
            .child(self.row("Fixed note length", "Release every key after a fixed time", hold_sw));
        if hold {
            let c = self.value_control(note_len, 56.0, window, cx);
            card = card.child(self.row("    Length (seconds)", "", c));
        }
        card = card.child(self.row("Human mistakes", "Play slightly off, like a human", fail_sw));
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
        card.child(self.divider(S))
            .child(self.row("Speed hotkey step (%)", "How much Slower / Faster change the speed", step_c))
            .child(self.row("Input method", "Try \"keyboard\" if a game ignores the keys", module_dd))
    }

    // ----------------------------------------------------------------- Hotkeys

    fn hotkey_settings(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let capturing = self.core.state.lock().capturing.is_some();
        let reset_all = self.small_button("hk-reset-all", "Reset all", cx, |this| {
            let defaults = Config::defaults().hotkeys;
            config::get().update(|c| c.hotkeys = defaults);
            this.core.bump();
            console::toast("Hotkeys reset to F1–F5");
        });
        let players = self
            .card(S, "Player hotkeys")
            .child(self.hint(
                S,
                if capturing {
                    "Press the new key…"
                } else {
                    "Work everywhere, also while your game is focused (on the MIDI and Drums tabs).\nClick a key to change it, right-click to reset it."
                },
            ))
            .child(self.hotkey_row(S, cx))
            .child(self.row("Defaults", "F1 Play · F2 Pause · F3 Stop · F4 Slower · F5 Faster", reset_all));

        let text_c = self.color(S, "TextColor");
        let dim = self.color(S, "TextColorDisabled");
        let mut slot_card = self
            .card(S, "Song slots · Alt + 1…9")
            .child(self.hint(S, "Press Alt + number anywhere to load that song into the MIDI Player.\nAssign songs by right-clicking them in the MIDI Hub."));
        for n in 0..slots::SLOT_COUNT {
            let song = slots::get(n);
            let key_chip = div()
                .w(px(52.))
                .h(px(24.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .bg(self.color(S, "ButtonColor"))
                .child(text(&self.theme, format!("Alt+{}", n + 1), 11.0, text_c));
            let mut row = div().flex().items_center().gap(px(10.)).min_h(px(28.)).child(key_chip);
            match song {
                Some(song) => {
                    let name = song.name.clone();
                    row = row
                        .child(div().flex_1().min_w_0().child(text(&self.theme, ellipsize(&song.name, 13.0, 200.0), 13.0, text_c).whitespace_nowrap()))
                        .child(self.styled_button(&format!("slot-load-{n}"), "Load", 60.0, true, ("ButtonColor", "ButtonHoverColor"), cx, move |this| {
                            slots::activate(&this.core, n);
                            this.show_tab_silent(crate::core::Tab::Midi);
                        }))
                        .child(self.styled_button(&format!("slot-clear-{n}"), "✕", 30.0, true, ("ButtonColor", "ButtonHoverColor"), cx, move |this| {
                            slots::clear(n);
                            console::toast(format!("Alt+{} cleared ({name})", n + 1));
                            this.core.bump();
                        }));
                }
                None => {
                    row = row.child(div().flex_1().child(text(&self.theme, "empty", 12.0, dim)));
                }
            }
            slot_card = slot_card.child(row);
        }
        vec![players, slot_card]
    }

    // ----------------------------------------------------------------- Library

    fn library_settings(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let ui = config::get().read().app_ui.clone();
        let kept = crate::hub::kept_download_count();
        let delete = self.styled_button("delete-downloads", &format!("Delete ({kept})"), 130.0, kept > 0, ("StopColor", "StopColorHover"), cx, |this| {
            let n = crate::hub::delete_kept_downloads();
            console::toast(format!("Deleted {n} downloaded songs"));
            this.core.load_saved_files(Target::Midi);
        });
        let open_midis = self.small_button("open-midis", "Open folder", cx, |_| net::open_folder(&config::midis_dir()));
        let (midi_count, drums_count) = {
            let st = self.core.state.lock();
            (st.midi.files.len(), st.drums.files.len())
        };
        let clear_midi = self.styled_button("clear-midi", &format!("Clear ({midi_count})"), 130.0, midi_count > 0, ("ButtonColor", "ButtonHoverColor"), cx, |this| {
            this.core.clear_list(Target::Midi);
            console::toast("MIDI Player list cleared");
        });
        let clear_drums = self.styled_button("clear-drums", &format!("Clear ({drums_count})"), 130.0, drums_count > 0, ("ButtonColor", "ButtonHoverColor"), cx, |this| {
            this.core.clear_list(Target::Drums);
            console::toast("Drums Player list cleared");
        });
        vec![
            self.card(S, "Downloads")
                .child(self.row(
                    "Keep all downloads",
                    &format!("Off: only the last {} songs played from the MIDI Hub are kept", crate::hub::RECENT_LIMIT),
                    self.set_switch("a-keep-dl", ui.keep_downloads, cx, |c, v| c.app_ui.keep_downloads = v),
                ))
                .child(self.row("Songs folder", "Documents/nanoMIDIPlayer/Midis", open_midis))
                .child(self.row("Saved songs", "Delete every song kept in the songs folder", delete)),
            self.card(S, "Song lists")
                .child(self.row("MIDI Player list", "Empties the song dropdown (files stay on disk)", clear_midi))
                .child(self.row("Drums Player list", "Empties the song dropdown (files stay on disk)", clear_drums)),
        ]
    }

    // ------------------------------------------------------------------- Theme

    fn theme_settings(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let ui = config::get().read().app_ui.clone();
        let (mut theme_names, loaded): (Vec<String>, bool) = {
            let t = self.themes.lock();
            (t.remote.iter().map(|(n, _)| n.clone()).collect(), t.loaded)
        };
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
            console::toast(format!("Loading theme {name}…"));
            std::thread::spawn(move || match theme::install_theme(&name, &remote) {
                Ok(t) => *pending.lock() = Some(t),
                Err(e) => console::toast(format!("Theme error: {e}")),
            });
            cx.notify();
        });
        let open_custom = self.small_button("open-custom-themes", "Open folder", cx, |_| net::open_folder(&theme::custom_themes_dir()));
        vec![self
            .card(S, "Theme")
            .child(self.row("Theme", if loaded { "Online themes + your custom themes" } else { "Loading online themes…" }, theme_dd))
            .child(self.row("Always use the default theme", "Reset to the online default theme on every start", self.set_switch("a-force", ui.force_theme, cx, |c, v| c.app_ui.force_theme = v)))
            .child(self.divider(S))
            .child(self.row("Custom themes", "Put theme .json files here, then pick them above", open_custom))]
    }

    // ----------------------------------------------------------------- Updates

    fn update_settings(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let ui = config::get().read().app_ui.clone();
        let (status, latest, last_checked) = {
            let st = update::state();
            (st.status.clone(), st.latest.clone(), st.last_checked.clone())
        };
        let current = format!("v{}", update::current_version());
        let text_c = self.color(S, "TextColor");
        let dim = self.color(S, "TextColorDisabled");
        let accent = self.accent();
        let latest_v = latest.as_ref().map(|r| r.version.clone()).unwrap_or_default();

        let (headline, detail, color) = match &status {
            Status::Idle => ("Not checked yet".to_string(), "Click \"Check now\" to look for a new version.".to_string(), dim),
            Status::Checking => ("Checking for updates…".to_string(), "Asking GitHub for the latest release.".to_string(), dim),
            Status::UpToDate => ("You're up to date".to_string(), format!("{current} is the newest version."), accent),
            Status::Available => (format!("Update {latest_v} available"), format!("You have {current}."), accent),
            Status::Downloading(p) => (
                match p {
                    Some(p) => format!("Downloading {latest_v}… {}%", (p * 100.0).round() as u32),
                    None => format!("Downloading {latest_v}…"),
                },
                "You can keep using the app.".to_string(),
                accent,
            ),
            Status::Installed => (format!("{latest_v} is installed"), "Restart the app to use the new version.".to_string(), accent),
            Status::Error(e) => ("Update failed".to_string(), e.clone(), self.color("MidiPlayer", "StopColor")),
        };

        let mut buttons = div().flex().flex_wrap().gap(px(8.));
        match &status {
            Status::Available => {
                buttons = buttons.child(self.styled_button("upd-install", &format!("Update to {latest_v}"), 170.0, true, ("PlayColor", "PlayColorHover"), cx, |_| update::install()).rounded_full());
            }
            Status::Installed => {
                buttons = buttons.child(
                    button(
                        &self.theme,
                        ButtonSpec {
                            id: "upd-restart-s".into(),
                            label: "Restart now".into(),
                            width: 140.0,
                            height: ROW_H,
                            enabled: true,
                            colors: self.button_colors("MidiPlayer", "PlayColor", "PlayColorHover"),
                            icon: None,
                        },
                        Self::click(cx, |this, _, cx| this.restart_app(cx)),
                    )
                    .rounded_full(),
                );
            }
            _ => {}
        }
        let busy = matches!(status, Status::Checking | Status::Downloading(_));
        let can_check = !busy && status != Status::Installed;
        buttons = buttons.child(
            self.styled_button("upd-check", if status == Status::Checking { "Checking…" } else { "Check now" }, 120.0, can_check, ("ButtonColor", "ButtonHoverColor"), cx, |_| {
                update::check(false)
            })
            .rounded_full(),
        );
        let page = latest.as_ref().map(|r| r.page_url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| format!("{}/releases", super::FORK_REPO));
        buttons = buttons.child(self.styled_button("upd-page", "Release page »", 140.0, true, ("ButtonColor", "ButtonHoverColor"), cx, move |_| net::open_url(&page)).rounded_full());

        let progress = match status {
            Status::Downloading(p) => Some(
                div()
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .bg(self.color(S, "SliderBackColor"))
                    .child(div().h_full().w(gpui::relative(p.unwrap_or(0.05))).rounded_full().bg(accent)),
            ),
            _ => None,
        };

        let status_card = self
            .card(S, "")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().w(px(10.)).h(px(10.)).rounded_full().bg(color).flex_shrink_0())
                    .child(text(&self.theme, headline, 16.0, text_c)),
            )
            .child(self.hint(S, &detail))
            .when_some(progress, |d, p| d.child(p))
            .child(buttons)
            .child(self.hint(
                S,
                &format!(
                    "Installed: {current}{}",
                    last_checked.map(|t| format!(" · last checked {t}")).unwrap_or_default()
                ),
            ));

        let mut cards = vec![status_card];
        if let Some(r) = latest.as_ref().filter(|r| !r.notes.is_empty() && update::is_newer(&r.version, &update::current_version())) {
            let mut notes = self.card(S, &format!("What's new in {}{}", r.version, if r.published.is_empty() { String::new() } else { format!(" · {}", r.published) }));
            for line in r.notes.lines().filter(|l| !l.trim().is_empty()).take(14) {
                let l = line.trim();
                let (l, size, c) = if let Some(h) = l.strip_prefix("## ").or_else(|| l.strip_prefix("# ")) {
                    (h.to_string(), 13.0, text_c)
                } else if let Some(item) = l.strip_prefix("* ").or_else(|| l.strip_prefix("- ")) {
                    (format!("• {item}"), 12.0, dim)
                } else {
                    (l.to_string(), 12.0, dim)
                };
                notes = notes.child(text(&self.theme, l.replace("**", ""), size, c));
            }
            cards.push(notes);
        }
        cards.push(
            self.card(S, "Options")
                .child(self.row(
                    "Check on startup",
                    "Look for a new version every time the app starts",
                    self.set_switch("a-updates", ui.check_for_updates, cx, |c, v| c.app_ui.check_for_updates = v),
                ))
                .child(self.row(
                    "Install updates automatically",
                    "Download new versions in the background; they're used after the next restart",
                    self.set_switch("a-auto-update", ui.auto_update, cx, |c, v| {
                        c.app_ui.auto_update = v;
                        if v {
                            c.app_ui.check_for_updates = true;
                        }
                    }),
                )),
        );
        cards
    }

    // ---------------------------------------------------------------- Advanced

    fn advanced_settings(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let ui = config::get().read().app_ui.clone();
        let console_buttons = div()
            .flex()
            .gap(px(8.))
            .child(self.styled_button("open-console", "Open", 70.0, true, ("ButtonColor", "ButtonHoverColor"), cx, |_| crate::logging::open_console()))
            .child(self.styled_button("close-console", "Close", 70.0, true, ("ButtonColor", "ButtonHoverColor"), cx, |_| crate::logging::close_console()));
        vec![
            self.card(S, "Diagnostics")
                .child(self.row("Debug console", "Shows the detailed log in a terminal", console_buttons))
                .child(self.row("Log files", "Documents/nanoMIDIPlayer/logs", self.small_button("open-logs", "Open folder", cx, |_| {
                    net::open_folder(&config::base_dir().join("logs"))
                }))),
            self.card(S, "Data")
                .child(self.row("Data folder", "Config, themes, cache and songs", self.small_button("open-data", "Open folder", cx, |_| net::open_folder(&config::base_dir()))))
                .child(self.row(
                    "Usage statistics",
                    "Sends an anonymous \"app is running\" ping to nanomidi.net (applies after restart)",
                    self.set_switch("a-telemetry", ui.send_telemetry, cx, |c, v| c.app_ui.send_telemetry = v),
                )),
        ]
    }

    /// Switches the tab without a `Context` (used from button callbacks that only get `&mut Self`).
    fn show_tab_silent(&mut self, tab: crate::core::Tab) {
        self.tab = tab;
        self.open_drop = None;
        self.core.set_tab(tab);
        self.preview.stop();
    }

    pub fn settings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page("settings-page", S).track_scroll(&self.settings_scroll);
        let Some(section) = self.settings_section else {
            let mut page = page.child(self.page_header(S, "Settings", "Pick a category. Changes are saved automatically."));
            for c in self.settings_overview(cx) {
                page = page.child(c);
            }
            return page;
        };
        let cards: Vec<Div> = match section {
            SettingsSection::General => self.general_settings(cx),
            SettingsSection::Players => vec![self.player_settings(Target::Midi, window, cx), self.player_settings(Target::Drums, window, cx)],
            SettingsSection::Hotkeys => self.hotkey_settings(cx),
            SettingsSection::Library => self.library_settings(cx),
            SettingsSection::Theme => self.theme_settings(cx),
            SettingsSection::Updates => self.update_settings(cx),
            SettingsSection::Advanced => self.advanced_settings(cx),
        };
        let title = SettingsSection::ALL.iter().find(|(s, _)| *s == section).map(|(_, l)| *l).unwrap_or("Settings");
        let mut page = page
            .child(div().flex().child(self.settings_back(cx)))
            .child(self.page_header(S, title, section.subtitle()));
        for c in cards {
            page = page.child(c);
        }
        page
    }
}
