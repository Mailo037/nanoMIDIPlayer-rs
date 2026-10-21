//! Info tab: renders INFO.md (remote, with offline fallback) as styled markdown.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use gpui::prelude::*;
use gpui::{Context, Div, ElementId, FontWeight, Hsla, Image, SharedString, Window, div, img, px, rgb};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::RootView;
use crate::net;

const INFO_URL: &str = "https://raw.githubusercontent.com/NotHammer043/nanoMIDIPlayer/refs/heads/main/api/INFO.md";
const OFFLINE: &str = include_str!("../../assets/INFO-offline.md");
const ACCENT: u32 = 0x00ff00;

#[derive(Clone, Debug, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub code: bool,
    pub link: Option<String>,
    pub image: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    Item(Vec<Span>),
    Code(String),
    Rule,
    Table(Vec<Vec<Vec<Span>>>),
}

pub struct Doc {
    pub blocks: Vec<Block>,
    pub images: HashMap<String, Option<Arc<Image>>>,
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn html_img_src(s: &str) -> Option<String> {
    let lower = s.to_lowercase();
    if !lower.trim_start().starts_with("<img") {
        return None;
    }
    let idx = lower.find("src=\"")? + 5;
    let end = s[idx..].find('"')? + idx;
    Some(s[idx..end].to_string())
}

pub fn parse(md: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut bold = 0;
    let mut link: Option<String> = None;
    let mut image: Option<String> = None;
    let mut heading: Option<u8> = None;
    let mut in_item = false;
    let mut code_block: Option<String> = None;
    let mut table: Option<Vec<Vec<Vec<Span>>>> = None;
    let mut row: Vec<Vec<Span>> = Vec::new();

    let push_text = |spans: &mut Vec<Span>, text: &str, bold: bool, code: bool, link: &Option<String>, image: &Option<String>| {
        if image.is_some() {
            return;
        }
        spans.push(Span { text: text.to_string(), bold, code, link: link.clone(), image: None });
    };

    for event in Parser::new_ext(md, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some(level as u8),
            Event::End(TagEnd::Heading(_)) => {
                blocks.push(Block::Heading(heading.take().unwrap_or(1), std::mem::take(&mut spans)));
            }
            Event::Start(Tag::Item) => in_item = true,
            Event::End(TagEnd::Item) => {
                in_item = false;
                if !spans.is_empty() {
                    blocks.push(Block::Item(std::mem::take(&mut spans)));
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if table.is_none() && !spans.is_empty() {
                    let s = std::mem::take(&mut spans);
                    blocks.push(if in_item { Block::Item(s) } else { Block::Paragraph(s) });
                }
            }
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold -= 1,
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => link = None,
            Event::Start(Tag::Image { dest_url, .. }) => {
                spans.push(Span { image: Some(dest_url.to_string()), link: link.clone(), ..Default::default() });
                image = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Image) => image = None,
            Event::Start(Tag::CodeBlock(_)) => code_block = Some(String::new()),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(code) = code_block.take() {
                    blocks.push(Block::Code(code));
                }
            }
            Event::Start(Tag::Table(_)) => table = Some(Vec::new()),
            Event::End(TagEnd::Table) => {
                if let Some(t) = table.take() {
                    blocks.push(Block::Table(t));
                }
            }
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.push(std::mem::take(&mut row));
                }
            }
            Event::End(TagEnd::TableCell) => row.push(std::mem::take(&mut spans)),
            Event::Rule => blocks.push(Block::Rule),
            Event::Text(t) => {
                if let Some(code) = code_block.as_mut() {
                    code.push_str(&t);
                } else {
                    push_text(&mut spans, &t, bold > 0, false, &link, &image);
                }
            }
            Event::Code(t) => push_text(&mut spans, &t, bold > 0, true, &link, &image),
            Event::InlineHtml(h) | Event::Html(h) => {
                if let Some(src) = html_img_src(&h) {
                    spans.push(Span { image: Some(src), link: link.clone(), ..Default::default() });
                } else {
                    let t = strip_html(&h);
                    if !t.trim().is_empty() {
                        push_text(&mut spans, &t, bold > 0, false, &link, &image);
                    }
                }
            }
            Event::SoftBreak => push_text(&mut spans, " ", bold > 0, false, &link, &image),
            Event::HardBreak => spans.push(Span { text: "\n".into(), ..Default::default() }),
            _ => {}
        }
    }
    if !spans.is_empty() {
        blocks.push(Block::Paragraph(spans));
    }
    blocks
}

