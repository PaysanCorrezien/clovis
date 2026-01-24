// At the top of your file with other imports:
// At the top with other imports
use clap::{value_parser, Command, CommandFactory};
use clap_complete::{generate, Generator, Shell};
use std::collections::HashMap;

use std::io::{self, BufRead};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};

use clap::{Parser, Subcommand};
use log::{error, info};
use simple_logger::SimpleLogger;

mod config;
mod icons;
mod platform;

use config::{generate_config, load_config, save_config, show_config, validate_config, Config};

#[derive(Parser)]
#[clap(
    name = "clovis",
    version = "0.1",
    about = "Clovis App Launcher - Launches applications based on environment configurations"
)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[clap(about = "Lists all available applications")]
    List {
        #[clap(
            long,
            short = 'f',
            help = "Output format: pretty (table), raw (one per line), or fzf (tab-separated)",
            default_value = "pretty",
            value_parser = ["pretty", "raw", "fzf"]
        )]
        format: String,
    },

    #[clap(about = "Search for applications by name")]
    Search {
        #[clap(help = "The search query (partial match)")]
        query: String,
    },

    #[clap(about = "Capture currently running applications")]
    Snapshot,

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
        #[clap(help = "Action to perform: add, remove, or set (set reads from stdin)")]
        action: String,
        #[clap(help = "The name of the application to add or remove (not used with 'set')")]
        app: Option<String>,
    },

    #[clap(about = "Opens the configuration file in the default editor")]
    Config,

    #[clap(about = "Generates a base example configuration")]
    Generate,

    #[clap(about = "Creates a desktop entry for the specified environment")]
    CreateDesktop {
        #[clap(help = "The name of the environment to create a desktop entry for")]
        env: String,
        #[clap(
            long,
            short = 'i',
            help = "Icon path or service name (e.g., 'firefox', './icon.png')"
        )]
        icon: Option<String>,
    },
    #[clap(about = "Generates shell completions")]
    Completions {
        #[clap(long = "generate", value_parser = value_parser!(Shell))]
        shell: Shell,
    },
}

