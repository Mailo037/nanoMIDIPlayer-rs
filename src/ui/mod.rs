//! The main window: sidebar navigation + tab pages, rendered with GPUI.

mod controls;
mod hub_page;
mod icons;
mod info_page;
mod menu;
mod pages;
mod settings_page;
pub mod widgets;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, CursorStyle, Decorations, ElementId, FocusHandle, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, ResizeEdge, ScrollHandle, SharedString, Window,
    WindowControlArea, div, img, px,
};
use parking_lot::Mutex;

pub use controls::Control;
pub use icons::{Assets, Icon};
pub use menu::MenuItem;
pub use settings_page::SettingsSection;

use crate::config::{self, HotkeySlot};
use crate::console;
use crate::core::{Core, Tab, Target};
use crate::hub::Hub;
use crate::live::PIANO;
use crate::preview::Preview;
use crate::theme::{self, Theme};
use crate::update;
use icons::icon;
use widgets::*;

/// Version of this fork (from Cargo.toml), shown in the title bar and sidebar.
pub const APP_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
/// This Rust fork.
pub const FORK_REPO: &str = "https://github.com/Mailo037/nanoMIDIPlayer-rs";
/// The original Python nanoMIDIPlayer this is forked from.
pub const UPSTREAM_REPO: &str = "https://github.com/NotHammer043/nanoMIDIPlayer";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubView {
    /// Large cards with cover, all details and a 2x2 button block.
    List,
    /// Small one-line rows.
    Compact,
    /// Cover tiles in a responsive grid.
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DropId {
    MidiOutput,
    MidiFile,
    Pitch,
    Key,
    DrumsFile,
    QwInput,
    QwModule,
    MidiModule,
    DrumsModule,
    Theme,
    HubSort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryId {
    Control(Control),
    Search,
}

pub struct EntryState {
    pub focus: FocusHandle,
    pub buffer: String,
    pub editing: bool,
}

#[derive(Default)]
pub struct ThemeList {
    pub remote: Vec<(String, String)>,
    pub loaded: bool,
}

pub struct RootView {
    pub core: Arc<Core>,
    pub theme: Rc<Theme>,
    pub tab: Tab,
    pub open_drop: Option<DropId>,
    pub entries: HashMap<EntryId, EntryState>,
    pub slider_bounds: Rc<RefCell<HashMap<Control, Bounds<Pixels>>>>,
    pub drag: Option<Control>,
    /// Timeline being dragged and the position (0..1) under the mouse.
    pub scrub: Option<(Target, f32)>,
    pub scrub_bounds: Rc<RefCell<HashMap<Target, Bounds<Pixels>>>>,
    pub hub: Arc<Mutex<Hub>>,
    pub hub_version: Arc<AtomicU64>,
    pub hub_requested: bool,
    pub hub_scroll: ScrollHandle,
    pub preview: Arc<Preview>,
    /// Hub song whose Alt+1…9 slot picker is open (right-click on a card).
    pub slot_picker: Option<String>,
    pub settings_scroll: ScrollHandle,
    pub info: Arc<Mutex<Option<info_page::Doc>>>,
    pub info_version: Arc<AtomicU64>,
    pub info_scroll: ScrollHandle,
    pub themes: Arc<Mutex<ThemeList>>,
    pub pending_theme: Arc<Mutex<Option<Theme>>>,
    /// Whether the "Advanced" section of the player tabs is expanded.
    pub advanced: bool,
    /// Layout of the MIDI Hub song list.
    pub hub_view: HubView,
    pub topmost_applied: Option<bool>,
    /// Open right-click menu.
    pub context_menu: Option<menu::ContextMenu>,
    /// Sub-page of the Settings tab.
    /// `None` = the category overview.
    pub settings_section: Option<SettingsSection>,
    last_signature: u64,
}

impl RootView {
    pub fn new(core: Arc<Core>, theme: Theme, cx: &mut Context<Self>) -> Self {
        // UI refresh loop: only re-renders when something actually changed.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        let themes = Arc::new(Mutex::new(ThemeList::default()));
        {
            let themes = themes.clone();
            std::thread::spawn(move || {
                let remote = theme::fetch_remote_list().map(|(_, l)| l).unwrap_or_default();
                let mut t = themes.lock();
                t.remote = remote;
                t.loaded = true;
            });
        }

        let mut view = Self {
            core,
            theme: Rc::new(theme),
            tab: Tab::Midi,
            open_drop: None,
            entries: HashMap::new(),
            slider_bounds: Rc::new(RefCell::new(HashMap::new())),
            drag: None,
            scrub: None,
            scrub_bounds: Rc::new(RefCell::new(HashMap::new())),
            hub: Arc::new(Mutex::new(Hub::new())),
            hub_version: Arc::new(AtomicU64::new(0)),
            hub_requested: false,
            hub_scroll: ScrollHandle::new(),
            preview: Preview::new(),
            slot_picker: None,
            settings_scroll: ScrollHandle::new(),
            info: Arc::new(Mutex::new(None)),
            info_version: Arc::new(AtomicU64::new(0)),
            info_scroll: ScrollHandle::new(),
            themes,
            pending_theme: Arc::new(Mutex::new(None)),
            advanced: false,
            hub_view: HubView::List,
            topmost_applied: None,
            context_menu: None,
            settings_section: None,
            last_signature: 0,
        };
        view.core.set_tab(Tab::Midi);
        console::insert("Hello! :)");
        view.load_info();
        // Load the song list (cache first, then the server) before the hub tab is opened.
        view.ensure_hub_loaded();
        view
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        console::flush();
        if let Some(theme) = self.pending_theme.lock().take() {
            self.theme = Rc::new(theme);
            self.last_signature = 0;
        }
        let pos_secs = |p: &crate::player::Player| if p.is_running() { p.position().0 as u64 } else { 0 };
        let mut sig = self.core.version();
        sig = sig.wrapping_mul(31).wrapping_add(console::version());
        sig = sig.wrapping_mul(31).wrapping_add(PIANO.version());
        sig = sig.wrapping_mul(31).wrapping_add(pos_secs(&self.core.piano));
        sig = sig.wrapping_mul(31).wrapping_add(pos_secs(&self.core.drums));
        sig = sig.wrapping_mul(31).wrapping_add(self.hub_version.load(Ordering::Acquire));
        sig = sig.wrapping_mul(31).wrapping_add(self.info_version.load(Ordering::Acquire));
        sig = sig.wrapping_mul(31).wrapping_add(config::get().generation());
        sig = sig.wrapping_mul(31).wrapping_add(self.themes.lock().loaded as u64);
        sig = sig.wrapping_mul(31).wrapping_add(update::version());
        let _ = console::current_toast(); // expires old toasts
        sig = sig.wrapping_mul(31).wrapping_add(console::toast_version());
        if sig != self.last_signature {
            self.last_signature = sig;
            cx.notify();
        }
    }

    // ------------------------------------------------------------- helpers

    /// Wraps a view method into a `'static` callback usable by widgets.
    pub fn handler<T: 'static>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(T, &mut Window, &mut App) + 'static {
        let view = cx.entity().downgrade();
        move |arg, window, cx| {
            let _ = view.update(cx, |this, cx| f(this, arg, window, cx));
        }
    }

    pub fn click(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> ClickHandler {
        let h = Self::handler(cx, move |this, _: (), window, cx| f(this, window, cx));
        Box::new(move |_, window, cx| h((), window, cx))
    }

    pub fn tooltips(&self) -> bool {
        config::get().read().app_ui.tooltip
    }

    pub fn color(&self, section: &str, key: &str) -> Hsla {
        self.theme.color(section, key)
    }

    pub fn switch_colors(&self, section: &str) -> SwitchColors {
        // "Off" is neutral instead of the themes' red, which read as an error.
        let base = self.color(section, "BackgroundColor");
        SwitchColors {
            on: self.color(section, "SwitchEnabled"),
            off: pages::shade(base, 0.2),
            knob: gpui::white(),
            knob_off: pages::shade(base, 0.58),
            text: self.color(section, "TextColor"),
            text_disabled: self.color(section, "TextColorDisabled"),
        }
    }

    pub fn button_colors(&self, section: &str, bg: &str, hover: &str) -> ButtonColors {
        ButtonColors {
            bg: self.color(section, bg),
            hover: self.color(section, hover),
            text: self.color(section, "TextColor"),
            text_disabled: self.color(section, "TextColorDisabled"),
        }
    }

    pub fn slider_colors(&self, section: &str, prefix: &str) -> SliderColors {
        SliderColors {
            track: self.color(section, &format!("{prefix}BackColor")),
            // Filled part and knob use the text colors so they stay visible on any theme.
            fill: self.color(section, "TextColorDisabled"),
            knob: self.color(section, "TextColor"),
        }
    }

    pub fn dropdown_colors(&self, section: &str) -> DropdownColors {
        DropdownColors {
            bg: self.color(section, "OptionBackColor"),
            hover: self.color(section, "OptionDropdownButtonColor"),
            menu_bg: self.color(section, "OptionDropdownBackground"),
            item_hover: self.color(section, "OptionDropdownButtonHoverColor"),
            text: self.color(section, "TextColor"),
            text_disabled: self.color(section, "TextColorDisabled"),
            border: None,
        }
    }

    pub fn label(&self, s: &str, section: &str) -> gpui::Div {
        lines(&self.theme, s, 13.0, self.color(section, "TextColor"))
    }

    /// Generic option menu bound to a [`DropId`].
    pub fn dropdown(
        &self,
        id: DropId,
        value: String,
        items: Vec<String>,
        width: f32,
        enabled: bool,
        colors: DropdownColors,
        cx: &mut Context<Self>,
        on_select: impl Fn(&mut Self, usize, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui::Div {
        let toggle = Self::handler(cx, move |this, _: (), _, cx| {
            this.open_drop = if this.open_drop == Some(id) { None } else { Some(id) };
            cx.notify();
        });
        let close = Self::handler(cx, |this, _: (), _, cx| {
            this.open_drop = None;
            cx.notify();
        });
        let select = Self::handler(cx, on_select);
        dropdown(
            &self.theme,
            DropdownSpec {
                id: SharedString::from(format!("{id:?}")),
                value,
                width,
                enabled,
                open: self.open_drop == Some(id),
                items,
                colors,
            },
            move |window, cx| toggle((), window, cx),
            Rc::new(move |window, cx| close((), window, cx)),
            Rc::new(select),
        )
    }

    /// Slider bound to a [`Control`].
    pub fn control_slider(&self, control: Control, width: f32, enabled: bool, colors: SliderColors, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let bounds_map = self.slider_bounds.clone();
        let on_down = Self::handler(cx, move |this, ev: MouseDownEvent, _, cx| {
            this.drag = Some(control);
            this.drag_to(ev.position.x);
            cx.notify();
        });
        slider(
            ElementId::Name(format!("slider-{control:?}").into()),
            control.fraction(&self.core),
            width,
            enabled,
            colors,
            move |b| {
                bounds_map.borrow_mut().insert(control, b);
            },
            move |ev, window, cx| on_down(ev.clone(), window, cx),
        )
    }

    fn drag_to(&mut self, x: Pixels) {
        let Some(control) = self.drag else { return };
        let Some(bounds) = self.slider_bounds.borrow().get(&control).copied() else { return };
        let knob = px(16.);
        let usable = (bounds.size.width - knob).max(px(1.));
        let frac = ((x - bounds.origin.x - knob / 2.0) / usable) as f32;
        control.set_from_fraction(&self.core, frac);
    }

    pub fn scrub_to(&mut self, x: Pixels) {
        let Some((target, _)) = self.scrub else { return };
        let Some(bounds) = self.scrub_bounds.borrow().get(&target).copied() else { return };
        let frac = ((x - bounds.origin.x) / bounds.size.width.max(px(1.))) as f32;
        self.scrub = Some((target, frac.clamp(0.0, 1.0)));
    }

    /// Releases the timeline: jumps there.
    fn finish_scrub(&mut self) -> bool {
        match self.scrub.take() {
            Some((target, frac)) => {
                self.core.seek(target, frac as f64);
                true
            }
            None => false,
        }
    }

    /// Text entry bound to a [`Control`] or the hub search field.
    pub fn entry(
        &mut self,
        id: EntryId,
        width: f32,
        placeholder: &str,
        enabled: bool,
        colors: EntryColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let current = match id {
            EntryId::Control(c) => c.format(c.get(&self.core)),
            EntryId::Search => self.hub.lock().query.clone(),
        };
        let state = self.entries.entry(id).or_insert_with(|| EntryState {
            focus: cx.focus_handle(),
            buffer: String::new(),
            editing: false,
        });
        let focused = state.focus.is_focused(window);
        if state.editing && !focused {
            // Focus lost: commit like the original `<FocusOut>` binding.
            state.editing = false;
            let buffer = std::mem::take(&mut state.buffer);
            if let EntryId::Control(c) = id {
                c.set_from_text(&self.core, &buffer);
            }
        }
        let state = self.entries.get(&id).expect("entry exists");
        let shown = if state.editing { state.buffer.clone() } else { current.clone() };
        let focus = state.focus.clone();

        let on_down = Self::handler(cx, move |this, _: (), window, cx| {
            if let Some(st) = this.entries.get_mut(&id) {
                if !st.editing {
                    st.editing = true;
                    st.buffer = match id {
                        EntryId::Control(c) => c.format(c.get(&this.core)),
                        EntryId::Search => this.hub.lock().query.clone(),
                    };
                }
                window.focus(&st.focus);
            }
            this.open_drop = None;
            cx.notify();
        });
        let on_key = Self::handler(cx, move |this, ev: KeyDownEvent, window, cx| {
            this.entry_key(id, &ev, window, cx);
        });
        let mut el = widgets::entry(&self.theme, ElementId::Name(format!("entry-{id:?}").into()), &shown, placeholder, focused, enabled, width, colors);
        if enabled {
            el = el
                .track_focus(&focus)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_down((), window, cx)
                })
                .on_key_down(move |ev, window, cx| {
                    cx.stop_propagation();
                    on_key(ev.clone(), window, cx)
                });
        }
        el
    }

    fn entry_key(&mut self, id: EntryId, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.entries.get_mut(&id) else { return };
        let ks = &ev.keystroke;
        let mut changed = false;
        match ks.key.as_str() {
            "backspace" => {
                state.buffer.pop();
                changed = true;
            }
            "enter" | "escape" | "tab" => {
                if id == EntryId::Search {
                    let q = state.buffer.clone();
                    let mut hub = self.hub.lock();
                    hub.query = q;
                    hub.page = 1;
                    hub.apply();
                    drop(hub);
                    self.hub_scroll.set_offset(gpui::point(px(0.), px(0.)));
                    self.hub_version.fetch_add(1, Ordering::Release);
                }
                window.blur();
            }
            "v" if ks.modifiers.control || ks.modifiers.platform => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    state.buffer.push_str(text.trim_end_matches(['\r', '\n']));
                    changed = true;
                }
            }
            "a" if ks.modifiers.control || ks.modifiers.platform => {
                state.buffer.clear();
                changed = true;
            }
            _ => {
                if !ks.modifiers.control && !ks.modifiers.platform && !ks.modifiers.alt {
                    if let Some(ch) = &ks.key_char {
                        if !ch.chars().any(|c| c.is_control()) {
                            state.buffer.push_str(ch);
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            let buffer = state.buffer.clone();
            match id {
                EntryId::Control(c) => c.set_from_text(&self.core, &buffer),
                EntryId::Search => {
                    // Live search: the full list is local, so filter on every keystroke.
                    let mut hub = self.hub.lock();
                    hub.query = buffer;
                    hub.page = 1;
                    hub.apply();
                    drop(hub);
                    self.hub_scroll.set_offset(gpui::point(px(0.), px(0.)));
                    self.hub_version.fetch_add(1, Ordering::Release);
                }
            }
        }
        cx.notify();
    }

    /// Hotkey selector button (shows "..." while waiting for a key).
    pub fn hotkey_button(&self, slot: HotkeySlot, section: &str, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let capturing = self.core.state.lock().capturing;
        let label = if capturing == Some(slot) {
            "Press a key".to_string()
        } else {
            config::get().read().hotkeys.get(slot).to_uppercase()
        };
        let tooltip = match slot {
            HotkeySlot::Play => "Start Playback Hotkey",
            HotkeySlot::Pause => "Pause Playback Hotkey",
            HotkeySlot::Stop => "Stop Playback Hotkey",
            HotkeySlot::SpeedUp => "Speed-up Playback Hotkey",
            HotkeySlot::SlowDown => "Slow-down Playback Hotkey",
        };
        let el = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(format!("hk-{section}-{slot:?}").into()),
                label: label.into(),
                width: if capturing == Some(slot) { 96.0 } else { 64.0 },
                height: ROW_H,
                enabled: capturing.is_none() || capturing == Some(slot),
                colors: self.button_colors(section, "HotkeySelectorColor", "HotkeySelectorHoverColor"),
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                this.core.begin_capture(slot);
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            Self::menu_trigger(cx, move |_| {
                let default = config::Config::defaults().hotkeys.get(slot).to_string();
                let current = config::get().read().hotkeys.get(slot).to_string();
                let is_default = current.eq_ignore_ascii_case(&default);
                vec![
                    MenuItem::title(tooltip.trim_end_matches(" Hotkey")),
                    MenuItem::new(Icon::Keycap, "Change key…", move |this, _, cx| {
                        this.core.begin_capture(slot);
                        cx.notify();
                    }),
                    MenuItem::new(Icon::Restart, format!("Reset to {}", default.to_uppercase()), move |this, _, cx| {
                        let taken = HotkeySlot::ALL
                            .iter()
                            .any(|s| *s != slot && config::get().read().hotkeys.get(*s).eq_ignore_ascii_case(&default));
                        if taken {
                            console::toast(format!("{} is used by another hotkey", default.to_uppercase()));
                        } else {
                            config::get().update(|c| c.hotkeys.set(slot, default.clone()));
                            this.core.bump();
                        }
                        cx.notify();
                    })
                    .enabled(!is_default),
                ]
            }),
        );
        with_tooltip(el, self.tooltips(), tooltip, &self.theme)
    }

    pub fn reset_button(&self, id: &str, section: &str, enabled: bool, tooltip: &'static str, on: ClickHandler) -> gpui::Stateful<gpui::Div> {
        let el = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::Name(id.to_string().into()),
                label: "".into(),
                width: ROW_H,
                height: ROW_H,
                enabled,
                colors: self.button_colors(section, "ButtonColor", "ButtonHoverColor"),
                icon: Some((Icon::Restart, 15.0)),
            },
            on,
        );
        with_tooltip(el, self.tooltips(), tooltip, &self.theme)
    }

    /// The black console box. `max` is the per-tab message limit of the original.
    pub fn console_box(&self, section: &str, w: f32, h: f32, max: usize) -> gpui::Div {
        let mut d = div()
            .w(px(w))
            .h(px(h))
            .overflow_hidden()
            .bg(self.color(section, "ConsoleBackground"))
            .flex()
            .flex_col();
        let color = self.color("MidiPlayer", "TextColor");
        for msg in console::lines(max) {
            // The log keeps the theme's monospace font.
            d = d.child(
                lines(&self.theme, &msg, 12.0, color)
                    .font_family(self.theme.font.clone())
                    .font_weight(gpui::FontWeight::NORMAL)
                    .line_height(px(15.)),
            );
        }
        d
    }

    // ------------------------------------------------------------- navigation

    pub fn show_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.context_menu = None;
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        self.open_drop = None;
        self.core.set_tab(tab);
        if tab == Tab::Hub {
            self.ensure_hub_loaded();
        } else {
            self.preview.stop();
        }
        cx.notify();
    }

    /// Opens a Settings sub-page directly.
    pub fn show_settings(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        self.settings_section = Some(section);
        self.settings_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.show_tab(Tab::Settings, cx);
        cx.notify();
    }

    /// Accent color of the theme (the green of the Play button in the default theme).
    pub fn accent(&self) -> Hsla {
        self.color("MidiPlayer", "PlayColor")
    }

    pub fn copy_text(&self, text: String, what: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        console::toast(format!("{what} copied"));
    }

    /// Small colored dot next to a sidebar entry (playing / paused / listening / update).
    fn nav_badge(&self, tab: Tab) -> Option<Hsla> {
        let player_dot = |p: &crate::player::Player, section: &str| {
            p.is_running().then(|| self.color(section, if p.is_paused() { "PausedColor" } else { "PlayingColor" }))
        };
        match tab {
            Tab::Midi => player_dot(&self.core.piano, "MidiPlayer"),
            Tab::Drums => player_dot(&self.core.drums, "DrumsMacro"),
            Tab::ToQwerty => self.core.live.is_running().then(|| self.accent()),
            Tab::Settings => matches!(update::state().status, update::Status::Available | update::Status::Installed).then(|| self.accent()),
            _ => None,
        }
    }

    fn nav_button(&self, id: &'static str, label: &str, glyph: Icon, tab: Tab, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let selected = self.tab == tab;
        let text = self.color("Navigation", "TextColor");
        let color = if selected { text } else { Hsla { a: text.a * 0.68, ..text } };
        let bg = if selected {
            pages::shade(self.color("Navigation", "TabButtonSelectedColor"), 0.04)
        } else {
            self.color("Navigation", "TabButtonColor")
        };
        let hover = self.color("Navigation", "TabButtonHoverColor");
        let mut el = div()
            .id(id)
            .h(px(36.))
            .px(px(10.))
            .rounded(px(R_CONTROL))
            .flex()
            .items_center()
            .gap(px(10.))
            .bg(bg)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(icon(glyph, 16.0, color))
            .child(div().flex_1().min_w_0().child(text_label(&self.theme, label, 13.0, color).whitespace_nowrap()))
            .on_click(cx.listener(move |this, _, _, cx| {
                // The sidebar entry always opens the settings overview.
                if tab == Tab::Settings && this.settings_section.take().is_some() {
                    this.settings_scroll.set_offset(gpui::point(px(0.), px(0.)));
                    cx.notify();
                }
                this.show_tab(tab, cx)
            }));
        if let Some(dot) = self.nav_badge(tab) {
            el = el.child(div().w(px(8.)).h(px(8.)).rounded_full().bg(dot).flex_shrink_0());
        }
        el
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = &self.theme;
        let custom_wm = self.color("Navigation", "CustomWatermarkColor");
        let dim = self.color("MidiPlayer", "TextColorDisabled");
        let caption = |s: &str| div().px(px(10.)).pt(px(12.)).pb(px(4.)).child(text_label(t, s, 12.0, dim));
        let mut bar = div()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .pb(px(8.))
            .bg(self.color("Navigation", "BackColor"));
        if let Some(banner) = t.icons.banner.clone() {
            bar = bar.child(div().h(px(48.)).px(px(10.)).flex().items_center().child(img(banner).w(px(86.)).h(px(26.))));
        }
        bar = bar
            .child(caption("Play"))
            .child(self.nav_button("nav-midi", "Play a song", Icon::Music, Tab::Midi, cx))
            .child(self.nav_button("nav-drums", "Play drums", Icon::Drum, Tab::Drums, cx))
            .child(self.nav_button("nav-qwerty", "Live keyboard", Icon::Keyboard, Tab::ToQwerty, cx))
            .child(caption("Get songs"))
            .child(self.nav_button("nav-hub", "MIDI Hub", Icon::Search, Tab::Hub, cx))
            .child(div().flex_1());
        for line in [&t.watermark_ln1, &t.watermark_ln2] {
            if !line.is_empty() {
                bar = bar.child(div().flex().justify_center().child(lines(t, line, 12.0, custom_wm)));
            }
        }
        bar = bar
            .child(self.nav_button("nav-settings", "Settings", Icon::Settings, Tab::Settings, cx))
            .child(self.nav_button("nav-info", "Help & about", Icon::Help, Tab::Info, cx));
        // Version + update hint at the bottom.
        let (status, latest) = {
            let st = update::state();
            (st.status.clone(), st.latest.as_ref().map(|r| r.version.clone()))
        };
        let (line2, color) = match status {
            update::Status::Available => (format!("{} available", latest.unwrap_or_default()), self.accent()),
            update::Status::Downloading(_) => ("Updating…".to_string(), self.accent()),
            update::Status::Installed => ("Restart to update".to_string(), self.accent()),
            _ => ("Up to date".to_string(), dim),
        };
        let show_line2 = matches!(
            status,
            update::Status::Available | update::Status::Downloading(_) | update::Status::Installed | update::Status::UpToDate
        );
        let hover = self.color("Navigation", "TabButtonHoverColor");
        bar.child(
            div()
                .id("nav-version")
                .mt(px(4.))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(R_CONTROL))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(text_label(t, &format!("nanoMIDIPlayer {APP_VERSION}"), 11.0, dim))
                .when(show_line2, |d| d.child(text_label(t, &line2, 11.0, color)))
                .on_click(cx.listener(|this, _, _, cx| this.show_settings(SettingsSection::Updates, cx))),
        )
    }

    /// Bar above the page while an update is available, downloading or installed.
    fn update_banner(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let (status, latest, dismissed) = {
            let st = update::state();
            (st.status.clone(), st.latest.clone(), st.dismissed)
        };
        if dismissed || (self.tab == Tab::Settings && self.settings_section == Some(SettingsSection::Updates)) {
            return None;
        }
        let version = latest.as_ref().map(|r| r.version.clone()).unwrap_or_default();
        let s = "MidiPlayer";
        let text_c = self.color(s, "TextColor");
        let dim = self.color(s, "TextColorDisabled");
        let small = |id: &'static str, label: &str, primary: bool, cx: &mut Context<Self>, f: fn(&mut Self, &mut Context<Self>)| {
            button(
                &self.theme,
                ButtonSpec {
                    id: id.into(),
                    label: label.to_string().into(),
                    width: 0.0,
                    height: 28.0,
                    enabled: true,
                    colors: if primary {
                        self.button_colors(s, "PlayColor", "PlayColorHover")
                    } else {
                        self.button_colors(s, "ButtonColor", "ButtonHoverColor")
                    },
                    icon: None,
                },
                Self::click(cx, move |this, _, cx| f(this, cx)),
            )
            .rounded_full()
            .into_any_element()
        };
        let (message, actions): (String, Vec<gpui::AnyElement>) = match status {
            update::Status::Available => (
                format!("Update {version} available"),
                vec![
                    small("upd-now", "Update now", true, cx, |_, _| update::install()),
                    small("upd-details", "Details", false, cx, |this, cx| this.show_settings(SettingsSection::Updates, cx)),
                ],
            ),
            update::Status::Downloading(p) => (
                match p {
                    Some(p) => format!("Downloading {version}… {}%", (p * 100.0).round() as u32),
                    None => format!("Downloading {version}…"),
                },
                Vec::new(),
            ),
            update::Status::Installed => (
                format!("{version} is installed"),
                vec![small("upd-restart", "Restart now", true, cx, |this, cx| this.restart_app(cx))],
            ),
            _ => return None,
        };
        let close = div()
            .id("upd-close")
            .w(px(24.))
            .h(px(24.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|s| s.bg(gpui::rgba(0xffffff14)))
            .child(icon(Icon::Close, 13.0, dim))
            .on_click(cx.listener(|_, _, _, cx| {
                update::dismiss();
                cx.notify();
            }));
        let mut row = div()
            .mx(px(16.))
            .mt(px(12.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(R_CARD))
            .bg(pages::shade(self.color(s, "BackgroundColor"), 0.05))
            .border_1()
            .border_color(self.accent())
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(self.accent()))
            .child(div().flex_1().min_w(px(120.)).child(text_label(&self.theme, &message, 12.0, text_c)));
        for a in actions {
            row = row.child(a);
        }
        Some(row.child(close))
    }

    /// Quits and starts the (updated) app again.
    pub fn restart_app(&mut self, cx: &mut Context<Self>) {
        if update::relaunch() {
            self.core.shutdown();
            cx.quit();
        } else {
            console::toast("Couldn't restart. Please start the app again.");
        }
    }

    fn toast(&self) -> Option<gpui::Div> {
        let msg = console::current_toast()?;
        Some(
            div()
                .absolute()
                .bottom(px(14.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .max_w(px(380.))
                        .px(px(14.))
                        .py(px(8.))
                        .rounded_full()
                        .bg(pages::shade(self.color("Navigation", "BackColor"), 0.06))
                        .border_1()
                        .border_color(self.accent())
                        .shadow_lg()
                        .child(text_label(&self.theme, &msg, 12.0, self.color("MidiPlayer", "TextColor"))),
                ),
        )
    }

    fn apply_topmost(&mut self, window: &Window) {
        let want = config::get().read().app_ui.topmost;
        if self.topmost_applied == Some(want) {
            return;
        }
        // A new window is not on top, so there is nothing to undo on the first frame.
        if self.topmost_applied.is_none() && !want {
            self.topmost_applied = Some(false);
            return;
        }
        use raw_window_handle::HasWindowHandle;
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            set_topmost(handle.as_raw(), want);
            self.topmost_applied = Some(want);
        }
    }
}

