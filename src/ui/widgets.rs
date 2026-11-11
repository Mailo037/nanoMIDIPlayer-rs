//! Widgets built from GPUI primitives (system UI font, theme colors).
//!
//! One scale for the whole app: controls are [`ROW_H`] high with a [`R_CONTROL`]
//! corner radius, cards use [`R_CARD`], and spacing moves in steps of 4 px.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Div, ElementId, FontWeight, Hsla, MouseButton, MouseDownEvent, SharedString,
    Stateful, Window, anchored, deferred, div, px, relative,
};

use super::icons::{Icon, icon};
use crate::theme::Theme;

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>;

/// Height of buttons, fields and dropdowns.
pub const ROW_H: f32 = 34.0;
/// Corner radius of buttons, fields and dropdowns.
pub const R_CONTROL: f32 = 10.0;
/// Corner radius of cards.
pub const R_CARD: f32 = 14.0;

/// Slightly lighter (dark themes) or darker (light themes) shade of a color.
pub fn shade(c: Hsla, amount: f32) -> Hsla {
    let l = if c.l < 0.5 { (c.l + amount).min(1.0) } else { (c.l - amount).max(0.0) };
    Hsla { l, ..c }
}

/// Base text element: theme font at the given size.
pub fn text(theme: &Theme, s: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    div()
        .font_family(theme.ui_font.clone())
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(size))
        .line_height(px(size + 4.0))
        .text_color(color)
        .child(s.into())
}

/// Multi-line label (one child per line, like Tk labels with `\n`).
pub fn lines(theme: &Theme, s: &str, size: f32, color: Hsla) -> Div {
    let mut d = div()
        .flex()
        .flex_col()
        .font_family(theme.ui_font.clone())
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(size))
        .line_height(px(size + 4.0))
        .text_color(color);
    for line in s.split('\n') {
        d = d.child(div().whitespace_nowrap().child(SharedString::from(line.to_string())));
    }
    d
}

/// Like [`lines`], but long lines wrap to the available width.
pub fn paragraph(theme: &Theme, s: &str, size: f32, color: Hsla) -> Div {
    let mut d = div()
        .flex()
        .flex_col()
        .min_w_0()
        .font_family(theme.ui_font.clone())
        .font_weight(FontWeight::NORMAL)
        .text_size(px(size))
        .line_height(px(size + 4.0))
        .text_color(color);
    for line in s.split('\n') {
        d = d.child(div().child(SharedString::from(line.to_string())));
    }
    d
}

/// A key name in a small outlined box ("F1").
pub fn keycap(theme: &Theme, label: &str, color: Hsla) -> Div {
    div()
        .h(px(18.))
        .min_w(px(18.))
        .px(px(5.))
        .flex_shrink_0()
        .rounded(px(5.))
        .border_1()
        .border_color(Hsla { a: color.a * 0.5, ..color })
        .flex()
        .items_center()
        .justify_center()
        .child(text(theme, label.to_string(), 11.0, color).font_weight(FontWeight::SEMIBOLD).whitespace_nowrap())
}

pub fn with_tooltip(el: Stateful<Div>, enabled: bool, text: &'static str, theme: &Theme) -> Stateful<Div> {
    if !enabled || text.is_empty() {
        return el;
    }
    let font = theme.ui_font.clone();
    el.tooltip(move |_window, cx| {
        let font = font.clone();
        cx.new(|_| TooltipView { text, font }).into()
    })
}

pub struct TooltipView {
    text: &'static str,
    font: String,
}

impl Render for TooltipView {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut d = div()
            .ml(px(14.))
            .mt(px(18.))
            .bg(gpui::rgb(0x1c1c1c))
            .border_1()
            .border_color(gpui::rgb(0x333333))
            .rounded(px(8.))
            .px(px(9.))
            .py(px(6.))
            .shadow_md()
            .font_family(self.font.clone())
            .font_weight(FontWeight::NORMAL)
            .text_size(px(12.))
            .line_height(px(17.))
            .text_color(gpui::white())
            .flex()
            .flex_col();
        for line in self.text.split('\n') {
            d = d.child(div().whitespace_nowrap().child(line));
        }
        d
    }
}

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ButtonColors {
    pub bg: Hsla,
    pub hover: Hsla,
    pub text: Hsla,
    pub text_disabled: Hsla,
}

pub struct ButtonSpec {
    pub id: ElementId,
    pub label: SharedString,
    /// Fixed width; `0.0` sizes the button to its content.
    pub width: f32,
    pub height: f32,
    pub enabled: bool,
    pub colors: ButtonColors,
    /// Icon and its size, shown before the label.
    pub icon: Option<(Icon, f32)>,
}

