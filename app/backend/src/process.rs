//! Starts, stops and watches the rtl_airband engine, keeping a rolling log.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOG_MAXLEN: usize = 2000;

#[derive(Default)]
struct Inner {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    log: VecDeque<String>,
    started_at: Option<f64>,
    last_exit_code: Option<i32>,
}

#[derive(Clone, Default)]
pub struct ProcessManager {
    inner: Arc<Mutex<Inner>>,
}

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct Status {
    pub running: bool,
    pub pid: Option<u32>,
    pub started_at: Option<f64>,
    pub last_exit_code: Option<i32>,
}

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn is_running(inner: &mut Inner) -> bool {
    match inner.child.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        Some(Ok(Some(status))) => {
            inner.last_exit_code = status.code();
            false
        }
        _ => false,
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(&self) -> Status {
        let mut inner = self.inner.lock().unwrap();
        let running = is_running(&mut inner);
        Status {
            running,
            pid: if running { inner.child.as_ref().map(|c| c.id()) } else { None },
            started_at: if running { inner.started_at } else { None },
            last_exit_code: inner.last_exit_code,
        }
    }

    pub fn logs(&self, since: usize) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        inner.log.iter().skip(since).cloned().collect()
    }

    pub fn start(&self, binary: &str, config_path: &Path) -> Result<(), String> {
        self.start_command(Command::new(binary), binary, &["-F", "-e", "-c"], config_path)
    }

    fn start_command(&self, mut cmd: Command, binary: &str, args: &[&str], config_path: &Path) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        if is_running(&mut inner) {
            return Err("Process is already running".into());
        }
        if binary.is_empty() {
            return Err("No binary path configured".into());
        }
        if !config_path.is_file() {
            return Err(format!("Config file not found: {}", config_path.display()));
        }
        inner.log.clear();
        inner.last_exit_code = None;

        cmd.args(args).arg(config_path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("Could not start {binary}: {e}"))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        inner.stdin = child.stdin.take();
        inner.child = Some(child);
        inner.started_at = Some(now_secs());
        drop(inner);

        if let Some(out) = stdout {
            self.spawn_reader(out);
        }
        if let Some(err) = stderr {
            self.spawn_reader(err);
        }
        Ok(())
    }

    fn spawn_reader<R: Read + Send + 'static>(&self, stream: R) {
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let mut inner = inner.lock().unwrap();
                if inner.log.len() >= LOG_MAXLEN {
                    inner.log.pop_front();
                }
                inner.log.push_back(line);
            }
        });
    }

    /// Asks the engine to exit (it finishes open recordings), and kills it only
    /// if it is still running after `timeout`.
    pub fn stop(&self, timeout: Duration) {
        let mut inner = self.inner.lock().unwrap();
        if !is_running(&mut inner) {
            return;
        }
        // On Windows the engine treats its stdin pipe closing as a stop request.
        inner.stdin = None;
        #[cfg(unix)]
        if let Some(pid) = inner.child.as_ref().map(|c| c.id()) {
            let _ = Command::new("kill").arg("-TERM").arg(pid.to_string()).status();
        }
        drop(inner);

        let deadline = Instant::now() + timeout;
        loop {
            let mut inner = self.inner.lock().unwrap();
            if !is_running(&mut inner) {
                let code = inner.last_exit_code;
                inner.log.push_back(format!("[process exited with code {}]", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())));
                return;
            }
            if Instant::now() >= deadline {
                if let Some(child) = inner.child.as_mut() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                inner.log.push_back("[process did not exit in time and was killed]".into());
                return;
            }
            drop(inner);
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn restart(&self, binary: &str, config_path: &Path) -> Result<(), String> {
        self.stop(Duration::from_secs(5));
        self.start(binary, config_path)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Closing stdin here makes the engine exit with the app.
        self.stdin = None;
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn wait_for_log(pm: &ProcessManager, needle: &str) -> bool {
        for _ in 0..50 {
            if pm.logs(0).iter().any(|l| l.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn rejects_missing_config() {
        let pm = ProcessManager::new();
        let err = pm.start("sh", Path::new("/nonexistent.conf")).unwrap_err();
        assert!(err.contains("Config file not found"));
    }

    #[test]
    fn captures_output_and_reports_exit() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("a.conf");
        std::fs::write(&conf, "").unwrap();
        let pm = ProcessManager::new();
        pm.start_command(Command::new("sh"), "sh", &["-c", "echo hello; echo oops >&2; exit 3", "sh"], &conf).unwrap();
        assert!(wait_for_log(&pm, "hello"));
        assert!(wait_for_log(&pm, "oops"));
        for _ in 0..50 {
            if !pm.status().running {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(pm.status().last_exit_code, Some(3));
    }

    #[test]
    fn stop_ends_a_running_process_and_refuses_double_start() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("a.conf");
        std::fs::write(&conf, "").unwrap();
        let pm = ProcessManager::new();
        pm.start_command(Command::new("sh"), "sh", &["-c", "sleep 30", "sh"], &conf).unwrap();
        assert!(pm.status().running);
        assert!(pm.start_command(Command::new("sh"), "sh", &["-c", "true", "sh"], &conf).is_err());
        pm.stop(Duration::from_secs(5));
        assert!(!pm.status().running);
    }
}
