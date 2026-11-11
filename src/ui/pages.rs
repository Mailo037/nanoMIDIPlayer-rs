//! "Play a song", "Play drums" and "Live keyboard" tabs.
//!
//! Every tab is three numbered steps: choose, play, adjust. The step that needs the
//! user next is highlighted. Less common options fold into "More settings"; right-click
//! songs, the player, hotkeys and the log for more actions. Colors come from the theme.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, ElementId, FontWeight, MouseButton, Stateful, Window, div, px, relative};

use super::icons::{Icon, icon};
use super::widgets::*;
use super::{Control, DropId, EntryId, MenuItem, RootView, SettingsSection};
use crate::config::{self, HotkeySlot};
use crate::console;
use crate::core::{Core, Tab, Target};
use crate::live::PIANO;
use crate::net;

pub use super::widgets::shade;

const PITCHES: [(&str, i64); 5] = [("Lowest", -24), ("Low", -12), ("Middle", 0), ("High", 12), ("Highest", 24)];
const KEYS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const SPEED_PRESETS: [f64; 7] = [25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0];

type Setter = fn(&mut crate::config::Config, bool);
/// Switch in the "Adjust" step: id, label, explanation, state, setter.
type OptionItem = (&'static str, &'static str, &'static str, bool, Setter);

/// Where a numbered step stands: finished, the one to do now, or still ahead.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Done,
    Active,
    Todo,
}

pub fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

