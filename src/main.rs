use std::collections::HashMap;
use std::io::{self};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};

use clap::{Parser, Subcommand};
use log::{error, info};
use simple_logger::SimpleLogger;

mod platform;
mod config;

use config::{Config, generate_config, load_config, save_config, show_config, validate_config};

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
    #[clap(about = "Lists all available applications")]
    List,

    #[clap(about = "Lists all applications in system startup folders")]
    StartupList,

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

    #[clap(about = "Generates a base example configuration")]
    Generate,
}

fn main() -> io::Result<()> {
    SimpleLogger::new().init().unwrap();
    info!("Starting application");

    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clovis");
    std::fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("config.yaml");

    let mut config = load_config(&config_path).unwrap_or_else(|_| {
        info!("Creating new config as loading failed");
        Config {
            environments: HashMap::new(),
        }
    });

    let cli = Cli::parse();

    match &cli.command {
        Commands::List => {
            println!("Available applications:");
            for app in platform::find_available_apps() {
                println!("  - {}", app);
            }
        }
        Commands::StartupList => {
            println!("System startup applications:");
            for app in platform::find_startup_apps() {
                println!("  - {}", app);
            }
        }
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
        Commands::Generate => {
            generate_config(&config_path)?;
        }
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

    let app_available = platform::is_desktop_file_available(app) || platform::is_command_available(app);

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
            let normalized_app = platform::strip_platform_extension(app);
            if apps.iter().any(|a| platform::strip_platform_extension(a) == normalized_app) {
                error!("Application '{}' is already in environment '{}'", normalized_app, env);
                return Ok(false);
            }
            apps.push(normalized_app.to_string());
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
    println!("\nAvailable environments:");
    for env in config.environments.keys() {
        println!("  - {}", env);
    }
    Ok(())
}

fn handle_launch_command(config: &Config, env: &Option<String>, force: bool) -> io::Result<()> {
    match env {
        Some(env_name) => {
            if !config.environments.contains_key(env_name) {
                println!("Environment '{}' not found.", env_name);
                print_launch_help_and_available_environments(config)?;
                return Ok(());
            }
            launch_apps(config, env_name, force)
        }
        None => {
            print_launch_help_and_available_environments(config)?;
            Ok(())
        }
    }
}

fn launch_apps(config: &Config, env: &str, force: bool) -> io::Result<()> {
    if let Some(apps) = config.environments.get(env) {
        for app in apps {
            if !force && platform::is_app_running(app) {
                println!("Skipping: {} (already running)", app);
                continue;
            }
            println!("Launching: {}", app);
            match platform::find_app_path(app) {
                Some(app_path) => {
                    let mut command = if cfg!(target_os = "windows") {
                        let mut cmd = ProcessCommand::new("cmd");
                        cmd.args(["/C", "start", "", &app_path]);
                        cmd
                    } else {
                        let mut cmd = ProcessCommand::new("gtk-launch");
                        cmd.arg(&app_path);
                        cmd.env("DISPLAY", ":0");
                        cmd
                    };

                    command.stdout(Stdio::null());
                    command.stderr(Stdio::null());

                    match command.spawn() {
                        Ok(_) => {
                            println!("Launched: {}", app);
                            info!("Launched {} in the background", app);
                        }
                        Err(e) => {
                            println!("Failed to launch {}: {}", app, e);
                            error!("Failed to launch {}: {}", app, e);
                        }
                    }
                }
                None => {
                    println!("Could not find application '{}'. Make sure it is installed correctly.", app);
                    error!("Could not find application path for '{}'", app);
                }
            }
        }
        info!("Launched apps for environment: {}", env);
    }
    Ok(())
}