pub fn button(theme: &Theme, spec: ButtonSpec, on_click: ClickHandler) -> Stateful<Div> {
    let ButtonSpec { id, label, width, height, enabled, colors, icon: glyph } = spec;
    let color = if enabled { colors.text } else { colors.text_disabled };
    let mut el = div()
        .id(id)
        .when(width > 0.0, |d| d.w(px(width)))
        .when(width <= 0.0, |d| d.px(px(14.)))
        .h(px(height))
        .flex_shrink_0()
        .rounded(px(R_CONTROL))
        .bg(colors.bg)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .overflow_hidden();
    if let Some((glyph, size)) = glyph {
        el = el.child(icon(glyph, size, color));
    }
    if !label.is_empty() {
        el = el.child(lines(theme, &label, 13.0, color).items_center());
    }
    if enabled {
        let hover = colors.hover;
        el = el
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click(move |ev, window, cx| {
                cx.stop_propagation();
                on_click(ev, window, cx)
            });
    }
    el
}

// ---------------------------------------------------------------------------
// Switch
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SwitchColors {
    pub on: Hsla,
    pub off: Hsla,
    pub knob: Hsla,
    pub knob_off: Hsla,
    pub text: Hsla,
    pub text_disabled: Hsla,
}

/// The switch itself, without label or click handling.
pub fn switch_track(checked: bool, colors: SwitchColors) -> Div {
    let (track_w, track_h, knob) = (34.0, 20.0, 14.0);
    let inset = (track_h - knob) / 2.0;
    div()
        .relative()
        .w(px(track_w))
        .h(px(track_h))
        .flex_shrink_0()
        .rounded_full()
        .bg(if checked { colors.on } else { colors.off })
        .child(
            div()
                .absolute()
                .top(px(inset))
                .left(px(if checked { track_w - knob - inset } else { inset }))
                .w(px(knob))
                .h(px(knob))
                .rounded_full()
                .bg(if checked { colors.knob } else { colors.knob_off }),
        )
}

pub fn switch(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: &str,
    checked: bool,
    enabled: bool,
    colors: SwitchColors,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let mut el = div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(24.))
        .child(switch_track(checked, colors))
        .when(!enabled, |d| d.opacity(0.5));
    if !label.is_empty() {
        el = el.child(lines(theme, label, 13.0, if enabled { colors.text } else { colors.text_disabled }));
    }
    if enabled {
        el = el.cursor_pointer().on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_toggle(!checked, window, cx)
        });
    }
    el
}

// ---------------------------------------------------------------------------
// Slider (dragging is handled by the root view, see `RootView::drag_to`)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SliderColors {
    pub track: Hsla,
    pub fill: Hsla,
    pub knob: Hsla,
}

/// Horizontal slider; `width <= 0` fills the available width.
pub fn slider(
    id: impl Into<ElementId>,
    fraction: f32,
    width: f32,
    enabled: bool,
    colors: SliderColors,
    on_bounds: impl Fn(gpui::Bounds<gpui::Pixels>) + 'static,
    on_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (h, knob) = (16.0, 16.0);
    let f = fraction.clamp(0.0, 1.0);
    let mut el = div()
        .id(id.into())
        .relative()
        .when(width > 0.0, |d| d.w(px(width)).flex_shrink_0())
        .when(width <= 0.0, |d| d.flex_1().min_w(px(90.)))
        .h(px(h))
        .child(
            gpui::canvas(move |bounds, _, _| on_bounds(bounds), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .left(px(knob / 2.0))
                .right(px(knob / 2.0))
                .top(px(h / 2.0 - 2.0))
                .h(px(4.))
                .rounded_full()
                .bg(colors.track)
                .child(div().h_full().w(relative(f)).rounded_full().bg(colors.fill)),
        )
        .child(
            // The knob travels from 0 to (width - knob): `left: f * 100%` minus `f * knob`.
            div()
                .absolute()
                .left(relative(f))
                .ml(px(-knob * f))
                .top(px((h - knob) / 2.0))
                .w(px(knob))
                .h(px(knob))
                .rounded_full()
                .bg(colors.knob)
                .shadow_sm(),
        );
    if enabled {
        el = el.cursor_pointer().on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            cx.stop_propagation();
            on_down(ev, window, cx)
        });
    } else {
        el = el.opacity(0.5);
    }
    el
}

// ---------------------------------------------------------------------------
// Option menu / combo box
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DropdownColors {
    pub bg: Hsla,
    /// Background while the pointer is over the field.
    pub hover: Hsla,
    pub menu_bg: Hsla,
    pub item_hover: Hsla,
    pub text: Hsla,
    pub text_disabled: Hsla,
    pub border: Option<Hsla>,
}

pub struct DropdownSpec {
    pub id: SharedString,
    pub value: String,
    pub width: f32,
    pub enabled: bool,
    pub open: bool,
    pub items: Vec<String>,
    pub colors: DropdownColors,
}

/// Shortens `s` with "…" to fit `max_w` pixels, estimating ~0.55 em per character
/// (generous for the UI font); GPUI's own `truncate()` measures too early in flex
/// layouts and would cut everything.
pub fn ellipsize(s: &str, size: f32, max_w: f32) -> String {
    let max_chars = ((max_w / (size * 0.55)).floor() as usize).max(4);
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max_chars - 1).collect();
    t.push('…');
    t
}

