//! MIDI Hub tab: browse, search, sort and download MIDIs from nanomidi.net.

use std::sync::atomic::Ordering;

use gpui::prelude::*;
use gpui::{Context, Div, ElementId, MouseButton, Window, div, img, px};

use super::widgets::*;
use super::{DropId, EntryId, RootView};
use crate::{console, net};
use crate::core::{Tab, Target};
use crate::hub::{self, HubStatus, SortMode};
use crate::preview::PreviewState;
use crate::slots;

const S: &str = "MIDIHub";

impl RootView {
    pub fn ensure_hub_loaded(&mut self) {
        if self.hub_requested {
            return;
        }
        self.hub_requested = true;
        let hub = self.hub.clone();
        let version = self.hub_version.clone();
        if !matches!(hub.lock().status, HubStatus::Ready) {
            hub.lock().status = HubStatus::Loading;
        }
        std::thread::spawn(move || {
            // Show the cached list right away, then refresh it from the server.
            let cached = matches!(hub.lock().status, HubStatus::Ready)
                || match hub::load_cached() {
                    Some(data) => {
                        hub.lock().set_data(data);
                        version.fetch_add(1, Ordering::Release);
                        true
                    }
                    None => false,
                };
            match hub::fetch_data() {
                Ok(data) => {
                    let mut hub = hub.lock();
                    hub.offline = false;
                    hub.set_data(data);
                }
                Err(e) => {
                    log::warn!("Failed to load MIDI data: {e}");
                    let mut hub = hub.lock();
                    if cached {
                        hub.offline = true;
                    } else {
                        let kind = if e.to_string().to_lowercase().contains("timeout") { "Timeout" } else { "ConnectionError" };
                        hub.status = HubStatus::Error(kind.into());
                    }
                }
            }
            version.fetch_add(1, Ordering::Release);
        });
    }

    /// Loads cover images for the visible and the next page in parallel (cached per file name).
    fn load_page_images(&self) {
        let missing: Vec<String> = {
            let mut hub = self.hub.lock();
            let names: Vec<String> = hub
                .wanted_images()
                .into_iter()
                .filter(|n| !hub.images.contains_key(n))
                .collect();
            for n in &names {
                hub.images.insert(n.clone(), None);
            }
            names
        };
        for name in missing {
            let hub = self.hub.clone();
            let version = self.hub_version.clone();
            std::thread::spawn(move || {
                let image = hub::fetch_image(&name);
                hub.lock().images.insert(name, image);
                version.fetch_add(1, Ordering::Release);
            });
        }
    }

    fn hub_reset_scroll(&self) {
        self.hub_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.hub_version.fetch_add(1, Ordering::Release);
    }

    /// Favorite, preview, open-page and download buttons for one song (`size` = button edge).
    fn hub_actions(&self, index: usize, midi: &hub::HubMidi, key: &str, size: f32, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let mut out = Vec::new();
        let icon_size = (size * 0.64).round();

        let favorite = self.hub.lock().is_favorite(key);
        let fav_key = key.to_string();
        let (fav_bg, fav_hover) = if favorite {
            ("DownloadButtonColor", "DownloadButtonHoverColor")
        } else {
            ("NextButtonColor", "NextButtonHoverColor")
        };
        let fav = button(
            &self.theme,
            ButtonSpec {
                id: ElementId::NamedInteger("hub-fav".into(), index as u64),
                label: if favorite { "★" } else { "☆" }.into(),
                width: size,
                height: size,
                enabled: true,
                colors: self.button_colors(S, fav_bg, fav_hover),
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                this.hub.lock().toggle_favorite(fav_key.clone());
                this.hub_version.fetch_add(1, Ordering::Release);
                cx.notify();
            }),
        );
        out.push(with_tooltip(fav, self.tooltips(), if favorite { "Remove from Favorites" } else { "Add to Favorites" }, &self.theme).into_any_element());

