//! Widgets built from GPUI primitives (system UI font, theme colors).

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, FontWeight, Hsla, Image, MouseButton, MouseDownEvent,
    SharedString, Stateful, Window, anchored, deferred, div, img, px,
};
use std::sync::Arc;

use crate::theme::Theme;

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>;

pub const ROW_H: f32 = 28.0;

/// Base text element: bold theme font at the given size.
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
            .ml(px(40.))
            .mt(px(20.))
            .bg(gpui::rgb(0x151515))
            .border_1()
            .border_color(gpui::rgb(0x000000))
            .px(px(2.))
            .font_family(self.font.clone())
            .font_weight(FontWeight::NORMAL)
            .text_size(px(14.))
            .line_height(px(18.))
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
    pub width: f32,
    pub height: f32,
    pub enabled: bool,
    pub colors: ButtonColors,
    pub icon: Option<(Arc<Image>, f32)>,
}

pub fn button(theme: &Theme, spec: ButtonSpec, on_click: ClickHandler) -> Stateful<Div> {
    let ButtonSpec { id, label, width, height, enabled, colors, icon } = spec;
    let mut el = div()
        .id(id)
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .rounded(px(10.))
        .bg(colors.bg)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .overflow_hidden();
    if let Some((image, size)) = icon {
        el = el.child(img(image).w(px(size)).h(px(size)).flex_shrink_0());
    }
    if !label.is_empty() {
        let color = if enabled { colors.text } else { colors.text_disabled };
        el = el.child(lines(theme, &label, 14.0, color).items_center());
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
    pub knob_hover: Hsla,
    pub text: Hsla,
    pub text_disabled: Hsla,
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
    let track_w = 36.0;
    let track_h = 18.0;
    let knob = 14.0;
    let knob_x = if checked { track_w - knob - 2.0 } else { 2.0 };
    let group: SharedString = format!("sw-{label}").into();
    let knob_hover = colors.knob_hover;
    let track = div()
        .relative()
        .w(px(track_w))
        .h(px(track_h))
        .flex_shrink_0()
        .rounded(px(track_h / 2.0))
        .bg(if checked { colors.on } else { colors.off })
        .child(
            div()
                .absolute()
                .top(px((track_h - knob) / 2.0))
                .left(px(knob_x))
                .w(px(knob))
                .h(px(knob))
                .rounded(px(knob / 2.0))
                .bg(colors.knob)
                .when(enabled, |d| d.group_hover(group.clone(), move |s| s.bg(knob_hover))),
        );
    let mut el = div()
        .id(id.into())
        .group(group)
        .flex()
        .items_center()
        .gap(px(6.))
        .min_h(px(24.))
        .child(track)
        .child(lines(theme, label, 14.0, if enabled { colors.text } else { colors.text_disabled }));
    if enabled {
        el = el.cursor_pointer().on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_toggle(!checked, window, cx)
        });
    }
    el
}

// ---------------------------------------------------------------------------
// Slider (dragging is handled by the root view, see `ui::SliderDrag`)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SliderColors {
    pub track: Hsla,
    pub fill: Hsla,
    pub knob: Hsla,
    pub knob_hover: Hsla,
}

pub fn slider(
    id: impl Into<ElementId>,
    fraction: f32,
    width: f32,
    enabled: bool,
    colors: SliderColors,
    on_bounds: impl Fn(gpui::Bounds<gpui::Pixels>) + 'static,
    on_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let h = 16.0;
    let knob = 16.0;
    let usable = width - knob;
    let f = fraction.clamp(0.0, 1.0);
    let knob_x = usable * f;
    let id: ElementId = id.into();
    let group: SharedString = format!("sl-{id:?}").into();
    let knob_hover = colors.knob_hover;
    let mut el = div()
        .id(id)
        .group(group.clone())
        .relative()
        .w(px(width))
        .h(px(h))
        .flex_shrink_0()
        .child(
            gpui::canvas(move |bounds, _, _| on_bounds(bounds), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .left(px(knob / 2.0 - 2.0))
                .top(px(h / 2.0 - 2.0))
                .w(px(usable + 4.0))
                .h(px(4.))
                .rounded(px(2.))
                .bg(colors.track),
        )
        .child(
            div()
                .absolute()
                .left(px(knob / 2.0 - 2.0))
                .top(px(h / 2.0 - 2.0))
                .w(px(knob_x + 4.0))
                .h(px(4.))
                .rounded(px(2.))
                .bg(colors.fill),
        )
        .child(
            div()
                .absolute()
                .left(px(knob_x))
                .top(px((h - knob) / 2.0))
                .w(px(knob))
                .h(px(knob))
                .rounded(px(knob / 2.0))
                .bg(colors.knob)
                .when(enabled, |d| d.group_hover(group, move |s| s.bg(knob_hover))),
        );
    if enabled {
        el = el.cursor_pointer().on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            cx.stop_propagation();
            on_down(ev, window, cx)
        });
    } else {
        el = el.opacity(0.6);
    }
    el
}

