//! The user's setup - receiver, frequencies, nets and settings - and the
//! engine config generated from it. The app never shows engine config files:
//! this is the only thing the user edits.

use chrono::{Datelike, NaiveDateTime, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// An RTL-SDR captures 2.56 MHz around the centre frequency; the edges roll off.
pub const WINDOW_HALF_MHZ: f64 = 1.28;
pub const GOOD_HALF_MHZ: f64 = 1.0;
pub const USABLE_HALF_MHZ: f64 = 1.2;
pub const LIVE_PORT_BASE: u16 = 17300;
pub const SPECTRUM_PORT: u16 = 17299;
pub const SPECTRUM_LINES_PER_SEC: u32 = 15;
pub const NETS_DIR: &str = "Nets";
pub const CTCSS_TONES: [f64; 50] = [
    67.0, 69.3, 71.9, 74.4, 77.0, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100.0, 103.5, 107.2, 110.9, 114.8, 118.8, 123.0, 127.3, 131.8, 136.5, 141.3, 146.2, 150.0, 151.4, 156.7, 159.8, 162.2,
    165.5, 167.9, 171.3, 173.8, 177.3, 179.9, 183.5, 186.2, 189.9, 192.8, 196.6, 199.5, 203.5, 206.5, 210.7, 218.1, 225.7, 229.1, 233.6, 241.8, 250.3,
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Station {
    pub receiver: Receiver,
    pub frequencies: Vec<Frequency>,
    pub nets: Vec<Net>,
    pub recording: RecordingOptions,
    pub settings: Settings,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Receiver {
    pub device_index: u32,
    pub gain_db: f64,
    pub correction_ppm: i32,
    /// Waterfall lines per second (5 to 30).
    pub spectrum_rate: u32,
}

impl Default for Receiver {
    fn default() -> Self {
        Receiver { device_index: 0, gain_db: 30.0, correction_ppm: 0, spectrum_rate: SPECTRUM_LINES_PER_SEC }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Modulation {
    #[default]
    Nfm,
    Am,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Frequency {
    pub id: String,
    pub mhz: f64,
    pub name: String,
    pub modulation: Modulation,
    /// 0 (always open, like SQL 0 on a handheld) to 9 (strong signals only), 2 dB SNR per step.
    pub squelch: u8,
    pub record: bool,
    pub ctcss_hz: Option<f64>,
    /// A Windows notification when it becomes active after a quiet spell.
    pub notify: bool,
}

impl Default for Frequency {
    fn default() -> Self {
        Frequency { id: String::new(), mhz: 0.0, name: String::new(), modulation: Modulation::Nfm, squelch: 5, record: true, ctcss_hz: None, notify: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Net {
    pub id: String,
    pub name: String,
    pub mhz: f64,
    /// 0 = Monday ... 6 = Sunday: the days the net starts on.
    pub days: Vec<u8>,
    pub start: String,
    pub end: String,
    pub enabled: bool,
}

impl Default for Net {
    fn default() -> Self {
        Net { id: String::new(), name: String::new(), mhz: 0.0, days: (0..7).collect(), start: "07:00".into(), end: "08:00".into(), enabled: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RecordingOptions {
    /// Outside nets: overs closer together than this share one file.
    pub clip_join_sec: f64,
    /// During a net: silences shorter than this keep the net in one file.
    pub net_join_sec: f64,
    /// A net keeps recording past its end time while it is still going, up to this long.
    pub net_overrun_max_min: u32,
    pub gap_tone: bool,
    pub normalize: bool,
    /// Transmissions shorter than this (kerchunks) are not kept.
    pub min_clip_sec: f64,
}

impl Default for RecordingOptions {
    fn default() -> Self {
        RecordingOptions { clip_join_sec: 10.0, net_join_sec: 60.0, net_overrun_max_min: 60, gap_tone: true, normalize: true, min_clip_sec: 0.5 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Empty means the default (Documents\Frequency Recordings).
    pub recordings_dir: String,
    /// 0 keeps recordings forever. Net recordings are never deleted automatically.
    pub retention_days: u32,
    pub local_time: bool,
    /// Warn when nothing at all has been heard for this long. 0 turns it off.
    pub silence_warning_hours: u32,
    pub autostart: bool,
    pub close_to_tray: bool,
    /// "system", "light" or "dark".
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { recordings_dir: String::new(), retention_days: 90, local_time: true, silence_warning_hours: 24, autostart: false, close_to_tray: true, theme: "system".into() }
    }
}

/// Where the receiver tunes and how well each frequency fits.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Plan {
    pub centre_mhz: f64,
    pub low_mhz: f64,
    pub high_mhz: f64,
    pub fits: Vec<Fit>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Good,
    Edge,
    Outside,
}

fn round_khz(mhz: f64) -> f64 {
    (mhz * 1000.0).round() / 1000.0
}

/// The centre goes midway between the outermost frequencies, moved off any
/// frequency it would land on: the dongle has a spike at its centre.
pub fn plan(mhz: &[f64]) -> Option<Plan> {
    let lo = mhz.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = mhz.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if !lo.is_finite() {
        return None;
    }
    let mut centre = round_khz((lo + hi) / 2.0);
    if mhz.iter().any(|f| (f - centre).abs() < 0.025) {
        centre = round_khz(if hi - centre < USABLE_HALF_MHZ - 0.05 { centre + 0.05 } else { centre - 0.05 });
    }
    let fits = mhz
        .iter()
        .map(|f| {
            let d = (f - centre).abs();
            if d <= GOOD_HALF_MHZ {
                Fit::Good
            } else if d <= USABLE_HALF_MHZ {
                Fit::Edge
            } else {
                Fit::Outside
            }
        })
        .collect();
    Some(Plan { centre_mhz: centre, low_mhz: round_khz(centre - WINDOW_HALF_MHZ), high_mhz: round_khz(centre + WINDOW_HALF_MHZ), fits })
}

/// "145.400", or more decimals when the frequency needs them ("145.4125").
pub fn fmt_mhz(mhz: f64) -> String {
    let s = format!("{mhz:.4}");
    if s.ends_with('0') {
        s[..s.len() - 1].to_string()
    } else {
        s
    }
}

/// A file or folder name made from user text.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name.trim().chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == ' ').to_string();
    if cleaned.is_empty() {
        "Net".into()
    } else {
        cleaned.chars().take(60).collect()
    }
}

pub fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

impl Net {
    /// Whether `now` (local time) is between this net's start and end. A net
    /// whose end is before its start runs past midnight; `days` are the days it starts on.
    pub fn in_window(&self, now: NaiveDateTime) -> bool {
        let (Some(start), Some(end)) = (parse_hhmm(&self.start), parse_hhmm(&self.end)) else { return false };
        if !self.enabled || start == end {
            return false;
        }
        let t = now.time().with_second(0).unwrap_or(now.time());
        let today = now.weekday().num_days_from_monday() as u8;
        let yesterday = (today + 6) % 7;
        if start < end {
            self.days.contains(&today) && t >= start && t < end
        } else {
            (self.days.contains(&today) && t >= start) || (self.days.contains(&yesterday) && t < end)
        }
    }
}

impl Station {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let r = &self.receiver;
        if !(0.0..=50.0).contains(&r.gain_db) {
            errors.push("Gain must be between 0 and 50 dB.".into());
        }
        if !(-200..=200).contains(&r.correction_ppm) {
            errors.push("Correction must be between -200 and 200 ppm.".into());
        }
        if !(5..=30).contains(&r.spectrum_rate) {
            errors.push("Waterfall speed must be 5 to 30 lines a second.".into());
        }
        if !["system", "light", "dark"].contains(&self.settings.theme.as_str()) {
            errors.push("Theme must be system, light or dark.".into());
        }
        for (i, f) in self.frequencies.iter().enumerate() {
            let label = if f.name.trim().is_empty() { fmt_mhz(f.mhz) } else { f.name.clone() };
            if !(24.0..=1766.0).contains(&f.mhz) {
                errors.push(format!("{label}: the frequency must be between 24 and 1766 MHz."));
            }
            if f.name.chars().count() > 40 {
                errors.push(format!("{label}: the name can be at most 40 characters."));
            }
            if f.squelch > 9 {
                errors.push(format!("{label}: squelch must be between 0 and 9."));
            }
            if let Some(tone) = f.ctcss_hz {
                if !CTCSS_TONES.iter().any(|t| (t - tone).abs() < 0.05) {
                    errors.push(format!("{label}: {tone} Hz is not a standard CTCSS tone."));
                }
            }
            if self.frequencies[..i].iter().any(|o| (o.mhz - f.mhz).abs() < 0.0005) {
                errors.push(format!("{} MHz is in the list twice.", fmt_mhz(f.mhz)));
            }
        }
        let mhz: Vec<f64> = self.frequencies.iter().map(|f| f.mhz).collect();
        if let Some(p) = plan(&mhz) {
            for (f, fit) in self.frequencies.iter().zip(&p.fits) {
                if *fit == Fit::Outside {
                    errors.push(format!(
                        "{} MHz is too far from the others: one dongle covers about {:.1} MHz. It needs a second dongle.",
                        fmt_mhz(f.mhz),
                        2.0 * USABLE_HALF_MHZ
                    ));
                }
            }
        }
        for (i, n) in self.nets.iter().enumerate() {
            let label = if n.name.trim().is_empty() { "A net".to_string() } else { n.name.clone() };
            if n.name.trim().is_empty() {
                errors.push("Every net needs a name.".into());
            }
            if self.nets[..i].iter().any(|o| o.name.trim().eq_ignore_ascii_case(n.name.trim())) {
                errors.push(format!("There are two nets called {label}."));
            }
            if !self.frequencies.iter().any(|f| (f.mhz - n.mhz).abs() < 0.0005) {
                errors.push(format!("{label}: pick one of your frequencies."));
            }
            match (parse_hhmm(&n.start), parse_hhmm(&n.end)) {
                (Some(s), Some(e)) if s != e => {}
                (Some(_), Some(_)) => errors.push(format!("{label}: the start and end time are the same.")),
                _ => errors.push(format!("{label}: times look like 07:00.")),
            }
            if n.days.is_empty() || n.days.iter().any(|d| *d > 6) {
                errors.push(format!("{label}: pick at least one day."));
            }
        }
        let o = &self.recording;
        if !(0.0..=60.0).contains(&o.clip_join_sec) {
            errors.push("Joining overs into one clip: 0 to 60 seconds.".into());
        }
        if !(5.0..=600.0).contains(&o.net_join_sec) {
            errors.push("Silence allowed during a net: 5 to 600 seconds.".into());
        }
        if o.net_overrun_max_min > 240 {
            errors.push("A net can run over by at most 240 minutes.".into());
        }
        if !(0.0..=5.0).contains(&o.min_clip_sec) {
            errors.push("Shortest clip kept: 0 to 5 seconds.".into());
        }
        if self.settings.retention_days > 3650 {
            errors.push("Recordings can be kept for at most 3650 days.".into());
        }
        if self.settings.silence_warning_hours > 168 {
            errors.push("The silence warning can be at most 168 hours.".into());
        }
        errors
    }

    pub fn frequency_for(&self, mhz: f64) -> Option<(usize, &Frequency)> {
        self.frequencies.iter().enumerate().find(|(_, f)| (f.mhz - mhz).abs() < 0.0005)
    }
}

fn q(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fl(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

fn slash_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The engine config for `station`. `active_nets` are the nets recording right now:
/// their frequency writes one joined file per net instead of clips. With
/// `record` false nothing is saved, but the dongle keeps running for live
/// listening and the waterfall.
pub fn engine_config(station: &Station, recordings_dir: &Path, active_nets: &[&Net], record: bool) -> String {
    let s = station;
    let o = &s.recording;
    let mhz: Vec<f64> = s.frequencies.iter().map(|f| f.mhz).collect();
    let centre = plan(&mhz).map(|p| p.centre_mhz).unwrap_or(100.0);
    let mut c = String::new();
    c.push_str("# Written by Frequency Recorder. Changes made here are overwritten.\n");
    c.push_str(&format!("localtime = {};\n", s.settings.local_time));
    c.push_str("devices:\n(\n  {\n    type = \"rtlsdr\";\n");
    c.push_str(&format!("    index = {};\n    gain = {};\n    correction = {};\n", s.receiver.device_index, fl(s.receiver.gain_db), s.receiver.correction_ppm));
    c.push_str(&format!("    centerfreq = {};\n", fl(centre)));
    c.push_str(&format!("    spectrum = {{ dest_address = \"127.0.0.1\"; dest_port = {SPECTRUM_PORT}; rate = {}; }};\n", s.receiver.spectrum_rate));
    c.push_str("    channels:\n    (\n");
    for (i, f) in s.frequencies.iter().enumerate() {
        let freq = fmt_mhz(f.mhz);
        c.push_str(&format!("      {{\n        freq = {};\n", fl(f.mhz)));
        if !f.name.trim().is_empty() {
            c.push_str(&format!("        label = {};\n", q(f.name.trim())));
        }
        c.push_str(&format!("        modulation = {};\n", q(if f.modulation == Modulation::Am { "am" } else { "nfm" })));
        c.push_str(&format!("        squelch_snr_threshold = {};\n", fl(f.squelch as f64 * 2.0)));
        if let Some(tone) = f.ctcss_hz {
            c.push_str(&format!("        ctcss = {};\n", fl(tone)));
        }
        c.push_str("        outputs:\n        (\n");
        if f.record && record {
            let net = active_nets.iter().find(|n| (n.mhz - f.mhz).abs() < 0.0005);
            let (dir, template, idle, max_time) = match net {
                Some(n) => (recordings_dir.join(NETS_DIR).join(safe_name(&n.name)), safe_name(&n.name), o.net_join_sec, 4.0 * 3600.0),
                // the engine needs an idle time above 0; half a second still splits every over
                None => (recordings_dir.join(&freq), freq.clone(), o.clip_join_sec.max(0.5), 3600.0),
            };
            c.push_str("          {\n            type = \"file\";\n");
            c.push_str(&format!("            directory = {};\n            filename_template = {};\n", q(&slash_path(&dir)), q(&template)));
            c.push_str("            dated_subdirectories = true;\n            split_on_transmission = true;\n");
            c.push_str(&format!("            split_max_idle_time = {};\n            split_max_file_time = {};\n", fl(idle), fl(max_time)));
            if o.min_clip_sec > 0.0 {
                c.push_str(&format!("            split_tx_drop_shorter_than = {};\n", fl(o.min_clip_sec)));
            }
            c.push_str(&format!("            split_gap_tone = {};\n            split_normalize = {};\n            split_segment_log = true;\n", o.gap_tone, o.normalize));
            c.push_str("          },\n");
        }
        c.push_str("          {\n            type = \"udp_stream\";\n            dest_address = \"127.0.0.1\";\n");
        c.push_str(&format!("            dest_port = {};\n            continuous = false;\n          }}\n", LIVE_PORT_BASE + i as u16));
        c.push_str(if i + 1 == s.frequencies.len() { "        );\n      }\n" } else { "        );\n      },\n" });
    }
    c.push_str("    );\n  }\n);\n");
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn freq(mhz: f64, name: &str) -> Frequency {
        Frequency { id: format!("f{mhz}"), mhz, name: name.into(), ..Default::default() }
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    #[test]
    fn plan_centres_between_frequencies() {
        let p = plan(&[145.2, 145.4]).unwrap();
        assert_eq!(p.centre_mhz, 145.3);
        assert_eq!((p.low_mhz, p.high_mhz), (144.02, 146.58));
        assert_eq!(p.fits, vec![Fit::Good, Fit::Good]);
    }

    #[test]
    fn plan_moves_the_centre_off_a_frequency() {
        let p = plan(&[145.5]).unwrap();
        assert_eq!(p.centre_mhz, 145.55);
        let p = plan(&[145.0, 145.2, 145.4]).unwrap();
        assert!((p.centre_mhz - 145.2).abs() > 0.02);
        assert!(p.fits.iter().all(|f| *f == Fit::Good));
    }

    #[test]
    fn plan_flags_frequencies_that_do_not_fit() {
        let p = plan(&[144.0, 146.3]).unwrap();
        assert_eq!(p.fits, vec![Fit::Edge, Fit::Edge]);
        let p = plan(&[144.0, 147.0]).unwrap();
        assert_eq!(p.fits, vec![Fit::Outside, Fit::Outside]);
        assert!(plan(&[]).is_none());
    }

    #[test]
    fn formats_frequencies() {
        assert_eq!(fmt_mhz(145.4), "145.400");
        assert_eq!(fmt_mhz(145.4125), "145.4125");
        assert_eq!(fmt_mhz(433.0), "433.000");
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("Morning net"), "Morning net");
        assert_eq!(safe_name(" a/b:c? "), "a_b_c_");
        assert_eq!(safe_name("..."), "Net");
    }

    #[test]
    fn validation_catches_mistakes() {
        let mut s = Station { frequencies: vec![freq(145.4, "A"), freq(145.4, "B"), freq(150.0, "C")], ..Default::default() };
        s.frequencies[0].squelch = 10;
        s.frequencies[1].ctcss_hz = Some(88.0);
        s.nets.push(Net { name: "Morning".into(), mhz: 146.0, start: "07:00".into(), end: "07:00".into(), days: vec![], ..Default::default() });
        let e = s.validate().join("\n");
        assert!(e.contains("squelch must be between 0 and 9"), "{e}");
        assert!(e.contains("88 Hz is not a standard CTCSS tone"), "{e}");
        assert!(e.contains("145.400 MHz is in the list twice"), "{e}");
        assert!(e.contains("150.000 MHz is too far"), "{e}");
        assert!(e.contains("Morning: pick one of your frequencies"), "{e}");
        assert!(e.contains("Morning: the start and end time are the same"), "{e}");
        assert!(e.contains("Morning: pick at least one day"), "{e}");
    }

    #[test]
    fn a_sensible_station_is_valid() {
        let mut s = Station { frequencies: vec![freq(145.4, "Repeater A"), freq(145.2, "Repeater B")], ..Default::default() };
        s.nets.push(Net { name: "Morning net".into(), mhz: 145.4, ..Default::default() });
        assert!(s.validate().is_empty(), "{:?}", s.validate());
    }

    #[test]
    fn net_windows() {
        let morning = Net { name: "m".into(), mhz: 145.4, days: vec![0, 1, 2, 3, 4], start: "07:00".into(), end: "08:00".into(), ..Default::default() };
        // 2026-11-12 is a Thursday
        assert!(morning.in_window(at(2026, 11, 12, 7, 0)));
        assert!(morning.in_window(at(2026, 11, 12, 7, 59)));
        assert!(!morning.in_window(at(2026, 11, 12, 8, 0)));
        assert!(!morning.in_window(at(2026, 11, 12, 6, 59)));
        assert!(!morning.in_window(at(2026, 11, 14, 7, 30)), "Saturday is not a net day");
        let late = Net { days: vec![3], start: "23:30".into(), end: "00:30".into(), ..morning.clone() };
        assert!(late.in_window(at(2026, 11, 12, 23, 45)));
        assert!(late.in_window(at(2026, 11, 13, 0, 15)), "runs past midnight from Thursday");
        assert!(!late.in_window(at(2026, 11, 12, 0, 15)), "Wednesday night is not a net day");
        let off = Net { enabled: false, ..morning };
        assert!(!off.in_window(at(2026, 11, 12, 7, 30)));
    }

    #[test]
    fn engine_config_for_clips_and_nets() {
        let mut s = Station { frequencies: vec![freq(145.4, "Repeater \"A\""), freq(145.2, "")], ..Default::default() };
        s.frequencies[1].ctcss_hz = Some(88.5);
        s.frequencies[1].record = false;
        s.frequencies[1].modulation = Modulation::Am;
        let net = Net { name: "Morning net".into(), mhz: 145.4, ..Default::default() };
        let dir = Path::new("C:\\Users\\me\\Documents\\Frequency Recordings");

        let clips = engine_config(&s, dir, &[], true);
        let tree = crate::libconfig::parse(&clips).expect("valid libconfig");
        let dev = &tree["devices"][0];
        assert_eq!(dev["centerfreq"]["value"], 145.3);
        assert_eq!(dev["spectrum"]["dest_port"]["value"], 17299);
        assert_eq!(dev["spectrum"]["rate"]["value"], 15);
        assert_eq!(dev["gain"]["type"], "float");
        let ch0 = &dev["channels"][0];
        assert_eq!(ch0["label"]["value"], "Repeater \"A\"");
        assert_eq!(ch0["squelch_snr_threshold"]["value"], 10.0);
        let out = &ch0["outputs"][0];
        assert_eq!(out["directory"]["value"], "C:/Users/me/Documents/Frequency Recordings/145.400");
        assert_eq!(out["filename_template"]["value"], "145.400");
        assert_eq!(out["split_max_idle_time"]["value"], 10.0);
        assert_eq!(out["split_gap_tone"]["value"], true);
        assert_eq!(out["split_segment_log"]["value"], true);
        assert_eq!(ch0["outputs"][1]["dest_port"]["value"], 17300);
        let ch1 = &dev["channels"][1];
        assert_eq!(ch1["modulation"]["value"], "am");
        assert_eq!(ch1["ctcss"]["value"], 88.5);
        assert_eq!(ch1["outputs"].as_array().unwrap().len(), 1, "not recorded: live audio only");
        assert_eq!(ch1["outputs"][0]["dest_port"]["value"], 17301);
        assert!(clips.contains("localtime = true;"));

        let netted = engine_config(&s, dir, &[&net], true);
        let tree = crate::libconfig::parse(&netted).unwrap();
        let out = &tree["devices"][0]["channels"][0]["outputs"][0];
        assert_eq!(out["directory"]["value"], "C:/Users/me/Documents/Frequency Recordings/Nets/Morning net");
        assert_eq!(out["filename_template"]["value"], "Morning net");
        assert_eq!(out["split_max_idle_time"]["value"], 60.0);

        let paused = engine_config(&s, dir, &[&net], false);
        let tree = crate::libconfig::parse(&paused).unwrap();
        let outs = tree["devices"][0]["channels"][0]["outputs"].as_array().unwrap();
        assert_eq!(outs.len(), 1, "recording off: live audio only");
        assert_eq!(outs[0]["type"]["value"], "udp_stream");
    }
}
