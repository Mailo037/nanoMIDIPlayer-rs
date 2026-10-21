//! Themes: the same JSON theme files as the original app (colors, font and base64 icons),
//! stored at `Documents/nanoMIDIPlayer/assets/activeTheme.json`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use gpui::{Hsla, Image, ImageFormat, Rgba};
use serde_json::Value;

use crate::config;

pub const DEFAULT_THEME: &str = include_str!("../assets/defaultTheme.json");
pub const REMOTE_THEME_LIST: &str =
    "https://raw.githubusercontent.com/NotHammer043/nanoMIDIPlayer/refs/heads/main/api/theme.json";

#[derive(Clone, Default)]
pub struct Icons {
    pub banner: Option<Arc<Image>>,
    pub reset: Option<Arc<Image>>,
    pub piano: Option<Arc<Image>>,
    pub drum: Option<Arc<Image>>,
    pub download: Option<Arc<Image>>,
    pub search: Option<Arc<Image>>,
    pub cogwheel: Option<Arc<Image>>,
    pub keyboard: Option<Arc<Image>>,
    pub info: Option<Arc<Image>>,
}

pub struct Theme {
    pub name: String,
    /// The theme's own font (monospace in the stock themes); only used for the log.
    pub font: String,
    /// Font of the interface: the system UI font, so the app looks native and modern.
    pub ui_font: String,
    colors: HashMap<String, HashMap<String, Hsla>>,
    pub icons: Icons,
    pub watermark_ln1: String,
    pub watermark_ln2: String,
}

pub fn active_theme_path() -> PathBuf {
    config::assets_dir().join("activeTheme.json")
}

pub fn custom_themes_dir() -> PathBuf {
    config::assets_dir().join("customThemes")
}

fn parse_hex(s: &str) -> Option<Hsla> {
    let hex = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(hex, 16).ok()?;
    let rgba = match hex.len() {
        6 => Rgba {
            r: ((v >> 16) & 0xFF) as f32 / 255.0,
            g: ((v >> 8) & 0xFF) as f32 / 255.0,
            b: (v & 0xFF) as f32 / 255.0,
            a: 1.0,
        },
        3 => Rgba {
            r: ((v >> 8) & 0xF) as f32 / 15.0,
            g: ((v >> 4) & 0xF) as f32 / 15.0,
            b: (v & 0xF) as f32 / 15.0,
            a: 1.0,
        },
        _ => return None,
    };
    Some(rgba.into())
}

/// Tk-style named colors used by some themes ("gray90", "white", ...).
fn parse_color(s: &str) -> Option<Hsla> {
    if s.starts_with('#') {
        return parse_hex(s);
    }
    let lower = s.to_lowercase();
    if let Some(n) = lower.strip_prefix("gray").or_else(|| lower.strip_prefix("grey")) {
        if let Ok(p) = n.parse::<f32>() {
            let v = p / 100.0;
            return Some(Rgba { r: v, g: v, b: v, a: 1.0 }.into());
        }
    }
    match lower.as_str() {
        "white" => parse_hex("#ffffff"),
        "black" => parse_hex("#000000"),
        "red" => parse_hex("#ff0000"),
        "green" => parse_hex("#008000"),
        "blue" => parse_hex("#0000ff"),
        "transparent" => Some(gpui::transparent_black()),
        _ => None,
    }
}

fn decode_image(b64: &str) -> Option<Arc<Image>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    let format = sniff_format(&bytes)?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

/// Detects an image format from its magic bytes.
pub fn sniff_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if bytes.starts_with(b"GIF8") {
        Some(ImageFormat::Gif)
    } else if bytes.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_lowercase();
        (head.contains("<svg") || head.starts_with("<?xml")).then_some(ImageFormat::Svg)
    }
}