impl RootView {
    pub fn load_info(&self) {
        let info = self.info.clone();
        let version = self.info_version.clone();
        std::thread::spawn(move || {
            let md = net::get_text(INFO_URL).unwrap_or_else(|e| {
                log::warn!("Failed to fetch remote INFO.md: {e}");
                OFFLINE.to_string()
            });
            let blocks = parse(&md);
            let urls: Vec<String> = blocks
                .iter()
                .flat_map(|b| match b {
                    Block::Heading(_, s) | Block::Paragraph(s) | Block::Item(s) => s.clone(),
                    Block::Table(rows) => rows.iter().flatten().flatten().cloned().collect(),
                    _ => Vec::new(),
                })
                .filter_map(|s| s.image)
                .collect();
            *info.lock() = Some(Doc { blocks, images: HashMap::new() });
            version.fetch_add(1, Ordering::Release);
            for url in urls {
                let info = info.clone();
                let version = version.clone();
                std::thread::spawn(move || {
                    let image = net::get_bytes(&url).ok().and_then(|bytes| {
                        let format = crate::theme::sniff_format(&bytes)?;
                        Some(Arc::new(Image::from_bytes(format, bytes)))
                    });
                    if let Some(doc) = info.lock().as_mut() {
                        doc.images.insert(url, image);
                    }
                    version.fetch_add(1, Ordering::Release);
                });
            }
        });
    }

    fn spans(&self, spans: &[Span], size: f32, color: Hsla, images: &HashMap<String, Option<Arc<Image>>>, id_base: &mut u64) -> Div {
        // Images inside table cells are small avatars/badges in INFO.md.
        let max_img_h = if size < 14.0 { 16.0 } else { 120.0 };
        let mut d = div().flex().flex_wrap().items_end();
        for span in spans {
            if let Some(url) = &span.image {
                if let Some(Some(image)) = images.get(url) {
                    *id_base += 1;
                    let mut el = div()
                        .id(ElementId::NamedInteger("md-img".into(), *id_base))
                        .mr(px(4.))
                        .child(img(image.clone()).max_w(px(360.)).max_h(px(max_img_h)));
                    if let Some(link) = span.link.clone() {
                        el = el.cursor_pointer().on_click(move |_, _, _| net::open_url(&link));
                    }
                    d = d.child(el);
                }
                continue;
            }
            let span_color: Hsla = if span.link.is_some() { rgb(ACCENT).into() } else { color };
            for (i, word) in span.text.split(' ').enumerate() {
                if word == "\n" {
                    d = d.child(div().w_full());
                    continue;
                }
                let text = if i == 0 { word.to_string() } else { format!(" {word}") };
                if text.is_empty() {
                    continue;
                }
                *id_base += 1;
                let mut w = div()
                    .id(ElementId::NamedInteger("md-w".into(), *id_base))
                    .text_size(px(size))
                    .text_color(span_color)
                    .whitespace_nowrap()
                    .when(span.bold, |d| d.font_weight(FontWeight::BOLD))
                    .when(span.code, |d| d.bg(rgb(0x2e2e2e)).rounded(px(3.)))
                    .child(SharedString::from(text));
                if let Some(link) = span.link.clone() {
                    w = w.cursor_pointer().on_click(move |_, _, _| net::open_url(&link));
                }
                d = d.child(w);
            }
        }
        d
    }

