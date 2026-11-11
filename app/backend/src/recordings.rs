//! The recordings folder: listing what the engine wrote (with each file's
//! transmissions from its .segments.tsv), and deleting old clips.
//!
//! Layout written by the engine config from station.rs:
//!   <dir>/145.400/2026/11/12/145.400_20261112_070201.mp3 (+ .segments.tsv)
//!   <dir>/Nets/Morning net/2026/11/12/Morning net_20261112_070012.mp3

use crate::station::NETS_DIR;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Over {
    pub start_unix: f64,
    /// Where it starts in the file, seconds.
    pub offset_sec: f64,
    pub duration_sec: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Recording {
    /// Relative to the recordings folder, with '/'.
    pub path: String,
    /// The frequency folder ("145.400"), or the frequency the net was on.
    pub freq: String,
    pub net: Option<String>,
    pub start_unix: f64,
    pub duration_sec: Option<f64>,
    pub bytes: u64,
    pub overs: Vec<Over>,
}

pub fn segment_log_for(mp3: &Path) -> PathBuf {
    let stem = mp3.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    mp3.with_file_name(format!("{stem}.segments.tsv"))
}

/// Reads an engine .segments.tsv: start_sec, end_sec, start_unix, start_utc, frequency_hz.
pub fn read_segments(text: &str) -> (Vec<Over>, Option<i64>) {
    let mut overs = Vec::new();
    let mut freq_hz = None;
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        let (Ok(start), Ok(end), Ok(unix)) = (cols[0].parse::<f64>(), cols[1].parse::<f64>(), cols[2].parse::<f64>()) else { continue };
        if let Some(hz) = cols.get(4).and_then(|c| c.trim().parse::<i64>().ok()) {
            freq_hz = Some(hz);
        }
        overs.push(Over { start_unix: unix, offset_sec: start, duration_sec: (end - start).max(0.0) });
    }
    (overs, freq_hz)
}

fn mtime_unix(meta: &std::fs::Metadata) -> f64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(base, &path, out),
            Ok(_) if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("mp3")) => out.push(path),
            _ => {}
        }
    }
}

fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Every finished recording under `base`, newest first. Files the engine is
/// still writing end in .tmp and are not listed.
pub fn scan(base: &Path) -> Vec<Recording> {
    let mut files = Vec::new();
    walk(base, base, &mut files);
    let mut out: Vec<Recording> = files
        .into_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            let rel = relative(base, &path);
            let mut parts = rel.split('/');
            let top = parts.next().unwrap_or_default().to_string();
            let (overs, freq_hz) = std::fs::read_to_string(segment_log_for(&path)).map(|t| read_segments(&t)).unwrap_or_default();
            let (freq, net) = if top == NETS_DIR {
                let name = parts.next().unwrap_or_default().to_string();
                (freq_hz.map(|hz| crate::station::fmt_mhz(hz as f64 / 1e6)).unwrap_or_default(), Some(name))
            } else {
                (top, None)
            };
            let duration = overs.last().map(|o| o.offset_sec + o.duration_sec);
            let start = overs.first().map(|o| o.start_unix).unwrap_or_else(|| mtime_unix(&meta) - duration.unwrap_or(0.0));
            Some(Recording { path: rel, freq, net, start_unix: start, duration_sec: duration, bytes: meta.len(), overs })
        })
        .collect();
    out.sort_by(|a, b| b.start_unix.partial_cmp(&a.start_unix).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn total_bytes(base: &Path) -> u64 {
    fn walk_size(dir: &Path) -> u64 {
        let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
        entries
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => walk_size(&e.path()),
                _ => e.metadata().map(|m| m.len()).unwrap_or(0),
            })
            .sum()
    }
    walk_size(base)
}

/// Deletes clips (and their segment logs) older than `days`. Net recordings
/// are kept. Returns how many recordings were deleted.
pub fn apply_retention(base: &Path, days: u32, now: SystemTime) -> usize {
    if days == 0 {
        return 0;
    }
    let Some(cutoff) = now.checked_sub(std::time::Duration::from_secs(days as u64 * 86400)) else { return 0 };
    let mut files = Vec::new();
    walk(base, base, &mut files);
    let mut deleted = 0;
    for path in files {
        if relative(base, &path).split('/').next() == Some(NETS_DIR) {
            continue;
        }
        let old = std::fs::metadata(&path).and_then(|m| m.modified()).map(|t| t < cutoff).unwrap_or(false);
        if old && std::fs::remove_file(&path).is_ok() {
            let _ = std::fs::remove_file(segment_log_for(&path));
            deleted += 1;
        }
    }
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const TSV: &str = "start_sec\tend_sec\tstart_unix\tstart_utc\tfrequency_hz\n0.000\t3.125\t1794431093.912\t2026-11-11T21:04:53.912Z\t145400000\n3.525\t6.150\t1794431095.829\t2026-11-11T21:04:55.829Z\t145400000\n";

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn reads_segment_logs() {
        let (overs, hz) = read_segments(TSV);
        assert_eq!(hz, Some(145_400_000));
        assert_eq!(overs.len(), 2);
        assert_eq!(overs[1], Over { start_unix: 1794431095.829, offset_sec: 3.525, duration_sec: 6.150 - 3.525 });
        assert_eq!(read_segments("start_sec\nbad\tline\n").0, vec![]);
    }

    #[test]
    fn scans_clips_and_nets() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        write(&base.join("145.400/2026/11/12/145.400_20261112_070201.mp3"), "mp3");
        write(&base.join("145.400/2026/11/12/145.400_20261112_070201.segments.tsv"), TSV);
        write(&base.join("145.400/2026/11/12/145.400_20261112_080000.mp3.tmp"), "in progress");
        write(&base.join("Nets/Morning net/2026/11/12/Morning net_20261112_070012.mp3"), "mp3");
        write(&base.join("Nets/Morning net/2026/11/12/Morning net_20261112_070012.segments.tsv"), &TSV.replace("1794431093.912", "1794431193.0"));
        write(&base.join("notes.txt"), "not audio");

        let recs = scan(base);
        assert_eq!(recs.len(), 2);
        let net = &recs[0];
        assert_eq!(net.net.as_deref(), Some("Morning net"));
        assert_eq!(net.freq, "145.400");
        assert_eq!(net.start_unix, 1794431193.0);
        let clip = &recs[1];
        assert_eq!(clip.path, "145.400/2026/11/12/145.400_20261112_070201.mp3");
        assert_eq!(clip.freq, "145.400");
        assert_eq!(clip.net, None);
        assert_eq!(clip.overs.len(), 2);
        assert_eq!(clip.duration_sec, Some(6.150));
        assert_eq!(clip.bytes, 3);
    }

    #[test]
    fn retention_deletes_old_clips_but_keeps_nets() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let clip = base.join("145.400/a.mp3");
        let tsv = base.join("145.400/a.segments.tsv");
        let net = base.join("Nets/Morning net/b.mp3");
        write(&clip, "x");
        write(&tsv, TSV);
        write(&net, "x");

        let now = SystemTime::now();
        assert_eq!(apply_retention(base, 90, now), 0);
        assert_eq!(apply_retention(base, 0, now + Duration::from_secs(400 * 86400)), 0, "0 days keeps everything");
        assert_eq!(apply_retention(base, 90, now + Duration::from_secs(91 * 86400)), 1);
        assert!(!clip.exists() && !tsv.exists());
        assert!(net.exists());
        assert!(total_bytes(base) >= 1);
    }
}