impl Theme {
    pub fn from_json(text: &str) -> anyhow::Result<Theme> {
        let json: Value = serde_json::from_str(text)?;
        let default: Value = serde_json::from_str(DEFAULT_THEME)?;
        let t = json.get("Theme").ok_or_else(|| anyhow::anyhow!("missing Theme"))?;
        let dt = &default["Theme"];

        let mut colors: HashMap<String, HashMap<String, Hsla>> = HashMap::new();
        for source in [dt, t] {
            if let Value::Object(sections) = source {
                for (section, values) in sections {
                    if let Value::Object(values) = values {
                        for (key, v) in values {
                            if let Some(c) = v.as_str().and_then(parse_color) {
                                colors
                                    .entry(section.clone())
                                    .or_insert_with(HashMap::new)
                                    .insert(key.clone(), c);
                            }
                        }
                    }
                }
            }
        }

        let font_key = if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else {
            "Windows"
        };
        let font = t["GlobalFont"][font_key]
            .as_str()
            .or_else(|| dt["GlobalFont"][font_key].as_str())
            .unwrap_or("Consolas")
            .to_string();

        let icon = |name: &str| {
            t["Icons"][name]
                .as_str()
                .and_then(decode_image)
                .or_else(|| dt["Icons"][name].as_str().and_then(decode_image))
        };
        let icons = Icons {
            banner: icon("banner"),
            reset: icon("reset"),
            piano: icon("piano"),
            drum: icon("drum"),
            download: icon("download"),
            search: icon("search"),
            cogwheel: icon("cogwheel"),
            keyboard: icon("keyboard"),
            info: icon("info"),
        };

        let nav = &t["Navigation"];
        Ok(Theme {
            name: json["Name"].as_str().unwrap_or("Unknown").to_string(),
            font,
            ui_font: ui_font().to_string(),
            colors,
            icons,
            watermark_ln1: nav["CustomThemeWatermarkLN1"].as_str().unwrap_or("").to_string(),
            watermark_ln2: nav["CustomThemeWatermarkLN2"].as_str().unwrap_or("").to_string(),
        })
    }

    pub fn default_theme() -> Theme {
        Theme::from_json(DEFAULT_THEME).expect("embedded theme is valid")
    }

    /// Loads the active theme from disk (creating it from the embedded default).
    pub fn load_active() -> Theme {
        let path = active_theme_path();
        if !path.exists() {
            let _ = std::fs::write(&path, DEFAULT_THEME);
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| Theme::from_json(&text).ok())
            .unwrap_or_else(|| {
                log::warn!("active theme is corrupted, restoring default");
                let _ = std::fs::write(&path, DEFAULT_THEME);
                Theme::default_theme()
            })
    }

    pub fn color(&self, section: &str, key: &str) -> Hsla {
        self.colors
            .get(section)
            .and_then(|s| s.get(key))
            .copied()
            .unwrap_or_else(|| gpui::white())
    }
}

fn ui_font() -> &'static str {
    if cfg!(target_os = "macos") {
        ".SystemUIFont"
    } else if cfg!(target_os = "linux") {
        "Noto Sans"
    } else {
        "Segoe UI"
    }
}

/// Theme list from the API: (name, url).
pub fn fetch_remote_list() -> anyhow::Result<(String, Vec<(String, String)>)> {
    let value: Value = crate::net::get_json(REMOTE_THEME_LIST)?;
    let default = value["defaultTheme"].as_str().unwrap_or("Dark").to_string();
    let mut themes = Vec::new();
    if let Value::Object(map) = &value["availableThemes"] {
        for (name, url) in map {
            if let Some(url) = url.as_str() {
                themes.push((name.clone(), url.to_string()));
            }
        }
    }
    Ok((default, themes))
}

pub fn custom_theme_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(custom_themes_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".json"))
                .map(|n| format!("{n} (Custom)"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Downloads `url` (or copies a custom theme) into activeTheme.json and validates it.
pub fn install_theme(name: &str, remote: &[(String, String)]) -> anyhow::Result<Theme> {
    let text = if let Some(file) = name.strip_suffix(" (Custom)") {
        std::fs::read_to_string(custom_themes_dir().join(file))?
    } else {
        let url = remote
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, u)| u.clone())
            .ok_or_else(|| anyhow::anyhow!("'{name}' does not exist."))?;
        crate::net::get_text(&url)?
    };
    let theme = Theme::from_json(&text)?;
    std::fs::write(active_theme_path(), text)?;
    Ok(theme)
}

/// "Force API Theme": replaces the active theme with the API's default theme.
pub fn force_api_theme() -> Option<Theme> {
    let (default, list) = fetch_remote_list().ok()?;
    install_theme(&default, &list).ok()
}
