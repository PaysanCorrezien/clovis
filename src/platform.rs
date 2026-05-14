// src/platform.rs
use crate::icons::{IconFormat, IconResolver};
use log::warn;
use std::io;
use std::process::{Command, Stdio};
use std::{env, path::PathBuf};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

#[cfg(target_os = "windows")]
fn get_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let user_profile = env::var("USERPROFILE").unwrap_or_default();

    // Static locations
    let static_paths = vec![
        PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs"),
        PathBuf::from(r"C:\Users\Public\Desktop"),
        PathBuf::from(format!(r"{user_profile}\Desktop")),
        PathBuf::from(format!(
            r"{user_profile}\AppData\Roaming\Microsoft\Windows\Start Menu\Programs"
        )),
        PathBuf::from(format!(r"{user_profile}\Start Menu\Programs")),
    ];
    paths.extend(static_paths);

    // Program Files locations
    if let Ok(pf) = env::var("ProgramFiles") {
        paths.push(PathBuf::from(pf));
    }
    if let Ok(pf86) = env::var("ProgramFiles(x86)") {
        paths.push(PathBuf::from(pf86));
    }

    // WindowsApps container
    if let Ok(pf) = env::var("ProgramFiles") {
        let windows_apps = PathBuf::from(pf).join("WindowsApps");
        if windows_apps.exists() {
            paths.push(windows_apps);
        }
    }

    // PATH environment variable
    if let Ok(path_var) = env::var("PATH") {
        paths.extend(path_var.split(';').map(PathBuf::from));
    }

    paths
}

#[cfg(not(target_os = "windows"))]
fn get_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home_dir = env::var("HOME").unwrap_or_default();

    // XDG-standard launcher locations
    paths.extend([
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from(format!("{home_dir}/.local/share/applications")),
        PathBuf::from("/run/current-system/sw/share/applications"),
        PathBuf::from(format!("{home_dir}/.nix-profile/share/applications")),
    ]);

    // Directories in $PATH (split on ':')
    if let Ok(path_var) = env::var("PATH") {
        paths.extend(path_var.split(':').map(PathBuf::from));
    }

    paths
}

#[cfg(not(target_os = "windows"))]
fn get_application_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home_dir = env::var("HOME").unwrap_or_default();

    // Start with XDG_DATA_DIRS if set
    if let Ok(xdg_data_dirs) = env::var("XDG_DATA_DIRS") {
        for dir in xdg_data_dirs.split(':') {
            if !dir.is_empty() {
                paths.push(PathBuf::from(dir).join("applications"));
            }
        }
    } else {
        // Default XDG locations if XDG_DATA_DIRS is not set
        paths.extend([
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
        ]);
    }

    // User-local applications (XDG_DATA_HOME)
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        paths.push(PathBuf::from(xdg_data_home).join("applications"));
    } else {
        paths.push(PathBuf::from(format!(
            "{home_dir}/.local/share/applications"
        )));
    }

    // Autostart directories
    paths.extend([
        PathBuf::from("/etc/xdg/autostart"),
        PathBuf::from(format!("{home_dir}/.config/autostart")),
    ]);

    // Flatpak locations
    paths.extend([
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        PathBuf::from(format!(
            "{home_dir}/.local/share/flatpak/exports/share/applications"
        )),
    ]);

    // Snap locations
    paths.push(PathBuf::from("/var/lib/snapd/desktop/applications"));

    // Nix locations
    paths.extend([
        PathBuf::from("/run/current-system/sw/share/applications"),
        PathBuf::from(format!("{home_dir}/.nix-profile/share/applications")),
    ]);

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));

    paths
}

