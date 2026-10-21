//! MIDI Hub (nanomidi.net) data model.
//!
//! The full song list is fetched in one request and cached in
//! `Documents/nanoMIDIPlayer/cache`, so the hub opens instantly from disk and
//! search/sort run locally. Cover images are cached there too.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::Image;
use serde::{Deserialize, Serialize};

use crate::{config, net, theme};

/// Paginated (30 per page); the old `/api/midiData` endpoint is no longer updated.
pub const MIDI_DATA_URL: &str = "https://api.nanomidi.net/api/v2/midiData";
pub const PAGE_SIZE: usize = 10;

/// Treats `null` like a missing field (the v2 API sends `null` for some entries).
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HubMidi {
    /// Page id on nanomidi.net (`/midi/<id>`).
    #[serde(rename = "$id", default)]
    pub page_id: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub artists: String,
    #[serde(default)]
    pub arranger: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub uploader: String,
    #[serde(rename = "midifile", alias = "midiFilename", default)]
    pub midi_filename: Option<String>,
    #[serde(rename = "imagefile", alias = "imageFilename", default)]
    pub image_filename: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub downloads: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub views: i64,
    /// ISO 8601 upload time, used for Newest/Oldest.
    #[serde(rename = "$createdAt", alias = "dateCreated", default, deserialize_with = "null_default")]
    pub created: String,
}

impl HubMidi {
    /// Stable key for favorites and per-card state.
    pub fn key(&self) -> String {
        self.page_id
            .clone()
            .or_else(|| self.midi_filename.clone())
            .unwrap_or_else(|| format!("{} - {}", self.name, self.artists))
    }

    pub fn page_url(&self) -> Option<String> {
        self.page_id.as_ref().map(|id| format!("https://nanomidi.net/midi/{id}"))
    }
}

#[derive(Deserialize)]
struct DataPage {
    #[serde(default)]
    data: Vec<HubMidi>,
    #[serde(rename = "totalPages", default)]
    total_pages: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortMode {
    Newest,
    Oldest,
    Downloads,
    Views,
}

impl SortMode {
    pub const NAMES: [&'static str; 4] = ["Newest", "Oldest", "Downloads", "Views"];

    pub fn from_name(name: &str) -> SortMode {
        match name {
            "Oldest" => SortMode::Oldest,
            "Downloads" => SortMode::Downloads,
            "Views" => SortMode::Views,
            _ => SortMode::Newest,
        }
    }
}

#[derive(Clone)]
pub enum HubStatus {
    Loading,
    Error(String),
    Ready,
}

/// Lowercased search text per song, built once when the data arrives.
struct SearchEntry {
    name: Vec<char>,
    rest: Vec<char>,
}

pub struct Hub {
    pub status: HubStatus,
    /// The list shown comes from the local cache because the server couldn't be reached.
    pub offline: bool,
    pub all: Vec<HubMidi>,
    index: Vec<SearchEntry>,
    /// Indices into `all` after the favorites filter, search and sort.
    pub filtered: Vec<usize>,
    pub page: usize,
    pub sort: SortMode,
    pub query: String,
    pub favorites: HashSet<String>,
    pub show_favorites: bool,
    pub images: HashMap<String, Option<Arc<Image>>>,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            status: HubStatus::Loading,
            offline: false,
            all: Vec::new(),
            index: Vec::new(),
            filtered: Vec::new(),
            page: 1,
            sort: SortMode::Newest,
            query: String::new(),
            favorites: load_favorites(),
            show_favorites: false,
            images: HashMap::new(),
        }
    }

    pub fn total_pages(&self) -> usize {
        self.filtered.len().div_ceil(PAGE_SIZE)
    }

    pub fn set_data(&mut self, data: Vec<HubMidi>) {
        self.index = data
            .iter()
            .map(|m| SearchEntry {
                name: m.name.to_lowercase().chars().collect(),
                rest: format!("{} {} {}", m.artists, m.arranger.as_deref().unwrap_or(""), m.uploader)
                    .to_lowercase()
                    .chars()
                    .collect(),
            })
            .collect();
        self.all = data;
        self.status = HubStatus::Ready;
        self.apply();
    }

    pub fn is_favorite(&self, key: &str) -> bool {
        self.favorites.contains(key)
    }

    pub fn favorite_count(&self) -> usize {
        self.all.iter().filter(|m| self.favorites.contains(&m.key())).count()
    }

