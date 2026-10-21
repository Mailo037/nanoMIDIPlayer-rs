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

pub fn post_json(url: &str, body: &Value) -> anyhow::Result<()> {
    agent().post(url).send_json(body)?;
    Ok(())
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
