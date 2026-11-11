//! Keeps the engine doing what the station says: started when recording is on,
//! restarted with a new config when the station changes or a net starts or
//! ends, restarted after a crash or a dongle replug (with backoff), and old
//! clips deleted. Runs every couple of seconds on a blocking thread.

use crate::live::Activity;
use crate::station::{engine_config, Net, Station, LIVE_PORT_BASE, NETS_DIR};
use crate::AppState;
use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const HEALTHY_AFTER: Duration = Duration::from_secs(60);
const MAX_BACKOFF_SEC: u64 = 60;
const RETENTION_EVERY: Duration = Duration::from_secs(3600);
const DISK_EVERY: Duration = Duration::from_secs(300);

#[derive(Clone, Default)]
pub struct SupervisorState {
    /// The config the running engine was started with (None: not started by us).
    pub engine_config: Option<String>,
    pub engine_started: Option<Instant>,
    pub engine_started_unix: Option<f64>,
    pub restarts: u32,
    pub backoff_sec: u64,
    pub retry_at: Option<Instant>,
    pub problem: Option<String>,
    /// Ids of the nets being recorded as one file right now.
    pub active_nets: Vec<String>,
    /// Net id -> unix time its window ended, while it runs over.
    pub net_overrun: HashMap<String, f64>,
    pub last_heard_any: Option<f64>,
    pub watching_since_unix: Option<f64>,
    pub disk_used: Option<u64>,
    pub disk_checked: Option<Instant>,
    pub retention_checked: Option<Instant>,
}

pub fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Which nets to record now. A net past its end time keeps going while its
/// frequency was heard within the net join gap, for at most the overrun limit,
/// so a net that runs late is not cut in two.
pub fn nets_to_record(station: &Station, now: NaiveDateTime, now_unix: f64, previously: &[String], overrun: &mut HashMap<String, f64>, last_heard: impl Fn(f64) -> Option<f64>) -> Vec<String> {
    let join = station.recording.net_join_sec;
    let max_overrun = station.recording.net_overrun_max_min as f64 * 60.0;
    let mut out = Vec::new();
    for net in &station.nets {
        if net.in_window(now) {
            overrun.remove(&net.id);
            out.push(net.id.clone());
            continue;
        }
        if !net.enabled || !(previously.contains(&net.id) || overrun.contains_key(&net.id)) {
            overrun.remove(&net.id);
            continue;
        }
        let ended = *overrun.entry(net.id.clone()).or_insert(now_unix);
        let still_going = last_heard(net.mhz).is_some_and(|t| now_unix - t < join);
        if still_going && now_unix - ended < max_overrun {
            out.push(net.id.clone());
        } else {
            overrun.remove(&net.id);
        }
    }
    out
}

/// A plain-language reason for the engine stopping, from its last log lines.
pub fn explain_exit(log: &[String]) -> String {
    let recent: Vec<&String> = log.iter().rev().take(40).collect();
    let has = |needle: &str| recent.iter().any(|l| l.contains(needle));
    if has("Failed to open rtlsdr device") || (has("not found") && has("RTLSDR")) || has("failed to initialize - aborting") {
        "The dongle isn't plugged in, or another program is using it. Recording starts again by itself once it's free.".into()
    } else if has("async read failed") || has("All receivers failed") {
        "The dongle stopped responding (unplugged?). Recording starts again by itself once it's back.".into()
    } else if let Some(line) = recent.iter().find(|l| l.contains("onfiguration error")) {
        format!("The recorder rejected its settings: {line}")
    } else {
        match recent.iter().find(|l| !l.trim().is_empty() && !l.starts_with('[')) {
            Some(line) => format!("The recorder stopped unexpectedly: {line}"),
            None => "The recorder stopped unexpectedly.".into(),
        }
    }
}

pub fn next_backoff(current: u64) -> u64 {
    (current * 2).clamp(5, MAX_BACKOFF_SEC)
}

