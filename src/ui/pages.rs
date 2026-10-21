//! MIDI Player, Drums Player and MIDI To QWERTY tabs.
//!
//! Every tab starts with a short header, then the essentials (song, playback, a few
//! toggles); less common options fold into "More settings". Right-click songs, the
//! player card, hotkeys and the log for more actions. Colors come from the theme.

use gpui::prelude::*;
use gpui::{Context, Div, ElementId, Hsla, MouseButton, Stateful, Window, div, px, relative};

use super::widgets::*;
use super::{Control, DropId, EntryId, MenuItem, RootView, SettingsSection};
use crate::config::{self, HotkeySlot};
use crate::console;
use crate::core::{Core, Tab, Target};
use crate::live::PIANO;
use crate::net;

const PITCHES: [(&str, i64); 5] = [("Lowest", -24), ("Low", -12), ("Middle", 0), ("High", 12), ("Highest", 24)];
const KEYS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const SPEED_PRESETS: [f64; 7] = [25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0];
const RADIUS: f32 = 16.0;

type Setter = fn(&mut crate::config::Config, bool);

pub fn file_name(path: &str) -> String {
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
            .px(px(14.))
            .pt(px(12.))
            .pb(px(18.))
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
            .pb(px(2.))
            .child(text(&self.theme, title.to_string(), 22.0, self.color(section, "TextColor")).font_weight(gpui::FontWeight::SEMIBOLD))
            .child(self.hint(section, subtitle))
    }

    /// A rounded card with an optional small caption.
    pub fn card(&self, section: &str, title: &str) -> Div {
        let base = self.color(section, "BackgroundColor");
        let mut d = div()
            .rounded(px(RADIUS))
            .bg(shade(base, 0.035))
            .border_1()
            .border_color(shade(base, 0.07))
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.));
        if !title.is_empty() {
            d = d.child(text(&self.theme, title.to_uppercase(), 10.0, self.color(section, "TextColorDisabled")).font_weight(gpui::FontWeight::SEMIBOLD));
        }
        d
    }

    pub(super) fn hint(&self, section: &str, s: &str) -> Div {
        paragraph(&self.theme, s, 11.0, self.color(section, "TextColorDisabled"))
    }

    /// A settings-style row: label (+ optional description) left, control right.
    pub(super) fn form_row(&self, section: &str, label: &str, description: &str, control: impl IntoElement) -> Div {
        let mut left = div().flex_1().min_w(px(110.)).flex().flex_col().gap(px(2.)).child(self.label(label, section));
        if !description.is_empty() {
            left = left.child(self.hint(section, description));
        }
        div().flex().flex_wrap().items_center().gap(px(10.)).min_h(px(32.)).child(left).child(control)
    }

    pub(super) fn divider(&self, section: &str) -> Div {
        div().h(px(1.)).w_full().bg(shade(self.color(section, "BackgroundColor"), 0.07))
    }

    pub(super) fn entry_colors(&self, section: &str, prefix: &str) -> EntryColors {
        EntryColors {
            bg: self.color(section, &format!("{prefix}BackColor")),
            border: self.color(section, &format!("{prefix}BorderColor")),
            text: self.color(section, "TextColor"),
            placeholder: self.color(section, "TextColorDisabled"),
        }
    }

    fn player_of(&self, target: Target) -> std::sync::Arc<crate::player::Player> {
        match target {
            Target::Midi => self.core.piano.clone(),
            Target::Drums => self.core.drums.clone(),
        }
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
                MenuItem::new("▶", "Play", move |this, _, cx| {
                    this.core.start(target);
                    cx.notify();
                })
                .hint(hk.play.to_uppercase())
                .enabled(has_file)
            } else {
                MenuItem::new(if paused { "▶" } else { "❚❚" }, if paused { "Resume" } else { "Pause" }, move |this, _, cx| {
                    this.core.pause(target);
                    cx.notify();
                })
                .hint(hk.pause.to_uppercase())
            },
            MenuItem::new("⟲", "Restart from the beginning", move |this, _, cx| {
                this.core.restart(target);
                cx.notify();
            })
            .enabled(has_file),
            MenuItem::new("■", "Stop", move |this, _, cx| {
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
        items.push(MenuItem::check("Loop song", loop_on, move |this, _, cx| {
            config::get().update(|c| match target {
                Target::Midi => c.midi_player.loop_song = !loop_on,
                Target::Drums => c.drums.loop_song = !loop_on,
            });
            this.core.bump();
            cx.notify();
        }));
        items.push(MenuItem::new("≡", "Player settings…", |this, _, cx| this.show_settings(SettingsSection::Players, cx)));
        items
    }

    /// Song actions (song dropdown card).
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
            MenuItem::new("▶", "Play from the beginning", move |this, _, cx| {
                this.core.restart(target);
                cx.notify();
            })
            .enabled(has),
            MenuItem::new("+", "Open a MIDI file…", move |this, _, cx| {
                this.core.select_file(target);
                cx.notify();
            }),
        ];
        if target == Target::Midi {
            items.push(MenuItem::new("⌕", "Find songs in the MIDI Hub", |this, _, cx| this.show_tab(Tab::Hub, cx)));
        }
        items.extend([
            MenuItem::Separator,
            MenuItem::new("»", "Show in Explorer", move |_, _, _| net::reveal(std::path::Path::new(&p1))).enabled(has),
            MenuItem::new("⧉", "Copy file path", move |this, _, cx| this.copy_text(p2.clone(), "File path", cx)).enabled(has),
            MenuItem::Separator,
            MenuItem::new("✕", if deletes { "Delete downloaded song" } else { "Remove from list" }, move |this, _, cx| {
                this.core.remove_file(target, &p3);
                cx.notify();
            })
            .enabled(has)
            .danger(),
            MenuItem::new("∅", "Clear the whole list", move |this, _, cx| {
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
            MenuItem::new("⧉", "Copy log", |this, _, cx| this.copy_text(console::all_text(), "Log", cx)),
            MenuItem::new("✕", "Clear log", |_, _, cx| {
                console::clear();
                cx.notify();
            }),
            MenuItem::Separator,
            MenuItem::new("≡", "Log settings…", |this, _, cx| this.show_settings(SettingsSection::General, cx)),
        ]
    }

    // ------------------------------------------------------------------ parts

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

    fn song_card(&self, target: Target, id: DropId, section: &str, cx: &mut Context<Self>) -> Div {
        let has_file = self.core.current_file(target).is_some();
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
        if !has_file {
            let mut empty = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(self.hint(section, if target == Target::Midi { "No song yet. Browse for a file or get one from the MIDI Hub." } else { "No song yet. Pick a MIDI file that has a drum track." }));
            if target == Target::Midi {
                empty = empty.child(
                    button(
                        &self.theme,
                        ButtonSpec {
                            id: "song-open-hub".into(),
                            label: "Open MIDI Hub".into(),
                            width: 124.0,
                            height: 26.0,
                            enabled: true,
                            colors: self.button_colors(section, "PlayColor", "PlayColorHover"),
                            icon: None,
                        },
                        Self::click(cx, |this, _, cx| this.show_tab(Tab::Hub, cx)),
                    )
                    .rounded_full(),
                );
            }
            card = card.child(empty);
        } else if target == Target::Drums {
            card = card.child(self.hint(section, "Needs a MIDI file with a drum track (GM drums)."));
        }
        card.on_mouse_down(MouseButton::Right, Self::menu_trigger(cx, move |this| this.song_menu(target)))
    }

    fn round_icon_button(&self, id: String, glyph: &str, section: &str, enabled: bool, tooltip: &'static str, on: ClickHandler) -> Stateful<Div> {
        let el = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(id.into()),
                label: glyph.to_string().into(),
                width: 36.0,
                height: 38.0,
                enabled,
                colors: self.button_colors(section, "ButtonColor", "ButtonHoverColor"),
                icon: None,
            },
            on,
        )
        .rounded_full();
        with_tooltip(el, self.tooltips(), tooltip, &self.theme)
    }

    /// Status, progress bar, Play/Pause + Stop + Restart and speed.
    fn player_card(&mut self, target: Target, section: &str, speed: Control, mode_label: &str, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let player = self.player_of(target);
        let running = player.is_running();
        let paused = player.is_paused();
        let has_file = self.core.current_file(target).is_some();
        let hotkeys = config::get().read().hotkeys.clone();

        let auto_paused = paused && self.core.auto_paused[target as usize].load(std::sync::atomic::Ordering::Acquire);
        let mode_label = if auto_paused { "· click back into your game" } else { mode_label };
        let (status, status_color) = if !running {
            (if has_file { "Ready" } else { "No song" }, self.color(section, "TextColorDisabled"))
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
                width: 150.0,
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
                width: 112.0,
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
        let restart = self.round_icon_button(
            format!("restart-{target:?}"),
            "⟲",
            section,
            has_file,
            "Restart from the beginning",
            Self::click(cx, move |this, _, cx| {
                this.core.restart(target);
                cx.notify();
            }),
        );
        let more = self.round_icon_button(
            format!("more-{target:?}"),
            "⋯",
            section,
            true,
            "More: speed presets, loop, …",
            {
                let open = Self::handler(cx, move |this, pos: gpui::Point<gpui::Pixels>, _, cx| {
                    let items = this.playback_menu(target);
                    this.open_drop = None;
                    this.context_menu = Some(super::menu::ContextMenu { position: pos, items });
                    cx.notify();
                });
                Box::new(move |ev: &gpui::ClickEvent, window, cx| open(ev.position(), window, cx))
            },
        );

        let progress = self.timeline_bar(target, section, cx);

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
            .on_mouse_down(MouseButton::Right, Self::menu_trigger(cx, move |this| this.playback_menu(target)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .min_w_0()
                            .child(div().w(px(10.)).h(px(10.)).rounded_full().bg(status_color).flex_shrink_0())
                            .child(text(&self.theme, status, 16.0, self.color(section, "TextColor")).whitespace_nowrap())
                            .overflow_hidden()
                            .child(lines(&self.theme, mode_label, 11.0, self.color(section, "TextColorDisabled"))),
                    )
                    .child(self.label(&self.timeline_text(target), section)),
            )
            .child(progress)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(with_tooltip(play, self.tooltips(), "Start / pause playback.\nClick into your game window after starting.", &self.theme))
                    .child(stop)
                    .child(restart)
                    .child(more),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(self.label("Speed", section))
                    .child(with_tooltip(slider, self.tooltips(), "Playback speed (1-500%)\nRight-click for presets", &self.theme))
                    .child(with_tooltip(entry, self.tooltips(), "Type an exact speed in %", &self.theme))
                    .child(reset),
            )
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
    fn toggle_grid(&self, section: &str, items: Vec<(&'static str, &str, bool, &'static str, Setter)>, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap_y(px(8.));
        for (id, label, checked, tip, set) in items {
            let sw = self.toggle(id, label, checked, true, section, tip, cx, move |_, on| config::get().update(|c| set(c, on)));
            grid = grid.child(div().w(relative(0.5)).min_w(px(150.)).child(sw));
        }
        grid
    }

    /// Finger limit slider + value + reset.
    fn finger_control(&self, control: Control, section: &str, cx: &mut Context<Self>) -> Div {
        let limit = control.get(&self.core);
        let display = if limit > 10.0 { "No limit".to_string() } else { format!("{} keys", limit as i64) };
        let slider = self.control_slider(control, 130.0, true, self.slider_colors(section, "SpeedSlider"), cx);
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
            .child(with_tooltip(slider, self.tooltips(), "Max simultaneous keys (1-10)\nSlide past 10 for No Limit (∞)", &self.theme))
            .child(div().w(px(70.)).child(self.label(&display, section)))
            .child(reset)
    }

    pub(super) fn hotkey_row(&self, section: &str, cx: &mut Context<Self>) -> Div {
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
                    .child(lines(&self.theme, label, 11.0, self.color(section, "TextColorDisabled")))
                    .child(self.hotkey_button(slot, section, cx)),
            );
        }
        row
    }

    fn hotkey_hint(&self, section: &str) -> String {
        if self.core.state.lock().capturing.is_some() {
            "Press the new key…".into()
        } else {
            let _ = section;
            "Global keys, also work while the game is focused. Click to change, right-click to reset.".into()
        }
    }

    fn log_box(&self, section: &str, max: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = config::get().read().app_ui.console;
        let body = if enabled {
            self.console_box(section, 0.0, 0.0, max).w_full().h(px(130.)).rounded(px(10.)).p(px(6.))
        } else {
            self.hint(section, "The log is turned off in Settings → General.")
        };
        div()
            .id(ElementId::Name(format!("log-{section}").into()))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(self.label("Log", section))
                    .child(self.hint(section, "right-click: copy / clear")),
            )
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

    /// Clickable "More settings" header.
    fn advanced_header(&self, section: &str, summary: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.advanced;
        let hover = shade(self.color(section, "BackgroundColor"), 0.06);
        div()
            .id("advanced-toggle")
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(12.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(text(&self.theme, if open { "▾" } else { "▸" }, 14.0, self.color(section, "TextColor")))
            .child(self.label("More settings", section))
            .child(div().flex_1().min_w_0().child(self.hint(section, if open { "" } else { summary })))
            .on_click(cx.listener(|this, _, _, cx| {
                this.advanced = !this.advanced;
                cx.notify();
            }))
    }

    /// Tip line at the bottom of the player pages.
    fn tip(&self, section: &str) -> Div {
        div().flex().justify_center().pt(px(2.)).child(self.hint(section, "Tip: right-click songs, the player and hotkeys for more options."))
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

        let quick = self.card(S, "Play options").child(self.toggle_grid(
            S,
            vec![
                ("mp-sustain", "Sustain", cfg.sustain, "Holds Space while the sustain pedal is down.\nOnly works on supported games.", |c, v| c.midi_player.sustain = v),
                ("mp-nodoubles", "No Doubles", cfg.no_doubles, "Prevents double-triggering of keys", |c, v| c.midi_player.no_doubles = v),
                ("mp-velocity", "Velocity", cfg.velocity, "Simulates how hard a key is pressed (Alt + key).\nOnly works on supported games.", |c, v| c.midi_player.velocity = v),
                ("mp-88", "88 Keys", cfg.keys_88, "Plays notes outside the 61-key range with Ctrl.\nOnly works on supported games.", |c, v| c.midi_player.keys_88 = v),
                ("mp-loop", "Loop song", cfg.loop_song, "Start again when the song ends", |c, v| c.midi_player.loop_song = v),
                ("mp-mistakes", "Human mistakes", cfg.random_fail.enabled, "Plays slightly off, like a human.\nStrength: Settings → Players", |c, v| c.midi_player.random_fail.enabled = v),
            ],
            cx,
        ));

        let mode_label = if use_out { "· MIDI device" } else { "· Keyboard" };
        let mut page = self
            .page("midi-page", S)
            .child(self.page_header(S, "MIDI Player", "Plays a MIDI file as key presses in your game."))
            .child(self.song_card(Target::Midi, DropId::MidiFile, S, cx))
            .child(self.player_card(Target::Midi, S, Control::MidiSpeed, mode_label, window, cx))
            .child(quick)
            .child(self.advanced_header(S, "output, octave, key limit, hotkeys, log", cx));

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
                let refresh = self.reset_button("refresh-out", S, true, "Refresh MIDI output devices",
                    Self::click(cx, |this, _, cx| {
                        this.core.refresh_outputs();
                        console::toast("MIDI devices refreshed");
                        cx.notify();
                    }));
                output_ctl = output_ctl.child(out_dd).child(refresh);
            }
            let output_hint = if !has_outputs {
                "No MIDI output device found (e.g. loopMIDI)"
            } else if use_out {
                "Sends notes to a MIDI device, no keys"
            } else {
                "Presses keys in the focused window"
            };

            let pitch_label = PITCHES.iter().find(|p| p.1 == cfg.pitch_offset).map(|p| p.0).unwrap_or("Middle");
            let key_label = if (0..12).contains(&cfg.transpose_offset) { KEYS[cfg.transpose_offset as usize] } else { "C" };
            let pitch = self.dropdown(DropId::Pitch, pitch_label.into(), PITCHES.iter().map(|p| p.0.to_string()).collect(), 110.0, true,
                self.dropdown_colors(S), cx, |_, i, _, cx| {
                    config::get().update(|c| c.midi_player.pitch_offset = PITCHES[i].1);
                    cx.notify();
                });
            let key = self.dropdown(DropId::Key, key_label.into(), KEYS.iter().map(|k| k.to_string()).collect(), 76.0, true,
                self.dropdown_colors(S), cx, |_, i, _, cx| {
                    config::get().update(|c| c.midi_player.transpose_offset = i as i64);
                    cx.notify();
                });

            page = page.child(
                self.card(S, "Sound")
                    .child(self.form_row(S, "Output", output_hint, output_ctl))
                    .child(self.divider(S))
                    .child(self.form_row(S, "Octave", "Shift every note up or down", with_tooltip(div().id("pitch-tt").child(pitch), self.tooltips(), "Shift playback by octaves", &self.theme)))
                    .child(self.form_row(S, "Transpose", "C = original key", with_tooltip(div().id("key-tt").child(key), self.tooltips(), "Transpose by semitones (C = none)", &self.theme)))
                    .child(self.divider(S))
                    .child(self.form_row(S, "Max. keys at once", "Limits chords for games that drop keys", self.finger_control(Control::MidiFingers, S, cx))),
            )
            .child(self.card(S, "Hotkeys").child(self.hint(S, &self.hotkey_hint(S))).child(self.hotkey_row(S, cx)))
            .child(self.card(S, "").child(self.log_box(S, 8, cx)));
        }
        page.child(self.tip(S))
    }

    // ----------------------------------------------------------------- Drums

    pub fn drums_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        const S: &str = "DrumsMacro";
        let cfg = config::get().read().drums.clone();
        let quick = self.card(S, "Play options").child(self.toggle_grid(
            S,
            vec![
                ("dr-loop", "Loop song", cfg.loop_song, "Start again when the song ends", |c, v| c.drums.loop_song = v),
                ("dr-release", "Release on pause", cfg.release_on_pause, "Let go of all held keys while paused", |c, v| c.drums.release_on_pause = v),
            ],
            cx,
        ));
        let mut page = self
            .page("drums-page", S)
            .child(self.page_header(S, "Drums Player", "Plays the drum track of a MIDI file on your keyboard."))
            .child(self.song_card(Target::Drums, DropId::DrumsFile, S, cx))
            .child(self.player_card(Target::Drums, S, Control::DrumsSpeed, "· Drums", window, cx))
            .child(quick)
            .child(self.advanced_header(S, "key limit, hotkeys, log", cx));
        if self.advanced {
            page = page
                .child(self.card(S, "Keys").child(self.form_row(S, "Max. keys at once", "Limits hits for games that drop keys", self.finger_control(Control::DrumsFingers, S, cx))))
                .child(self.card(S, "Hotkeys").child(self.hint(S, &self.hotkey_hint(S))).child(self.hotkey_row(S, cx)))
                .child(self.card(S, "").child(self.log_box(S, 11, cx)));
        }
        page.child(self.tip(S)).child(
            div().flex().justify_center().child(
                lines(&self.theme, "♡ Drums Macro created by fearsomeorc1406", 11.0, self.color("Navigation", "WatermarkColor")),
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
                console::toast("MIDI devices refreshed");
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
        let status = if running {
            "Listening — play your MIDI keyboard"
        } else if has_inputs {
            "Pick your keyboard, then start"
        } else {
            "Connect a MIDI keyboard and press ⟲"
        };
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
            )
            .on_mouse_down(
                MouseButton::Right,
                Self::menu_trigger(cx, move |_| {
                    vec![
                        MenuItem::title("MIDI keyboard"),
                        MenuItem::new(if running { "■" } else { "▶" }, if running { "Stop listening" } else { "Start listening" }, |this, _, cx| {
                            this.core.toggle_live();
                            cx.notify();
                        })
                        .enabled(running || has_inputs),
                        MenuItem::new("⟲", "Refresh devices", |this, _, cx| {
                            this.core.refresh_inputs();
                            console::toast("MIDI devices refreshed");
                            cx.notify();
                        })
                        .enabled(!running),
                    ]
                }),
            );

        let quick = self.card(S, "Play options").child(self.toggle_grid(
            S,
            vec![
                ("qw-sustain", "Sustain", cfg.sustain, "Holds Space while the sustain pedal is down.\nOnly works on supported games.", |c, v| c.midi_to_qwerty.sustain = v),
                ("qw-nodoubles", "No Doubles", cfg.no_doubles, "Prevents double-triggering of keys", |c, v| c.midi_to_qwerty.no_doubles = v),
                ("qw-velocity", "Velocity", cfg.velocity, "Simulates how hard a key is pressed (Alt + key).\nOnly works on supported games.", |c, v| c.midi_to_qwerty.velocity = v),
                ("qw-88", "88 Keys", cfg.keys_88, "Plays notes outside the 61-key range with Ctrl.\nOnly works on supported games.", |c, v| c.midi_to_qwerty.keys_88 = v),
            ],
            cx,
        ));

        let mut page = self
            .page("qwerty-page", S)
            .child(self.page_header(S, "MIDI To QWERTY", "Turns a connected MIDI keyboard into key presses, live."))
            .child(device)
            .child(self.card(S, "").child(self.piano_widget(S)))
            .child(quick)
            .child(self.advanced_header(S, "pedal, note length, input method, log", cx));

        if self.advanced {
            let entry_colors = self.entry_colors(SET, "ValueBox");
            let cutoff_entry = self.entry(EntryId::Control(Control::QwSustainCutoff), 44.0, "63", true, entry_colors, window, cx);
            let cutoff_slider = self.control_slider(Control::QwSustainCutoff, 120.0, true, self.slider_colors(SET, "Slider"), cx);
            let cutoff_reset = self.reset_button("reset-cutoff", SET, true, "Reset Sustain Cutoff (63)",
                Self::click(cx, |this, _, cx| {
                    Control::QwSustainCutoff.reset(&this.core);
                    cx.notify();
                }));

            let hold_on = cfg.custom_hold_length.enabled;
            let hold_sw = self.toggle("qw-hold", "", hold_on, true, S, "Release every key after a fixed time\ninstead of on note-off", cx,
                |_, on| config::get().update(|c| c.midi_to_qwerty.custom_hold_length.enabled = on));
            let hold_slider = self.control_slider(Control::QwNoteLength, 120.0, hold_on, self.slider_colors(SET, "Slider"), cx);
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

            let mut card = self
                .card(S, "Advanced")
                .child(self.form_row(
                    S,
                    "Pedal threshold",
                    "Pedal values above this count as pressed",
                    div().flex().items_center().gap(px(8.)).child(cutoff_slider).child(cutoff_entry).child(cutoff_reset),
                ))
                .child(self.divider(S))
                .child(self.form_row(S, "Fixed note length", "Release keys after a fixed time", hold_sw));
            if hold_on {
                card = card.child(self.form_row(
                    S,
                    "    Length (seconds)",
                    "",
                    div().flex().items_center().gap(px(8.)).child(hold_slider).child(hold_entry).child(hold_reset),
                ));
            }
            page = page
                .child(
                    card.child(self.divider(S)).child(self.form_row(
                        S,
                        "Input method",
                        "Try \"keyboard\" if a game ignores the keys",
                        with_tooltip(div().id("qw-module").child(module), self.tooltips(), "pynput: virtual keys\nkeyboard: scan codes (try if a game ignores keys)", &self.theme),
                    )),
                )
                .child(self.card(S, "").child(self.log_box(S, 13, cx)));
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
