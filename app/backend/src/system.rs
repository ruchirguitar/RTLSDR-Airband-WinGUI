//! The few things that touch the desktop: free disk space, showing a folder
//! in the file manager, a folder picker and starting with Windows.

use std::path::{Path, PathBuf};

/// Free bytes on the drive holding `dir` (or its nearest existing parent).
pub fn free_space(dir: &Path) -> Option<u64> {
    let mut p = Some(dir);
    while let Some(d) = p {
        if d.exists() {
            return fs2::available_space(d).ok();
        }
        p = d.parent();
    }
    None
}

/// Opens `dir` in the file manager, with `file` selected when given.
pub fn reveal(dir: &Path, file: Option<&Path>) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer");
        match file {
            Some(f) if f.is_file() => cmd.arg(format!("/select,{}", f.display())),
            Some(f) => cmd.arg(f),
            None => cmd.arg(dir),
        };
        cmd.spawn().map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let target = file.and_then(|f| if f.is_dir() { Some(f) } else { f.parent() }).unwrap_or(dir);
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(opener).arg(target).spawn().map(|_| ())
    }
}

/// Shows a folder picker. Ok(None) when the user cancels.
pub fn pick_folder(start: &Path) -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    {
        let mut dialog = rfd::FileDialog::new().set_title("Choose where to save recordings");
        if start.is_dir() {
            dialog = dialog.set_directory(start);
        }
        Ok(dialog.pick_folder())
    }
    #[cfg(not(windows))]
    {
        let _ = start;
        Err("Choosing a folder this way only works on Windows. Type the path instead.".into())
    }
}

/// Adds or removes the app from the programs Windows starts at sign-in
/// (HKCU\...\Run). It starts hidden, in the tray.
pub fn set_autostart(exe: &Path, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let launcher = auto_launch::AutoLaunchBuilder::new()
            .set_app_name("Frequency Recorder")
            .set_app_path(&exe.to_string_lossy())
            .set_args(&["--hidden"])
            .build()
            .map_err(|e| e.to_string())?;
        let result = if enabled { launcher.enable() } else { launcher.disable() };
        result.map_err(|e| format!("Could not change starting with Windows: {e}"))
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, enabled);
        Err("Starting with the system is only available on Windows.".into())
    }
}