fn activities(app: &AppState, station: &Station) -> Vec<Activity> {
    (0..station.frequencies.len()).map(|i| app.live.get(LIVE_PORT_BASE + i as u16).activity()).collect()
}

pub fn tick(app: &AppState, now_local: NaiveDateTime) {
    let station = app.station();
    let enabled = app.recording_enabled();
    let rec_dir = app.recordings_dir();
    let mut sup = app.supervisor.lock().unwrap().clone();
    let now = now_unix();
    sup.watching_since_unix.get_or_insert(now);

    let heard = activities(app, &station);
    for a in &heard {
        if let Some(t) = a.last_heard_unix {
            sup.last_heard_any = Some(sup.last_heard_any.map_or(t, |old: f64| old.max(t)));
        }
    }
    let last_heard = |mhz: f64| station.frequency_for(mhz).and_then(|(i, _)| heard.get(i)).and_then(|a| if a.on_air { Some(now) } else { a.last_heard_unix });
    let previously = sup.active_nets.clone();
    sup.active_nets = nets_to_record(&station, now_local, now, &previously, &mut sup.net_overrun, last_heard);
    let active: Vec<&Net> = station.nets.iter().filter(|n| sup.active_nets.contains(&n.id)).collect();
    // recording off still runs the dongle, for live listening and the waterfall
    let config = engine_config(&station, &rec_dir, &active, enabled);
    let running = app.process.status().running;

    let blocked = if station.frequencies.is_empty() {
        Some(Some("Add a frequency to start recording.".to_string()))
    } else if let Some(e) = station.validate().first() {
        Some(Some(e.clone()))
    } else {
        None
    };

    if let Some(reason) = blocked {
        if running {
            app.process.stop(STOP_TIMEOUT);
        }
        sup.engine_config = None;
        sup.engine_started = None;
        sup.engine_started_unix = None;
        sup.retry_at = None;
        sup.problem = reason;
    } else {
        let _ = std::fs::create_dir_all(rec_dir.join(NETS_DIR));
        let start = |sup: &mut SupervisorState, restart: bool| {
            let conf_path = app.paths.data_dir.join("engine.conf");
            let result = std::fs::write(&conf_path, &config).map_err(|e| format!("Could not write {}: {e}", conf_path.display())).and_then(|_| {
                if restart {
                    app.process.restart(&app.paths.engine_binary, &conf_path)
                } else {
                    app.process.start(&app.paths.engine_binary, &conf_path)
                }
            });
            match result {
                Ok(()) => {
                    sup.problem = None;
                    sup.engine_config = Some(config.clone());
                    sup.engine_started = Some(Instant::now());
                    sup.engine_started_unix = Some(now_unix());
                    sup.retry_at = None;
                }
                Err(e) => {
                    sup.problem = Some(e);
                    sup.backoff_sec = next_backoff(sup.backoff_sec);
                    sup.retry_at = Some(Instant::now() + Duration::from_secs(sup.backoff_sec));
                }
            }
        };
        if running {
            if sup.engine_config.as_deref() != Some(config.as_str()) {
                start(&mut sup, true);
            } else if sup.engine_started.is_some_and(|t| t.elapsed() > HEALTHY_AFTER) {
                sup.backoff_sec = 0;
                sup.problem = None;
            }
        } else {
            if sup.engine_config.take().is_some() {
                sup.restarts += 1;
                sup.problem = Some(explain_exit(&app.process.logs(0)));
                sup.backoff_sec = next_backoff(sup.backoff_sec);
                sup.retry_at = Some(Instant::now() + Duration::from_secs(sup.backoff_sec));
                sup.engine_started = None;
                sup.engine_started_unix = None;
            }
            if sup.retry_at.map_or(true, |t| Instant::now() >= t) {
                start(&mut sup, false);
            }
        }
    }

    if sup.retention_checked.map_or(true, |t| t.elapsed() > RETENTION_EVERY) {
        sup.retention_checked = Some(Instant::now());
        crate::recordings::apply_retention(&rec_dir, station.settings.retention_days, SystemTime::now());
    }
    if sup.disk_checked.map_or(true, |t| t.elapsed() > DISK_EVERY) {
        sup.disk_checked = Some(Instant::now());
        sup.disk_used = Some(crate::recordings::total_bytes(&rec_dir));
    }
    *app.supervisor.lock().unwrap() = sup;
}

