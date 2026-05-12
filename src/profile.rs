use crate::config::{load_config, save_config, Config};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub apps: Vec<String>,
}

pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clovis")
        .join("config.yaml")
}

pub fn ensure_config_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn load_or_default(path: &PathBuf) -> Config {
    load_config(path).unwrap_or_else(|_| Config {
        environments: HashMap::new(),
    })
}

pub fn load_profiles(path: &PathBuf) -> io::Result<Vec<Profile>> {
    let config = load_config(path)?;
    Ok(profiles_from_config(&config))
}

pub fn profiles_from_config(config: &Config) -> Vec<Profile> {
    let mut profiles: Vec<_> = config
        .environments
        .iter()
        .map(|(name, apps)| Profile {
            name: name.clone(),
            apps: apps.clone(),
        })
        .collect();
    profiles.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    profiles
}

pub fn profile_names(config: &Config) -> Vec<String> {
    profiles_from_config(config)
        .into_iter()
        .map(|profile| profile.name)
        .collect()
}

pub fn get_profile(config: &Config, name: &str) -> Option<Profile> {
    config.environments.get(name).map(|apps| Profile {
        name: name.to_string(),
        apps: apps.clone(),
    })
}

pub fn create_profile(config: &mut Config, name: &str) -> Result<(), String> {
    let name = clean_profile_name(name)?;
    if config.environments.contains_key(&name) {
        return Err(format!("Profile '{name}' already exists"));
    }
    config.environments.insert(name, Vec::new());
    Ok(())
}

pub fn rename_profile(config: &mut Config, old_name: &str, new_name: &str) -> Result<(), String> {
    let new_name = clean_profile_name(new_name)?;
    if old_name == new_name {
        return Ok(());
    }
    if !config.environments.contains_key(old_name) {
        return Err(format!("Profile '{old_name}' does not exist"));
    }
    if config.environments.contains_key(&new_name) {
        return Err(format!("Profile '{new_name}' already exists"));
    }
    let apps = config.environments.remove(old_name).unwrap_or_default();
    config.environments.insert(new_name, apps);
    Ok(())
}

pub fn delete_profile(config: &mut Config, name: &str) -> Result<(), String> {
    config
        .environments
        .remove(name)
        .map(|_| ())
        .ok_or_else(|| format!("Profile '{name}' does not exist"))
}

pub fn set_profile_apps(config: &mut Config, name: &str, apps: Vec<String>) -> Result<(), String> {
    if !config.environments.contains_key(name) {
        return Err(format!("Profile '{name}' does not exist"));
    }
    config
        .environments
        .insert(name.to_string(), normalize_apps(apps));
    Ok(())
}

pub fn add_profile_app(config: &mut Config, name: &str, app: &str) -> Result<(), String> {
    if !config.environments.contains_key(name) {
        return Err(format!("Profile '{name}' does not exist"));
    }
    let normalized = normalize_app(app);
    let apps = config.environments.get_mut(name).expect("profile checked");
    if apps
        .iter()
        .any(|existing| normalize_app(existing).eq_ignore_ascii_case(&normalized))
    {
        return Err(format!(
            "Application '{normalized}' is already in profile '{name}'"
        ));
    }
    apps.push(normalized);
    Ok(())
}

pub fn remove_profile_app(config: &mut Config, name: &str, app: &str) -> Result<(), String> {
    if !config.environments.contains_key(name) {
        return Err(format!("Profile '{name}' does not exist"));
    }
    let normalized = normalize_app(app);
    let apps = config.environments.get_mut(name).expect("profile checked");
    let Some(index) = apps
        .iter()
        .position(|existing| normalize_app(existing).eq_ignore_ascii_case(&normalized))
    else {
        return Err(format!(
            "Application '{normalized}' is not in profile '{name}'"
        ));
    };
    apps.remove(index);
    Ok(())
}

pub fn save_profiles(path: &PathBuf, config: &Config) -> io::Result<()> {
    ensure_config_parent(path)?;
    save_config(path, config)
}

fn clean_profile_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Profile name is required".to_string());
    }
    Ok(trimmed.to_string())
}