fn main() -> io::Result<()> {
    fn create_desktop(config: &Config, env: &str, icon: Option<&str>) -> io::Result<()> {
        if !config.environments.contains_key(env) {
            println!("Environment '{}' not found.", env);
            return Ok(());
        }

        platform::create_desktop_entry(env, icon)?;
        println!("Desktop entry created for environment '{}'.", env);
        Ok(())
    }

    let cli = Cli::parse();

    // Handle completions command early, before any logging or config loading
    if let Commands::Completions { shell } = &cli.command {
        let mut cmd = Cli::command();
        eprintln!("Generating completion file for {shell}...");
        print_completions(*shell, &mut cmd);
        return Ok(());
    }

    SimpleLogger::new().init().unwrap();
    //info!("Starting application");

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

    match &cli.command {
        Commands::List { format } => {
            let apps = platform::find_available_apps_with_paths();
            let format = match format.to_lowercase().as_str() {
                "pretty" => platform::AppListFormat::Pretty,
                "raw" => platform::AppListFormat::Raw,
                "fzf" => platform::AppListFormat::Fzf,
                _ => {
                    println!("Invalid format. Using 'pretty' format.");
                    platform::AppListFormat::Pretty
                }
            };
            println!("{}", platform::format_app_list(&apps, format));
        }
        Commands::Search { query } => {
            handle_search_command(query);
        }
        Commands::Snapshot => {
            handle_snapshot_command();
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
        Commands::CreateDesktop { env, icon } => {
            create_desktop(&config, env, icon.as_deref())?;
        }
        Commands::Completions { .. } => {
            // Handled early in main, before logger initialization
            unreachable!()
        }
    }

    Ok(())
}

fn handle_search_command(query: &str) {
    let apps = platform::find_available_apps_with_paths();
    let query_lower = query.to_lowercase();

    let matches: Vec<_> = apps
        .iter()
        .filter(|app| app.name.to_lowercase().contains(&query_lower))
        .collect();

    if matches.is_empty() {
        println!("No applications found matching '{}'", query);
        return;
    }

    println!("Found {} application(s) matching '{}':\n", matches.len(), query);

    for app in matches {
        println!("Name: {}", app.name);
        println!("Path: {}", app.path.display());

        // Try to extract additional info from .desktop files on Linux
        if !cfg!(target_os = "windows") && app.path.extension().and_then(|s| s.to_str()) == Some("desktop") {
            if let Ok(content) = std::fs::read_to_string(&app.path) {
                for line in content.lines() {
                    if let Some(display_name) = line.strip_prefix("Name=") {
                        println!("Display Name: {}", display_name);
                    } else if let Some(comment) = line.strip_prefix("Comment=") {
                        println!("Description: {}", comment);
                    } else if let Some(exec) = line.strip_prefix("Exec=") {
                        println!("Executable: {}", exec);
                    }
                }
            }
        }

        println!();
    }
}

fn handle_snapshot_command() {
    let running_apps = platform::get_running_apps();

    for app in running_apps {
        println!("{}", app);
    }
}

fn handle_edit_command(
    config: &mut Config,
    env: &str,
    action: &str,
    app: &Option<String>,
) -> io::Result<bool> {
    if !config.environments.contains_key(env) {
        error!("Environment '{}' does not exist.", env);
        return Ok(false);
    }

    match action {
        "set" => {
            // Read apps from stdin
            let stdin = io::stdin();
            let reader = io::BufReader::new(stdin);
            let mut new_apps = Vec::new();

            for line in reader.lines() {
                let line = line?;
                let app_name = line.trim();
                if !app_name.is_empty() {
                    let normalized_app = platform::strip_platform_extension(app_name);
                    new_apps.push(normalized_app.to_string());
                }
            }

            // Replace the entire app list
            config.environments.insert(env.to_string(), new_apps.clone());
            println!("Set environment '{}' with {} applications", env, new_apps.len());
            info!("Set environment '{}' with {} applications", env, new_apps.len());
        }
        "add" | "remove" => {
            let app = match app {
                Some(a) => a,
                None => {
                    error!("App name required for '{}' action", action);
                    return Ok(false);
                }
            };

            let app_available =
                platform::is_desktop_file_available(app) || platform::is_command_available(app);

            if !app_available {
                println!(
                    "Warning: Application '{}' is not installed or not in PATH.",
                    app
                );
            }

            if action == "add" {
                let apps = config
                    .environments
                    .entry(env.to_string())
                    .or_insert_with(Vec::new);
                let normalized_app = platform::strip_platform_extension(app);
                if apps
                    .iter()
                    .any(|a| platform::strip_platform_extension(a) == normalized_app)
                {
                    error!(
                        "Application '{}' is already in environment '{}'",
                        normalized_app, env
                    );
                    return Ok(false);
                }
                apps.push(normalized_app.to_string());
                println!("Added '{}' to environment '{}'", app, env);
                info!("Added '{}' to environment '{}'", app, env);
            } else {
                // remove
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
        }
        _ => {
            println!("Invalid action '{}'. Use 'add', 'remove', or 'set'.", action);
            error!("Invalid action '{}'. Use 'add', 'remove', or 'set'.", action);
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
                        //TODO: change this to not start cmd but use windows run command via windows.rs
                        let mut cmd = ProcessCommand::new("cmd");
                        cmd.args(["/C", "start", "", &app_path.to_string_lossy()]);
                        cmd
                    } else {
                        if app_path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                            let mut cmd = ProcessCommand::new("gio");
                            cmd.args(["launch", app_path.to_string_lossy().as_ref()]);
                            cmd
                        } else {
                            ProcessCommand::new(&app_path)
                        }
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
                    println!(
                        "Could not find application '{}'. Make sure it is installed correctly.",
                        app
                    );
                    error!("Could not find application path for '{}'", app);
                }
            }
        }
        info!("Launched apps for environment: {}", env);
    }
    Ok(())
}

fn print_completions<G: Generator>(gen: G, cmd: &mut Command) {
    generate(gen, cmd, cmd.get_name().to_string(), &mut io::stdout());
}
