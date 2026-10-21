//! Update check and self-update from this fork's GitHub releases.
//!
//! The new exe is downloaded next to the running one and then swapped in. Windows
//! allows renaming a running exe, so the current file becomes `<exe>.old` (deleted on
//! the next start) and the new one takes its place: the next launch, or "Restart now",
//! already runs the new version.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde_json::Value;

use crate::{config, console, net};

/// This fork's repository (owner/name).
const REPO: &str = "Mailo037/nanoMIDIPlayer-rs";

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Not checked yet in this session.
    Idle,
    Checking,
    UpToDate,
    Available,
    /// Download progress 0..1 (`None` while the size is unknown).
    Downloading(Option<f32>),
    /// The new exe is in place; it runs after a restart.
    Installed,
    Error(String),
}

#[derive(Clone, Debug, Default)]
pub struct Release {
    pub version: String,
    pub page_url: String,
    pub asset_url: Option<String>,
    pub size: u64,
    pub notes: String,
    pub published: String,
}

pub struct State {
    pub status: Status,
    pub latest: Option<Release>,
    pub last_checked: Option<String>,
    /// The update banner was closed for this session.
    pub dismissed: bool,
}

static STATE: Mutex<State> = parking_lot::const_mutex(State {
    status: Status::Idle,
    latest: None,
    last_checked: None,
    dismissed: false,
});
static VERSION: AtomicU64 = AtomicU64::new(0);

fn set_status(status: Status) {
    STATE.lock().status = status;
    VERSION.fetch_add(1, Ordering::Release);
}

/// Changes whenever the update state changes (for the UI redraw check).
pub fn version() -> u64 {
    VERSION.load(Ordering::Acquire)
}

pub fn state() -> parking_lot::MutexGuard<'static, State> {
    STATE.lock()
}

pub fn dismiss() {
    STATE.lock().dismissed = true;
    VERSION.fetch_add(1, Ordering::Release);
}