#[cfg(not(target_os = "windows"))]
fn scan_opt_directory() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    let opt_path = PathBuf::from("/opt");

    if let Ok(entries) = std::fs::read_dir(&opt_path) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    // Check for .desktop files in this subdirectory
                    if let Ok(subentries) = std::fs::read_dir(entry.path()) {
                        for subentry in subentries.flatten() {
                            if let Some(file_name) = subentry.file_name().to_str() {
                                if file_name.ends_with(".desktop") {
                                    let full_path = entry.path().join(file_name);
                                    if full_path.exists() {
                                        apps.push(AppInfo {
                                            name: strip_platform_extension(file_name).to_string(),
                                            path: full_path,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    apps
}

/// Try to locate an application launcher (`.lnk` on Windows, `.desktop` on Unix)
/// **or** an executable with the given base name that is directly on the file‑
/// system or reachable through the system's *PATH*.
///
/// Returns the fully‑qualified path if it exists.
pub fn find_app_path(app: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // Normalize the requested name
        let shortcut = if app.ends_with(".lnk") {
            app.to_owned()
        } else {
            format!("{}.lnk", app)
        };
        let exe_name = if app.to_ascii_lowercase().ends_with(".exe") {
            app.to_owned()
        } else {
            format!("{}.exe", app)
        };

        // Get all possible search paths
        let search_paths = get_search_paths();

        // Search through all paths
        for dir in search_paths {
            // Try launcher first, then direct exe
            for name in [&shortcut, &exe_name] {
                let full = dir.join(name);
                if full.exists() {
                    return Some(full);
                }
            }
        }
        None
    }

    #[cfg(not(target_os = "windows"))]
    {
        // 1. "desktop-file" launcher (XDG)
        let desktop_name = if app.ends_with(".desktop") {
            app.to_owned()
        } else {
            format!("{}.desktop", app)
        };

        // 2. raw executable name (as given)
        let exec_name = app.to_owned();

        // Get all possible search paths
        let search_paths = get_search_paths();

        // Search through all paths
        for dir in search_paths {
            for name in [&desktop_name, &exec_name] {
                let full = dir.join(name);
                if full.exists() {
                    return Some(full);
                }
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

#[derive(Debug, Clone, Copy)]
pub enum AppListFormat {
    Pretty,
    Raw,
    Fzf,
}

pub struct AppInfo {
    pub name: String,
    pub path: PathBuf,
}

pub fn find_available_apps_with_paths() -> Vec<AppInfo> {
    crate::discovery::discover_installed_apps(true)
        .apps
        .into_iter()
        .map(|app| AppInfo {
            name: app.name,
            path: app.path,
        })
        .collect()
}

pub fn format_app_list(apps: &[AppInfo], format: AppListFormat) -> String {
    match format {
        AppListFormat::Pretty => {
            let mut output = String::new();
            output.push_str("Name\tPath\n");
            output.push_str("----\t----\n");
            for app in apps {
                output.push_str(&format!("{}\t{}\n", app.name, app.path.display()));
            }
            output
        }
        AppListFormat::Raw => apps
            .iter()
            .map(|app| format!("{}", app.name))
            .collect::<Vec<_>>()
            .join("\n"),
        AppListFormat::Fzf => apps
            .iter()
            .map(|app| format!("{}\t{}", app.name, app.path.display()))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

// TODO: Implement more than this for win ( the one that are on taskbar -> startup)
pub fn create_desktop_entry(env: &str, icon: Option<&str>) -> std::io::Result<()> {
    if cfg!(target_os = "windows") {
        create_windows_desktop_entry(env, icon)
    } else {
        create_linux_desktop_entry(env, icon)
    }
}

fn get_default_icon_path() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| {
            p.join(".assets")
                .join("icon.ico")
                .to_str()
                .map(String::from)
        })
        .unwrap_or_else(|| String::from(""))
}

fn create_windows_desktop_entry(env: &str, icon: Option<&str>) -> std::io::Result<()> {
    let desktop_path = format!(
        "{}\\Desktop\\Clovis {}.lnk",
        std::env::var("USERPROFILE").unwrap_or_default(),
        env
    );

    let current_exe = std::env::current_exe()?;
    let target_path = current_exe.to_str().unwrap_or("clovis.exe");

    let icon_path = if let Some(icon_input) = icon {
        let resolver = IconResolver::new()?;
        let source = resolver.resolve_icon(icon_input);
        match resolver.get_icon_path(source, IconFormat::Ico) {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(e) => {
                warn!(
                    "Failed to resolve icon '{}': {}. Using default.",
                    icon_input, e
                );
                get_default_icon_path()
            }
        }
    } else {
        get_default_icon_path()
    };

    // Create PowerShell script to make a proper Windows shortcut
    let ps_script = format!(
        r#"
$WshShell = New-Object -comObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut("{}")
$Shortcut.TargetPath = "{}"
$Shortcut.Arguments = "launch {}"
$Shortcut.IconLocation = "{}"
$Shortcut.Save()
"#,
        desktop_path.replace("\\", "\\\\"),
        target_path.replace("\\", "\\\\"),
        env,
        icon_path.replace("\\", "\\\\")
    );

    // Execute the PowerShell script
    let mut command = Command::new("powershell");
    hide_windows_console(&mut command);
    let status = command.arg("-Command").arg(&ps_script).status()?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Failed to create shortcut",
        ));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn hide_windows_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

fn get_linux_applications_dir() -> io::Result<PathBuf> {
    let path = if let Ok(xdg_data_home) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg_data_home).join("applications")
    } else {
        let home = std::env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        PathBuf::from(home).join(".local/share/applications")
    };

    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn create_linux_desktop_entry(env: &str, icon: Option<&str>) -> std::io::Result<()> {
    let applications_dir = get_linux_applications_dir()?;
    let desktop_path = applications_dir.join(format!("clovis-{}.desktop", env));
    let current_exe = std::env::current_exe()?;
    let exec_path = current_exe.to_string_lossy().replace(' ', "\\ ");

    let icon_path = if let Some(icon_input) = icon {
        let resolver = IconResolver::new()?;
        let source = resolver.resolve_icon(icon_input);
        match resolver.get_icon_path(source, IconFormat::Png) {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(e) => {
                warn!(
                    "Failed to resolve icon '{}': {}. Using default.",
                    icon_input, e
                );
                get_default_icon_path()
            }
        }
    } else {
        get_default_icon_path()
    };

    let desktop_entry = format!(
        r#"[Desktop Entry]
Version=1.0
Name=Clovis {}
Exec={} launch {}
Icon={}
Type=Application
Terminal=false
"#,
        env, exec_path, env, icon_path
    );

    std::fs::write(&desktop_path, desktop_entry)?;
    Ok(())
}

pub fn is_app_running(app: &str) -> bool {
    if cfg!(target_os = "windows") {
        let app_name = strip_platform_extension(app).to_lowercase();

        // Check window titles first
        let window_titles = get_windows_window_titles();
        if window_titles
            .iter()
            .any(|title| title.contains(&app_name) || app_name.contains(title))
        {
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
        if let Ok(output) = Command::new("wmctrl").arg("-l").output() {
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
        if let Ok(output) = Command::new("pgrep").arg("-f").arg(&app_name).output() {
            if !output.stdout.is_empty() {
                return true;
            }
        }
        false
    }
}

pub fn find_startup_apps() -> Vec<String> {
    let mut apps = Vec::new();

    if cfg!(target_os = "windows") {
        // 1. Existing folder check (keep this part)
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

        // 2. Check Registry Run keys
        apps.extend(get_registry_startup_apps());

        // 3. Check Task Scheduler tasks
        apps.extend(get_task_scheduler_startup_apps());

        // 4. Get Task Manager Startup items using PowerShell
        apps.extend(get_taskmanager_startup_apps());
    } else {
        // Linux implementation (keep as is)
        // ...
    }

    apps.sort();
    apps.dedup();
    apps
}

fn get_registry_startup_apps() -> Vec<String> {
    let mut apps = Vec::new();

    // Use PowerShell to query Registry Run keys
    let registry_query = r#"
        Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' | 
        Select-Object * -Exclude PSPath,PSParentPath,PSChildName,PSProvider |
        ForEach-Object { $_.PSObject.Properties } | 
        ForEach-Object { $_.Name }
        
        Get-ItemProperty -Path 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' | 
        Select-Object * -Exclude PSPath,PSParentPath,PSChildName,PSProvider |
        ForEach-Object { $_.PSObject.Properties } | 
        ForEach-Object { $_.Name }
    "#;

    if let Ok(output) = Command::new("powershell")
        .arg("-Command")
        .arg(registry_query)
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            for line in output_str.lines() {
                let app_name = line.trim();
                if !app_name.is_empty() {
                    apps.push(app_name.to_string());
                }
            }
        }
    }

    apps
}

fn get_task_scheduler_startup_apps() -> Vec<String> {
    let mut apps = Vec::new();

    // Query tasks that run at logon
    let task_query = r#"
        Get-ScheduledTask | 
        Where-Object { $_.Triggers.LogonTrigger -ne $null -or $_.Triggers.BootTrigger -ne $null } |
        Select-Object -ExpandProperty TaskName
    "#;

    if let Ok(output) = Command::new("powershell")
        .arg("-Command")
        .arg(task_query)
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            for line in output_str.lines() {
                let task_name = line.trim();
                if !task_name.is_empty() {
                    // Extract just the application name from task path
                    if let Some(app_name) = task_name.split('\\').last() {
                        apps.push(app_name.to_string());
                    }
                }
            }
        }
    }

    apps
}

fn get_taskmanager_startup_apps() -> Vec<String> {
    let mut apps = Vec::new();

    // This PowerShell command uses WMI to get startup apps similar to Task Manager
    let ps_query = r#"
        Get-CimInstance -ClassName Win32_StartupCommand | 
        Select-Object -ExpandProperty Name
    "#;

    if let Ok(output) = Command::new("powershell")
        .arg("-Command")
        .arg(ps_query)
        .output()
    {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            for line in output_str.lines() {
                let app_name = line.trim();
                if !app_name.is_empty() {
                    apps.push(app_name.to_string());
                }
            }
        }
    }

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

struct DesktopExecInfo {
    executable: String,
    full_command: String,
    is_webapp: bool,
    is_terminal_app: bool,
}

fn extract_executable_from_desktop(desktop_path: &PathBuf) -> Option<DesktopExecInfo> {
    if let Ok(content) = std::fs::read_to_string(desktop_path) {
        let mut exec_line_opt = None;
        let mut is_terminal = false;

        for line in content.lines() {
            if let Some(exec_line) = line.strip_prefix("Exec=") {
                exec_line_opt = Some(exec_line.to_string());
            } else if let Some(terminal_value) = line.strip_prefix("Terminal=") {
                is_terminal = terminal_value.trim().eq_ignore_ascii_case("true");
            }
        }

        if let Some(exec_line) = exec_line_opt {
            // Extract the executable name (first word, before any arguments)
            let exec = exec_line.split_whitespace().next()?;
            // Remove quotes if present
            let exec = exec.trim_matches('"').trim_matches('\'');
            // Remove any path components to get just the executable name
            let exec_name = std::path::Path::new(exec).file_name()?.to_str()?;

            // Webapps (--class= or --app=) are not reliably detectable on Wayland
            // so we skip them entirely for snapshot purposes
            let is_webapp = exec_line.contains("--class=") || exec_line.contains("--app=");

            return Some(DesktopExecInfo {
                executable: exec_name.to_string(),
                full_command: exec_line.to_string(),
                is_webapp,
                is_terminal_app: is_terminal,
            });
        }
    }
    None
}

fn is_system_service(app_name: &str) -> bool {
    let system_services = [
        "at-spi-dbus-bus",
        "at-spi2-registryd",
        "gnome-keyring",
        "polkit-kde-authentication-agent",
        "polkit-gnome-authentication-agent",
        "xdg-desktop-portal",
        "xdg-permission-store",
        "org.freedesktop.",
        "org.gnome.Shell",
        "gvfsd",
        "dconf-service",
        "evolution-",
        "goa-daemon",
        "system-config-printer",
    ];

    let lower_name = app_name.to_lowercase();
    system_services
        .iter()
        .any(|service| lower_name.contains(&service.to_lowercase()))
}

pub fn get_running_apps() -> Vec<String> {
    let mut running_apps = Vec::new();
    let mut seen_executables = std::collections::HashSet::new();
    let all_apps = find_available_apps_with_paths();

    if cfg!(target_os = "windows") {
        // Get window titles
        let window_titles = get_windows_window_titles();

        // Get running processes
        let mut process_names = Vec::new();
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
                        process_names.push(process_name);
                    }
                }
            }
        }

        // Match apps with running processes/windows
        for app in all_apps {
            let app_name = app.name.to_lowercase();

            // Get exec info for deduplication and filtering
            let exec_info = if app.path.extension().and_then(|s| s.to_str()) == Some("lnk") {
                None // Windows shortcuts don't have same structure
            } else {
                extract_executable_from_desktop(&app.path)
            };

            // Skip terminal applications and webapps
            if let Some(ref exec) = exec_info {
                if exec.is_terminal_app || exec.is_webapp {
                    continue;
                }
            }

            let is_running = window_titles
                .iter()
                .any(|title| title.contains(&app_name) || app_name.contains(title))
                || process_names
                    .iter()
                    .any(|proc| proc.contains(&app_name) || app_name.contains(proc));

            if is_running {
                let exec_key = if let Some(ref exec) = exec_info {
                    exec.executable.clone()
                } else {
                    app_name.clone()
                };

                if seen_executables.insert(exec_key) {
                    running_apps.push(app.name);
                }
            }
        }
    } else {
        // Get running processes with full command lines
        let mut process_info = Vec::new();
        if let Ok(output) = Command::new("ps").arg("-eo").arg("args").output() {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                for line in output_str.lines().skip(1) {
                    // Skip header
                    process_info.push(line.to_lowercase());
                }
            }
        }

        // Match apps with running processes
        for app in all_apps {
            // Skip system services
            if is_system_service(&app.name) {
                continue;
            }

            let app_name = app.name.to_lowercase();
            let mut is_running = false;

            // Try to get the executable info from .desktop file
            let exec_info = if app.path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                extract_executable_from_desktop(&app.path)
            } else {
                None
            };

            // Skip terminal applications and webapps
            if let Some(ref exec) = exec_info {
                if exec.is_terminal_app || exec.is_webapp {
                    continue;
                }
            }

            // Check if app is running by matching executable name or full command line
            for proc in &process_info {
                let proc_parts: Vec<&str> = proc.split_whitespace().collect();
                if proc_parts.is_empty() {
                    continue;
                }

                let process_name = proc_parts[0];

                // Match against executable from .desktop file
                if let Some(ref exec) = exec_info {
                    // Regular app: match on executable name
                    let exec_lower = exec.executable.to_lowercase();
                    let full_exec_path = exec.full_command.split_whitespace().next().unwrap_or("");
                    let full_exec_path_lower = full_exec_path
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_lowercase();

                    // Check if executable name matches process name exactly
                    if process_name == exec_lower
                        || process_name.ends_with(&format!("/{}", exec_lower))
                    {
                        is_running = true;
                        break;
                    }

                    // For Electron/wrapped apps, check if exec path appears in command line
                    // e.g., for cursor: "/usr/share/cursor/" appears in "electron --app=/usr/share/cursor/resources/app"
                    if !full_exec_path_lower.is_empty() && full_exec_path_lower != exec_lower {
                        // Check if full path appears
                        if proc.contains(&full_exec_path_lower) {
                            is_running = true;
                            break;
                        }

                        // For wrapped apps (like Electron), check if the parent directory appears
                        // Only do this for app-specific directories (not common system dirs like /usr/bin)
                        if let Some(parent_dir) =
                            std::path::Path::new(&full_exec_path_lower).parent()
                        {
                            let parent_str = parent_dir.to_string_lossy().to_lowercase();
                            // Only match specific app directories (not /usr/bin, /usr/local/bin, etc.)
                            let is_app_specific_dir = parent_str.starts_with("/opt/")
                                || parent_str.starts_with("/usr/share/")
                                    && parent_str.split('/').count() > 3
                                || parent_str.starts_with("/usr/local/share/")
                                    && parent_str.split('/').count() > 4;

                            if is_app_specific_dir && proc.contains(&parent_str) {
                                is_running = true;
                                break;
                            }
                        }
                    }
                }

                // Match against app name (exact or in path)
                if process_name == app_name || process_name.ends_with(&format!("/{}", app_name)) {
                    is_running = true;
                    break;
                }
            }

            if is_running {
                // Deduplicate by executable name to avoid multiple .desktop files for same app
                let exec_key = if let Some(ref exec) = exec_info {
                    exec.executable.clone()
                } else {
                    app_name.clone()
                };

                if seen_executables.insert(exec_key) {
                    running_apps.push(app.name);
                }
            }
        }
    }

    running_apps.sort();
    running_apps.dedup();
    running_apps
}
