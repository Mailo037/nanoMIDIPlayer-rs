#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod console;
mod core;
mod hub;
mod keys;
mod live;
mod logging;
mod midi_file;
mod net;
mod player;
mod preview;
mod scheduler;
mod simulate;
mod slots;
mod theme;
mod ui;

use std::time::Duration;

use gpui::{App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use crate::theme::Theme;
use crate::ui::{APP_VERSION, RootView};

fn start_telemetry() {
    std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(|| {
            while net::get_bytes("https://api.nanomidi.net").is_err() {
                std::thread::sleep(Duration::from_secs(5));
            }
            loop {
                if let Err(e) = net::post_json(
                    "https://api.nanomidi.net/api/nanomidiplayer/statistics/ping",
                    &serde_json::json!({}),
                ) {
                    log::warn!("ping failed: {e}");
                }
                std::thread::sleep(Duration::from_secs(50));
            }
        })
        .ok();
}

/// Same version.json check as the original. Instead of downloading the Python
/// bootstrapper it opens the releases page.
fn check_for_updates() {
    std::thread::spawn(|| {
        let Ok(info) = net::get_json(
            "https://raw.githubusercontent.com/NotHammer043/nanoMIDIPlayer/refs/heads/main/api/version.json",
        ) else {
            return;
        };
        let contains = |key: &str| {
            info[key]
                .as_array()
                .map(|a| a.iter().any(|v| v.as_str() == Some(APP_VERSION)))
                .unwrap_or(false)
        };
        let releases = "https://github.com/NotHammer043/nanoMIDIPlayer/releases/latest";
        if contains("avoid") {
            let msg = info["alertMessage"].as_str().unwrap_or("").to_string();
            rfd::MessageDialog::new()
                .set_title("Critical Update Required")
                .set_description(msg)
                .set_level(rfd::MessageLevel::Warning)
                .show();
            net::open_url(releases);
        } else if !contains("latest") {
            let yes = rfd::MessageDialog::new()
                .set_title("Update Available")
                .set_description("A new version of nanoMIDIPlayer is available.\n\nWould you like to update now?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if yes == rfd::MessageDialogResult::Yes {
                net::open_url(releases);
            }
        }
    });
}

fn main() {
    logging::init();
    let store = config::init();
    let (ui_cfg, debug) = {
        let cfg = store.read();
        (cfg.app_ui.clone(), std::env::args().any(|a| a == "--debug"))
    };
    console::set_enabled(ui_cfg.console);
    if debug {
        logging::open_console();
    }

    let theme = Theme::load_active();
    let core = core::Core::new();
    {
        let core = core.clone();
        std::thread::spawn(move || {
            core.refresh_outputs();
            core.refresh_inputs();
        });
    }
    core::start_hotkeys(core.clone());
    if ui_cfg.send_telemetry {
        start_telemetry();
    }
    if ui_cfg.check_for_updates && cfg!(windows) {
        check_for_updates();
    }

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(600.), px(560.)), cx);
        let window_core = core.clone();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("nanoMIDIPlayer".into()),
                        appears_transparent: true,
                        traffic_light_position: None,
                    }),
                    is_resizable: true,
                    window_min_size: Some(size(px(560.), px(520.))),
                    app_id: Some("nanoMIDIPlayer".into()),
                    ..Default::default()
                },
                move |_, cx| cx.new(|cx| RootView::new(window_core, theme, cx)),
            )
            .expect("failed to open window");

        if ui_cfg.force_theme {
            let pending = window
                .update(cx, |view, _, _| view.pending_theme.clone())
                .expect("window alive");
            std::thread::spawn(move || {
                if let Some(t) = theme::force_api_theme() {
                    *pending.lock() = Some(t);
                }
            });
        }

        let quit_core = core.clone();
        cx.on_window_closed(move |cx| {
            if cx.windows().is_empty() {
                quit_core.shutdown();
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
