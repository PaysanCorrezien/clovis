use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};

use clap::{CommandFactory, Parser, Subcommand};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_yaml;
use simple_logger::SimpleLogger;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    environments: HashMap<String, Vec<String>>,
}

#[derive(Parser)]
#[clap(
    name = "Clovis App Launcher",
    version = "0.1",
    about = "Launches applications based on environment configurations"
)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[clap(about = "Shows the current configuration")]
    Show,

    #[clap(about = "Launches all apps in the specified environment")]
    Launch {
        #[clap(help = "The name of the environment to launch")]
        env: Option<String>,
        #[clap(
            long,
            help = "Force launch applications even if they are already running"
        )]
        force: bool,
    },

    #[clap(about = "Validates the configuration to ensure all apps are installed")]
    Validate,

    #[clap(about = "Edits the configuration for a specific environment")]
    Edit {
        #[clap(help = "The name of the environment to edit")]
        env: String,
        #[clap(help = "Action to perform: add or remove")]
        action: String,
        #[clap(help = "The name of the application to add or remove")]
        app: String,
    },

    #[clap(about = "Opens the configuration file in the default editor")]
    Config,
}

fn main() -> io::Result<()> {
    SimpleLogger::new().init().unwrap();
    info!("Starting application");

    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clovis");
    let config_path = config_dir.join("config.yaml");

    let mut config = load_config(&config_path).unwrap_or_else(|_| {
        info!("Creating new config as loading failed");
        Config {
            environments: HashMap::new(),
        }
    });

    let cli = Cli::parse();

    match &cli.command {
        Commands::Show => show_config(&config),
        Commands::Launch { env, force } => {
            handle_launch_command(&config, env, *force)?;
        }
        Commands::Validate => validate_config(&config),
        Commands::Edit { env, action, app } => {
            if handle_edit_command(&mut config, env, action, app)? {
                save_config(&config_path, &config)?;
            } else {
                info!("No changes made to the config");
            }
        }
        Commands::Config => open_config_in_editor(&config_path)?,
    }

    Ok(())
}

fn handle_launch_command(config: &Config, env: &Option<String>, force: bool) -> io::Result<()> {
    if env.is_none() || std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        print_launch_help_and_available_environments(&config)?;
    } else {
        launch_apps(&config, env.as_deref().unwrap(), force)?;
    }
    Ok(())
}

fn handle_edit_command(
    config: &mut Config,
    env: &str,
    action: &str,
    app: &str,
) -> io::Result<bool> {
    if !config.environments.contains_key(env) {
        error!("Environment '{}' does not exist.", env);
        return Ok(false);
    }

    let app_available = if app.ends_with(".desktop") {
        is_desktop_file_available(app)
    } else {
        is_command_available(app)
    };

    if !app_available {
        println!(
            "Warning: Application '{}' is not installed or not in PATH.",
            app
        );
    }

    match action {
        "add" => {
            let apps = config
                .environments
                .entry(env.to_string())
                .or_insert_with(Vec::new);
            if apps.contains(&app.to_string()) {
                error!("Application '{}' is already in environment '{}'", app, env);
                return Ok(false);
            }
            apps.push(app.to_string());
            println!("Added '{}' to environment '{}'", app, env);
            info!("Added '{}' to environment '{}'", app, env);
        }
        "remove" => {
            if let Some(apps) = config.environments.get_mut(env) {
                if let Some(pos) = apps.iter().position(|x| x == app) {
                    apps.remove(pos);
                    println!("Removed '{}' from environment '{}'", app, env);
                    info!("Removed '{}' from environment '{}'", app, env);
                } else {
                    println!("App '{}' not found in environment '{}'", app, env);
                    return Ok(false);
                }
            }
        }
        _ => {
            println!("Invalid action '{}'. Use 'add' or 'remove'.", action);
            error!("Invalid action '{}'. Use 'add' or 'remove'.", action);
            return Ok(false);
        }
    }
    Ok(true)
}

fn open_config_in_editor(config_path: &PathBuf) -> io::Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = ProcessCommand::new(editor.clone())
        .arg(config_path)
        .status()?;

    if !status.success() {
        error!("Failed to open config file with editor '{}'", editor);
    }

    Ok(())
}

fn print_launch_help_and_available_environments(config: &Config) -> io::Result<()> {
    let mut cmd = Cli::command();
    let launch_cmd = cmd.find_subcommand_mut("launch").unwrap();

    // Capture the help output
    let mut help_output = Vec::new();
    launch_cmd.write_help(&mut help_output)?;

    // Print the captured help output
    io::stdout().write_all(&help_output)?;

    println!("\nAvailable environments:");
    for env in config.environments.keys() {
        println!("  - {}", env);
    }
    Ok(())
}

fn load_config(path: &PathBuf) -> io::Result<Config> {
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

fn save_config(path: &PathBuf, config: &Config) -> io::Result<()> {
    let mut file = File::create(path)?;
    let contents = serde_yaml::to_string(config).map_err(|e| {
        error!("Failed to serialize config: {}", e);
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;
    file.write_all(contents.as_bytes())?;
    info!("Config saved successfully");
    Ok(())
}

fn show_config(config: &Config) {
    for (env, apps) in &config.environments {
        println!("{}:", env);
        for app in apps {
            println!("  - {}", app);
        }
    }
}

fn launch_apps(config: &Config, env: &str, force: bool) -> io::Result<()> {
    if let Some(apps) = config.environments.get(env) {
        for app in apps {
            if !force && is_app_running(app) {
                println!("Skipping: {} (already running)", app);
                continue;
            }
            println!("Launching: {}", app);
            ProcessCommand::new("gtk-launch")
                .arg(app)
                .spawn()
                .expect("Failed to launch application");
        }
        info!("Launched apps for environment: {}", env);
    } else {
        eprintln!("Environment '{}' not found.", env);
        error!("Environment '{}' not found", env);
    }
    Ok(())
}

fn validate_config(config: &Config) {
    let mut all_valid = true;
    for (env, apps) in &config.environments {
        for app in apps {
            if app.ends_with(".desktop") {
                if !is_desktop_file_available(app) {
                    println!("Warning: Application '{}' in environment '{}' is not installed or not in PATH.", app, env);
                    all_valid = false;
                }
            } else if !is_command_available(app) {
                println!("Warning: Application '{}' in environment '{}' is not installed or not in PATH.", app, env);
                all_valid = false;
            }
        }
    }
    if all_valid {
        println!("All applications are properly installed.");
    }
}

fn is_command_available(cmd: &str) -> bool {
    ProcessCommand::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn is_desktop_file_available(file: &str) -> bool {
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
        if std::path::Path::new(&desktop_file_path).exists() {
            return true;
        }
    }
    false
}

fn is_app_running(app: &str) -> bool {
    let app_name = app.strip_suffix(".desktop").unwrap_or(app);
    let output = ProcessCommand::new("pgrep")
        .arg("-f")
        .arg(app_name)
        .output()
        .expect("Failed to execute pgrep");

    !output.stdout.is_empty()
}