        if let Some(file) = midi.midi_filename.clone().filter(|f| !f.is_empty()) {
            let state = self.preview.state_of(key);
            let (label, tooltip) = match state {
                Some(PreviewState::Loading) => ("…", "Loading preview..."),
                Some(PreviewState::Playing) => ("■", "Stop Preview"),
                None => ("▶", "Preview (first 30 seconds)"),
            };
            let preview_key = key.to_string();
            let play = button(
                &self.theme,
                ButtonSpec {
                    id: ElementId::NamedInteger("hub-preview".into(), index as u64),
                    label: label.into(),
                    width: size,
                    height: size,
                    enabled: true,
                    colors: self.button_colors(
                        S,
                        if state.is_some() { "DownloadButtonColor" } else { "NextButtonColor" },
                        if state.is_some() { "DownloadButtonHoverColor" } else { "NextButtonHoverColor" },
                    ),
                    icon: None,
                },
                Self::click(cx, move |this, _, cx| {
                    if this.preview.state_of(&preview_key).is_some() {
                        this.preview.stop();
                    } else {
                        this.preview.start(preview_key.clone(), file.clone(), this.hub_version.clone());
                    }
                    this.hub_version.fetch_add(1, Ordering::Release);
                    cx.notify();
                }),
            );
            out.push(with_tooltip(play, self.tooltips(), tooltip, &self.theme).into_any_element());
        }

        if let Some(url) = midi.page_url() {
            let open = button(
                &self.theme,
                ButtonSpec {
                    id: ElementId::NamedInteger("hub-open".into(), index as u64),
                    label: "↗".into(),
                    width: size,
                    height: size,
                    enabled: true,
                    colors: self.button_colors(S, "NextButtonColor", "NextButtonHoverColor"),
                    icon: None,
                },
                Self::click(cx, move |_, _, _| net::open_url(&url)),
            );
            out.push(with_tooltip(open, self.tooltips(), "Open on nanomidi.net", &self.theme).into_any_element());
        }

