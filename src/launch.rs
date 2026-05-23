use crate::config::Config;
use crate::discovery::{find_app_by_name, AppInfo};
use crate::platform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

const LAUNCH_PLAN_CACHE_FILE: &str = "launch-plans-v1.json";
const FAST_LAUNCH_CACHE_DIR: &str = "fast-launch-v1";

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOptions {
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub profile: String,
    pub apps: Vec<LaunchPlanItem>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchPlanItem {
    pub name: String,
    pub launch_path: PathBuf,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedLaunchPlans {
    config_path: PathBuf,
    config_len: u64,
    config_modified_ms: u128,
    profiles: HashMap<String, LaunchPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileLaunchReport {
    pub profile: String,
    pub milestone: String,
    pub total_dispatch_ms: f64,
    pub results: Vec<AppLaunchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLaunchResult {
    pub name: String,
    pub launch_path: Option<PathBuf>,
    pub skipped: bool,
    pub success: bool,
    pub dispatch_ms: f64,
    pub error: Option<String>,
}

pub fn build_launch_plan(
    config: &Config,
    profile: &str,
    discovered_apps: &[AppInfo],
) -> Result<LaunchPlan, String> {
    let Some(app_names) = config.environments.get(profile) else {
        return Err(format!("Profile '{profile}' not found"));
    };

    let mut apps = Vec::new();
    let mut missing = Vec::new();
    for app_name in app_names {
        if let Some(path) = explicit_launch_path(app_name) {
            apps.push(LaunchPlanItem {
                name: explicit_launch_name(app_name, &path),
                launch_path: path,
                source: "explicit_path".to_string(),
            });
        } else if let Some(app) = find_app_by_name(app_name, discovered_apps) {
            apps.push(LaunchPlanItem {
                name: app_name.clone(),
                launch_path: app.launch_target,
                source: app.source,
            });
        } else if let Some(path) = platform::find_app_path(app_name) {
            apps.push(LaunchPlanItem {
                name: app_name.clone(),
                launch_path: path,
                source: "legacy_path_scan".to_string(),
            });
        } else {
            missing.push(app_name.clone());
        }
    }

    Ok(LaunchPlan {
        profile: profile.to_string(),
        apps,
        missing,
    })
}

fn explicit_launch_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return None;
    }

    if is_url(trimmed) || trimmed.to_lowercase().starts_with(r"shell:appsfolder\") {
        return Some(PathBuf::from(trimmed));
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() || trimmed.contains('\\') || trimmed.contains('/') {
        return Some(path);
    }

    None
}

fn explicit_launch_name(value: &str, path: &Path) -> String {
    let trimmed = value.trim().trim_matches('"').trim_matches('\'');
    if is_url(trimmed) {
        return url_display_name(trimmed);
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| trimmed.to_string())
}

/// Recognises `scheme://…` links, `mailto:` / `tel:` links, and bare `www.` domains.
fn is_url(value: &str) -> bool {
    let v = value.trim();
    if let Some(end) = v.find("://") {
        let scheme = &v[..end];
        return scheme
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic())
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'));
    }
    let lower = v.to_lowercase();
    lower.starts_with("mailto:") || lower.starts_with("tel:") || lower.starts_with("www.")
}

fn url_display_name(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

pub fn launch_profile(
    config: &Config,
    profile: &str,
    options: LaunchOptions,
    discovered_apps: &[AppInfo],
) -> Result<ProfileLaunchReport, String> {
    let plan = build_launch_plan(config, profile, discovered_apps)?;
    Ok(launch_plan(plan, options))
}

pub fn launch_plan_cache_path() -> PathBuf {
    if let Ok(cache_dir) = std::env::var("CLOVIS_CACHE_DIR") {
        return PathBuf::from(cache_dir).join(LAUNCH_PLAN_CACHE_FILE);
    }

    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clovis")
        .join(LAUNCH_PLAN_CACHE_FILE)
}

pub fn save_launch_plan_cache(
    config_path: &Path,
    config: &Config,
    discovered_apps: &[AppInfo],
) -> io::Result<()> {
    let Some((config_len, config_modified_ms)) = config_fingerprint(config_path) else {
        return Ok(());
    };

    let mut profiles = HashMap::new();
    for profile in config.environments.keys() {
        if let Ok(plan) = build_launch_plan(config, profile, discovered_apps) {
            profiles.insert(profile.clone(), plan);
        }
    }

    let cache = CachedLaunchPlans {
        config_path: config_path.to_path_buf(),
        config_len,
        config_modified_ms,
        profiles,
    };
    let path = launch_plan_cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string(&cache)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, raw).and_then(|_| {
        save_fast_launch_cache(config_path, &cache.profiles, config_len, config_modified_ms)
    })
}

pub fn load_cached_launch_plan(config_path: &Path, profile: &str) -> io::Result<LaunchPlan> {
    let Some((config_len, config_modified_ms)) = config_fingerprint(config_path) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "config file metadata unavailable",
        ));
    };

    let raw = fs::read_to_string(launch_plan_cache_path())?;
    let cache: CachedLaunchPlans = serde_json::from_str(&raw)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    if cache.config_path != config_path
        || cache.config_len != config_len
        || cache.config_modified_ms != config_modified_ms
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "launch plan cache is stale",
        ));
    }

    cache.profiles.get(profile).cloned().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("profile '{profile}' is not in launch plan cache"),
        )
    })
}

