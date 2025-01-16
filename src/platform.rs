// src/platform.rs
use std::process::{Command, Stdio};
use std::path::Path;

pub fn is_command_available(cmd: &str) -> bool {
    if cfg!(target_os = "windows") {
        Command::new("where")
            .arg(cmd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        Command::new("which")
            .arg(cmd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

pub fn is_desktop_file_available(file: &str) -> bool {
    if cfg!(target_os = "windows") {
        let paths = [
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs",
            "C:\\Users\\Public\\Desktop",
        ];

        for path in paths.iter() {
            let desktop_file_path = format!("{}\\{}", path, file);
            if Path::new(&desktop_file_path).exists() {
                return true;
            }
        }
        false
    } else {
        let home_dir = std::env::var("HOME").unwrap();
        let paths = [
            "/usr/share/applications",
            "/usr/local/share/applications",
            &format!("{}/.local/share/applications", home_dir),
            "/run/current-system/sw/share/applications",
            &format!("{}/.nix-profile/share/applications", home_dir),
        ];

        for path in paths.iter() {
            let desktop_file_path = format!("{}/{}", path, file);
            if Path::new(&desktop_file_path).exists() {
                return true;
            }
        }
        false
    }
}

pub fn is_app_running(app: &str) -> bool {
    let app_name = app.strip_suffix(".desktop").unwrap_or(app);
    let output = if cfg!(target_os = "windows") {
        Command::new("tasklist")
            .arg("/FI")
            .arg(format!("IMAGENAME eq {}", app_name))
            .output()
            .expect("Failed to execute tasklist")
    } else {
        Command::new("pgrep")
            .arg("-f")
            .arg(app_name)
            .output()
            .expect("Failed to execute pgrep")
    };

    !output.stdout.is_empty()
}