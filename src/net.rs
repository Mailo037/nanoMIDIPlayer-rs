//! Small blocking HTTP helpers (always called from background threads).

use std::io::Read;
use std::time::Duration;

use serde_json::Value;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .user_agent("nanoMIDIPlayer-rs")
        .build()
        .into()
}

pub fn get_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut resp = agent().get(url).call()?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn get_text(url: &str) -> anyhow::Result<String> {
    Ok(String::from_utf8_lossy(&get_bytes(url)?).into_owned())
}

pub fn get_json(url: &str) -> anyhow::Result<Value> {
    Ok(serde_json::from_slice(&get_bytes(url)?)?)
}

/// Streams a (large) download into `out`, reporting the bytes received so far.
/// No overall time limit, only a stall timeout, so slow connections still finish.
pub fn download(url: &str, out: &mut impl std::io::Write, mut progress: impl FnMut(u64)) -> anyhow::Result<u64> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(600)))
        .user_agent("nanoMIDIPlayer-rs")
        .build()
        .into();
    let mut resp = agent.get(url).call()?;
    let mut reader = resp.body_mut().with_config().limit(512 * 1024 * 1024).reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        total += n as u64;
        progress(total);
    }
    Ok(total)
}

pub fn post_json(url: &str, body: &Value) -> anyhow::Result<()> {
    agent().post(url).send_json(body)?;
    Ok(())
}

/// Opens Explorer with `path` selected (or just the folder on other systems).
pub fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(not(windows))]
    {
        if let Some(dir) = path.parent() {
            open_url(&dir.to_string_lossy());
        }
    }
}

/// Opens a folder in the file manager.
pub fn open_folder(dir: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(not(windows))]
    {
        open_url(&dir.to_string_lossy());
    }
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
