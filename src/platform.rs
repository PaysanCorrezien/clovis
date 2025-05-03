// src/platform.rs
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::{
    env,
    path::{PathBuf},
};

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
fn get_windows_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let user_profile = env::var("USERPROFILE").unwrap_or_default();
    
    // Static locations
    let static_paths = vec![
        PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs"),
        PathBuf::from(r"C:\Users\Public\Desktop"),
        PathBuf::from(format!(r"{user_profile}\Desktop")),
        PathBuf::from(format!(r"{user_profile}\AppData\Roaming\Microsoft\Windows\Start Menu\Programs")),
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
        let windows_apps = Path::new(&pf).join("WindowsApps");
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
        let search_paths = get_windows_search_paths();

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
        use std::ffi::OsStr;

        let mut candidates = Vec::<PathBuf>::new();

        // 1. "desktop-file" launcher (XDG)
        let desktop_name = if app.ends_with(".desktop") {
            app.to_owned()
        } else {
            format!("{}.desktop", app)
        };

        // 2. raw executable name (as given)
        let exec_name = app.to_owned();

        // ---------- build search directories ----------
        let home_dir = env::var("HOME").unwrap_or_default();

        // XDG-standard launcher locations
        candidates.extend([
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
            PathBuf::from(format!("{home_dir}/.local/share/applications")),
            PathBuf::from("/run/current-system/sw/share/applications"),
            PathBuf::from(format!("{home_dir}/.nix-profile/share/applications")),
        ]);

        // Directories in $PATH (split on ':')
        if let Ok(path_var) = env::var("PATH") {
            candidates.extend(path_var.split(':').map(PathBuf::from));
        }

        // ---------- search ----------
        for dir in &candidates {
            for name in [&desktop_name, &exec_name] {
                let full = dir.join(name);
                // On Unix we treat both files that *exist* and files that are
                // *executable* in $PATH the same; `exists()` is enough here.
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

pub fn find_available_apps() -> Vec<String> {
    find_available_apps_with_paths()
        .into_iter()
        .map(|app| app.name)
        .collect()
}

pub fn find_available_apps_with_paths() -> Vec<AppInfo> {
    let mut apps = Vec::new();

    if cfg!(target_os = "windows") {
        let search_paths = get_windows_search_paths();

        for path in search_paths {
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.ends_with(".lnk") || file_name.ends_with(".exe") {
                            let full_path = path.join(file_name);
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
    } else {
        let home_dir = std::env::var("HOME").unwrap();
        let paths = [
            "/usr/share/applications",
            "/usr/local/share/applications",
            &format!("{}/.local/share/applications", home_dir),
            "/run/current-system/sw/share/applications",
            &format!("{}/.nix-profile/share/applications", home_dir),
        ];

        for path_str in paths.iter() {
            let path = PathBuf::from(path_str);
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.ends_with(".desktop") {
                            let full_path = path.join(file_name);
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

    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps.dedup_by(|a, b| a.name == b.name);
    apps
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
        AppListFormat::Raw => {
            apps.iter()
                .map(|app| format!("{}", app.name))
                .collect::<Vec<_>>()
                .join("\n")
        }
        AppListFormat::Fzf => {
            apps.iter()
                .map(|app| format!("{}\t{}", app.name, app.path.display()))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

// TODO: Implement more than this for win ( the one that are on taskbar -> startup)
pub fn create_desktop_entry(env: &str) -> std::io::Result<()> {
    if cfg!(target_os = "windows") {
        create_windows_desktop_entry(env)
    } else {
        create_linux_desktop_entry(env)
    }
}

fn create_windows_desktop_entry(env: &str) -> std::io::Result<()> {
    let desktop_path = format!(
        "{}\\Desktop\\Clovis {}.lnk",
        std::env::var("USERPROFILE").unwrap_or_default(),
        env
    );

    let current_exe = std::env::current_exe()?;
    let target_path = current_exe.to_str().unwrap_or("clovis.exe");

    let icon_path = std::env::current_dir()?
        .join(".assets")
        .join("icon.ico")
        .to_str()
        .unwrap_or("")
        .to_string();

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
    let status = Command::new("powershell")
        .arg("-Command")
        .arg(&ps_script)
        .status()?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Failed to create shortcut",
        ));
    }

    Ok(())
}

fn create_linux_desktop_entry(env: &str) -> std::io::Result<()> {
    let desktop_path = format!(
        "{}/Desktop/clovis-{}.desktop",
        std::env::var("HOME").unwrap_or_default(),
        env
    );

    let icon_path = std::env::current_dir()?
        .join(".assets")
        .join("icon.ico")
        .to_str()
        .unwrap()
        .to_string();

    let desktop_entry = r#"
    [Desktop Entry]
    Version=1.0
    Name=Clovis {}
    Exec=clovis launch {}
    Icon={}
    Type=Application
    Terminal=false
    "#
    .replace("{}", env)
    .replace("{}", env)
    .replace("{}", &icon_path);

    std::fs::write(desktop_path, desktop_entry)?;
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
