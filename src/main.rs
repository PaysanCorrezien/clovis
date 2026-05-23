use clap::{value_parser, Command, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Generator, Shell};
use clovis::benchmark::{
    benchmark_discovery, benchmark_profile_launch, format_discovery_benchmark,
    format_profile_launch_benchmark,
};
use clovis::config::{
    generate_config, load_config, save_config, show_config, validate_config, Config,
};
use clovis::discovery::discover_installed_apps;
use clovis::launch::{
    launch_plan, launch_profile, load_cached_launch_plan, save_launch_plan_cache, LaunchOptions,
};
use clovis::profile;
use clovis::{gui, platform};
use log::{error, info};
use simple_logger::SimpleLogger;
use std::collections::HashMap;
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

#[derive(Parser)]
#[clap(
    name = "clovis",
    version = "0.1",
    about = "Clovis App Launcher - profile-based app launching from CLI or GUI"
)]
struct Cli {
    #[clap(
        long,
        global = true,
        value_name = "PATH",
        help = "Use an explicit config YAML path instead of the default user config"
    )]
    config_path: Option<PathBuf>,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[clap(about = "Opens the iced GUI profile launcher")]
    Gui,

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
        #[clap(long, help = "Refresh the native installed-app cache before listing")]
        refresh: bool,
    },

    #[clap(about = "Lists installed applications using the native discovery engine")]
    ListInstalledApps {
        #[clap(
            long,
            short = 'f',
            default_value = "pretty",
            value_parser = ["pretty", "raw", "fzf"]
        )]
        format: String,
        #[clap(long, help = "Refresh the native installed-app cache before listing")]
        refresh: bool,
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

    #[clap(about = "Shows the current configuration or one profile")]
    Show {
        #[clap(help = "Optional profile name to inspect")]
        profile: Option<String>,
    },

    #[clap(about = "Inspects one profile")]
    Inspect {
        #[clap(help = "The profile name")]
        profile: String,
    },

    #[clap(about = "Profile automation commands")]
    Profiles {
        #[clap(subcommand)]
        command: ProfileCommands,
    },

    #[clap(about = "Launches all apps in the specified profile/environment")]
    Launch {
        #[clap(help = "The name of the profile/environment to launch")]
        env: Option<String>,
        #[clap(
            long,
            help = "Force launch applications even if they are already running"
        )]
        force: bool,
        #[clap(
            long,
            help = "Use a pre-resolved launch plan cache and skip config/discovery parsing"
        )]
        fast: bool,
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

    #[clap(about = "Repeatable benchmark commands")]
    Benchmark {
        #[clap(subcommand)]
        command: BenchmarkCommands,
    },

    #[clap(about = "Generates shell completions")]
    Completions {
        #[clap(long = "generate", value_parser = value_parser!(Shell))]
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum ProfileCommands {
    #[clap(about = "List profiles")]
    List,
    #[clap(about = "Show one profile")]
    Show { profile: String },
}

#[derive(Subcommand)]
enum BenchmarkCommands {
    #[clap(about = "Benchmark installed app discovery cold and warm/cache paths")]
    Discovery,
    #[clap(about = "Benchmark profile launch dispatch time")]
    ProfileLaunch {
        profile: String,
        #[clap(
            long,
            help = "Force launch applications even if they are already running"
        )]
        force: bool,
    },
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    if let Some(Commands::Completions { shell }) = &cli.command {
        let mut cmd = Cli::command();
        eprintln!("Generating completion file for {shell}...");
        print_completions(*shell, &mut cmd);
        return Ok(());
    }

    let config_path = cli
        .config_path
        .clone()
        .unwrap_or_else(profile::default_config_path);

    if let Some(Commands::Launch {
        env: Some(env),
        force,
        fast: true,
    }) = &cli.command
    {
        return handle_fast_launch_command(&config_path, env, *force);
    }

    if cli.command.is_none() || matches!(cli.command, Some(Commands::Gui)) {
        return gui::run_gui(config_path).map_err(|err| io::Error::new(io::ErrorKind::Other, err));
    }

    let _ = SimpleLogger::new().init();
    profile::ensure_config_parent(&config_path)?;

    let mut config = load_config(&config_path).unwrap_or_else(|_| {
        info!("Creating new config as loading failed");
        Config {
            environments: HashMap::new(),
        }
    });

    match cli.command.expect("handled GUI and completions above") {
        Commands::Gui => unreachable!(),
        Commands::List { format, refresh } | Commands::ListInstalledApps { format, refresh } => {
            handle_list_command(&format, refresh);
        }
        Commands::Search { query } => {
            handle_search_command(&query);
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
        Commands::Show {
            profile: profile_name,
        } => match profile_name {
            Some(name) => inspect_profile(&config, &name),
            None => show_config(&config),
        },
        Commands::Inspect { profile } => inspect_profile(&config, &profile),
        Commands::Profiles { command } => match command {
            ProfileCommands::List => {
                for name in profile::profile_names(&config) {
                    println!("{name}");
                }
            }
            ProfileCommands::Show { profile } => inspect_profile(&config, &profile),
        },
        Commands::Launch { env, force, .. } => {
            handle_launch_command(&config_path, &config, &env, force)?;
        }
        Commands::Validate => validate_config(&config),
        Commands::Edit { env, action, app } => {
            if handle_edit_command(&mut config, &env, &action, &app)? {
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
            create_desktop(&config_path, &config, &env, icon.as_deref())?;
        }
        Commands::Benchmark { command } => match command {
            BenchmarkCommands::Discovery => {
                let bench = benchmark_discovery();
                print!("{}", format_discovery_benchmark(&bench));
            }
            BenchmarkCommands::ProfileLaunch { profile, force } => {
                match benchmark_profile_launch(&config, &profile, force) {
                    Ok(bench) => print!("{}", format_profile_launch_benchmark(&bench)),
                    Err(err) => println!("{err}"),
                }
            }
        },
        Commands::Completions { .. } => unreachable!(),
    }

    Ok(())
}

fn create_desktop(
    config_path: &PathBuf,
    config: &Config,
    env: &str,
    icon: Option<&str>,
) -> io::Result<()> {
    if !config.environments.contains_key(env) {
        println!("Environment '{}' not found.", env);
        return Ok(());
    }

    let discovery = discover_installed_apps(true);
    let _ = save_launch_plan_cache(&config_path, config, &discovery.apps);
    platform::create_desktop_entry(env, icon)?;
    println!("Desktop entry created for environment '{}'.", env);
    Ok(())
}

fn handle_list_command(format: &str, refresh: bool) {
    let report = discover_installed_apps(!refresh);
    for error in report.errors {
        eprintln!("Warning: {error}");
    }
    let apps: Vec<_> = report
        .apps
        .into_iter()
        .map(|app| platform::AppInfo {
            name: app.name,
            path: app.path,
        })
        .collect();
    let format = match format.to_lowercase().as_str() {
        "pretty" => platform::AppListFormat::Pretty,
        "raw" => platform::AppListFormat::Raw,
        "fzf" => platform::AppListFormat::Fzf,
        _ => platform::AppListFormat::Pretty,
    };
    println!("{}", platform::format_app_list(&apps, format));
}

fn handle_search_command(query: &str) {
    let report = discover_installed_apps(true);
    let query_lower = query.to_lowercase();

    let matches: Vec<_> = report
        .apps
        .iter()
        .filter(|app| {
            app.name.to_lowercase().contains(&query_lower)
                || app
                    .launch_target
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&query_lower)
        })
        .collect();

    if matches.is_empty() {
        println!("No applications found matching '{}'", query);
        return;
    }

    println!(
        "Found {} application(s) matching '{}':\n",
        matches.len(),
        query
    );

    for app in matches {
        println!("Name: {}", app.name);
        println!("Path: {}", app.path.display());
        println!("Launch Target: {}", app.launch_target.display());
        println!("Source: {}", app.source);
        if !app.provenance.is_empty() {
            println!("Provenance: {}", app.provenance.join(" | "));
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
            let stdin = io::stdin();
            let reader = io::BufReader::new(stdin);
            let mut new_apps = Vec::new();

            for line in reader.lines() {
                let line = line?;
                let app_name = line.trim();
                if !app_name.is_empty() {
                    new_apps.push(platform::strip_platform_extension(app_name).to_string());
                }
            }

            config
                .environments
                .insert(env.to_string(), new_apps.clone());
            println!(
                "Set environment '{}' with {} applications",
                env,
                new_apps.len()
            );
            info!(
                "Set environment '{}' with {} applications",
                env,
                new_apps.len()
            );
        }
        "add" | "remove" => {
            let app = match app {
                Some(a) => a,
                None => {
                    error!("App name required for '{}' action", action);
                    return Ok(false);
                }
            };

            if action == "add" {
                if let Err(err) = profile::add_profile_app(config, env, app) {
                    error!("{err}");
                    return Ok(false);
                }
                println!("Added '{}' to environment '{}'", app, env);
                info!("Added '{}' to environment '{}'", app, env);
            } else if let Err(err) = profile::remove_profile_app(config, env, app) {
                println!("{err}");
                return Ok(false);
            } else {
                println!("Removed '{}' from environment '{}'", app, env);
                info!("Removed '{}' from environment '{}'", app, env);
            }
        }
        _ => {
            println!(
                "Invalid action '{}'. Use 'add', 'remove', or 'set'.",
                action
            );
            error!(
                "Invalid action '{}'. Use 'add', 'remove', or 'set'.",
                action
            );
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
    println!("\nAvailable profiles/environments:");
    for env in profile::profile_names(config) {
        println!("  - {}", env);
    }
    Ok(())
}

fn inspect_profile(config: &Config, profile_name: &str) {
    match profile::get_profile(config, profile_name) {
        Some(profile) => {
            println!("{}:", profile.name);
            for app in profile.apps {
                println!("  - {}", app);
            }
        }
        None => println!("Profile '{}' not found.", profile_name),
    }
}

fn handle_launch_command(
    config_path: &PathBuf,
    config: &Config,
    env: &Option<String>,
    force: bool,
) -> io::Result<()> {
    match env {
        Some(env_name) => {
            if !config.environments.contains_key(env_name) {
                println!("Environment '{}' not found.", env_name);
                print_launch_help_and_available_environments(config)?;
                return Ok(());
            }
            let discovery = discover_installed_apps(true);
            let _ = save_launch_plan_cache(config_path, config, &discovery.apps);
            let report = launch_profile(config, env_name, LaunchOptions { force }, &discovery.apps)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            println!("Milestone: {}", report.milestone);
            println!(
                "Profile '{}' dispatched in {:.2} ms",
                report.profile, report.total_dispatch_ms
            );
            for result in report.results {
                if result.skipped {
                    println!("Skipping: {} (already running)", result.name);
                } else if result.success {
                    println!("Launched: {} ({:.2} ms)", result.name, result.dispatch_ms);
                } else {
                    println!(
                        "Failed to launch {}: {}",
                        result.name,
                        result.error.unwrap_or_else(|| "unknown error".to_string())
                    );
                }
            }
            Ok(())
        }
        None => {
            print_launch_help_and_available_environments(config)?;
            Ok(())
        }
    }
}

fn handle_fast_launch_command(config_path: &PathBuf, env: &str, force: bool) -> io::Result<()> {
    let plan = match load_cached_launch_plan(config_path, env) {
        Ok(plan) => plan,
        Err(_) => {
            let config = load_config(config_path)?;
            let discovery = discover_installed_apps(true);
            let _ = save_launch_plan_cache(config_path, &config, &discovery.apps);
            load_cached_launch_plan(config_path, env)?
        }
    };
    let report = launch_plan(plan, LaunchOptions { force });
    println!("Milestone: {}", report.milestone);
    println!(
        "Profile '{}' dispatched in {:.2} ms",
        report.profile, report.total_dispatch_ms
    );
    for result in report.results {
        if result.skipped {
            println!("Skipping: {} (already running)", result.name);
        } else if result.success {
            println!("Launched: {} ({:.2} ms)", result.name, result.dispatch_ms);
        } else {
            println!(
                "Failed to launch {}: {}",
                result.name,
                result.error.unwrap_or_else(|| "unknown error".to_string())
            );
        }
    }
    Ok(())
}

fn print_completions<G: Generator>(gen: G, cmd: &mut Command) {
    generate(gen, cmd, cmd.get_name().to_string(), &mut io::stdout());
}