fn config_fingerprint(config_path: &Path) -> Option<(u64, u128)> {
    let metadata = fs::metadata(config_path).ok()?;
    let modified = metadata.modified().ok()?;
    let modified_ms = modified.duration_since(UNIX_EPOCH).ok()?.as_millis();
    Some((metadata.len(), modified_ms))
}

fn save_fast_launch_cache(
    config_path: &Path,
    profiles: &HashMap<String, LaunchPlan>,
    config_len: u64,
    config_modified_ms: u128,
) -> io::Result<()> {
    let dir = fast_launch_cache_dir();
    fs::create_dir_all(&dir)?;
    for (profile, plan) in profiles {
        let path = dir.join(format!("{}.txt", profile_cache_key(profile)));
        let mut raw = String::new();
        raw.push_str(&format!("{}\n", config_path.display()));
        raw.push_str(&format!("{config_len}\n"));
        raw.push_str(&format!("{config_modified_ms}\n"));
        for item in &plan.apps {
            raw.push_str(&item.launch_path.to_string_lossy());
            raw.push('\n');
        }
        fs::write(path, raw)?;
    }
    Ok(())
}

fn fast_launch_cache_dir() -> PathBuf {
    if let Ok(cache_dir) = std::env::var("CLOVIS_CACHE_DIR") {
        return PathBuf::from(cache_dir).join(FAST_LAUNCH_CACHE_DIR);
    }

    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clovis")
        .join(FAST_LAUNCH_CACHE_DIR)
}

fn profile_cache_key(profile: &str) -> String {
    profile
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn launch_plan(plan: LaunchPlan, options: LaunchOptions) -> ProfileLaunchReport {
    let started = Instant::now();
    let mut handles = Vec::new();

    for item in plan.apps {
        handles.push(thread::spawn(move || launch_item(item, options)));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(result) => results.push(result),
            Err(_) => results.push(AppLaunchResult {
                name: "unknown".to_string(),
                launch_path: None,
                skipped: false,
                success: false,
                dispatch_ms: 0.0,
                error: Some("launch thread panicked".to_string()),
            }),
        }
    }

    for missing in plan.missing {
        results.push(AppLaunchResult {
            name: missing,
            launch_path: None,
            skipped: false,
            success: false,
            dispatch_ms: 0.0,
            error: Some("application could not be resolved to a launch target".to_string()),
        });
    }

    ProfileLaunchReport {
        profile: plan.profile,
        milestone: "process_spawned_or_shell_dispatch_returned".to_string(),
        total_dispatch_ms: duration_ms(started.elapsed()),
        results,
    }
}

fn launch_item(item: LaunchPlanItem, options: LaunchOptions) -> AppLaunchResult {
    let started = Instant::now();
    if !options.force && platform::is_app_running(&item.name) {
        return AppLaunchResult {
            name: item.name,
            launch_path: Some(item.launch_path),
            skipped: true,
            success: true,
            dispatch_ms: duration_ms(started.elapsed()),
            error: None,
        };
    }

    let result = dispatch_launch(&item.launch_path);
    AppLaunchResult {
        name: item.name,
        launch_path: Some(item.launch_path),
        skipped: false,
        success: result.is_ok(),
        dispatch_ms: duration_ms(started.elapsed()),
        error: result.err().map(|err| err.to_string()),
    }
}

fn dispatch_launch(path: &PathBuf) -> io::Result<()> {
    let path_text = path.to_string_lossy();
    if is_url(path_text.as_ref()) {
        return dispatch_url(path_text.as_ref());
    }

    #[cfg(target_os = "windows")]
    {
        let path_text = path.to_string_lossy();
        if path_text.to_lowercase().starts_with(r"shell:appsfolder\") {
            Command::new("explorer.exe")
                .arg(path_text.as_ref())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            return Ok(());
        }

        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();

        let mut command = if ext.eq_ignore_ascii_case("lnk") || is_shell_script_extension(ext) {
            let mut command = Command::new("cmd");
            command.args(["/D", "/C", "start", "", &path.to_string_lossy()]);
            command
        } else {
            Command::new(path)
        };
        hide_windows_console(&mut command);
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut command = if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("desktop"))
            .unwrap_or(false)
        {
            let mut command = Command::new("gio");
            command.args(["launch", path.to_string_lossy().as_ref()]);
            command
        } else {
            Command::new(path)
        };
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }
}

