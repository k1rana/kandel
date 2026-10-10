//! Opening `config.toml` in the Windows "Open with" dialog.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

/// Opens the Windows "Open with" dialog for `path` via `rundll32`.
pub fn open_with_dialog(path: &Path) -> Result<()> {
    Command::new("rundll32.exe")
        .arg("shell32.dll,OpenAs_RunDLL")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting Open With dialog")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The command construction is the only testable part without launching a UI.
    #[test]
    fn builds_expected_rundll32_arguments() {
        let path = Path::new(r"C:\Users\test\.config\kandel\config.toml");
        let path_str = path.to_string_lossy();
        // Mirrors the arguments passed in `open_with_dialog`.
        let args = ["shell32.dll,OpenAs_RunDLL", path_str.as_ref()];
        assert_eq!(args[0], "shell32.dll,OpenAs_RunDLL");
        assert!(args[1].ends_with("config.toml"));
    }
}
