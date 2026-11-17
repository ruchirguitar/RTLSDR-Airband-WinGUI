//! Receives the engine's spectrum lines (UDP, see engine/src/spectrum.h) and
//! keeps the latest ones for the waterfall.

use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// About 40 s at 15 lines a second.
pub const MAX_LINES: usize = 600;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub centre_hz: u32,
    pub sample_rate: u32,
    pub db: Vec<f32>,
    pub channels: Vec<ChannelLevel>,
}

/// A channel's squelch as the engine sees it, in the line's dB scale.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ChannelLevel {
    pub freq_hz: u32,
    pub noise_db: f32,
    pub threshold_db: f32,
    pub signal_db: f32,
    pub open: bool,
}

/// "FRSP", u8 version, 3 zero bytes, u32 centre_hz, u32 sample_rate, u32 bins, f32 dB[bins], little-endian.
/// Version 2 adds u32 channel count and per channel u32 freq_hz, f32 noise_db, threshold_db, signal_db, u8 open, 3 zero bytes.
pub fn parse_packet(p: &[u8]) -> Option<Line> {
    if p.len() < 20 || &p[0..4] != b"FRSP" || !(1..=2).contains(&p[4]) {
        return None;
    }
    let u32_at = |i: usize| u32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
    let f32_at = |i: usize| f32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
    let bins = u32_at(16) as usize;
    let line_end = 20 + bins * 4;
    if bins == 0 || bins > 65536 || p.len() < line_end {
        return None;
    }
    let db = (0..bins).map(|i| f32_at(20 + i * 4)).collect();
    let mut channels = Vec::new();
    if p[4] == 1 {
        if p.len() != line_end {
            return None;
        }
    } else {
        if p.len() < line_end + 4 {
            return None;
        }
        let count = u32_at(line_end) as usize;
        if p.len() != line_end + 4 + count * 20 {
            return None;
        }
        for i in 0..count {
            let c = line_end + 4 + i * 20;
            channels.push(ChannelLevel { freq_hz: u32_at(c), noise_db: f32_at(c + 4), threshold_db: f32_at(c + 8), signal_db: f32_at(c + 12), open: p[c + 16] != 0 });
        }
    }
    Some(Line { centre_hz: u32_at(8), sample_rate: u32_at(12), db, channels })
}

#[derive(Default)]
struct Store {
    lines: VecDeque<Line>,
    /// Sequence number of lines[0].
    first_seq: u64,
    last_at: Option<Instant>,
}

impl Store {
    fn push(&mut self, line: Line) {
        // a new tuning or FFT size makes the old lines meaningless
        if self.lines.back().is_some_and(|l| l.centre_hz != line.centre_hz || l.db.len() != line.db.len()) {
            self.first_seq += self.lines.len() as u64;
            self.lines.clear();
        }
        self.lines.push_back(line);
        if self.lines.len() > MAX_LINES {
            self.lines.pop_front();
            self.first_seq += 1;
        }
        self.last_at = Some(Instant::now());
    }

    fn next_seq(&self) -> u64 {
        self.first_seq + self.lines.len() as u64
    }
}

/// What the UI gets: the lines after `since`, oldest first.
pub struct Update {
    pub lines: Vec<Line>,
    pub next: u64,
    pub live: bool,
}

#[derive(Clone, Default)]
pub struct SpectrumFeed {
    store: Arc<Mutex<Store>>,
    started: Arc<OnceLock<Option<String>>>,
}

impl SpectrumFeed {
    /// Binds the port the first time it is called; returns the bind error, if any.
    pub fn ensure_listening(&self, port: u16) -> Option<String> {
        self.started
            .get_or_init(|| match UdpSocket::bind(("127.0.0.1", port)) {
                Ok(sock) => {
                    let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
                    let store = self.store.clone();
                    std::thread::spawn(move || {
                        let mut buf = vec![0u8; 300_000];
                        loop {
                            match sock.recv(&mut buf) {
                                Ok(n) => {
                                    if let Some(line) = parse_packet(&buf[..n]) {
                                        store.lock().unwrap().push(line);
                                    }
                                }
                                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::ConnectionReset) => continue,
                                Err(_) => break,
                            }
                        }
                    });
                    None
                }
                Err(e) => Some(e.to_string()),
            })
            .clone()
    }

    pub fn since(&self, since: u64) -> Update {
        let s = self.store.lock().unwrap();
        let next = s.next_seq();
        let skip = since.saturating_sub(s.first_seq).min(s.lines.len() as u64) as usize;
        Update { lines: s.lines.iter().skip(skip).cloned().collect(), next, live: s.last_at.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) }
    }

    #[cfg(test)]
    fn push(&self, line: Line) {
        self.store.lock().unwrap().push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(centre: u32, db: &[f32]) -> Vec<u8> {
        let mut p = b"FRSP\x01\0\0\0".to_vec();
        p.extend(centre.to_le_bytes());
        p.extend(2_560_000u32.to_le_bytes());
        p.extend((db.len() as u32).to_le_bytes());
        for v in db {
            p.extend(v.to_le_bytes());
        }
        p
    }

    #[test]
    fn parses_engine_packets_with_channel_levels() {
        let mut p = packet(145_300_000, &[-50.0]);
        p[4] = 2;
        p.extend(1u32.to_le_bytes());
        p.extend(145_400_000u32.to_le_bytes());
        for v in [-90.0f32, -80.0, -60.0] {
            p.extend(v.to_le_bytes());
        }
        p.extend([1, 0, 0, 0]);
        let line = parse_packet(&p).unwrap();
        assert_eq!(line.channels, vec![ChannelLevel { freq_hz: 145_400_000, noise_db: -90.0, threshold_db: -80.0, signal_db: -60.0, open: true }]);
        p.pop();
        assert!(parse_packet(&p).is_none());
    }

    #[test]
    fn parses_engine_packets() {
        let line = parse_packet(&packet(145_300_000, &[-50.0, -60.5])).unwrap();
        assert_eq!(line, Line { centre_hz: 145_300_000, sample_rate: 2_560_000, db: vec![-50.0, -60.5], channels: vec![] });
        assert!(parse_packet(b"nope").is_none());
        let mut short = packet(1, &[1.0, 2.0]);
        short.pop();
        assert!(parse_packet(&short).is_none());
    }

    #[test]
    fn keeps_recent_lines_and_resets_on_retune() {
        let feed = SpectrumFeed::default();
        let line = |c: u32| Line { centre_hz: c, sample_rate: 1, db: vec![0.0; 4], channels: vec![] };
        for _ in 0..(MAX_LINES + 10) {
            feed.push(line(1));
        }
        let u = feed.since(0);
        assert_eq!(u.lines.len(), MAX_LINES);
        assert_eq!(u.next, MAX_LINES as u64 + 10);
        assert!(u.live);
        assert_eq!(feed.since(u.next - 3).lines.len(), 3);
        assert!(feed.since(u.next).lines.is_empty());

        feed.push(line(2));
        let u2 = feed.since(u.next);
        assert_eq!(u2.lines.len(), 1, "only the line after the retune");
        assert_eq!(u2.lines[0].centre_hz, 2);
    }

    #[test]
    fn receives_over_udp() {
        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let feed = SpectrumFeed::default();
        assert_eq!(feed.ensure_listening(port), None);
        UdpSocket::bind("127.0.0.1:0").unwrap().send_to(&packet(7, &[1.0]), ("127.0.0.1", port)).unwrap();
        for _ in 0..50 {
            if feed.since(0).next == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(feed.since(0).lines[0].centre_hz, 7);
    }
}
