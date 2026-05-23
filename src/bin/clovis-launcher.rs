#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(profile_name) = args.first().cloned() else {
        return Ok(());
    };

    let mut config_path = default_config_path();
    let mut index = 1;
    while index < args.len() {
        if args[index] == "--config-path" && index + 1 < args.len() {
            config_path = PathBuf::from(&args[index + 1]);
            index += 2;
        } else {
            index += 1;
        }
    }

    match load_fast_targets(&config_path, &profile_name) {
        Ok(targets) => {
            for target in targets {
                let _ = dispatch(&target);
            }
        }
        Err(_) => {
            let _ = delegate_to_full_clovis(&config_path, &profile_name);
        }
    }
    Ok(())
}

fn load_fast_targets(config_path: &Path, profile_name: &str) -> io::Result<Vec<PathBuf>> {
    let raw = fs::read_to_string(fast_cache_path(profile_name))?;
    let mut lines = raw.lines();
    let cached_config = PathBuf::from(lines.next().unwrap_or_default());
    let cached_len = lines.next().unwrap_or_default().parse::<u64>().ok();
    let cached_modified_ms = lines.next().unwrap_or_default().parse::<u128>().ok();
    let (config_len, config_modified_ms) = config_fingerprint(config_path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config metadata unavailable"))?;

    if cached_config != config_path
        || cached_len != Some(config_len)
        || cached_modified_ms != Some(config_modified_ms)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fast launch cache is stale",
        ));
    }

    Ok(lines
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect())
}

fn dispatch(target: &Path) -> io::Result<()> {
    let text = target.to_string_lossy();
    if is_url(&text) {
        return dispatch_with_shell_handler(&text);
    }

    #[cfg(target_os = "windows")]
    {
        let lower = text.to_lowercase();
        if lower.starts_with(r"shell:appsfolder\") {
            return dispatch_with_shell_handler(&text);
        }

        let ext = target
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if matches!(ext.as_str(), "lnk" | "cmd" | "bat" | "ps1") {
            let mut command = Command::new("cmd");
            command.args(["/D", "/C", "start", "", &text]);
            hide_windows_console(&mut command);
            command
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            return Ok(());
        }
    }

    let mut command = Command::new(target);
    hide_windows_console(&mut command);
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn dispatch_with_shell_handler(target: &str) -> io::Result<()> {
    let target = if target.starts_with("www.") {
        format!("https://{target}")
    } else {
        target.to_string()
    };

    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    #[cfg(not(target_os = "windows"))]
    let program = "xdg-open";

    let mut command = Command::new(program);
    hide_windows_console(&mut command);
    command
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn default_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("clovis")
            .join("config.yaml");
    }

    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("clovis").join("config.yaml")
    }
}

fn fast_cache_path(profile_name: &str) -> PathBuf {
    fast_cache_dir().join(format!("{}.txt", profile_cache_key(profile_name)))
}

fn fast_cache_dir() -> PathBuf {
    if let Ok(cache_dir) = std::env::var("CLOVIS_CACHE_DIR") {
        return PathBuf::from(cache_dir).join("fast-launch-v1");
    }

    #[cfg(target_os = "windows")]
    {
        return std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("clovis")
            .join("fast-launch-v1");
    }

    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("clovis").join("fast-launch-v1")
    }
}

fn profile_cache_key(profile: &str) -> String {
    profile
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn config_fingerprint(config_path: &Path) -> Option<(u64, u128)> {
    let metadata = fs::metadata(config_path).ok()?;
    let modified = metadata.modified().ok()?;
    let modified_ms = modified.duration_since(UNIX_EPOCH).ok()?.as_millis();
    Some((metadata.len(), modified_ms))
}

fn is_url(value: &str) -> bool {
    if value.starts_with("www.") {
        return true;
    }
    let lower = value.to_lowercase();
    lower.starts_with("mailto:")
        || lower.starts_with("tel:")
        || value
            .find("://")
            .map(|end| {
                let scheme = &value[..end];
                scheme
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic())
                    && scheme
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'))
            })
            .unwrap_or(false)
}

fn delegate_to_full_clovis(config_path: &Path, profile_name: &str) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let full = exe
        .parent()
        .map(|parent| parent.join("clovis-cli.exe"))
        .unwrap_or_else(|| PathBuf::from("clovis-cli.exe"));

    let mut command = Command::new(full);
    command
        .arg("--config-path")
        .arg(config_path)
        .arg("launch")
        .arg(profile_name)
        .arg("--fast")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_windows_console(&mut command);
    command.spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn hide_windows_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_windows_console(_command: &mut Command) {}