fn normalize_apps(apps: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for app in apps {
        let normalized = normalize_app(&app);
        if normalized.is_empty() {
            continue;
        }
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&normalized))
        {
            out.push(normalized);
        }
    }
    out
}

pub fn normalize_app(app: &str) -> String {
    let trimmed = app.trim();
    if is_path_like_app(trimmed) {
        return trimmed.trim_matches('"').trim_matches('\'').to_string();
    }

    let lower = trimmed.to_lowercase();
    for ext in [".lnk", ".exe", ".desktop"] {
        if lower.ends_with(ext) {
            let end = trimmed.len().saturating_sub(ext.len());
            return trimmed[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

/// True when the entry is an explicit launch target (a file path, an
/// `shell:AppsFolder\…` AUMID, or a URL/protocol link) rather than an app name
/// that needs to be resolved against the installed-app catalog.
pub fn is_explicit_app(app: &str) -> bool {
    is_path_like_app(app.trim())
}

fn is_path_like_app(app: &str) -> bool {
    let unquoted = app.trim_matches('"').trim_matches('\'');
    is_url_like(unquoted)
        || unquoted.contains('\\')
        || unquoted.contains('/')
        || (unquoted.len() > 2 && unquoted.as_bytes()[1] == b':')
        || unquoted.to_lowercase().starts_with("shell:appsfolder\\")
}

/// Recognises `scheme://…` links (http, https, obsidian, vscode, …),
/// `mailto:` / `tel:` style links, and bare `www.` domains.
pub fn is_url_like(value: &str) -> bool {
    let v = value.trim();
    if let Some(end) = v.find("://") {
        let scheme = &v[..end];
        return scheme.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'));
    }
    let lower = v.to_lowercase();
    lower.starts_with("mailto:") || lower.starts_with("tel:") || lower.starts_with("www.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_profiles_without_duplicates() {
        let mut config = Config {
            environments: HashMap::new(),
        };

        create_profile(&mut config, "work").unwrap();
        add_profile_app(&mut config, "work", "Code.exe").unwrap();
        assert!(add_profile_app(&mut config, "work", "code").is_err());
        add_profile_app(&mut config, "work", "Firefox").unwrap();
        remove_profile_app(&mut config, "work", "code").unwrap();

        assert_eq!(
            config.environments.get("work").unwrap(),
            &vec!["Firefox".to_string()]
        );
    }

    #[test]
    fn preserves_yaml_shape_on_read_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let mut config = Config {
            environments: HashMap::new(),
        };
        create_profile(&mut config, "personal").unwrap();
        add_profile_app(&mut config, "personal", "firefox").unwrap();

        save_profiles(&path, &config).unwrap();
        let loaded = load_config(&path).unwrap();

        assert_eq!(loaded, config);
    }

    #[test]
    fn normalize_app_preserves_explicit_paths() {
        assert_eq!(
            normalize_app(r#""D:\Tools\Example App\example.exe""#),
            r"D:\Tools\Example App\example.exe"
        );
        assert_eq!(normalize_app("Code.exe"), "Code");
    }

    #[test]
    fn recognizes_urls_and_keeps_them_verbatim() {
        assert!(is_url_like("https://example.com"));
        assert!(is_url_like("http://example.com/path?a=1&b=2"));
        assert!(is_url_like("obsidian://open?vault=notes"));
        assert!(is_url_like("mailto:user@example.com"));
        assert!(is_url_like("www.example.com"));
        assert!(!is_url_like("Visual Studio Code"));
        assert!(!is_url_like("C:\\Tools\\app.exe"));

        assert!(is_explicit_app("https://example.com"));
        assert!(is_explicit_app(r"C:\Tools\app.exe"));
        assert!(!is_explicit_app("Firefox"));

        assert_eq!(normalize_app("  https://example.com/app  "), "https://example.com/app");
    }

    #[test]
    fn adds_url_entry_to_profile() {
        let mut config = Config {
            environments: HashMap::new(),
        };
        create_profile(&mut config, "web").unwrap();
        add_profile_app(&mut config, "web", "https://example.com").unwrap();
        assert_eq!(
            config.environments.get("web").unwrap(),
            &vec!["https://example.com".to_string()]
        );
    }
}