    pub fn toggle_favorite(&mut self, key: String) {
        if !self.favorites.remove(&key) {
            self.favorites.insert(key);
        }
        save_favorites(&self.favorites);
        if self.show_favorites {
            self.apply();
        }
    }

    /// Re-applies the favorites filter, fuzzy search and sort.
    pub fn apply(&mut self) {
        let tokens: Vec<Vec<char>> = self
            .query
            .to_lowercase()
            .split_whitespace()
            .map(|t| t.chars().collect())
            .collect();
        let mut scored: Vec<(usize, i32)> = (0..self.all.len())
            .filter(|&i| !self.show_favorites || self.favorites.contains(&self.all[i].key()))
            .filter_map(|i| {
                let e = &self.index[i];
                let mut total = 0;
                for t in &tokens {
                    // Title matches count double.
                    let best = fuzzy_score(t, &e.name).map(|s| s * 2).max(fuzzy_score(t, &e.rest));
                    total += best?;
                }
                Some((i, total))
            })
            .collect();
        let all = &self.all;
        let sort = self.sort;
        let by_mode = |a: usize, b: usize| match sort {
            SortMode::Newest => all[b].created.cmp(&all[a].created),
            SortMode::Oldest => all[a].created.cmp(&all[b].created),
            SortMode::Downloads => all[b].downloads.cmp(&all[a].downloads),
            SortMode::Views => all[b].views.cmp(&all[a].views),
        };
        // With a query the best match comes first; the sort mode breaks ties.
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| by_mode(a.0, b.0)));
        self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        self.page = self.page.clamp(1, self.total_pages().max(1));
    }

    fn page_range(&self, page: usize) -> &[usize] {
        let start = (page.max(1) - 1) * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.filtered.len());
        if start >= end { &[] } else { &self.filtered[start..end] }
    }

    pub fn page_items(&self) -> Vec<HubMidi> {
        self.page_range(self.page).iter().map(|&i| self.all[i].clone()).collect()
    }

    /// Cover images of the current and the next page (the next one is prefetched).
    pub fn wanted_images(&self) -> Vec<String> {
        self.page_range(self.page)
            .iter()
            .chain(self.page_range(self.page + 1))
            .filter_map(|&i| self.all[i].image_filename.clone())
            .collect()
    }
}

// ---------------------------------------------------------------- fuzzy search

fn is_boundary(hay: &[char], i: usize) -> bool {
    i == 0 || !hay[i - 1].is_alphanumeric()
}

/// Scores how well `needle` matches `hay` (both lowercase), or `None` if it doesn't.
///
/// Substring hits score highest, then in-order subsequence hits (fzf style, with
/// bonuses for consecutive chars and word starts), then words within a small
/// edit distance (typos like "beehtoven").
pub fn fuzzy_score(needle: &[char], hay: &[char]) -> Option<i32> {
    let n = needle.len();
    if n == 0 {
        return Some(0);
    }
    if n > hay.len() + 2 {
        return None;
    }
    if let Some(pos) = hay.windows(n).position(|w| w == needle) {
        let bonus = if is_boundary(hay, pos) { 40 } else { 0 };
        return Some(200 + 10 * n as i32 + bonus - (pos as i32).min(30));
    }

    // Subsequence match.
    let mut score = 0;
    let mut from = 0;
    let mut first = None;
    let mut last: Option<usize> = None;
    let mut matched = true;
    for &c in needle {
        let Some(off) = hay[from..].iter().position(|&h| h == c) else {
            matched = false;
            break;
        };
        let pos = from + off;
        first.get_or_insert(pos);
        score += 4;
        if is_boundary(hay, pos) {
            score += 6;
        }
        if let Some(l) = last {
            score += if l + 1 == pos { 8 } else { -((pos - l - 1) as i32).min(6) };
        }
        last = Some(pos);
        from = pos + 1;
    }
    if matched {
        // Too scattered to mean anything (e.g. "abc" spread over a whole title).
        let span = last.unwrap_or(0) - first.unwrap_or(0) + 1;
        if span <= n * 3 + 2 {
            return Some(score.max(1));
        }
    }

    // Typo tolerance against the start of each word.
    if n >= 4 {
        let max_dist = if n >= 7 { 2 } else { 1 };
        let mut i = 0;
        while i < hay.len() {
            if !hay[i].is_alphanumeric() {
                i += 1;
                continue;
            }
            let end = (i..hay.len()).find(|&j| !hay[j].is_alphanumeric()).unwrap_or(hay.len());
            let word = &hay[i..end];
            if word.len() + max_dist >= n {
                let dist = edit_distance(needle, &word[..word.len().min(n)]);
                if dist <= max_dist {
                    return Some(20 - 5 * dist as i32);
                }
            }
            i = end;
        }
    }
    None
}