// ---------------------------------------------------------------------------
// Option menu / combo box
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DropdownColors {
    pub bg: Hsla,
    pub button: Hsla,
    pub button_hover: Hsla,
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

fn chevron(color: Hsla) -> AnyElement {
    // Small "▼" like CustomTkinter's option menu arrow.
    div()
        .text_size(px(9.))
        .text_color(color)
        .child("▼")
        .into_any_element()
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
    let button_w = 28.0;
    let text_color = if enabled { colors.text } else { colors.text_disabled };
    let button_hover = colors.button_hover;
    let group: SharedString = format!("dd-{id}").into();

    let fill = width <= 0.0;
    let field_w = FIELD_WIDTHS
        .with(|m| m.borrow().get(&id).copied())
        .unwrap_or(if fill { 260.0 } else { width });
    let mut field = div()
        .id(ElementId::Name(id.clone()))
        .group(group.clone())
        .when(fill, |d| d.w_full())
        .when(!fill, |d| d.w(px(width)))
        .h(px(ROW_H))
        .rounded(px(10.))
        .overflow_hidden()
        .flex()
        .bg(colors.bg)
        .when_some(colors.border, |d, b| d.border_2().border_color(b))
        .child(
            div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .min_w_0()
                .pl(px(10.))
                .pr(px(6.))
                .overflow_hidden()
                .child(text(theme, ellipsize(&value, 14.0, field_w - 44.0), 14.0, text_color).whitespace_nowrap()),
        )
        .child(
            div()
                .w(px(button_w))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(colors.button)
                .when(enabled, |d| d.group_hover(group, move |s| s.bg(button_hover)))
                .child(chevron(text_color)),
        );
    if enabled {
        field = field.cursor_pointer().on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            on_toggle(window, cx)
        });
    }

    let measure_id = id.clone();
    let mut root = div()
        .relative()
        .when(fill, |d| d.w_full())
        .when(!fill, |d| d.w(px(width)))
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
        // Menu is exactly as wide as the field (at least 120 px) and stays aligned with it.
        let menu_w = FIELD_WIDTHS
            .with(|m| m.borrow().get(&id).copied())
            .unwrap_or(if fill { 260.0 } else { width })
            .max(120.0);
        let mut menu = div()
            .id(ElementId::Name(format!("{id}-menu").into()))
            .w(px(menu_w))
            .max_h(px(280.))
            .overflow_y_scroll()
            .bg(colors.menu_bg)
            .border_1()
            .border_color(gpui::rgb(0x2b2b2b))
            .rounded(px(10.))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(1.))
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
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(hover))
                    .hover(move |s| s.bg(hover))
                    .child(text(theme, ellipsize(item, 14.0, menu_w - 26.0), 14.0, colors.text).whitespace_nowrap())
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
        .rounded(px(10.))
        .border_2()
        .border_color(colors.border)
        .bg(colors.bg)
        .px(px(6.))
        .flex()
        .items_center()
        .overflow_hidden()
        .when(enabled, |d| d.cursor_text())
        .child(text(theme, shown, 14.0, if enabled { color } else { colors.placeholder }).whitespace_nowrap())
        .when(focused, |d| d.child(caret))
}
