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
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("explorer");
        match file {
            // Explorer wants /select,"C:\with spaces\file.mp3": Rust's own quoting would wrap
            // the whole argument, which Explorer can't read, and it then opens Documents.
            Some(f) if f.is_file() => cmd.raw_arg(explorer_select_arg(f)),
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

/// The Explorer argument that opens a file's folder with the file selected.
/// Explorer only accepts backslashes here.
pub fn explorer_select_arg(file: &Path) -> String {
    format!("/select,\"{}\"", file.display().to_string().replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn select_arg_quotes_the_path_only() {
        let arg = super::explorer_select_arg(std::path::Path::new("C:/Users/me/Documents/Frequency Recordings/145.400/a b.mp3"));
        assert_eq!(arg, r#"/select,"C:\Users\me\Documents\Frequency Recordings\145.400\a b.mp3""#);
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
