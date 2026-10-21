#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod console;
mod core;
mod focus;
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

/// Parses "v1.2.3" / "1.2.3" into comparable numbers.
fn parse_version(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Checks the latest GitHub release of this fork and offers to open it if newer.
fn check_for_updates() {
    std::thread::spawn(|| {
        let api = format!(
            "https://api.github.com/repos/{}/releases/latest",
            ui::FORK_REPO.trim_start_matches("https://github.com/")
        );
        let Ok(info) = net::get_json(&api) else { return };
        let Some(latest) = info["tag_name"].as_str() else { return };
        if parse_version(latest) <= parse_version(APP_VERSION) {
            return;
        }
        let url = info["html_url"].as_str().unwrap_or(ui::FORK_REPO).to_string();
        let yes = rfd::MessageDialog::new()
            .set_title("Update Available")
            .set_description(format!(
                "nanoMIDIPlayer-rs {latest} is available (you have {APP_VERSION}).\n\nOpen the download page?"
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if yes == rfd::MessageDialogResult::Yes {
            net::open_url(&url);
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
    focus::start(core.clone());
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
