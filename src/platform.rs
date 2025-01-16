// src/platform.rs
use std::path::Path;
use std::process::{Command, Stdio};

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

pub fn find_app_path(app: &str) -> Option<String> {
    if cfg!(target_os = "windows") {
        let app_name = if !app.ends_with(".lnk") {
            format!("{}.lnk", app)
        } else {
            app.to_string()
        };

        let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
        let paths = vec![
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs".to_string(),
            "C:\\Users\\Public\\Desktop".to_string(),
            format!("{}\\Desktop", user_profile),
            format!("{}\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs", user_profile),
            format!("{}\\Start Menu\\Programs", user_profile),
        ];

        for path in paths {
            let full_path = format!("{}\\{}", path, app_name);
            if std::path::Path::new(&full_path).exists() {
                return Some(full_path);
            }
        }
        None
    } else {
        let app_name = if !app.ends_with(".desktop") {
            format!("{}.desktop", app)
        } else {
            app.to_string()
        };

        let home_dir = std::env::var("HOME").unwrap_or_default();
        let paths = [
            "/usr/share/applications",
            "/usr/local/share/applications",
            &format!("{}/.local/share/applications", home_dir),
            "/run/current-system/sw/share/applications",
            &format!("{}/.nix-profile/share/applications", home_dir),
        ];

        for path in paths.iter() {
            let full_path = format!("{}/{}", path, app_name);
            if std::path::Path::new(&full_path).exists() {
                return Some(full_path);
            }
        }
        None
    }
}

pub fn strip_platform_extension(app: &str) -> &str {
    if cfg!(target_os = "windows") {
        app.strip_suffix(".lnk").unwrap_or(app)
    } else {
        app.strip_suffix(".desktop").unwrap_or(app)
    }
}

pub fn is_desktop_file_available(app: &str) -> bool {
    find_app_path(app).is_some()
}

pub fn find_available_apps() -> Vec<String> {
    let mut apps = Vec::new();

    if cfg!(target_os = "windows") {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
        let paths = vec![
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs".to_string(),
            "C:\\Users\\Public\\Desktop".to_string(),
            format!("{}\\Desktop", user_profile),
            format!(
                "{}\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs",
                user_profile
            ),
            format!("{}\\Start Menu\\Programs", user_profile),
        ];

        for path in paths.iter() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.ends_with(".lnk") {
                            apps.push(strip_platform_extension(file_name).to_string());
                        }
                    }
                }
            }
        }
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
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.ends_with(".desktop") {
                            apps.push(strip_platform_extension(file_name).to_string());
                        }
                    }
                }
            }
        }
    }

    apps.sort();
    apps.dedup();
    apps
}

// TODO: Implement more than this for win ( the one that are on taskbar -> startup)
pub fn find_startup_apps() -> Vec<String> {
    let mut apps = Vec::new();

    if cfg!(target_os = "windows") {
        let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
        let startup_paths = vec![
            format!(
                "{}\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup",
                user_profile
            ),
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\StartUp".to_string(),
        ];

        for path in startup_paths.iter() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.ends_with(".lnk") {
                            apps.push(strip_platform_extension(file_name).to_string());
                        }
                    }
                }
            }
        }
    } else {
        let home_dir = std::env::var("HOME").unwrap();
        let autostart_paths = vec![
            format!("{}/.config/autostart", home_dir),
            "/etc/xdg/autostart".to_string(),
        ];

        for path in autostart_paths.iter() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.ends_with(".desktop") {
                            apps.push(strip_platform_extension(file_name).to_string());
                        }
                    }
                }
            }
        }
    }

    apps.sort();
    apps.dedup();
    apps
}

fn get_windows_window_titles() -> Vec<String> {
    let mut titles = Vec::new();
    if let Ok(output) = Command::new("powershell")
        .arg("-Command")
        .arg("Get-Process | Where-Object {$_.MainWindowTitle -ne ''} | Select-Object MainWindowTitle | Format-Table -HideTableHeaders")
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            titles.extend(
                output_str
                    .lines()
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
            );
        }
    }
    titles
}

pub fn is_app_running(app: &str) -> bool {
    if cfg!(target_os = "windows") {
        let app_name = strip_platform_extension(app).to_lowercase();
        
        // Check window titles first
        let window_titles = get_windows_window_titles();
        if window_titles.iter().any(|title| {
            title.contains(&app_name) || app_name.contains(title)
        }) {
            return true;
        }

        // Then check processes
        if let Ok(output) = Command::new("tasklist")
            .arg("/FO")
            .arg("CSV")
            .arg("/NH")
            .output()
        {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                for line in output_str.lines() {
                    if let Some(process_name) = line.split(',').next() {
                        let process_name = process_name.trim_matches('"').to_lowercase();
                        if process_name.contains(&app_name) || app_name.contains(&process_name) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    } else {
        let app_name = strip_platform_extension(app).to_lowercase();
        
        // Check window titles using wmctrl
        if let Ok(output) = Command::new("wmctrl")
            .arg("-l")
            .output()
        {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                for line in output_str.lines() {
                    let window_title = line.to_lowercase();
                    if window_title.contains(&app_name) {
                        return true;
                    }
                }
            }
        }

        // Fallback to process check
        if let Ok(output) = Command::new("pgrep")
            .arg("-f")
            .arg(&app_name)
            .output()
        {
            if !output.stdout.is_empty() {
                return true;
            }
        }
        false
    }
}