impl RootView {
    /// Scrollable page body (the window can be resized smaller than the content).
    pub(super) fn page(&self, id: &'static str, section: &str) -> Stateful<Div> {
        div()
            .id(id)
            .size_full()
            .overflow_y_scroll()
            .px(px(16.))
            .pt(px(14.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .bg(self.color(section, "BackgroundColor"))
    }

    /// Page title with a one-line explanation of what the tab does.
    pub(super) fn page_header(&self, section: &str, title: &str, subtitle: &str) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .pb(px(4.))
            .child(text(&self.theme, title.to_string(), 20.0, self.color(section, "TextColor")).font_weight(FontWeight::SEMIBOLD))
            .child(self.hint(section, subtitle))
    }

    /// A rounded card with an optional title.
    pub fn card(&self, section: &str, title: &str) -> Div {
        let base = self.color(section, "BackgroundColor");
        let mut d = div()
            .rounded(px(R_CARD))
            .bg(shade(base, 0.035))
            .border_1()
            .border_color(shade(base, 0.08))
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.));
        if !title.is_empty() {
            d = d.child(text(&self.theme, title.to_string(), 13.0, self.color(section, "TextColor")).font_weight(FontWeight::SEMIBOLD));
        }
        d
    }

    /// One numbered step: badge on the left, title (+ optional trailing element) and body.
    pub(super) fn step_card(&self, section: &str, number: u8, step: Step, title: &str, trailing: Option<AnyElement>, body: Div) -> Div {
        let base = self.color(section, "BackgroundColor");
        let text_c = self.color(section, "TextColor");
        let dim = self.color(section, "TextColorDisabled");
        let accent = self.color(section, "PlayColor");
        let badge = div()
            .w(px(24.))
            .h(px(24.))
            .flex_shrink_0()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if step == Step::Active { accent } else { shade(base, 0.13) })
            .child(match step {
                Step::Done => icon(Icon::Check, 14.0, text_c).into_any_element(),
                _ => text(&self.theme, number.to_string(), 12.0, if step == Step::Active { text_c } else { dim })
                    .font_weight(FontWeight::BOLD)
                    .into_any_element(),
            });
        let head = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(24.))
            .child(div().flex_1().min_w_0().child(text(&self.theme, title.to_string(), 14.0, text_c).font_weight(FontWeight::SEMIBOLD)))
            .when_some(trailing, |d, t| d.child(t));
        div()
            .rounded(px(R_CARD))
            .bg(shade(base, 0.035))
            .border_1()
            .border_color(if step == Step::Active { accent } else { shade(base, 0.08) })
            .p(px(12.))
            .flex()
            .gap(px(12.))
            .child(badge)
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(10.)).child(head).child(body))
    }

    pub(super) fn hint(&self, section: &str, s: &str) -> Div {
        paragraph(&self.theme, s, 12.0, self.color(section, "TextColorDisabled"))
    }

    /// A settings-style row: label (+ optional description) left, control right.
    pub(super) fn form_row(&self, section: &str, label: &str, description: &str, control: impl IntoElement) -> Div {
        let mut left = div().flex_1().min_w(px(120.)).flex().flex_col().gap(px(2.)).child(self.label(label, section));
        if !description.is_empty() {
            left = left.child(self.hint(section, description));
        }
        div().flex().flex_wrap().items_center().gap(px(12.)).min_h(px(ROW_H)).child(left).child(control)
    }

    /// A row that belongs to the switch above it (shown indented).
    pub(super) fn sub_row(&self, section: &str, label: &str, control: impl IntoElement) -> Div {
        self.form_row(section, label, "", control).pl(px(16.))
    }

    pub(super) fn divider(&self, section: &str) -> Div {
        div().h(px(1.)).w_full().bg(shade(self.color(section, "BackgroundColor"), 0.08))
    }

    pub(super) fn entry_colors(&self, section: &str, prefix: &str) -> EntryColors {
        EntryColors {
            bg: self.color(section, &format!("{prefix}BackColor")),
            border: self.color(section, &format!("{prefix}BorderColor")),
            text: self.color(section, "TextColor"),
            placeholder: self.color(section, "TextColorDisabled"),
        }
    }

    /// Key name in a small outlined box, in the section's secondary text color.
    pub(super) fn key_hint(&self, section: &str, key: &str) -> Div {
        keycap(&self.theme, &key.to_uppercase(), self.color(section, "TextColorDisabled"))
    }

    /// Neutral button sized to its label, with an optional icon.
    pub(super) fn plain_button(
        &self,
        id: impl Into<String>,
        glyph: Option<Icon>,
        label: &str,
        section: &str,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(id.into().into()),
                label: label.to_string().into(),
                width: 0.0,
                height: ROW_H,
                enabled: true,
                colors: self.button_colors(section, "ButtonColor", "ButtonHoverColor"),
                icon: glyph.map(|g| (g, 15.0)),
            },
            Self::click(cx, move |this, _, cx| {
                f(this, cx);
                cx.notify();
            }),
        )
    }

    fn player_of(&self, target: Target) -> std::sync::Arc<crate::player::Player> {
        match target {
            Target::Midi => self.core.piano.clone(),
            Target::Drums => self.core.drums.clone(),
        }
    }

    /// Shown instead of silently doing nothing when the system blocks key presses.
    fn keys_problem(&self, section: &str) -> Option<Div> {
        let message = crate::keys::problem()?;
        let warn = self.color("MidiPlayer", "PausedColor");
        Some(
            div()
                .rounded(px(R_CARD))
                .bg(shade(self.color(section, "BackgroundColor"), 0.035))
                .border_1()
                .border_color(warn)
                .p(px(12.))
                .flex()
                .gap(px(10.))
                .child(icon(Icon::Warning, 18.0, warn))
                .child(div().flex_1().min_w_0().child(paragraph(&self.theme, message, 12.0, self.color(section, "TextColor")))),
        )
    }

    // ------------------------------------------------------------ right-click menus

    /// Playback actions (player card / Play button).
    pub(super) fn playback_menu(&self, target: Target) -> Vec<MenuItem> {
        let player = self.player_of(target);
        let running = player.is_running();
        let paused = player.is_paused();
        let has_file = self.core.current_file(target).is_some();
        let hk = config::get().read().hotkeys.clone();
        let speed = (player.speed() * 100.0).round();
        let loop_on = {
            let c = config::get().read();
            if target == Target::Midi { c.midi_player.loop_song } else { c.drums.loop_song }
        };
        let mut items = vec![
            MenuItem::title("Playback"),
            if !running {
                MenuItem::new(Icon::Play, "Play", move |this, _, cx| {
                    this.core.start(target);
                    cx.notify();
                })
                .hint(hk.play.to_uppercase())
                .enabled(has_file)
            } else {
                MenuItem::new(if paused { Icon::Play } else { Icon::Pause }, if paused { "Resume" } else { "Pause" }, move |this, _, cx| {
                    this.core.pause(target);
                    cx.notify();
                })
                .hint(hk.pause.to_uppercase())
            },
            MenuItem::new(Icon::Restart, "Start again from the beginning", move |this, _, cx| {
                this.core.restart(target);
                cx.notify();
            })
            .enabled(has_file),
            MenuItem::new(Icon::Stop, "Stop", move |this, _, cx| {
                this.core.stop(target);
                cx.notify();
            })
            .hint(hk.stop.to_uppercase())
            .enabled(running),
            MenuItem::Separator,
            MenuItem::title(format!("Speed · now {speed}%")),
        ];
        for p in SPEED_PRESETS {
            let label = if p == 100.0 { "100%  (normal)".to_string() } else { format!("{p}%") };
            items.push(MenuItem::check(label, (speed - p).abs() < 0.5, move |this, _, cx| {
                this.core.set_speed(target, p);
                this.core.bump();
                console::toast(format!("Speed {p}%"));
                cx.notify();
            }));
        }
        items.push(MenuItem::Separator);
        items.push(MenuItem::check("Repeat song", loop_on, move |this, _, cx| {
            config::get().update(|c| match target {
                Target::Midi => c.midi_player.loop_song = !loop_on,
                Target::Drums => c.drums.loop_song = !loop_on,
            });
            this.core.bump();
            cx.notify();
        }));
        items.push(MenuItem::new(Icon::Sliders, "Player settings…", |this, _, cx| this.show_settings(SettingsSection::Players, cx)));
        items
    }

    /// Song actions (song step).
    fn song_menu(&self, target: Target) -> Vec<MenuItem> {
        let current = self.core.current_file(target);
        let has = current.is_some();
        let path = current.clone().unwrap_or_default();
        let deletes = has && Core::removal_deletes_file(&path);
        let count = {
            let st = self.core.state.lock();
            match target {
                Target::Midi => st.midi.files.len(),
                Target::Drums => st.drums.files.len(),
            }
        };
        let (p1, p2, p3) = (path.clone(), path.clone(), path.clone());
        let mut items = vec![
            MenuItem::title(if has { ellipsize(&file_name(&path), 11.0, 220.0) } else { "Song".into() }),
            MenuItem::new(Icon::Play, "Play from the beginning", move |this, _, cx| {
                this.core.restart(target);
                cx.notify();
            })
            .enabled(has),
            MenuItem::new(Icon::Folder, "Open a MIDI file…", move |this, _, cx| {
                this.core.select_file(target);
                cx.notify();
            }),
        ];
        if target == Target::Midi {
            items.push(MenuItem::new(Icon::Search, "Find songs in the MIDI Hub", |this, _, cx| this.show_tab(Tab::Hub, cx)));
        }
        items.extend([
            MenuItem::Separator,
            MenuItem::new(Icon::External, format!("Show in {}", net::FILE_MANAGER), move |_, _, _| net::reveal(std::path::Path::new(&p1))).enabled(has),
            MenuItem::new(Icon::Copy, "Copy file path", move |this, _, cx| this.copy_text(p2.clone(), "File path", cx)).enabled(has),
            MenuItem::Separator,
            MenuItem::new(Icon::Trash, if deletes { "Delete downloaded song" } else { "Remove from list" }, move |this, _, cx| {
                this.core.remove_file(target, &p3);
                cx.notify();
            })
            .enabled(has)
            .danger(),
            MenuItem::new(Icon::Close, "Clear the whole list", move |this, _, cx| {
                this.core.clear_list(target);
                console::toast("Song list cleared");
                cx.notify();
            })
            .hint(format!("{count} songs"))
            .enabled(count > 0)
            .danger(),
        ]);
        items
    }

    fn log_menu(&self) -> Vec<MenuItem> {
        vec![
            MenuItem::title("Log"),
            MenuItem::new(Icon::Copy, "Copy log", |this, _, cx| this.copy_text(console::all_text(), "Log", cx)),
            MenuItem::new(Icon::Trash, "Clear log", |_, _, cx| {
                console::clear();
                cx.notify();
            }),
            MenuItem::Separator,
            MenuItem::new(Icon::Sliders, "Log settings…", |this, _, cx| this.show_settings(SettingsSection::General, cx)),
        ]
    }

    // ------------------------------------------------------------------ step 1: song

    fn file_dropdown(&self, target: Target, id: DropId, section: &str, cx: &mut Context<Self>) -> Div {
        let files = {
            let state = self.core.state.lock();
            match target {
                Target::Midi => state.midi.clone(),
                Target::Drums => state.drums.clone(),
            }
        };
        let value = files.current.as_deref().map(file_name).unwrap_or_else(|| "No song selected".into());
        let names = files.files.iter().map(|f| file_name(f)).collect();
        let paths = files.files.clone();
        self.dropdown(id, value, names, 0.0, !files.files.is_empty(), self.dropdown_colors(section), cx, move |this, i, _, cx| {
            if let Some(path) = paths.get(i) {
                this.core.switch_file(target, path.clone());
            }
            cx.notify();
        })
    }

    fn song_step(&self, target: Target, id: DropId, section: &str, cx: &mut Context<Self>) -> Div {
        let has_file = self.core.current_file(target).is_some();
        let open = self.plain_button(format!("select-{target:?}"), Some(Icon::Folder), "Open file…", section, cx, move |this, _| {
            this.core.select_file(target);
        });
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().min_w(px(130.)).child(self.file_dropdown(target, id, section, cx)))
            .child(with_tooltip(open, self.tooltips(), "Open a MIDI file (.mid | .midi)", &self.theme));
        if target == Target::Midi {
            let hub = self.plain_button("song-open-hub", Some(Icon::Search), "Find in Hub", section, cx, |this, cx| this.show_tab(Tab::Hub, cx));
            row = row.child(with_tooltip(hub, self.tooltips(), "Browse and download songs from nanomidi.net", &self.theme));
        }
        let mut body = div().flex().flex_col().gap(px(8.)).child(row);
        if !has_file {
            body = body.child(self.hint(
                section,
                if target == Target::Midi {
                    "No song yet. Open a MIDI file from your computer or find one in the MIDI Hub."
                } else {
                    "No song yet. Open a MIDI file that has a drum track."
                },
            ));
        } else if target == Target::Drums {
            body = body.child(self.hint(section, "Only the drum track of the file is played (GM drums)."));
        }
        self.step_card(section, 1, if has_file { Step::Done } else { Step::Active }, "Choose a song", None, body)
            .on_mouse_down(MouseButton::Right, Self::menu_trigger(cx, move |this| this.song_menu(target)))
    }

    // ------------------------------------------------------------------ step 2: play

    /// Pill button with icon, label and the hotkey that does the same.
    fn transport_button(&self, id: String, glyph: Icon, label: &str, key: &str, enabled: bool, colors: ButtonColors, on: ClickHandler) -> Stateful<Div> {
        let color = if enabled { colors.text } else { colors.text_disabled };
        button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(id.into()),
                label: label.to_string().into(),
                width: 0.0,
                height: 40.0,
                enabled,
                colors,
                icon: Some((glyph, 16.0)),
            },
            on,
        )
        .rounded_full()
        .px(px(14.))
        .gap(px(8.))
        .child(keycap(&self.theme, &key.to_uppercase(), color))
    }

    fn round_icon_button(&self, id: String, glyph: Icon, section: &str, enabled: bool, tooltip: &'static str, on: ClickHandler) -> Stateful<Div> {
        let el = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(id.into()),
                label: "".into(),
                width: 40.0,
                height: 40.0,
                enabled,
                colors: self.button_colors(section, "ButtonColor", "ButtonHoverColor"),
                icon: Some((glyph, 16.0)),
            },
            on,
        )
        .rounded_full();
        with_tooltip(el, self.tooltips(), tooltip, &self.theme)
    }

    /// Current position and total length of the song, as text.
    fn timeline_times(&self, target: Target) -> (String, String) {
        let player = self.player_of(target);
        let length = {
            let state = self.core.state.lock();
            match target {
                Target::Midi => state.midi.length,
                Target::Drums => state.drums.length,
            }
        };
        let show = config::get().read().app_ui.timestamp;
        let (mut pos, len) = if player.is_running() { player.position() } else { (0.0, length) };
        if let Some((_, frac)) = self.scrub.filter(|(t, _)| *t == target) {
            pos = len * frac as f64;
        }
        let position = if show { crate::midi_file::format_time(pos) } else { "–:––".to_string() };
        (position, crate::midi_file::format_time(len))
    }

    /// Step 2: Play/Pause + Stop + Restart, the timeline and the speed.
    fn player_step(&mut self, target: Target, section: &str, speed: Control, keyboard: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let player = self.player_of(target);
        let running = player.is_running();
        let paused = player.is_paused();
        let has_file = self.core.current_file(target).is_some();
        let hotkeys = config::get().read().hotkeys.clone();
        let text_c = self.color(section, "TextColor");
        let dim = self.color(section, "TextColorDisabled");

        let auto_paused = paused && self.core.auto_paused[target as usize].load(std::sync::atomic::Ordering::Acquire);
        let (status, status_color) = if !running {
            (if has_file { "Ready" } else { "No song" }, dim)
        } else if paused {
            ("Paused", self.color(section, "PausedColor"))
        } else {
            ("Playing", self.color(section, "PlayingColor"))
        };
        let title = if !has_file && !running {
            "Play"
        } else if !running {
            if keyboard { "Press Play, then click into your game" } else { "Press Play to send the notes to your MIDI device" }
        } else if auto_paused {
            "Paused. Click back into your game to continue"
        } else if paused {
            "Paused"
        } else if keyboard {
            "Playing. The keys go to the window you click into"
        } else {
            "Playing on your MIDI device"
        };

        let (label, glyph, bg, hover, key) = if !running && !has_file {
            // Nothing to play yet: neutral instead of a green button that does nothing.
            ("Play", Icon::Play, "ButtonColor", "ButtonHoverColor", hotkeys.play.clone())
        } else if !running {
            ("Play", Icon::Play, "PlayColor", "PlayColorHover", hotkeys.play.clone())
        } else if paused {
            ("Resume", Icon::Play, "PausedColor", "PausedColorHover", hotkeys.pause.clone())
        } else {
            ("Pause", Icon::Pause, "PlayingColor", "PlayingColorHover", hotkeys.pause.clone())
        };
        let play = self.transport_button(
            format!("play-{target:?}"),
            glyph,
            label,
            &key,
            running || has_file,
            self.button_colors(section, bg, hover),
            Self::click(cx, move |this, _, cx| {
                this.core.play_button(target);
                cx.notify();
            }),
        );
        let stop_colors = if running {
            self.button_colors(section, "StopColor", "StopColorHover")
        } else {
            self.button_colors(section, "ButtonColor", "ButtonHoverColor")
        };
        let stop = self.transport_button(
            format!("stop-{target:?}"),
            Icon::Stop,
            "Stop",
            &hotkeys.stop,
            running,
            stop_colors,
            Self::click(cx, move |this, _, cx| {
                this.core.stop(target);
                cx.notify();
            }),
        );
        let restart = self.round_icon_button(
            format!("restart-{target:?}"),
            Icon::Restart,
            section,
            has_file,
            "Start again from the beginning",
            Self::click(cx, move |this, _, cx| {
                this.core.restart(target);
                cx.notify();
            }),
        );
        let more = self.round_icon_button(format!("more-{target:?}"), Icon::More, section, true, "More: speed presets, repeat, …", {
            let open = Self::handler(cx, move |this, pos: gpui::Point<gpui::Pixels>, _, cx| {
                let items = this.playback_menu(target);
                this.open_drop = None;
                this.context_menu = Some(super::menu::ContextMenu { position: pos, items });
                cx.notify();
            });
            Box::new(move |ev: &gpui::ClickEvent, window, cx| open(ev.position(), window, cx))
        });

        let (position, total) = self.timeline_times(target);
        let timeline = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(text(&self.theme, position, 12.0, text_c).whitespace_nowrap())
            .child(div().flex_1().min_w_0().child(self.timeline_bar(target, section, cx)))
            .child(text(&self.theme, total, 12.0, dim).whitespace_nowrap());

        let slider = self.control_slider(speed, 0.0, true, self.slider_colors(section, "SpeedSlider"), cx);
        let entry = self.entry(EntryId::Control(speed), 56.0, "100", true, self.entry_colors(section, "SpeedValueBox"), window, cx);
        let reset = self.reset_button(
            &format!("reset-{speed:?}"),
            section,
            true,
            "Back to normal speed (100%)",
            Self::click(cx, move |this, _, cx| {
                speed.reset(&this.core);
                cx.notify();
            }),
        );
        // The "speedup" slot of the original config slows down and "slowdown" speeds up.
        let speed_keys = div()
            .flex()
            .items_center()
            .gap(px(5.))
            .child(self.key_hint(section, &hotkeys.speedup))
            .child(text(&self.theme, "slower", 11.0, dim))
            .child(div().w(px(4.)))
            .child(self.key_hint(section, &hotkeys.slowdown))
            .child(text(&self.theme, "faster", 11.0, dim));
        let speed_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.))
            .child(self.label("Speed", section))
            .child(with_tooltip(slider, self.tooltips(), "Playback speed (1-500%)\nRight-click the player for presets", &self.theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(with_tooltip(entry, self.tooltips(), "Type an exact speed in %", &self.theme))
                    .child(text(&self.theme, "%", 13.0, dim))
                    .child(reset),
            )
            .child(speed_keys);

        let status_chip = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .flex_shrink_0()
            .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(status_color))
            .child(text(&self.theme, status, 12.0, text_c).whitespace_nowrap());

        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(with_tooltip(play, self.tooltips(), "Start or pause the song.\nClick into your game window after starting.", &self.theme))
                    .child(stop)
                    .child(restart)
                    .child(more),
            )
            .child(timeline)
            .child(speed_row);

        let step = if has_file || running { Step::Active } else { Step::Todo };
        self.step_card(section, 2, step, title, Some(status_chip.into_any_element()), body)
            .on_mouse_down(MouseButton::Right, Self::menu_trigger(cx, move |this| this.playback_menu(target)))
    }

    /// Progress bar of the song; click or drag it to jump (while a song is playing or paused).
    fn timeline_bar(&self, target: Target, section: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let player = self.player_of(target);
        let running = player.is_running();
        let (pos, len) = if running { player.position() } else { (0.0, 0.0) };
        let show_time = config::get().read().app_ui.timestamp;
        let scrubbing = self.scrub.filter(|(t, _)| *t == target).map(|(_, f)| f);
        let frac = scrubbing.unwrap_or(if len > 0.0 && show_time { (pos / len).clamp(0.0, 1.0) as f32 } else { 0.0 });
        let group: gpui::SharedString = format!("timeline-{target:?}").into();
        let bounds_map = self.scrub_bounds.clone();
        let knob = div()
            .absolute()
            .top(px(2.))
            .left(relative(frac))
            .ml(px(-6.))
            .w(px(12.))
            .h(px(12.))
            .rounded_full()
            .bg(self.color(section, "TextColor"))
            .shadow_md()
            .when(scrubbing.is_none(), |d| d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)));
        let on_down = Self::handler(cx, move |this, ev: gpui::MouseDownEvent, _, cx| {
            this.scrub = Some((target, 0.0));
            this.scrub_to(ev.position.x);
            cx.notify();
        });
        let bar = div()
            .id(ElementId::Name(format!("timeline-{target:?}").into()))
            .group(group)
            .relative()
            .w_full()
            .h(px(16.))
            .flex()
            .items_center()
            .child(
                gpui::canvas(move |b, _, _| {
                    bounds_map.borrow_mut().insert(target, b);
                }, |_, _, _, _| {})
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .h(px(if scrubbing.is_some() { 8. } else { 6. }))
                    .w_full()
                    .rounded_full()
                    .bg(self.color(section, "SpeedSliderBackColor"))
                    .child(div().h_full().w(relative(frac)).rounded_full().bg(self.color(section, "PlayColor"))),
            );
        if !running {
            return with_tooltip(bar, self.tooltips(), "Start a song to jump around in it", &self.theme);
        }
        let bar = bar.child(knob).cursor_pointer().on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            cx.stop_propagation();
            on_down(ev.clone(), window, cx)
        });
        with_tooltip(bar, self.tooltips(), "Click or drag to jump", &self.theme)
    }

    // ---------------------------------------------------------------- step 3: adjust

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

    /// Switches with their explanation always visible, two per row when there is room.
    fn option_grid(&self, section: &str, items: Vec<OptionItem>, cx: &mut Context<Self>) -> Div {
        let text_c = self.color(section, "TextColor");
        let dim = self.color(section, "TextColorDisabled");
        let colors = self.switch_colors(section);
        let mut grid = div().flex().flex_wrap().gap_y(px(12.));
        for (id, label, description, checked, set) in items {
            let toggle = Self::handler(cx, move |this, _: (), _, cx| {
                config::get().update(|c| set(c, !checked));
                this.core.bump();
                cx.notify();
            });
            grid = grid.child(
                div()
                    .id(id)
                    .flex_basis(relative(0.5))
                    .flex_grow()
                    .min_w(px(200.))
                    .pr(px(12.))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(switch_track(checked, colors))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(text(&self.theme, label, 13.0, text_c))
                            .child(paragraph(&self.theme, description, 11.0, dim)),
                    )
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        toggle((), window, cx)
                    }),
            );
        }
        grid
    }

    /// Step 3: the everyday options plus the "More settings" switch.
    fn adjust_step(&self, section: &str, items: Vec<OptionItem>, summary: &str, cx: &mut Context<Self>) -> Div {
        let base = self.color(section, "BackgroundColor");
        let optional = div()
            .h(px(18.))
            .px(px(8.))
            .flex_shrink_0()
            .rounded_full()
            .border_1()
            .border_color(shade(base, 0.18))
            .flex()
            .items_center()
            .child(text(&self.theme, "optional", 11.0, self.color(section, "TextColorDisabled")));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.option_grid(section, items, cx))
            .child(self.more_toggle(section, summary, cx));
        self.step_card(section, 3, Step::Todo, "Adjust how it plays", Some(optional.into_any_element()), body)
    }

    /// Clickable "More settings" row.
    fn more_toggle(&self, section: &str, summary: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.advanced;
        let base = self.color(section, "BackgroundColor");
        let hover = shade(base, 0.12);
        div()
            .id("advanced-toggle")
            .h(px(ROW_H))
            .px(px(10.))
            .rounded(px(R_CONTROL))
            .bg(shade(base, 0.08))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }, 14.0, self.color(section, "TextColor")))
            .child(text(&self.theme, if open { "Fewer settings" } else { "More settings" }, 13.0, self.color(section, "TextColor")).whitespace_nowrap())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(text(&self.theme, if open { String::new() } else { summary.to_string() }, 12.0, self.color(section, "TextColorDisabled")).whitespace_nowrap()),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.advanced = !this.advanced;
                cx.notify();
            }))
    }

    /// Slider + value + reset for "max keys at once".
    fn finger_control(&self, control: Control, section: &str, cx: &mut Context<Self>) -> Div {
        let limit = control.get(&self.core);
        let display = if limit > 10.0 { "No limit".to_string() } else { format!("{} keys", limit as i64) };
        let slider = self.control_slider(control, 130.0, true, self.slider_colors(section, "SpeedSlider"), cx);
        let reset = self.reset_button(
            &format!("reset-{control:?}"),
            section,
            true,
            "Back to no limit",
            Self::click(cx, move |this, _, cx| {
                control.reset(&this.core);
                cx.notify();
            }),
        );
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(with_tooltip(slider, self.tooltips(), "Most keys held at the same time (1-10)\nSlide to the end for no limit", &self.theme))
            .child(div().w(px(62.)).child(self.label(&display, section)))
            .child(reset)
    }

    pub(super) fn hotkey_row(&self, section: &str, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().flex_wrap().gap(px(10.));
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
                    .gap(px(4.))
                    .child(text(&self.theme, label, 12.0, self.color(section, "TextColorDisabled")))
                    .child(self.hotkey_button(slot, section, cx)),
            );
        }
        row
    }

    fn hotkey_hint(&self) -> &'static str {
        if self.core.state.lock().capturing.is_some() {
            "Press the new key now…"
        } else {
            "These keys work everywhere, also while your game is in front. Click one to change it, right-click to reset it."
        }
    }

    fn log_box(&self, section: &str, max: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = config::get().read().app_ui.console;
        let body = if enabled {
            self.console_box(section, 0.0, 0.0, max).w_full().h(px(130.)).rounded(px(R_CONTROL)).p(px(8.))
        } else {
            self.hint(section, "The log is turned off in Settings → General.")
        };
        self.card(section, "Log")
            .id(ElementId::Name(format!("log-{section}").into()))
            .child(body)
            .on_mouse_down(MouseButton::Right, Self::menu_trigger(cx, |this| this.log_menu()))
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
        let mut row = div().id(id).h(px(ROW_H)).flex().items_center().rounded_full().overflow_hidden().bg(off).p(px(3.)).gap(px(3.));
        for (i, (label, enabled)) in options.into_iter().enumerate() {
            let is_sel = i == selected;
            let select = on_select.clone();
            let mut seg = div()
                .id(ElementId::NamedInteger(id.into(), i as u64))
                .h(px(ROW_H - 6.0))
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

    /// Tip line at the bottom of the player pages.
    fn tip(&self, section: &str) -> Div {
        div().flex().justify_center().pt(px(4.)).child(self.hint(section, "Tip: right-click a song, the player or a hotkey for more options."))
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

        let options: Vec<OptionItem> = vec![
            ("mp-sustain", "Hold pedal", "Sustain: holds Space while the song uses the pedal", cfg.sustain, |c, v| c.midi_player.sustain = v),
            ("mp-nodoubles", "No double presses", "Skips a key that is already held down", cfg.no_doubles, |c, v| c.midi_player.no_doubles = v),
            ("mp-velocity", "Loud and soft notes", "Velocity: Alt + key. Needs a game that supports it", cfg.velocity, |c, v| c.midi_player.velocity = v),
            ("mp-88", "Full 88 keys", "Very low and high notes with Ctrl + key. Needs a game that supports it", cfg.keys_88, |c, v| c.midi_player.keys_88 = v),
            ("mp-loop", "Repeat song", "Starts again when the song ends", cfg.loop_song, |c, v| c.midi_player.loop_song = v),
            ("mp-mistakes", "Human mistakes", "Plays slightly off, like a person would", cfg.random_fail.enabled, |c, v| c.midi_player.random_fail.enabled = v),
        ];

        let problem = if use_out { None } else { self.keys_problem(S) };
        let mut page = self
            .page("midi-page", S)
            .child(self.page_header(S, "Play a song", "Turns a MIDI file into key presses in your game."))
            .when_some(problem, |p, b| p.child(b))
            .child(self.song_step(Target::Midi, DropId::MidiFile, S, cx))
            .child(self.player_step(Target::Midi, S, Control::MidiSpeed, !use_out, window, cx))
            .child(self.adjust_step(S, options, "output device, octave, key limit, hotkeys, log", cx));

        if self.advanced {
            let mode = self.segmented(
                "mode",
                S,
                [("Keyboard keys", true), ("MIDI device", has_outputs)],
                use_out as usize,
                "Keyboard keys: presses keys in your game.\nMIDI device: sends the notes to a MIDI output\n(no keys are pressed).",
                cx,
                |this, i| this.core.set_use_midi_output(i == 1),
            );
            let mut output_ctl = div().flex().flex_wrap().items_center().gap(px(8.)).child(mode);
            if use_out {
                let out_items = outputs.clone();
                let out_dd = self.dropdown(DropId::MidiOutput, output.clone().unwrap_or_default(), out_items.clone(), 200.0, true,
                    self.dropdown_colors(S), cx, move |this, i, _, cx| {
                        if let Some(name) = out_items.get(i) {
                            this.core.state.lock().output = Some(name.clone());
                            this.core.bump();
                        }
                        cx.notify();
                    });
                let refresh = self.reset_button("refresh-out", S, true, "Look for MIDI output devices again",
                    Self::click(cx, |this, _, cx| {
                        this.core.refresh_outputs();
                        console::toast("MIDI devices refreshed");
                        cx.notify();
                    }));
                output_ctl = output_ctl.child(out_dd).child(refresh);
            }
            let output_hint = if !has_outputs {
                "No MIDI output device found (for example loopMIDI)"
            } else if use_out {
                "Sends the notes to a MIDI device, no keys are pressed"
            } else {
                "Presses keys in the window that is in front"
            };

            let pitch_label = PITCHES.iter().find(|p| p.1 == cfg.pitch_offset).map(|p| p.0).unwrap_or("Middle");
            let key_label = if (0..12).contains(&cfg.transpose_offset) { KEYS[cfg.transpose_offset as usize] } else { "C" };
            let pitch = self.dropdown(DropId::Pitch, pitch_label.into(), PITCHES.iter().map(|p| p.0.to_string()).collect(), 120.0, true,
                self.dropdown_colors(S), cx, |_, i, _, cx| {
                    config::get().update(|c| c.midi_player.pitch_offset = PITCHES[i].1);
                    cx.notify();
                });
            let key = self.dropdown(DropId::Key, key_label.into(), KEYS.iter().map(|k| k.to_string()).collect(), 120.0, true,
                self.dropdown_colors(S), cx, |_, i, _, cx| {
                    config::get().update(|c| c.midi_player.transpose_offset = i as i64);
                    cx.notify();
                });

            page = page
                .child(
                    self.card(S, "Sound")
                        .child(self.form_row(S, "Send notes to", output_hint, output_ctl))
                        .child(self.divider(S))
                        .child(self.form_row(S, "Octave", "Moves every note up or down", pitch))
                        .child(self.form_row(S, "Key", "Transposes the song. C is the original key", key))
                        .child(self.divider(S))
                        .child(self.form_row(S, "Most keys at once", "Limits chords for games that drop keys", self.finger_control(Control::MidiFingers, S, cx))),
                )
                .child(self.card(S, "Hotkeys").child(self.hint(S, self.hotkey_hint())).child(self.hotkey_row(S, cx)))
                .child(self.log_box(S, 8, cx));
        }
        page.child(self.tip(S))
    }

    // ----------------------------------------------------------------- Drums

    pub fn drums_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        const S: &str = "DrumsMacro";
        let cfg = config::get().read().drums.clone();
        let options: Vec<OptionItem> = vec![
            ("dr-loop", "Repeat song", "Starts again when the song ends", cfg.loop_song, |c, v| c.drums.loop_song = v),
            ("dr-release", "Release keys on pause", "Lets go of all held keys while paused", cfg.release_on_pause, |c, v| c.drums.release_on_pause = v),
        ];
        let problem = self.keys_problem(S);
        let mut page = self
            .page("drums-page", S)
            .child(self.page_header(S, "Play drums", "Plays the drum track of a MIDI file as key presses."))
            .when_some(problem, |p, b| p.child(b))
            .child(self.song_step(Target::Drums, DropId::DrumsFile, S, cx))
            .child(self.player_step(Target::Drums, S, Control::DrumsSpeed, true, window, cx))
            .child(self.adjust_step(S, options, "key limit, hotkeys, log", cx));
        if self.advanced {
            page = page
                .child(self.card(S, "Keys").child(self.form_row(S, "Most keys at once", "Limits hits for games that drop keys", self.finger_control(Control::DrumsFingers, S, cx))))
                .child(self.card(S, "Hotkeys").child(self.hint(S, self.hotkey_hint())).child(self.hotkey_row(S, cx)))
                .child(self.log_box(S, 11, cx));
        }
        page.child(self.tip(S)).child(
            div().flex().justify_center().child(text(&self.theme, "Drums macro created by fearsomeorc1406", 11.0, self.color(S, "TextColorDisabled"))),
        )
    }

    // ---------------------------------------------------------- Live keyboard

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
        let text_c = self.color(S, "TextColor");

        // Step 1: the MIDI keyboard.
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
        let refresh = self.reset_button("refresh-in", "MidiPlayer", !running, "Look for MIDI keyboards again",
            Self::click(cx, |this, _, cx| {
                this.core.refresh_inputs();
                console::toast("MIDI devices refreshed");
                cx.notify();
            }));
        let mut device_body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().items_center().gap(px(8.)).child(div().flex_1().min_w_0().child(dd)).child(refresh));
        if !has_inputs {
            device_body = device_body.child(self.hint(S, "No MIDI keyboard found. Plug one in, then press the refresh button."));
        }
        let device = self
            .step_card(S, 1, if has_inputs { Step::Done } else { Step::Active }, "Choose your MIDI keyboard", None, device_body)
            .on_mouse_down(
                MouseButton::Right,
                Self::menu_trigger(cx, move |_| {
                    vec![
                        MenuItem::title("MIDI keyboard"),
                        MenuItem::new(if running { Icon::Stop } else { Icon::Play }, if running { "Stop listening" } else { "Start listening" }, |this, _, cx| {
                            this.core.toggle_live();
                            cx.notify();
                        })
                        .enabled(running || has_inputs),
                        MenuItem::new(Icon::Restart, "Refresh devices", |this, _, cx| {
                            this.core.refresh_inputs();
                            console::toast("MIDI devices refreshed");
                            cx.notify();
                        })
                        .enabled(!running),
                    ]
                }),
            );

        // Step 2: listen.
        let (label, glyph, bg, hover) = if running {
            ("Stop listening", Icon::Stop, "StopColor", "StopColorHover")
        } else if has_inputs {
            ("Start listening", Icon::Play, "PlayColor", "PlayColorHover")
        } else {
            // Nothing to listen to yet: neutral instead of a green button that does nothing.
            ("Start listening", Icon::Play, "ButtonColor", "ButtonHoverColor")
        };
        let enable = button(
            &self.theme,
            ButtonSpec {
                id: "qw-enable".into(),
                label: label.into(),
                width: 0.0,
                height: 40.0,
                enabled: running || has_inputs,
                colors: self.button_colors(S, bg, hover),
                icon: Some((glyph, 16.0)),
            },
            Self::click(cx, |this, _, cx| {
                this.core.toggle_live();
                cx.notify();
            }),
        )
        .rounded_full()
        .px(px(18.));
        let status_chip = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .flex_shrink_0()
            .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(if running { self.color(S, "PlayColor") } else { self.color(S, "TextColorDisabled") }))
            .child(text(&self.theme, if running { "Listening" } else { "Off" }, 12.0, text_c));
        let listen_body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().flex().child(with_tooltip(enable, self.tooltips(), "Turn the notes you play into key presses", &self.theme)))
            .child(self.piano_widget(S));
        let listen = self.step_card(
            S,
            2,
            if has_inputs || running { Step::Active } else { Step::Todo },
            if running { "Listening. Click into your game and play" } else { "Start listening, then play your keyboard" },
            Some(status_chip.into_any_element()),
            listen_body,
        );

        let options: Vec<OptionItem> = vec![
            ("qw-sustain", "Hold pedal", "Sustain: holds Space while your pedal is down", cfg.sustain, |c, v| c.midi_to_qwerty.sustain = v),
            ("qw-nodoubles", "No double presses", "Skips a key that is already held down", cfg.no_doubles, |c, v| c.midi_to_qwerty.no_doubles = v),
            ("qw-velocity", "Loud and soft notes", "Velocity: Alt + key. Needs a game that supports it", cfg.velocity, |c, v| c.midi_to_qwerty.velocity = v),
            ("qw-88", "Full 88 keys", "Very low and high notes with Ctrl + key. Needs a game that supports it", cfg.keys_88, |c, v| c.midi_to_qwerty.keys_88 = v),
        ];

        let problem = self.keys_problem(S);
        let mut page = self
            .page("qwerty-page", S)
            .child(self.page_header(S, "Live keyboard", "Turns a connected MIDI keyboard into key presses while you play."))
            .when_some(problem, |p, b| p.child(b))
            .child(device)
            .child(listen)
            .child(self.adjust_step(S, options, "pedal, note length, log", cx));

        if self.advanced {
            let entry_colors = self.entry_colors(SET, "ValueBox");
            let cutoff_entry = self.entry(EntryId::Control(Control::QwSustainCutoff), 52.0, "63", true, entry_colors, window, cx);
            let cutoff_slider = self.control_slider(Control::QwSustainCutoff, 120.0, true, self.slider_colors(SET, "Slider"), cx);
            let cutoff_reset = self.reset_button("reset-cutoff", SET, true, "Back to the default (63)",
                Self::click(cx, |this, _, cx| {
                    Control::QwSustainCutoff.reset(&this.core);
                    cx.notify();
                }));

            let hold_on = cfg.custom_hold_length.enabled;
            let hold_sw = self.toggle("qw-hold", "", hold_on, true, S, "Release every key after a fixed time\ninstead of when you let go", cx,
                |_, on| config::get().update(|c| c.midi_to_qwerty.custom_hold_length.enabled = on));

            let mut card = self
                .card(S, "Fine tuning")
                .child(self.form_row(
                    S,
                    "Pedal threshold",
                    "Pedal values above this count as pressed",
                    div().flex().items_center().gap(px(8.)).child(cutoff_slider).child(cutoff_entry).child(cutoff_reset),
                ))
                .child(self.divider(S))
                .child(self.form_row(S, "Fixed note length", "Releases keys after a fixed time", hold_sw));
            if hold_on {
                let hold_slider = self.control_slider(Control::QwNoteLength, 120.0, true, self.slider_colors(SET, "Slider"), cx);
                let hold_entry = self.entry(EntryId::Control(Control::QwNoteLength), 56.0, "0.10", true, entry_colors, window, cx);
                let hold_reset = self.reset_button("reset-qwhold", SET, true, "Back to the default length",
                    Self::click(cx, |this, _, cx| {
                        Control::QwNoteLength.reset(&this.core);
                        cx.notify();
                    }));
                card = card.child(self.sub_row(
                    S,
                    "Length in seconds",
                    div().flex().items_center().gap(px(8.)).child(hold_slider).child(hold_entry).child(hold_reset),
                ));
            }
            // The two key-press methods only exist on Windows.
            if cfg!(windows) {
                let modules = vec!["pynput".to_string(), "keyboard".to_string()];
                let module = self.dropdown(DropId::QwModule, cfg.input_module.clone(), modules.clone(), 130.0, true, self.dropdown_colors(SET), cx,
                    move |_, i, _, cx| {
                        let m = modules[i].clone();
                        config::get().update(|c| c.midi_to_qwerty.input_module = m);
                        cx.notify();
                    });
                card = card.child(self.divider(S)).child(self.form_row(
                    S,
                    "Key press method",
                    "Try \"keyboard\" if a game ignores the keys",
                    with_tooltip(div().id("qw-module").child(module), self.tooltips(), "pynput: virtual keys\nkeyboard: scan codes (try if a game ignores keys)", &self.theme),
                ));
            }
            page = page.child(card).child(self.log_box(S, 13, cx));
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

        let mut piano = div().relative().w_full().h(px(64.)).bg(gpui::rgb(0x333333)).rounded(px(R_CONTROL)).overflow_hidden();
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