/// The running version without the "v". `NANOMIDI_PRETEND_VERSION` overrides it,
/// which makes it possible to test the update flow against the latest release.
pub fn current_version() -> String {
    std::env::var("NANOMIDI_PRETEND_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
        .trim_start_matches('v')
        .to_string()
}

/// Parses "v1.2.3" / "1.2.3" into comparable numbers.
fn parse_version(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

pub fn busy() -> bool {
    matches!(STATE.lock().status, Status::Checking | Status::Downloading(_))
}

fn fetch_latest() -> anyhow::Result<Release> {
    let info: Value = net::get_json(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let version = info["tag_name"].as_str().ok_or_else(|| anyhow::anyhow!("no release found"))?.to_string();
    // The release workflow uploads `nanoMIDIPlayer-<tag>-windows-x64.exe`.
    let asset = info["assets"].as_array().and_then(|assets| {
        assets.iter().find(|a| {
            let name = a["name"].as_str().unwrap_or("").to_lowercase();
            name.ends_with(".exe") && name.contains("windows")
        })
    });
    Ok(Release {
        page_url: info["html_url"].as_str().unwrap_or("").to_string(),
        asset_url: asset.and_then(|a| a["browser_download_url"].as_str()).map(str::to_string),
        size: asset.and_then(|a| a["size"].as_u64()).unwrap_or(0),
        notes: info["body"].as_str().unwrap_or("").trim().to_string(),
        published: info["published_at"].as_str().unwrap_or("").chars().take(10).collect(),
        version,
    })
}

/// Looks for a newer release in the background. With `install`, a found update is
/// downloaded and installed right away ("Install updates automatically").
pub fn check(install_found: bool) {
    if busy() || STATE.lock().status == Status::Installed {
        return;
    }
    set_status(Status::Checking);
    std::thread::spawn(move || {
        let result = fetch_latest();
        let now = chrono::Local::now().format("%H:%M").to_string();
        let found = {
            let mut st = STATE.lock();
            st.last_checked = Some(now);
            match result {
                Ok(release) => {
                    let newer = is_newer(&release.version, &current_version());
                    st.status = if newer { Status::Available } else { Status::UpToDate };
                    if newer {
                        console::insert(format!("Update {} is available", release.version));
                    }
                    st.latest = Some(release);
                    newer
                }
                Err(e) => {
                    log::warn!("update check failed: {e}");
                    st.status = Status::Error(short_error(&e));
                    false
                }
            }
        };
        VERSION.fetch_add(1, Ordering::Release);
        if found && install_found {
            install();
        }
    });
}

fn short_error(e: &anyhow::Error) -> String {
    let s = e.to_string();
    if s.to_lowercase().contains("timeout") || s.to_lowercase().contains("timed out") {
        "Timed out. Check your internet connection.".into()
    } else if s.to_lowercase().contains("dns") || s.to_lowercase().contains("connect") || s.to_lowercase().contains("resolve") {
        "Couldn't reach GitHub. Are you offline?".into()
    } else {
        s.chars().take(120).collect()
    }
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Downloads the latest release and swaps it in for the running exe.
pub fn install() {
    let release = {
        let st = STATE.lock();
        if matches!(st.status, Status::Checking | Status::Downloading(_) | Status::Installed) {
            return;
        }
        st.latest.clone()
    };
    let Some(release) = release.filter(|r| is_newer(&r.version, &current_version())) else { return };
    let Some(url) = release.asset_url.clone() else {
        set_status(Status::Error("This release has no Windows download. Use the download page.".into()));
        return;
    };
    set_status(Status::Downloading(None));
    std::thread::spawn(move || match download_and_swap(&url, release.size) {
        Ok(()) => {
            console::insert(format!("Update {} installed. Restart to use it.", release.version));
            crate::console::toast(format!("Update {} installed — restart to use it", release.version));
            set_status(Status::Installed);
        }
        Err(e) => {
            log::warn!("update failed: {e}");
            set_status(Status::Error(short_error(&e)));
        }
    });
}

fn download_and_swap(url: &str, size: u64) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let new = exe.with_extension("exe.new");
    let mut file = std::fs::File::create(&new).map_err(|e| {
        anyhow::anyhow!("Can't write to {} ({e}). Download the update manually.", exe.parent().unwrap_or(&exe).display())
    })?;
    let mut last = 0u64;
    let result = net::download(url, &mut file, |done| {
        // Report every ~256 KB so the UI isn't redrawn for every chunk.
        if size > 0 && done - last >= 256 * 1024 {
            last = done;
            set_status(Status::Downloading(Some((done as f32 / size as f32).min(1.0))));
        }
    });
    file.flush()?;
    drop(file);
    let written = match result {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_file(&new);
            return Err(e);
        }
    };
    // Sanity checks: complete file and a Windows executable ("MZ" header).
    let head = std::fs::read(&new).map(|b| b.get(..2) == Some(b"MZ")).unwrap_or(false);
    if !head || (size > 0 && written != size) || written < 1024 * 1024 {
        let _ = std::fs::remove_file(&new);
        anyhow::bail!("The download was incomplete. Please try again.");
    }
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old)?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&new);
        return Err(e.into());
    }
    Ok(())
}

/// Starts the (updated) exe again. The caller quits this process afterwards.
pub fn relaunch() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::Command::new(exe).args(args).spawn().is_ok()
}

/// Deletes the exe left over from the last update. The previous process may still
/// be shutting down, so this retries for a few seconds.
pub fn cleanup_old() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = old_path(&exe);
    let new = exe.with_extension("exe.new");
    let _ = std::fs::remove_file(new);
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

/// Startup: clean up, then check (and maybe install) as configured.
pub fn on_startup() {
    cleanup_old();
    let (check_on_start, auto) = {
        let c = config::get().read();
        (c.app_ui.check_for_updates, c.app_ui.auto_update)
    };
    if check_on_start && cfg!(windows) {
        check(auto);
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn versions() {
        assert!(is_newer("v1.2.0", "1.1.0"));
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(!is_newer("v1.1.0", "1.1.0"));
        assert!(!is_newer("v1.0.9", "1.1.0"));
    }
}