/// Optimal string alignment distance (Levenshtein plus adjacent transpositions).
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

// ----------------------------------------------------------------- local cache

fn cache_dir() -> PathBuf {
    let dir = config::base_dir().join("cache");
    let _ = std::fs::create_dir_all(dir.join("images"));
    dir
}

fn safe_name(filename: &str) -> String {
    filename
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect()
}

/// Writes through a temp file so a crash never leaves a half-written file behind.
fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn data_cache_path() -> PathBuf {
    cache_dir().join("midiData-v2.json")
}

/// The song list from the last successful fetch, if any.
pub fn load_cached() -> Option<Vec<HubMidi>> {
    let bytes = std::fs::read(data_cache_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn fetch_page(page: usize) -> anyhow::Result<DataPage> {
    let bytes = net::get_bytes(&format!("{MIDI_DATA_URL}?page={page}&type=newest"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Fetches every page of the song list (pages after the first in parallel) and
/// stores the result in the local cache.
pub fn fetch_data() -> anyhow::Result<Vec<HubMidi>> {
    let first = fetch_page(1)?;
    let mut data = first.data;
    let rest: Vec<anyhow::Result<DataPage>> = std::thread::scope(|s| {
        let handles: Vec<_> = (2..=first.total_pages).map(|p| s.spawn(move || fetch_page(p))).collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("page fetch panicked"))))
            .collect()
    });
    for page in rest {
        data.extend(page?.data);
    }
    // Pages can shift while fetching if a song is uploaded meanwhile.
    let mut seen = HashSet::new();
    data.retain(|m| seen.insert(m.key()));
    if let Ok(bytes) = serde_json::to_vec(&data) {
        write_atomic(&data_cache_path(), &bytes);
    }
    Ok(data)
}

/// Cover image from the disk cache, downloaded and cached on a miss.
pub fn fetch_image(filename: &str) -> Option<Arc<Image>> {
    let path = cache_dir().join("images").join(safe_name(filename));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) if theme::sniff_format(&bytes).is_some() => bytes,
        _ => {
            let url = format!("https://api.nanomidi.net/api/v2/images/{filename}?size=100x100");
            let bytes = net::get_bytes(&url).ok()?;
            theme::sniff_format(&bytes)?;
            write_atomic(&path, &bytes);
            bytes
        }
    };
    let format = theme::sniff_format(&bytes)?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn favorites_path() -> PathBuf {
    config::base_dir().join("favorites.json")
}

fn load_favorites() -> HashSet<String> {
    std::fs::read(favorites_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn save_favorites(favorites: &HashSet<String>) {
    let mut ids: Vec<&String> = favorites.iter().collect();
    ids.sort_unstable();
    if let Ok(bytes) = serde_json::to_vec(&ids) {
        write_atomic(&favorites_path(), &bytes);
    }
}

/// Downloads a MIDI into `Documents/nanoMIDIPlayer/Midis` and returns the local path.
/// A file downloaded before is reused without touching the network.
pub fn download_midi(filename: &str) -> anyhow::Result<PathBuf> {
    let path = config::midis_dir().join(safe_name(filename));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
        return Ok(path);
    }
    let url = format!("https://api.nanomidi.net/api/midis/{filename}");
    let bytes = net::get_bytes(&url)?;
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::fuzzy_score;

    fn score(q: &str, hay: &str) -> Option<i32> {
        let q: Vec<char> = q.chars().collect();
        let h: Vec<char> = hay.chars().collect();
        fuzzy_score(&q, &h)
    }

    #[test]
    fn fuzzy_matches() {
        assert!(score("moon", "moonlight sonata").is_some());
        assert!(score("mlsn", "moonlight sonata").is_some());
        assert!(score("beehtoven", "ludwig van beethoven").is_some());
        assert!(score("sonta", "moonlight sonata").is_some());
        assert!(score("xyz", "moonlight sonata").is_none());
        assert!(score("moon", "moonlight sonata") > score("mlsn", "moonlight sonata"));
        assert!(score("son", "sonata") > score("son", "person"));
    }
}