        if let Some(file) = midi.midi_filename.clone().filter(|f| !f.is_empty()) {
            let dl = button(
                &self.theme,
                ButtonSpec {
                    id: ElementId::NamedInteger("hub-dl".into(), index as u64),
                    label: "".into(),
                    width: size,
                    height: size,
                    enabled: true,
                    colors: self.button_colors(S, "DownloadButtonColor", "DownloadButtonHoverColor"),
                    icon: self.theme.icons.download.clone().map(|i| (i, icon_size)),
                },
                Self::click(cx, move |this, _, cx| {
                    let core = this.core.clone();
                    let file = file.clone();
                    std::thread::spawn(move || match hub::download_midi(&file) {
                        Ok(path) => {
                            core.use_file(Target::Midi, &path);
                            console::log("Downloaded MIDI File:");
                            console::insert(path.display().to_string());
                        }
                        Err(e) => console::insert(format!("Download failed: {e}")),
                    });
                    this.show_tab(Tab::Midi, cx);
                }),
            );
            out.push(with_tooltip(dl, self.tooltips(), "Download and open in MIDI Player", &self.theme).into_any_element());
        }
        out
    }

    /// One line of card text, shortened with "…" to `max_w` (see [`ellipsize`]).
    fn card_line(&self, s: &str, size: f32, max_w: f32) -> Div {
        text(&self.theme, ellipsize(s, size, max_w), size, self.color(S, "TextColor")).whitespace_nowrap().overflow_hidden()
    }

    fn cover(&self, image: Option<std::sync::Arc<gpui::Image>>, size: f32) -> Div {
        let mut art = div()
            .w(px(size))
            .h(px(size))
            .flex_shrink_0()
            .rounded(px(10.))
            .overflow_hidden()
            .bg(super::pages::shade(self.color(S, "MidiCardBackColor"), 0.06));
        if let Some(image) = image {
            art = art.child(img(image).w(px(size)).h(px(size)));
        }
        art
    }

    /// Card for one song in the current view mode. Text always shrinks/truncates,
    /// so the buttons keep their size and never get pushed out of the card.
    fn hub_card(&self, index: usize, midi: &hub::HubMidi, image: Option<std::sync::Arc<gpui::Image>>, width: f32, cx: &mut Context<Self>) -> Div {
        let key = midi.key();
        let slot = slots::slot_of(&key);
        let arranger = midi.arranger.as_deref().filter(|a| !a.is_empty()).unwrap_or("N/A");
        let hotkey = slot.map(|n| format!("Hotkey: Alt+{}", n + 1));

        // Right-click opens the Alt+1…9 slot picker below the card.
        let picker_key = key.clone();
        let toggle_picker = Self::handler(cx, move |this, _: (), _, cx| {
            this.slot_picker = if this.slot_picker.as_deref() == Some(picker_key.as_str()) { None } else { Some(picker_key.clone()) };
            this.hub_version.fetch_add(1, Ordering::Release);
            cx.notify();
        });
        let base = div()
            .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                cx.stop_propagation();
                toggle_picker((), window, cx)
            })
            .rounded(px(14.))
            .bg(self.color(S, "MidiCardBackColor"))
            .overflow_hidden();

        let card = match self.hub_view {
            super::HubView::List => {
                // card padding 16 + cover 92 + gaps 28 + buttons 64
                let text_w = width - 208.0;
                let mut info = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.))
                    .child(self.card_line(&midi.name, 15.0, text_w))
                    .child(self.card_line(&midi.artists, 12.0, text_w))
                    .child(self.card_line(&format!("Arr: {arranger}"), 11.0, text_w))
                    .child(self.card_line(&format!("Uploader: {}", midi.uploader), 11.0, text_w));
                if let Some(h) = &hotkey {
                    info = info.child(text(&self.theme, h.clone(), 11.0, self.color(S, "DownloadButtonColor")));
                }
                let mut actions = div().w(px(64.)).flex_shrink_0().flex().flex_wrap().content_center().gap(px(4.));
                for a in self.hub_actions(index, midi, &key, 30.0, cx) {
                    actions = actions.child(a);
                }
                base.flex()
                    .items_center()
                    .gap(px(14.))
                    .p(px(8.))
                    .child(self.cover(image, 92.0))
                    .child(info)
                    .child(actions)
            }
            super::HubView::Compact => {
                // padding 12 + cover 40 + gaps 20 + up to four 26 px buttons
                let text_w = width - 196.0;
                let mut actions = div().flex_shrink_0().flex().items_center().gap(px(4.));
                for a in self.hub_actions(index, midi, &key, 26.0, cx) {
                    actions = actions.child(a);
                }
                let subtitle = match &hotkey {
                    Some(h) => format!("{} · {h}", midi.artists),
                    None => midi.artists.clone(),
                };
                base.flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(6.))
                    .child(self.cover(image, 40.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(self.card_line(&midi.name, 13.0, text_w))
                            .child(self.card_line(&subtitle, 11.0, text_w)),
                    )
                    .child(actions)
            }
            super::HubView::Grid => {
                let mut actions = div().flex().items_center().justify_center().gap(px(4.));
                for a in self.hub_actions(index, midi, &key, 26.0, cx) {
                    actions = actions.child(a);
                }
                let tile_w = width;
                base.w(px(tile_w))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .child(self.cover(image, tile_w - 16.0))
                    .child(self.card_line(&midi.name, 13.0, tile_w - 16.0))
                    .child(self.card_line(&midi.artists, 11.0, tile_w - 16.0))
                    .child(actions)
            }
        };

        if self.slot_picker.as_deref() != Some(key.as_str()) || midi.midi_filename.as_deref().is_none_or(str::is_empty) {
            return card;
        }
        div().flex().flex_col().gap(px(4.)).child(card).child(self.slot_picker_row(midi, slot, cx))
    }

    /// Small button group for switching between List / Compact / Grid.
    fn hub_view_switch(&self, cx: &mut Context<Self>) -> Div {
        let on = self.color(S, "DownloadButtonColor");
        let off = self.color(S, "NextButtonColor");
        let hover = self.color(S, "NextButtonHoverColor");
        let text_c = self.color(S, "TextColor");
        let mut row = div().flex().rounded_full().bg(off).p(px(2.)).gap(px(2.));
        for (i, (glyph, mode, tip)) in [
            ("☰", super::HubView::List, "List"),
            ("≡", super::HubView::Compact, "Compact list"),
            ("▦", super::HubView::Grid, "Grid"),
        ]
        .into_iter()
        .enumerate()
        {
            let selected = self.hub_view == mode;
            let seg = div()
                .id(ElementId::NamedInteger("hub-view".into(), i as u64))
                .w(px(30.))
                .h(px(24.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(text_c)
                .when(selected, |d| d.bg(on))
                .when(!selected, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
                .child(glyph)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.hub_view = mode;
                    this.hub_version.fetch_add(1, Ordering::Release);
                    cx.notify();
                }));
            row = row.child(with_tooltip(seg, self.tooltips(), tip, &self.theme));
        }
        row
    }

    /// Row of Alt+1…9 buttons for assigning the song a global hotkey slot.
    fn slot_picker_row(&self, midi: &hub::HubMidi, current: Option<usize>, cx: &mut Context<Self>) -> Div {
        let text_color = self.color(S, "TextColor");
        let mut row = div()
            .mx(px(20.))
            .p(px(6.))
            .rounded(px(6.))
            .bg(self.color(S, "PageControlFrameBackground"))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(lines(&self.theme, "Alt +", 12.0, text_color).mr(px(4.)));
        for n in 0..slots::SLOT_COUNT {
            let active = current == Some(n);
            let (bg, hover) = if active {
                ("DownloadButtonColor", "DownloadButtonHoverColor")
            } else {
                ("NextButtonColor", "NextButtonHoverColor")
            };
            let song = midi.clone();
            row = row.child(button(
                &self.theme,
                ButtonSpec {
                    id: ElementId::NamedInteger("hub-slot".into(), n as u64),
                    label: format!("{}", n + 1).into(),
                    width: 26.0,
                    height: 26.0,
                    enabled: true,
                    colors: self.button_colors(S, bg, hover),
                    icon: None,
                },
                Self::click(cx, move |this, _, cx| {
                    if active {
                        slots::clear_song(&song.key());
                    } else {
                        slots::assign(n, &song);
                        console::insert(format!("Alt+{} → {}", n + 1, song.name));
                    }
                    this.slot_picker = None;
                    this.hub_version.fetch_add(1, Ordering::Release);
                    cx.notify();
                }),
            ));
        }
        let close = button(
            &self.theme,
            ButtonSpec {
                id: "hub-slot-close".into(),
                label: "×".into(),
                width: 26.0,
                height: 26.0,
                enabled: true,
                colors: self.button_colors(S, "PreviousButtonColor", "PreviousButtonHoverColor"),
                icon: None,
            },
            Self::click(cx, |this, _, cx| {
                this.slot_picker = None;
                cx.notify();
            }),
        );
        row.child(div().flex_1()).child(close)
    }

    fn hub_filter_tab(&self, id: &'static str, label: String, active: bool, favorites: bool, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let (bg, hover) = if active {
            ("DownloadButtonColor", "DownloadButtonHoverColor")
        } else {
            ("NextButtonColor", "NextButtonHoverColor")
        };
        let off = self.color(S, "NextButtonColor");
        let colors = if active { self.button_colors(S, bg, hover) } else { ButtonColors { bg: off, ..self.button_colors(S, bg, hover) } };
        button(
            &self.theme,
            ButtonSpec {
                id: id.into(),
                label: label.into(),
                width: 92.0,
                height: 24.0,
                enabled: true,
                colors,
                icon: None,
            },
            Self::click(cx, move |this, _, cx| {
                {
                    let mut hub = this.hub.lock();
                    if hub.show_favorites == favorites {
                        return;
                    }
                    hub.show_favorites = favorites;
                    hub.page = 1;
                    hub.apply();
                }
                this.hub_reset_scroll();
                cx.notify();
            }),
        )
    }

    pub fn hub_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_hub_loaded();
        let text_color = self.color(S, "TextColor");

        // Header: search + sort
        let search_colors = EntryColors {
            bg: self.color(S, "SearchBarColor"),
            border: self.color(S, "SearchBarBorderColor"),
            text: text_color,
            placeholder: self.color("Settings", "TextColorDisabled"),
        };
        let search = self.entry(EntryId::Search, 0.0, "Search songs or artists…", true, search_colors, window, cx);
        let search_btn = button(
            &self.theme,
            ButtonSpec {
                id: "hub-search".into(),
                label: "".into(),
                width: 28.0,
                height: 28.0,
                enabled: true,
                colors: self.button_colors(S, "SearchButtonColor", "SearchButtonHoverColor"),
                icon: self.theme.icons.search.clone().map(|i| (i, 18.0)),
            },
            Self::click(cx, |this, window, cx| {
                let q = this.entries.get(&EntryId::Search).filter(|e| e.editing).map(|e| e.buffer.clone());
                {
                    let mut hub = this.hub.lock();
                    if let Some(q) = q {
                        hub.query = q;
                    }
                    hub.page = 1;
                    hub.apply();
                }
                window.blur();
                this.hub_reset_scroll();
                cx.notify();
            }),
        );
        let sort_name = SortMode::NAMES[[SortMode::Newest, SortMode::Oldest, SortMode::Downloads, SortMode::Views]
            .iter()
            .position(|m| *m == self.hub.lock().sort)
            .unwrap_or(0)];
        let sort_colors = DropdownColors {
            bg: self.color(S, "SortOptionBackColor"),
            button: self.color(S, "SortOptionDropdownButtonColor"),
            button_hover: self.color(S, "SortOptionDropdownButtonHoverColor"),
            menu_bg: self.color(S, "SortOptionDropdownBackground"),
            item_hover: self.color(S, "SortOptionDropdownItemHover"),
            text: text_color,
            text_disabled: text_color,
            border: Some(self.color(S, "SortOptionBorderColor")),
        };
        let sort = self.dropdown(DropId::HubSort, sort_name.into(), SortMode::NAMES.iter().map(|s| s.to_string()).collect(), 105.0, true, sort_colors, cx,
            |this, i, _, cx| {
                {
                    let mut hub = this.hub.lock();
                    hub.sort = SortMode::from_name(SortMode::NAMES[i]);
                    hub.apply();
                }
                this.hub_reset_scroll();
                cx.notify();
            });
        let search_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().min_w_0().child(search))
            .child(search_btn)
            .child(sort);
        let mut toolbar = div()
            .mx(px(20.))
            .mt(px(12.))
            .p(px(10.))
            .rounded(px(16.))
            .bg(super::pages::shade(self.color(S, "BackgroundColor"), 0.04))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(search_row);

        let mut body = div().flex().flex_col().gap(px(10.)).pb(px(10.));

        let status = self.hub.lock().status.clone();
        match status {
            HubStatus::Loading => {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .pt(px(150.))
                        .child(lines(&self.theme, "Please Wait...", 40.0, text_color))
                        .child(lines(&self.theme, "Fetching from nanomidi.net...", 14.0, text_color)),
                );
            }
            HubStatus::Error(kind) => {
                let retry = button(
                    &self.theme,
                    ButtonSpec {
                        id: "hub-retry".into(),
                        label: "Retry".into(),
                        width: 118.0,
                        height: ROW_H,
                        enabled: true,
                        colors: self.button_colors(S, "NextButtonColor", "NextButtonHoverColor"),
                        icon: None,
                    },
                    Self::click(cx, |this, _, cx| {
                        this.hub_requested = false;
                        this.ensure_hub_loaded();
                        cx.notify();
                    }),
                );
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(14.))
                        .pt(px(30.))
                        .px(px(10.))
                        .child(lines(&self.theme, "Couldn't reach\nnanomidi.net!", 40.0, text_color).items_center())
                        .child(lines(
                            &self.theme,
                            &format!(
                                "Error: {kind}\n\nThis may have happened for one of the\nfollowing reasons:\n\n1. You are offline.\n2. Something is blocking your connection\nto https://nanomidi.net\n3. The server is currently offline or\nencountered an issue — please try again later.\n\nIf nanoMIDI is Offline, please report it to the\ndeveloper or check server status here\nhttps://status.nanomidi.net"
                            ),
                            12.0,
                            text_color,
                        ))
                        .child(retry),
                );
            }
            HubStatus::Ready => {
                self.load_page_images();
                let (items, page, total, all_count, fav_count, showing_favs, matches, searching, offline) = {
                    let hub = self.hub.lock();
                    (
                        hub.page_items(),
                        hub.page,
                        hub.total_pages(),
                        hub.all.len(),
                        hub.favorite_count(),
                        hub.show_favorites,
                        hub.filtered.len(),
                        !hub.query.trim().is_empty(),
                        hub.offline,
                    )
                };
                let mut info = String::new();
                if searching {
                    info.push_str(&format!("{matches} match{}", if matches == 1 { "" } else { "es" }));
                }
                if offline {
                    info.push_str(if info.is_empty() { "Offline (cached)" } else { " · Offline (cached)" });
                }
                toolbar = toolbar.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .rounded_full()
                                .bg(self.color(S, "NextButtonColor"))
                                .p(px(2.))
                                .gap(px(2.))
                                .child(self.hub_filter_tab("hub-tab-all", format!("All  {all_count}"), !showing_favs, false, cx))
                                .child(self.hub_filter_tab("hub-tab-fav", format!("★  {fav_count}"), showing_favs, true, cx)),
                        )
                        .child(div().flex_1().min_w_0().child(lines(&self.theme, &info, 11.0, self.color("Settings", "TextColorDisabled"))))
                        .child(self.hub_view_switch(cx)),
                );
                if items.is_empty() {
                    let msg = if showing_favs && fav_count == 0 {
                        "No favorites yet.\nClick ☆ on a song to add it here."
                    } else {
                        "No songs found."
                    };
                    body = body.child(div().flex().justify_center().pt(px(40.)).child(lines(&self.theme, msg, 14.0, text_color).items_center()));
                }
                // Grid tiles split the available width evenly (min ~150 px per tile).
                let content_w: f32 = (window.viewport_size().width - px(super::SIDEBAR_W)).into();
                let avail = (content_w - 40.0).max(150.0);
                let columns = ((avail + 8.0) / 158.0).floor().max(1.0);
                let tile_w = ((avail - 8.0 * (columns - 1.0)) / columns).floor();
                let mut list = match self.hub_view {
                    super::HubView::Grid => div().mx(px(20.)).flex().flex_wrap().gap(px(8.)),
                    super::HubView::List => div().mx(px(20.)).flex().flex_col().gap(px(8.)),
                    super::HubView::Compact => div().mx(px(20.)).flex().flex_col().gap(px(4.)),
                };
                for (i, midi) in items.iter().enumerate() {
                    let image = midi
                        .image_filename
                        .as_ref()
                        .and_then(|n| self.hub.lock().images.get(n).cloned().flatten());
                    let card_w = if self.hub_view == super::HubView::Grid { tile_w } else { avail };
                    list = list.child(self.hub_card(i, midi, image, card_w, cx));
                }
                body = body.child(list);
                let nav = |this: &Self, id: &'static str, label: &'static str, delta: i64, cx: &mut Context<Self>| {
                    button(
                        &this.theme,
                        ButtonSpec {
                            id: id.into(),
                            label: label.into(),
                            width: 118.0,
                            height: ROW_H,
                            enabled: true,
                            colors: this.button_colors(S, if delta < 0 { "PreviousButtonColor" } else { "NextButtonColor" }, if delta < 0 { "PreviousButtonHoverColor" } else { "NextButtonHoverColor" }),
                            icon: None,
                        },
                        Self::click(cx, move |this, _, cx| {
                            {
                                let mut hub = this.hub.lock();
                                let total = hub.total_pages().max(1) as i64;
                                let next = (hub.page as i64 + delta).clamp(1, total) as usize;
                                if next == hub.page {
                                    return;
                                }
                                hub.page = next;
                            }
                            this.hub_reset_scroll();
                            cx.notify();
                        }),
                    )
                };
                body = body.child(
                    div()
                        .mx(px(20.))
                        .rounded(px(6.))
                        .bg(self.color(S, "PageControlFrameBackground"))
                        .p(px(10.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(nav(self, "hub-prev", "Previous", -1, cx))
                        .child(lines(&self.theme, &format!("Page {page}/{total}"), 14.0, text_color))
                        .child(nav(self, "hub-next", "Next", 1, cx)),
                );
            }
        }

        let body = div().flex().flex_col().gap(px(10.)).pb(px(10.)).child(toolbar).child(body);
        div()
            .id("hub-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.hub_scroll)
            .bg(self.color(S, "BackgroundColor"))
            .child(body)
    }
}