pub const SIDEBAR_W: f32 = 164.0;

#[cfg(windows)]
fn set_topmost(handle: raw_window_handle::RawWindowHandle, on: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::*;
    let raw_window_handle::RawWindowHandle::Win32(h) = handle else { return };
    unsafe {
        let _ = SetWindowPos(
            HWND(h.hwnd.get() as *mut _),
            Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// X11 and XWayland: ask the window manager. Wayland has no way to request this.
#[cfg(target_os = "linux")]
fn set_topmost(handle: raw_window_handle::RawWindowHandle, on: bool) {
    use raw_window_handle::RawWindowHandle;
    let window = match handle {
        RawWindowHandle::Xcb(h) => h.window.get(),
        RawWindowHandle::Xlib(h) => h.window as u32,
        _ => {
            log::warn!("\"Always on top\" is not available on Wayland");
            return;
        }
    };
    if let Some(x) = crate::x11::get() {
        x.set_above(window, on);
    }
}

#[cfg(target_os = "macos")]
fn set_topmost(handle: raw_window_handle::RawWindowHandle, on: bool) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    let raw_window_handle::RawWindowHandle::AppKit(h) = handle else { return };
    unsafe {
        let view = h.ns_view.as_ptr() as *mut Object;
        let window: *mut Object = msg_send![view, window];
        if !window.is_null() {
            // NSFloatingWindowLevel = 3, NSNormalWindowLevel = 0
            let level: isize = if on { 3 } else { 0 };
            let _: () = msg_send![window, setLevel: level];
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn set_topmost(_handle: raw_window_handle::RawWindowHandle, _on: bool) {}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_topmost(window);
        let page = match self.tab {
            Tab::Midi => self.midi_page(window, cx).into_any_element(),
            Tab::Drums => self.drums_page(window, cx).into_any_element(),
            Tab::Hub => self.hub_page(window, cx).into_any_element(),
            Tab::ToQwerty => self.qwerty_page(window, cx).into_any_element(),
            Tab::Settings => self.settings_page(window, cx).into_any_element(),
            Tab::Info => self.info_page(window, cx).into_any_element(),
        };
        let banner = self.update_banner(cx);
        let toast = self.toast();
        let menu = self.render_menu(cx);
        let titlebar = self.titlebar(window);
        let resize = self.resize_handles(window);
        let page_bg = self.color(
            match self.tab {
                Tab::Midi => "MidiPlayer",
                Tab::Drums => "DrumsMacro",
                Tab::Hub => "MIDIHub",
                Tab::ToQwerty => "MidiToQWERTY",
                Tab::Settings | Tab::Info => "Settings",
            },
            "BackgroundColor",
        );
        let content = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(page_bg)
            .when_some(banner, |d, b| d.child(b))
            .child(div().flex_1().min_h_0().child(page))
            .when_some(toast, |d, t| d.child(t));
        div()
            .id("root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(self.theme.ui_font.clone())
            .bg(self.color("MidiPlayer", "BackgroundColor"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.blur();
                    let closed_menu = this.close_menu();
                    if this.open_drop.take().is_some() || closed_menu {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    if this.close_menu() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.scrub.is_some() {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        this.scrub_to(ev.position.x);
                    } else {
                        this.finish_scrub();
                    }
                    cx.notify();
                }
                if this.drag.is_some() {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        this.drag_to(ev.position.x);
                    } else {
                        this.drag = None;
                    }
                    cx.notify();
                }
            }))
            .capture_any_mouse_up(cx.listener(|this, _, _, cx| {
                if this.drag.take().is_some() | this.finish_scrub() {
                    cx.notify();
                }
            }))
            .when_some(titlebar, |d, t| d.child(t))
            .child(div().flex().flex_1().min_h_0().child(self.sidebar(cx)).child(content))
            .when_some(menu, |d, m| d.child(m))
            .children(resize)
    }
}

pub const TITLEBAR_H: f32 = 34.0;

impl RootView {
    /// Custom title bar: app name, drag area and window buttons.
    ///
    /// - Windows: the buttons are hit-tested natively through `WindowControlArea`, so
    ///   snap layouts keep working.
    /// - macOS: the system draws its own buttons on the left; this is only the title.
    /// - Linux: only shown when the window has no server-side decorations (for example
    ///   GNOME on Wayland); otherwise the window manager's title bar is used.
    fn titlebar(&self, window: &Window) -> Option<gpui::Stateful<gpui::Div>> {
        let linux = cfg!(target_os = "linux");
        let mac = cfg!(target_os = "macos");
        if linux && matches!(window.window_decorations(), Decorations::Server) {
            return None;
        }
        let bg = self.color("Navigation", "BackColor");
        let text = self.color("Navigation", "TextColor");
        let dim = self.color("MidiPlayer", "TextColorDisabled");
        let hover = self.color("Navigation", "TabButtonHoverColor");
        let maximized = window.is_maximized();

        let control = |id: &'static str, glyph: Icon, area: WindowControlArea, close: bool| {
            let hover_bg = if close { gpui::rgb(0xc42b1c).into() } else { hover };
            div()
                .id(id)
                .w(px(46.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(move |s| s.bg(hover_bg))
                .window_control_area(area)
                .child(icon(glyph, 14.0, text))
                .when(!cfg!(windows), |d| {
                    d.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, window, _| match area {
                        WindowControlArea::Min => window.minimize_window(),
                        WindowControlArea::Max => window.zoom_window(),
                        WindowControlArea::Close => window.remove_window(),
                        WindowControlArea::Drag => {}
                    })
                })
        };

        let title = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(if mac { 78. } else { 14. }))
            .child(text_label(&self.theme, "nanoMIDIPlayer", 12.0, text))
            .child(text_label(&self.theme, APP_VERSION, 11.0, dim));

        let bar = div()
            .id("titlebar")
            .h(px(TITLEBAR_H))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(bg)
            .child(
                div()
                    .id("titlebar-drag")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
                    .when(linux, |d| d.on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()))
                    .child(title),
            );
        if mac {
            return Some(bar);
        }
        Some(
            bar.child(control("tb-min", Icon::Minimize, WindowControlArea::Min, false))
                .child(control("tb-max", if maximized { Icon::Restore } else { Icon::Maximize }, WindowControlArea::Max, false))
                .child(control("tb-close", Icon::Close, WindowControlArea::Close, true)),
        )
    }

    /// Invisible resize zones along the window edges, for Linux windows that draw their
    /// own decorations (there the window manager provides no resize border).
    fn resize_handles(&self, window: &Window) -> Vec<gpui::Div> {
        if !cfg!(target_os = "linux") || window.is_maximized() || !matches!(window.window_decorations(), Decorations::Client { .. }) {
            return Vec::new();
        }
        let edge = px(5.);
        let corner = px(12.);
        let zone = |cursor: CursorStyle, edge: ResizeEdge| {
            div()
                .absolute()
                .occlude()
                .cursor(cursor)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    window.start_window_resize(edge)
                })
        };
        vec![
            zone(CursorStyle::ResizeUpDown, ResizeEdge::Top).top_0().left(corner).right(corner).h(edge),
            zone(CursorStyle::ResizeUpDown, ResizeEdge::Bottom).bottom_0().left(corner).right(corner).h(edge),
            zone(CursorStyle::ResizeLeftRight, ResizeEdge::Left).left_0().top(corner).bottom(corner).w(edge),
            zone(CursorStyle::ResizeLeftRight, ResizeEdge::Right).right_0().top(corner).bottom(corner).w(edge),
            zone(CursorStyle::ResizeUpLeftDownRight, ResizeEdge::TopLeft).top_0().left_0().w(corner).h(corner),
            zone(CursorStyle::ResizeUpRightDownLeft, ResizeEdge::TopRight).top_0().right_0().w(corner).h(corner),
            zone(CursorStyle::ResizeUpRightDownLeft, ResizeEdge::BottomLeft).bottom_0().left_0().w(corner).h(corner),
            zone(CursorStyle::ResizeUpLeftDownRight, ResizeEdge::BottomRight).bottom_0().right_0().w(corner).h(corner),
        ]
    }
}

fn text_label(theme: &Theme, s: &str, size: f32, color: Hsla) -> gpui::Div {
    widgets::text(theme, s.to_string(), size, color)
}
