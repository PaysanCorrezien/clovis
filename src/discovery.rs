use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CACHE_FILE: &str = "installed-apps-v6.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub launch_target: PathBuf,
    pub source: String,
    pub publisher: Option<String>,
    pub provenance: Vec<String>,
    #[serde(default)]
    pub icon_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedAppList {
    pub generated_at_ms: u128,
    pub apps: Vec<AppInfo>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryReport {
    pub apps: Vec<AppInfo>,
    pub source: DiscoverySource,
    pub elapsed: Duration,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    FreshNative,
    Cache,
}

pub fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clovis")
        .join(CACHE_FILE)
}

pub fn load_cached_apps() -> io::Result<CachedAppList> {
    let raw = std::fs::read_to_string(cache_path())?;
    serde_json::from_str(&raw).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub fn save_cached_apps(apps: &[AppInfo]) -> io::Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cached = CachedAppList {
        generated_at_ms: now_ms(),
        apps: apps.to_vec(),
    };
    let raw = serde_json::to_string_pretty(&cached)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    std::fs::write(path, raw)
}

pub fn discover_installed_apps(use_cache: bool) -> DiscoveryReport {
    if use_cache {
        let started = Instant::now();
        if let Ok(cached) = load_cached_apps() {
            return DiscoveryReport {
                apps: cached.apps,
                source: DiscoverySource::Cache,
                elapsed: started.elapsed(),
                errors: Vec::new(),
            };
        }
    }

    let started = Instant::now();
    let (apps, errors) = discover_installed_apps_fresh();
    let elapsed = started.elapsed();
    let mut sorted = apps;
    sort_and_dedupe(&mut sorted);
    if let Err(err) = save_cached_apps(&sorted) {
        let mut errors = errors;
        errors.push(format!("failed to update discovery cache: {err}"));
        return DiscoveryReport {
            apps: sorted,
            source: DiscoverySource::FreshNative,
            elapsed,
            errors,
        };
    }

    DiscoveryReport {
        apps: sorted,
        source: DiscoverySource::FreshNative,
        elapsed,
        errors,
    }
}

pub fn discover_installed_apps_fresh() -> (Vec<AppInfo>, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::discover_windows_apps()
    }

    #[cfg(not(target_os = "windows"))]
    {
        discover_unix_apps()
    }
}

pub fn find_app_by_name(name: &str, apps: &[AppInfo]) -> Option<AppInfo> {
    let normalized = strip_known_extension(name).to_lowercase();
    apps.iter()
        .find(|app| {
            app.name.eq_ignore_ascii_case(&normalized)
                || strip_known_extension(&app.name).eq_ignore_ascii_case(&normalized)
        })
        .cloned()
        .or_else(|| {
            apps.iter()
                .find(|app| app.name.to_lowercase().contains(&normalized))
                .cloned()
        })
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn stable_id(source: &str, path: &Path) -> String {
    format!("{}:{}", source, path.to_string_lossy().to_lowercase())
}

fn strip_known_extension(name: &str) -> String {
    let trimmed = name.trim();
    for ext in [".lnk", ".exe", ".desktop"] {
        if let Some(value) = trimmed.strip_suffix(ext) {
            return value.to_string();
        }
    }
    trimmed.to_string()
}

fn sort_and_dedupe(apps: &mut Vec<AppInfo>) {
    apps.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| source_priority(&left.source).cmp(&source_priority(&right.source)))
            .then_with(|| left.launch_target.cmp(&right.launch_target))
    });

    let mut by_target: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<AppInfo> = Vec::new();
    for app in apps.drain(..) {
        let key = if matches!(
            app.source.as_str(),
            "apps_folder" | "start_menu" | "app_alias" | "path_command"
        ) {
            format!("name:{}", app.name.to_lowercase())
        } else {
            app.launch_target.to_string_lossy().to_lowercase()
        };
        if let Some(index) = by_target.get(&key).copied() {
            out[index].provenance.extend(app.provenance);
            if out[index].icon_path.is_none() {
                out[index].icon_path = app.icon_path;
            }
            continue;
        }
        by_target.insert(key, out.len());
        out.push(app);
    }
    *apps = out;
}

fn source_priority(source: &str) -> u8 {
    match source {
        "apps_folder" => 0,
        "start_menu" => 1,
        "app_alias" => 2,
        "path_command" => 3,
        _ => 9,
    }
}

