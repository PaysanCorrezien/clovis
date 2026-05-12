use crate::config::Config;
use crate::discovery::{discover_installed_apps, DiscoverySource};
use crate::launch::{launch_profile, AppLaunchResult, LaunchOptions};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryBenchmark {
    pub cold_ms: f64,
    pub warm_ms: f64,
    pub cold_count: usize,
    pub warm_count: usize,
    pub cold_errors: Vec<String>,
    pub warm_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileLaunchBenchmark {
    pub profile: String,
    pub milestone: String,
    pub total_dispatch_ms: f64,
    pub app_timings: Vec<AppLaunchResult>,
}

pub fn benchmark_discovery() -> DiscoveryBenchmark {
    let cold = discover_installed_apps(false);
    let warm = discover_installed_apps(true);
    let warm_errors = if warm.source == DiscoverySource::Cache {
        warm.errors
    } else {
        let mut errors = warm.errors;
        errors.push("warm run did not use cache".to_string());
        errors
    };

    DiscoveryBenchmark {
        cold_ms: duration_ms(cold.elapsed),
        warm_ms: duration_ms(warm.elapsed),
        cold_count: cold.apps.len(),
        warm_count: warm.apps.len(),
        cold_errors: cold.errors,
        warm_errors,
    }
}

pub fn benchmark_profile_launch(
    config: &Config,
    profile: &str,
    force: bool,
) -> Result<ProfileLaunchBenchmark, String> {
    let discovery = discover_installed_apps(true);
    let report = launch_profile(config, profile, LaunchOptions { force }, &discovery.apps)?;
    Ok(ProfileLaunchBenchmark {
        profile: report.profile,
        milestone: report.milestone,
        total_dispatch_ms: report.total_dispatch_ms,
        app_timings: report.results,
    })
}

pub fn format_discovery_benchmark(bench: &DiscoveryBenchmark) -> String {
    let mut out = String::new();
    out.push_str("Installed app discovery benchmark\n");
    out.push_str("Milestone: app list payload ready\n");
    out.push_str(&format!(
        "Cold fresh native: {:.2} ms ({} apps)\n",
        bench.cold_ms, bench.cold_count
    ));
    out.push_str(&format!(
        "Warm cache: {:.2} ms ({} apps)\n",
        bench.warm_ms, bench.warm_count
    ));
    for error in bench.cold_errors.iter().chain(bench.warm_errors.iter()) {
        out.push_str(&format!("Warning: {error}\n"));
    }
    out
}

pub fn format_profile_launch_benchmark(bench: &ProfileLaunchBenchmark) -> String {
    let mut out = String::new();
    out.push_str(&format!("Profile launch benchmark: {}\n", bench.profile));
    out.push_str(&format!("Milestone: {}\n", bench.milestone));
    out.push_str(&format!(
        "Total dispatch: {:.2} ms\n",
        bench.total_dispatch_ms
    ));
    out.push_str("Per-app dispatch:\n");
    for result in &bench.app_timings {
        let status = if result.skipped {
            "skipped"
        } else if result.success {
            "ok"
        } else {
            "failed"
        };
        out.push_str(&format!(
            "  - {}: {} ({:.2} ms)",
            result.name, status, result.dispatch_ms
        ));
        if let Some(error) = &result.error {
            out.push_str(&format!(" - {error}"));
        }
        out.push('\n');
    }
    out
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_discovery_benchmark_shape() {
        let bench = DiscoveryBenchmark {
            cold_ms: 12.5,
            warm_ms: 1.2,
            cold_count: 2,
            warm_count: 2,
            cold_errors: vec![],
            warm_errors: vec![],
        };

        let output = format_discovery_benchmark(&bench);

        assert!(output.contains("Cold fresh native"));
        assert!(output.contains("Warm cache"));
    }
}