    /// Explains that this is a Rust fork and links to both repositories.
    fn fork_banner(&self) -> Div {
        let accent: Hsla = rgb(ACCENT).into();
        let link = |id: &'static str, label: &'static str, url: &'static str, primary: bool| {
            div()
                .id(id)
                .px(px(14.))
                .py(px(7.))
                .rounded_full()
                .cursor_pointer()
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .when(primary, |d| d.bg(rgb(0x008000)).hover(|s| s.bg(rgb(0x006400))))
                .when(!primary, |d| d.bg(rgb(0x2a2a2a)).hover(|s| s.bg(rgb(0x3a3a3a))))
                .child(label)
                .on_click(move |_, _, _| net::open_url(url))
        };
        div()
            .rounded(px(16.))
            .bg(rgb(0x161616))
            .border_1()
            .border_color(accent)
            .p(px(14.))
            .mb(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(accent)
                    .child(SharedString::from(format!("nanoMIDIPlayer-rs {}", super::APP_VERSION))),
            )
            .child(div().text_size(px(13.)).child(
                "This is an unofficial fork of nanoMIDIPlayer, rebuilt in Rust with GPUI. \
                 Bugs in this version belong in the fork's issues, not the original project's.",
            ))
            .child(div().text_size(px(13.)).text_color(rgb(0xb0b0b0)).child(
                "All credit for the original app, MIDI Hub and community goes to NotHammer043 and the nanoMIDI team (see below).",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(link("fork-original", "Original nanoMIDIPlayer ↗", super::UPSTREAM_REPO, true))
                    .child(link("fork-repo", "This Rust fork ↗", super::FORK_REPO, false)),
            )
    }

    pub fn info_page(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let white: Hsla = gpui::white();
        let accent: Hsla = rgb(ACCENT).into();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(20.))
            .font_family("Segoe UI")
            .text_color(white);
        body = body.child(self.fork_banner());
        let guard = self.info.lock();
        match guard.as_ref() {
            None => body = body.child(div().child("Loading info...")),
            Some(doc) => {
                let mut id = 0u64;
                for block in &doc.blocks {
                    body = body.child(match block {
                        Block::Heading(level, s) => {
                            let size = match level { 1 => 26.0, 2 => 21.0, 3 => 18.0, _ => 16.0 };
                            self.spans(s, size, accent, &doc.images, &mut id).font_weight(FontWeight::BOLD)
                        }
                        Block::Paragraph(s) => self.spans(s, 14.0, white, &doc.images, &mut id),
                        Block::Item(s) => div()
                            .flex()
                            .gap(px(6.))
                            .child(div().text_size(px(14.)).child("•"))
                            .child(self.spans(s, 14.0, white, &doc.images, &mut id).flex_1()),
                        Block::Code(code) => div()
                            .bg(rgb(0x2e2e2e))
                            .rounded(px(5.))
                            .p(px(5.))
                            .font_family("Consolas")
                            .text_size(px(13.))
                            .child(SharedString::from(code.clone())),
                        Block::Rule => div().h(px(1.)).w_full().bg(rgb(0x444444)),
                        Block::Table(rows) => {
                            let mut t = div().flex().flex_col().border_1().border_color(accent);
                            for (ri, row) in rows.iter().enumerate() {
                                let mut r = div()
                                    .flex()
                                    .when(ri == 0, |d| d.bg(rgb(0x2e2e2e)))
                                    .when(ri > 0 && ri % 2 == 0, |d| d.bg(rgb(0x252525)));
                                let n = row.len().max(1) as f32;
                                for cell in row {
                                    let color = if ri == 0 { accent } else { white };
                                    r = r.child(
                                        div()
                                            .w(gpui::relative(1.0 / n))
                                            .p(px(5.))
                                            .border_1()
                                            .border_color(accent)
                                            .child(self.spans(cell, 12.0, color, &doc.images, &mut id).max_h(px(400.))),
                                    );
                                }
                                t = t.child(r);
                            }
                            t
                        }
                    });
                }
            }
        }
        drop(guard);
        div()
            .id("info-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.info_scroll)
            .bg(rgb(0x0c0c0c))
            .child(body)
    }
}