fn dispatch_url(url: &str) -> io::Result<()> {
    // Bare `www.` domains need an explicit scheme to open in a browser.
    let url = if url.contains("://")
        || url.to_lowercase().starts_with("mailto:")
        || url.to_lowercase().starts_with("tel:")
    {
        url.to_string()
    } else {
        format!("https://{url}")
    };

    #[cfg(target_os = "windows")]
    {
        // `explorer.exe <url>` hands the link to the registered protocol
        // handler (browser, mail client, …) without going through `cmd`,
        // so `&` and friends in query strings are passed through verbatim.
        Command::new("explorer.exe")
            .arg(&url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open")
            .arg(&url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn is_shell_script_extension(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "cmd" | "bat" | "ps1")
}

#[cfg(target_os = "windows")]
fn hide_windows_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn builds_launch_plan_from_discovered_apps_and_reports_missing() {
        let config = Config {
            environments: HashMap::from([(
                "work".to_string(),
                vec!["Editor".to_string(), "Missing".to_string()],
            )]),
        };
        let apps = vec![AppInfo {
            id: "editor".to_string(),
            name: "Editor".to_string(),
            path: PathBuf::from("editor.lnk"),
            launch_target: PathBuf::from("editor.exe"),
            source: "start_menu".to_string(),
            publisher: None,
            provenance: vec!["editor.lnk".to_string()],
            icon_path: None,
        }];

        let plan = build_launch_plan(&config, "work", &apps).unwrap();

        assert_eq!(plan.apps.len(), 1);
        assert_eq!(plan.missing, vec!["Missing".to_string()]);
    }

    #[test]
    fn builds_launch_plan_for_url_entries() {
        let config = Config {
            environments: HashMap::from([(
                "web".to_string(),
                vec![
                    "https://example.com/app?a=1&b=2".to_string(),
                    "www.rust-lang.org".to_string(),
                ],
            )]),
        };

        let plan = build_launch_plan(&config, "web", &[]).unwrap();

        assert_eq!(plan.apps.len(), 2);
        assert!(plan.missing.is_empty());
        assert_eq!(plan.apps[0].source, "explicit_path");
        assert_eq!(plan.apps[0].name, "example.com");
        assert_eq!(
            plan.apps[0].launch_path,
            PathBuf::from("https://example.com/app?a=1&b=2")
        );
        assert_eq!(plan.apps[1].name, "rust-lang.org");
    }

    #[test]
    fn builds_launch_plan_from_explicit_binary_path() {
        let path = if cfg!(windows) {
            r"D:\Tools\Example App\example.exe"
        } else {
            "/opt/example/bin/example"
        };
        let config = Config {
            environments: HashMap::from([("work".to_string(), vec![path.to_string()])]),
        };

        let plan = build_launch_plan(&config, "work", &[]).unwrap();

        assert_eq!(plan.apps.len(), 1);
        assert_eq!(plan.apps[0].launch_path, PathBuf::from(path));
        assert_eq!(plan.apps[0].source, "explicit_path");
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn saves_and_loads_profile_launch_plan_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CLOVIS_CACHE_DIR", dir.path());
        let config_path = dir.path().join("config.yaml");
        std::fs::write(&config_path, "environments:\n  work:\n    - Editor\n").unwrap();
        let config = Config {
            environments: HashMap::from([("work".to_string(), vec!["Editor".to_string()])]),
        };
        let apps = vec![AppInfo {
            id: "editor".to_string(),
            name: "Editor".to_string(),
            path: PathBuf::from("editor.lnk"),
            launch_target: PathBuf::from("editor.exe"),
            source: "start_menu".to_string(),
            publisher: None,
            provenance: vec!["editor.lnk".to_string()],
            icon_path: None,
        }];

        save_launch_plan_cache(&config_path, &config, &apps).unwrap();
        let plan = load_cached_launch_plan(&config_path, "work").unwrap();

        assert_eq!(plan.profile, "work");
        assert_eq!(plan.apps.len(), 1);
        assert_eq!(plan.apps[0].launch_path, PathBuf::from("editor.exe"));
        std::env::remove_var("CLOVIS_CACHE_DIR");
    }
}