#[cfg(not(target_os = "windows"))]
fn discover_unix_apps() -> (Vec<AppInfo>, Vec<String>) {
    let mut apps = Vec::new();
    let mut errors = Vec::new();
    let mut roots = Vec::new();
    let home_dir = env::var("HOME").unwrap_or_default();

    if let Ok(xdg_data_dirs) = env::var("XDG_DATA_DIRS") {
        for dir in xdg_data_dirs.split(':').filter(|dir| !dir.is_empty()) {
            roots.push(PathBuf::from(dir).join("applications"));
        }
    } else {
        roots.push(PathBuf::from("/usr/share/applications"));
        roots.push(PathBuf::from("/usr/local/share/applications"));
    }
    roots.push(PathBuf::from(format!(
        "{home_dir}/.local/share/applications"
    )));
    roots.push(PathBuf::from("/run/current-system/sw/share/applications"));
    roots.push(PathBuf::from(format!(
        "{home_dir}/.nix-profile/share/applications"
    )));

    let mut seen_roots = std::collections::HashSet::new();
    roots.retain(|root| seen_roots.insert(root.clone()));

    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("desktop"))
                .unwrap_or(false)
            {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let launch_target = extract_desktop_exec(&path).unwrap_or_else(|| path.clone());
            apps.push(AppInfo {
                id: stable_id("desktop", &path),
                name: stem.to_string(),
                path: path.clone(),
                launch_target,
                source: "desktop".to_string(),
                publisher: None,
                provenance: vec![path.display().to_string()],
                icon_path: None,
            });
        }
    }

    if apps.is_empty() {
        errors.push("no desktop application entries found".to_string());
    }
    (apps, errors)
}

