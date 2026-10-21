//! Right-click menus: any element can open one with [`RootView::menu_trigger`].
//! The menu floats at the mouse position and closes on the next click.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, ElementId, MouseButton, MouseDownEvent, Pixels, Point, Window, anchored, deferred, div, px};

use super::RootView;
use super::widgets::*;

pub type MenuAction = Rc<dyn Fn(&mut RootView, &mut Window, &mut Context<RootView>)>;

pub enum MenuItem {
    Action {
        icon: &'static str,
        label: String,
        hint: String,
        enabled: bool,
        danger: bool,
        checked: Option<bool>,
        action: MenuAction,
    },
    Title(String),
    Separator,
}

impl MenuItem {
    pub fn new(icon: &'static str, label: impl Into<String>, action: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static) -> Self {
        MenuItem::Action {
            icon,
            label: label.into(),
            hint: String::new(),
            enabled: true,
            danger: false,
            checked: None,
            action: Rc::new(action),
        }
    }

    /// Item with a checkmark column (for on/off settings).
    pub fn check(label: impl Into<String>, on: bool, action: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static) -> Self {
        let mut item = Self::new("", label, action);
        if let MenuItem::Action { checked, .. } = &mut item {
            *checked = Some(on);
        }
        item
    }

    pub fn title(s: impl Into<String>) -> Self {
        MenuItem::Title(s.into())
    }

    /// Shortcut or extra info shown dimmed on the right.
    pub fn hint(mut self, h: impl Into<String>) -> Self {
        if let MenuItem::Action { hint, .. } = &mut self {
            *hint = h.into();
        }
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        if let MenuItem::Action { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }

    /// Red label for destructive actions.
    pub fn danger(mut self) -> Self {
        if let MenuItem::Action { danger, .. } = &mut self {
            *danger = true;
        }
        self
    }
}

pub struct ContextMenu {
    pub position: Point<Pixels>,
    pub items: Vec<MenuItem>,
}

const S: &str = "MidiPlayer";

impl RootView {
    /// Right-click handler that builds the menu when it is opened.
    pub fn menu_trigger(
        cx: &Context<Self>,
        build: impl Fn(&mut Self) -> Vec<MenuItem> + 'static,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        let open = Self::handler(cx, move |this, pos: Point<Pixels>, _, cx| {
            let items = build(this);
            this.open_drop = None;
            this.context_menu = (!items.is_empty()).then_some(ContextMenu { position: pos, items });
            cx.notify();
        });
        move |ev, window, cx| {
            cx.stop_propagation();
            open(ev.position, window, cx)
        }
    }

    pub fn close_menu(&mut self) -> bool {
        self.context_menu.take().is_some()
    }

    pub(super) fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let text_c = self.color(S, "TextColor");
        let dim = self.color(S, "TextColorDisabled");
        let danger = self.color(S, "StopColor");
        let accent = self.color(S, "PlayColor");
        let hover = self.color(S, "ButtonHoverColor");
        let bg = super::pages::shade(self.color(S, "OptionDropdownBackground"), 0.03);
        let border = super::pages::shade(self.color(S, "BackgroundColor"), 0.12);

        let mut list = div()
            .id("context-menu")
            .min_w(px(240.))
            .max_w(px(340.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .rounded(px(12.))
            .p(px(5.))
            .flex()
            .flex_col()
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation());
        for (i, item) in menu.items.iter().enumerate() {
            list = list.child(match item {
                MenuItem::Separator => div().my(px(4.)).mx(px(6.)).h(px(1.)).bg(border).into_any_element(),
                MenuItem::Title(t) => div()
                    .px(px(10.))
                    .pt(px(6.))
                    .pb(px(2.))
                    .child(text(&self.theme, t.clone(), 11.0, dim).font_weight(gpui::FontWeight::SEMIBOLD))
                    .into_any_element(),
                MenuItem::Action { icon, label, hint, enabled, danger: is_danger, checked, .. } => {
                    let color = if !enabled { dim } else if *is_danger { danger } else { text_c };
                    let lead: String = match checked {
                        Some(true) => "✓".into(),
                        Some(false) => "".into(),
                        None => icon.to_string(),
                    };
                    let lead_color = if checked.is_some() { accent } else { color };
                    div()
                        .id(ElementId::NamedInteger("menu-item".into(), i as u64))
                        .h(px(30.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(18.)).flex().justify_center().child(text(&self.theme, lead, 13.0, lead_color)))
                        .child(div().flex_1().min_w_0().overflow_hidden().child(text(&self.theme, ellipsize(label, 13.0, 250.0), 13.0, color).font_weight(gpui::FontWeight::NORMAL).whitespace_nowrap()))
                        .when(!hint.is_empty(), |d| d.child(text(&self.theme, hint.clone(), 11.0, dim).whitespace_nowrap()))
                        .when(*enabled, |d| {
                            d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                if let Some(menu) = this.context_menu.take() {
                                    if let Some(MenuItem::Action { action, .. }) = menu.items.get(i) {
                                        let action = action.clone();
                                        action(this, window, cx);
                                    }
                                }
                                cx.notify();
                            }))
                        })
                        .into_any_element()
                }
            });
        }
        Some(
            deferred(anchored().position(menu.position).snap_to_window_with_margin(px(8.)).child(list))
                .with_priority(3)
                .into_any_element(),
        )
    }
}
