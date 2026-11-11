//! The app's own icon set: 24 px line icons with rounded strokes (`assets/icons`),
//! embedded in the binary and tinted with the theme's text colors.

use std::borrow::Cow;

use gpui::prelude::*;
use gpui::{AssetSource, Hsla, SharedString, Svg, px, svg};

macro_rules! icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($variant),*
        }

        impl Icon {
            fn path(self) -> &'static str {
                match self {
                    $(Icon::$variant => concat!("icons/", $file, ".svg")),*
                }
            }
        }

        const FILES: &[(&str, &str)] = &[
            $((concat!("icons/", $file, ".svg"), include_str!(concat!("../../assets/icons/", $file, ".svg")))),*
        ];
    };
}

icons! {
    Music => "music",
    Drum => "drum",
    Keyboard => "keyboard",
    Search => "search",
    Settings => "settings",
    Help => "help",
    Play => "play",
    Pause => "pause",
    Stop => "stop",
    Restart => "restart",
    ChevronDown => "chevron-down",
    ChevronRight => "chevron-right",
    ChevronLeft => "chevron-left",
    Check => "check",
    Close => "close",
    Minimize => "minimize",
    Maximize => "maximize",
    Restore => "restore",
    More => "more",
    Folder => "folder",
    Download => "download",
    Copy => "copy",
    Trash => "trash",
    Star => "star",
    StarFilled => "star-filled",
    External => "external",
    ViewList => "view-list",
    ViewCompact => "view-compact",
    ViewGrid => "view-grid",
    Keycap => "keycap",
    Sliders => "sliders",
    Warning => "warning",
}

/// Serves the embedded icons to GPUI's SVG renderer.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES.iter().find(|(name, _)| *name == path).map(|(_, data)| Cow::Borrowed(data.as_bytes())))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(FILES.iter().map(|(name, _)| SharedString::from(*name)).collect())
    }
}

/// A square icon in the given color.
pub fn icon(icon: Icon, size: f32, color: Hsla) -> Svg {
    svg().path(icon.path()).w(px(size)).h(px(size)).flex_shrink_0().text_color(color)
}