#[cfg(not(target_os = "windows"))]
fn extract_desktop_exec(path: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let Some(exec) = line.strip_prefix("Exec=") else {
            continue;
        };
        let executable = exec
            .split_whitespace()
            .next()
            .map(|value| value.trim_matches('"').trim_matches('\''))?;
        return Some(PathBuf::from(executable));
    }
    None
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use serde::Deserialize;
    use std::fs;
    use std::hash::{Hash, Hasher};
    use std::os::windows::ffi::OsStrExt;
    use std::process::Command;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};

    pub fn discover_windows_apps() -> (Vec<AppInfo>, Vec<String>) {
        let mut apps = Vec::new();
        let mut errors = Vec::new();

        apps.extend(collect_apps_folder_entries(&mut errors));

        for lnk in collect_lnk_paths() {
            let Some(stem) = file_stem_string(&lnk) else {
                continue;
            };
            let stem_lower = stem.to_lowercase();
            if matches!(
                stem_lower.as_str(),
                "uninstall" | "readme" | "release notes" | "help" | "documentation"
            ) || stem_lower.starts_with("uninstall ")
            {
                continue;
            }

            if !lnk.exists() {
                errors.push(format!("shortcut missing: {}", lnk.display()));
                continue;
            }

            apps.push(AppInfo {
                id: stable_id("start_menu", &lnk),
                name: stem,
                path: lnk.clone(),
                launch_target: lnk.clone(),
                source: "start_menu".to_string(),
                publisher: None,
                provenance: vec![lnk.display().to_string()],
                icon_path: None,
            });
        }

        apps.extend(collect_app_aliases());
        apps.extend(collect_path_commands());
        attach_icon_paths(&mut apps, &mut errors);

        (apps, errors)
    }

    #[derive(Debug, Deserialize)]
    struct AppsFolderEntry {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "AppId")]
        app_id: String,
    }

    fn collect_apps_folder_entries(errors: &mut Vec<String>) -> Vec<AppInfo> {
        let script = r#"
$ErrorActionPreference = 'Stop'
$shell = New-Object -ComObject Shell.Application
$folder = $shell.Namespace('shell:AppsFolder')
@($folder.Items() | ForEach-Object {
  [pscustomobject]@{
    Name = $_.Name
    AppId = $_.Path
  }
}) | ConvertTo-Json -Compress
"#;

        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .output();

        let Ok(output) = output else {
            errors.push("failed to enumerate Windows AppsFolder".to_string());
            return Vec::new();
        };

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            errors.push(if err.is_empty() {
                "Windows AppsFolder enumeration failed".to_string()
            } else {
                format!("Windows AppsFolder enumeration failed: {err}")
            });
            return Vec::new();
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let entries: Vec<AppsFolderEntry> = match serde_json::from_str(&raw) {
            Ok(entries) => entries,
            Err(err) => {
                errors.push(format!("failed to parse Windows AppsFolder output: {err}"));
                return Vec::new();
            }
        };

        entries
            .into_iter()
            .filter(|entry| !entry.name.trim().is_empty() && !entry.app_id.trim().is_empty())
            .map(|entry| {
                let target = PathBuf::from(format!(r"shell:AppsFolder\{}", entry.app_id));
                AppInfo {
                    id: stable_id("apps_folder", Path::new(&entry.app_id)),
                    name: entry.name,
                    path: target.clone(),
                    launch_target: target,
                    source: "apps_folder".to_string(),
                    publisher: None,
                    provenance: vec![format!("shell:AppsFolder\\{}", entry.app_id)],
                    icon_path: None,
                }
            })
            .collect()
    }

    fn start_menu_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Ok(program_data) = env::var("ProgramData") {
            roots.push(PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
        if let Ok(appdata) = env::var("APPDATA") {
            roots.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
        roots
    }

    fn collect_lnk_paths() -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in start_menu_roots() {
            walk_dir(&root, &mut out);
        }
        out
    }

    fn app_alias_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            roots.push(PathBuf::from(local_app_data).join(r"Microsoft\WindowsApps"));
        }
        roots
    }

    fn collect_app_aliases() -> Vec<AppInfo> {
        let mut apps = Vec::new();
        for root in app_alias_roots() {
            let Ok(entries) = fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("exe"))
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(stem) = file_stem_string(&path) else {
                    continue;
                };
                if should_skip_alias(&stem) {
                    continue;
                }

                apps.push(AppInfo {
                    id: stable_id("app_alias", &path),
                    name: alias_display_name(&stem),
                    path: path.clone(),
                    launch_target: path.clone(),
                    source: "app_alias".to_string(),
                    publisher: None,
                    provenance: vec![path.display().to_string()],
                    icon_path: None,
                });
            }
        }
        apps
    }

    fn collect_path_commands() -> Vec<AppInfo> {
        let mut apps = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();
        let mut seen_names = std::collections::HashSet::new();
        let pathexts = executable_extensions();

        let Ok(path_value) = env::var("PATH") else {
            return apps;
        };

        for root in env::split_paths(&path_value) {
            if !is_user_command_root(&root) || !seen_paths.insert(normalized_path_key(&root)) {
                continue;
            }

            let Ok(entries) = fs::read_dir(&root) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || !is_path_executable(&path, &pathexts) {
                    continue;
                }

                let Some(stem) = file_stem_string(&path) else {
                    continue;
                };
                if should_skip_path_command(&stem) {
                    continue;
                }

                let display_name = command_display_name(&stem);
                if !seen_names.insert(display_name.to_lowercase()) {
                    continue;
                }

                apps.push(AppInfo {
                    id: stable_id("path_command", &path),
                    name: display_name,
                    path: path.clone(),
                    launch_target: path.clone(),
                    source: "path_command".to_string(),
                    publisher: None,
                    provenance: vec![path.display().to_string()],
                    icon_path: None,
                });
            }
        }

        apps
    }

    fn executable_extensions() -> std::collections::HashSet<String> {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD;.PS1".to_string())
            .split(';')
            .map(|ext| ext.trim().trim_start_matches('.').to_lowercase())
            .filter(|ext| matches!(ext.as_str(), "exe" | "cmd" | "bat" | "com" | "ps1"))
            .collect()
    }

    fn is_path_executable(path: &Path, pathexts: &std::collections::HashSet<String>) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| pathexts.contains(&ext.to_lowercase()))
            .unwrap_or(false)
    }

    fn is_user_command_root(root: &Path) -> bool {
        let key = normalized_path_key(root);
        if key.contains(r"\.codex\tmp\")
            || key.contains(r"\node_modules\")
            || key.contains(r"\vendor\")
        {
            return false;
        }

        let mut allowed = Vec::new();
        if let Ok(appdata) = env::var("APPDATA") {
            allowed.push(PathBuf::from(appdata).join("npm"));
        }
        if let Ok(home) = env::var("USERPROFILE") {
            allowed.push(PathBuf::from(home).join(".local").join("bin"));
        }

        allowed
            .iter()
            .map(|path| normalized_path_key(path))
            .any(|allowed| key == allowed)
    }

    fn should_skip_path_command(stem: &str) -> bool {
        let lower = stem.to_lowercase();
        lower.starts_with('_')
            || matches!(
                lower.as_str(),
                "activate"
                    | "activate.bat"
                    | "deactivate"
                    | "pip"
                    | "pip3"
                    | "python"
                    | "python3"
                    | "pythonw"
                    | "wsl"
                    | "wslconfig"
            )
    }

    fn normalized_path_key(path: &Path) -> String {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    }

    fn command_display_name(stem: &str) -> String {
        match stem.to_lowercase().as_str() {
            "codex" => "Codex".to_string(),
            "claude" => "Claude".to_string(),
            "gh" => "GitHub CLI".to_string(),
            "pwsh" => "PowerShell 7".to_string(),
            other => other
                .split(['-', '_', '.'])
                .filter(|part| !part.is_empty())
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    fn attach_icon_paths(apps: &mut [AppInfo], errors: &mut Vec<String>) {
        for app in apps {
            let Some(path) = icon_cache_path(app) else {
                continue;
            };
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    if let Err(err) = fs::create_dir_all(parent) {
                        errors.push(format!("failed to create icon cache: {err}"));
                        continue;
                    }
                }
                if extract_icon_png(&app.launch_target, &path).is_err() {
                    continue;
                }
            }
            app.icon_path = Some(path);
        }
    }

    fn icon_cache_path(app: &AppInfo) -> Option<PathBuf> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        app.id.hash(&mut hasher);
        let hash = hasher.finish();
        let cache = dirs::cache_dir()?.join("clovis").join("app-icons");
        Some(cache.join(format!("{hash:016x}.png")))
    }

    fn extract_icon_png(source: &Path, output: &Path) -> io::Result<()> {
        const ICON_SIZE: i32 = 48;

        let is_apps_folder = source
            .to_string_lossy()
            .to_lowercase()
            .starts_with(r"shell:appsfolder\");
        let icon_source = if is_apps_folder {
            resolve_shell_icon_source(source).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no filesystem icon source for {}", source.display()),
                )
            })?
        } else {
            source.to_path_buf()
        };
        let mut wide_path = icon_source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();

        let mut file_info = SHFILEINFOW::default();
        let result = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide_path.as_mut_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut file_info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        if result == 0 || file_info.hIcon.0 == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no shell icon for {}", icon_source.display()),
            ));
        }

        let mut bits = std::ptr::null_mut();
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ICON_SIZE,
                biHeight: -ICON_SIZE,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let screen_dc = unsafe { GetDC(HWND(0)) };
        if screen_dc.0 == 0 {
            unsafe {
                let _ = DestroyIcon(file_info.hIcon);
            }
            return Err(io::Error::last_os_error());
        }

        let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
        if memory_dc.0 == 0 {
            unsafe {
                let _ = ReleaseDC(HWND(0), screen_dc);
                let _ = DestroyIcon(file_info.hIcon);
            }
            return Err(io::Error::last_os_error());
        }

        let bitmap = unsafe {
            CreateDIBSection(memory_dc, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
        }
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

        let old_object = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };
        let draw_result = unsafe {
            DrawIconEx(
                memory_dc,
                0,
                0,
                file_info.hIcon,
                ICON_SIZE,
                ICON_SIZE,
                0,
                None,
                DI_NORMAL,
            )
        };

        if let Err(err) = draw_result {
            cleanup_gdi(screen_dc, memory_dc, bitmap, old_object, file_info.hIcon);
            return Err(io::Error::new(io::ErrorKind::Other, err.to_string()));
        }

        let byte_len = (ICON_SIZE * ICON_SIZE * 4) as usize;
        let bgra = unsafe { std::slice::from_raw_parts(bits as *const u8, byte_len) };
        let mut rgba = Vec::with_capacity(byte_len);
        for px in bgra.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }

        cleanup_gdi(screen_dc, memory_dc, bitmap, old_object, file_info.hIcon);

        let image = image::RgbaImage::from_raw(ICON_SIZE as u32, ICON_SIZE as u32, rgba)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid icon pixels"))?;
        image
            .save(output)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    }

    fn cleanup_gdi(
        screen_dc: windows::Win32::Graphics::Gdi::HDC,
        memory_dc: windows::Win32::Graphics::Gdi::HDC,
        bitmap: HBITMAP,
        old_object: HGDIOBJ,
        icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    ) {
        unsafe {
            if old_object.0 != 0 {
                let _ = SelectObject(memory_dc, old_object);
            }
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(HWND(0), screen_dc);
            let _ = DestroyIcon(icon);
        }
    }

    fn resolve_shell_icon_source(source: &Path) -> Option<PathBuf> {
        let text = source.to_string_lossy();
        let rest = text.strip_prefix(r"shell:AppsFolder\")?;

        if rest.len() > 2 && rest.as_bytes().get(1) == Some(&b':') {
            return Some(PathBuf::from(rest));
        }

        let (known_folder, relative) = rest.split_once('\\')?;
        let base = match known_folder.to_uppercase().as_str() {
            "{6D809377-6AF0-444B-8957-A3773F02200E}" => env::var("ProgramFiles").ok()?,
            "{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}" => env::var("ProgramFiles(x86)").ok()?,
            "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}" => {
                let windows = env::var("WINDIR")
                    .or_else(|_| env::var("SystemRoot"))
                    .ok()?;
                PathBuf::from(windows)
                    .join("System32")
                    .display()
                    .to_string()
            }
            _ => return None,
        };

        Some(PathBuf::from(base).join(relative))
    }

    fn should_skip_alias(stem: &str) -> bool {
        let lower = stem.to_lowercase();
        matches!(
            lower.as_str(),
            "python" | "python3" | "pip" | "pip3" | "wsl" | "wslconfig"
        )
    }

    fn alias_display_name(stem: &str) -> String {
        match stem.to_lowercase().as_str() {
            "chatgpt" => "ChatGPT".to_string(),
            "ms-teams" => "Microsoft Teams".to_string(),
            "microsoftstore" => "Microsoft Store".to_string(),
            other => other
                .split(['-', '_'])
                .filter(|part| !part.is_empty())
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => {
                            format!("{}{}", first.to_uppercase(), chars.as_str())
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, out);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("lnk"))
                .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }

    fn file_stem_string(path: &Path) -> Option<String> {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_by_launch_target_and_keeps_provenance() {
        let target = PathBuf::from("/bin/app");
        let mut apps = vec![
            AppInfo {
                id: "a".into(),
                name: "App".into(),
                path: PathBuf::from("/one/app.desktop"),
                launch_target: target.clone(),
                source: "desktop".into(),
                publisher: None,
                provenance: vec!["one".into()],
                icon_path: None,
            },
            AppInfo {
                id: "b".into(),
                name: "App Copy".into(),
                path: PathBuf::from("/two/app.desktop"),
                launch_target: target,
                source: "desktop".into(),
                publisher: None,
                provenance: vec!["two".into()],
                icon_path: None,
            },
        ];

        sort_and_dedupe(&mut apps);

        assert_eq!(apps.len(), 1);
        assert_eq!(
            apps[0].provenance,
            vec!["one".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn keeps_apps_folder_target_and_merges_shortcut_icon_for_duplicate_names() {
        let mut apps = vec![
            AppInfo {
                id: "shortcut".into(),
                name: "Example".into(),
                path: PathBuf::from("Example.lnk"),
                launch_target: PathBuf::from("Example.lnk"),
                source: "start_menu".into(),
                publisher: None,
                provenance: vec!["shortcut".into()],
                icon_path: Some(PathBuf::from("example.png")),
            },
            AppInfo {
                id: "apps-folder".into(),
                name: "Example".into(),
                path: PathBuf::from(r"shell:AppsFolder\Example.App!App"),
                launch_target: PathBuf::from(r"shell:AppsFolder\Example.App!App"),
                source: "apps_folder".into(),
                publisher: None,
                provenance: vec!["apps-folder".into()],
                icon_path: None,
            },
        ];

        sort_and_dedupe(&mut apps);

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].source, "apps_folder");
        assert_eq!(
            apps[0].launch_target,
            PathBuf::from(r"shell:AppsFolder\Example.App!App")
        );
        assert_eq!(apps[0].icon_path, Some(PathBuf::from("example.png")));
    }
}