/// Runs `tick` every two seconds until the process exits.
pub fn spawn(app: std::sync::Arc<AppState>) {
    tokio::spawn(async move {
        // The newest recording tells when something was last heard, across restarts of the app.
        let dir = app.recordings_dir();
        if let Ok(recs) = tokio::task::spawn_blocking(move || crate::recordings::scan(&dir)).await {
            if let Some(r) = recs.first() {
                app.supervisor.lock().unwrap().last_heard_any = Some(r.start_unix + r.duration_sec.unwrap_or(0.0));
            }
        }
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            let a = app.clone();
            let _ = tokio::task::spawn_blocking(move || tick(&a, chrono::Local::now().naive_local())).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::station::Frequency;
    use chrono::NaiveDate;

    fn station() -> Station {
        let mut s = Station::default();
        s.frequencies.push(Frequency { id: "a".into(), mhz: 145.4, ..Default::default() });
        s.nets.push(Net { id: "m".into(), name: "Morning".into(), mhz: 145.4, start: "07:00".into(), end: "08:00".into(), ..Default::default() });
        s
    }

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn nets_record_in_their_window_and_run_over_while_busy() {
        let s = station();
        let mut overrun = HashMap::new();
        let t0 = 1_000_000.0;
        assert_eq!(nets_to_record(&s, at(7, 30), t0, &[], &mut overrun, |_| None), vec!["m"]);
        assert!(nets_to_record(&s, at(6, 59), t0, &[], &mut overrun, |_| None).is_empty());

        // past the end but someone spoke 10 s ago: keeps going
        let active = nets_to_record(&s, at(8, 1), t0, &["m".into()], &mut overrun, |_| Some(t0 - 10.0));
        assert_eq!(active, vec!["m"]);
        // quiet for longer than the join gap: done
        let active = nets_to_record(&s, at(8, 3), t0 + 120.0, &active, &mut overrun, |_| Some(t0));
        assert!(active.is_empty());
        assert!(overrun.is_empty());

        // still busy, but past the overrun limit
        let mut overrun = HashMap::new();
        let active = nets_to_record(&s, at(8, 0), t0, &["m".into()], &mut overrun, |_| Some(t0));
        assert_eq!(active, vec!["m"]);
        let late = t0 + 61.0 * 60.0;
        assert!(nets_to_record(&s, at(9, 1), late, &active, &mut overrun, |_| Some(late)).is_empty());
    }

    #[test]
    fn a_net_that_never_started_does_not_run_over() {
        let s = station();
        let mut overrun = HashMap::new();
        assert!(nets_to_record(&s, at(8, 1), 1.0, &[], &mut overrun, |_| Some(1.0)).is_empty());
    }

    #[test]
    fn exit_explanations() {
        let lines = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(explain_exit(&lines(&["Failed to open rtlsdr device #0.", "1 device(s) failed to initialize - aborting"])).contains("isn't plugged in"));
        assert!(explain_exit(&lines(&["RTLSDR device #0: async read failed, disabling", "All receivers failed, exiting"])).contains("stopped responding"));
        assert!(explain_exit(&lines(&["Configuration error: devices.[0]: gain is weird"])).contains("gain is weird"));
        assert_eq!(explain_exit(&lines(&["something odd", "[process exited with code 1]"])), "The recorder stopped unexpectedly: something odd");
        assert_eq!(explain_exit(&[]), "The recorder stopped unexpectedly.");
    }

    #[test]
    fn backoff_grows_to_a_minute() {
        assert_eq!(next_backoff(0), 5);
        assert_eq!(next_backoff(5), 10);
        assert_eq!(next_backoff(40), 60);
        assert_eq!(next_backoff(60), 60);
    }
}