thread_local! {
    /// Last measured width of each dropdown field, so its menu can match it exactly.
    static FIELD_WIDTHS: std::cell::RefCell<std::collections::HashMap<SharedString, f32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub fn dropdown(
    theme: &Theme,
    spec: DropdownSpec,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    on_close: Rc<dyn Fn(&mut Window, &mut App) + 'static>,
    on_select: SelectHandler,
) -> Div {
    let DropdownSpec { id, value, width, enabled, open, items, colors } = spec;
    let text_color = if enabled { colors.text } else { colors.text_disabled };
    let hover = colors.hover;
    let border = colors.border.unwrap_or_else(|| shade(colors.bg, 0.12));

    let fill = width <= 0.0;
    let field_w = FIELD_WIDTHS
        .with(|m| m.borrow().get(&id).copied())
        .unwrap_or(if fill { 260.0 } else { width });
    let mut field = div()
        .id(ElementId::Name(id.clone()))
        .when(fill, |d| d.w_full())
        .when(!fill, |d| d.w(px(width)))
        .h(px(ROW_H))
        .rounded(px(R_CONTROL))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(6.))
        .pl(px(11.))
        .pr(px(9.))
        .bg(colors.bg)
        .border_1()
        .border_color(border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(text(theme, ellipsize(&value, 13.0, field_w - 44.0), 13.0, text_color).whitespace_nowrap()),
        )
        .child(icon(Icon::ChevronDown, 14.0, colors.text_disabled));
    if enabled {
        field = field
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_toggle(window, cx)
            });
    }

    let measure_id = id.clone();
    let mut root = div()
        .relative()
        .when(fill, |d| d.w_full())
        .when(!fill, |d| d.w(px(width)).flex_shrink_0())
        .h(px(ROW_H))
        .child(field)
        .child(
            gpui::canvas(
                move |bounds, _, _| {
                    let w: f32 = bounds.size.width.into();
                    FIELD_WIDTHS.with(|m| m.borrow_mut().insert(measure_id, w));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
    if open && enabled {
        // Menu is exactly as wide as the field (at least 140 px) and stays aligned with it.
        let menu_w = FIELD_WIDTHS
            .with(|m| m.borrow().get(&id).copied())
            .unwrap_or(if fill { 260.0 } else { width })
            .max(140.0);
        let mut menu = div()
            .id(ElementId::Name(format!("{id}-menu").into()))
            .w(px(menu_w))
            .max_h(px(280.))
            .overflow_y_scroll()
            .bg(colors.menu_bg)
            .border_1()
            .border_color(shade(colors.menu_bg, 0.16))
            .rounded(px(12.))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (i, item) in items.iter().enumerate() {
            let select = on_select.clone();
            let close = on_close.clone();
            let hover = colors.item_hover;
            let selected = *item == value;
            menu = menu.child(
                div()
                    .id(ElementId::NamedInteger(format!("{id}-item").into(), i as u64))
                    .w_full()
                    .h(px(30.))
                    .flex_shrink_0()
                    .px(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(text(theme, ellipsize(item, 13.0, menu_w - 44.0), 13.0, colors.text).whitespace_nowrap()),
                    )
                    .when(selected, |d| d.child(icon(Icon::Check, 14.0, colors.text)))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        close(window, cx);
                        select(i, window, cx);
                    }),
            );
        }
        root = root.child(
            div()
                .absolute()
                .top(px(ROW_H + 4.0))
                .left_0()
                .child(deferred(anchored().snap_to_window_with_margin(px(8.)).child(menu)).with_priority(1)),
        );
    }
    root
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct EntryColors {
    pub bg: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub placeholder: Hsla,
}

pub fn entry(
    theme: &Theme,
    id: impl Into<ElementId>,
    value: &str,
    placeholder: &str,
    focused: bool,
    enabled: bool,
    width: f32,
    colors: EntryColors,
) -> Stateful<Div> {
    let (shown, color) = if value.is_empty() && !focused {
        (placeholder.to_string(), colors.placeholder)
    } else {
        (value.to_string(), colors.text)
    };
    let caret = div().w(px(1.)).h(px(16.)).bg(colors.text).flex_shrink_0();
    div()
        .id(id.into())
        .when(width > 0.0, |d| d.w(px(width)).flex_shrink_0())
        .when(width <= 0.0, |d| d.w_full())
        .h(px(ROW_H))
        .rounded(px(R_CONTROL))
        .border_1()
        .border_color(if focused { colors.text } else { shade(colors.border, 0.08) })
        .bg(colors.bg)
        .px(px(10.))
        .flex()
        .items_center()
        .overflow_hidden()
        .when(enabled, |d| d.cursor_text())
        .when(!enabled, |d| d.opacity(0.5))
        .child(text(theme, shown, 13.0, if enabled { color } else { colors.placeholder }).whitespace_nowrap())
        .when(focused, |d| d.child(caret))
}
