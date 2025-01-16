// src/config.rs

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_yaml;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub environments: HashMap<String, Vec<String>>,
}

impl Config {
    pub fn new() -> Self {
        Config {
            environments: HashMap::new(),
        }
    }
}

pub fn generate_config(config_path: &PathBuf) -> io::Result<()> {
    if config_path.exists() {
        println!("Configuration file already exists at: {}", config_path.display());
        println!("Use 'clovis config' to edit the existing configuration.");
        return Ok(());
    }

    let example_config = Config {
        environments: [
            (
                "personal".to_string(),
                vec![
                    "firefox".to_string(),
                    "thunderbird".to_string(),
                    "gedit".to_string(),
                ],
            ),
            (
                "work".to_string(),
                vec![
                    "chrome".to_string(),
                    "slack".to_string(),
                    "code".to_string(),
                ],
            ),
        ]
        .iter()
        .cloned()
        .collect(),
    };

    save_config(config_path, &example_config)?;
    println!("Example configuration generated successfully at: {}", config_path.display());
    Ok(())
}

pub fn load_config(path: &PathBuf) -> io::Result<Config> {
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: Config = serde_yaml::from_str(&contents).map_err(|e| {
        error!("Failed to parse config file: {}", e);
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;
    info!("Config loaded successfully");
    Ok(config)
}

pub fn save_config(path: &PathBuf, config: &Config) -> io::Result<()> {
    let mut file = File::create(path)?;
    let contents = serde_yaml::to_string(config).map_err(|e| {
        error!("Failed to serialize config: {}", e);
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;
    file.write_all(contents.as_bytes())?;
    info!("Config saved successfully");
    Ok(())
}

pub fn show_config(config: &Config) {
    for (env, apps) in &config.environments {
        println!("{}:", env);
        for app in apps {
            println!("  - {}", app);
        }
    }
}

pub fn validate_config(config: &Config) {
    let mut all_valid = true;
    for (env, apps) in &config.environments {
        for app in apps {
            if app.ends_with(".desktop") {
                if !crate::platform::is_desktop_file_available(app) {
                    println!("Warning: Application '{}' in environment '{}' is not installed or not in PATH.", app, env);
                    all_valid = false;
                }
            } else if !crate::platform::is_command_available(app) {
                println!("Warning: Application '{}' in environment '{}' is not installed or not in PATH.", app, env);
                all_valid = false;
            }
        }
    }
    if all_valid {
        println!("All applications are properly installed.");
    }
}
