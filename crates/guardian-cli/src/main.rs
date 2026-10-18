use clap::{Parser, Subcommand};
use guardian_core::Config;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[cfg(windows)]
mod windows;

#[derive(Debug, Parser)]
#[command(
    name = "guardian",
    version,
    about = "Keep a Windows agent session awake and inspect connectivity safely"
)]
struct Args {
    /// Print machine-readable JSON where the command supports it.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Hold a foreground power lease until the target process exits or the lease expires.
    Start {
        /// Bind the lease to a process in this Windows user and session. Defaults to Guardian itself.
        #[arg(long)]
        pid: Option<u32>,
        /// Maximum lease duration (1 to 1440 minutes).
        #[arg(long)]
        ttl_minutes: Option<u32>,
        /// Explicitly permit a short lease while on battery.
        #[arg(long)]
        allow_battery: bool,
        /// Read limits from this TOML file instead of the per-user default.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Query Windows power-request status with requestor details redacted.
    Status,
    /// Read-only environment and power diagnostics.
    Doctor,
    /// Inspect network state and optionally request one saved-profile reconnect.
    Network {
        /// Run one bounded connectivity check using fixed DNS, route, gateway, and web evidence.
        #[arg(long)]
        probe: bool,
        /// Explicitly request one saved-profile reconnect if Wi-Fi is confirmed disconnected and no default route exists.
        #[arg(long, conflicts_with = "probe")]
        reconnect_saved_wifi: bool,
        /// Read journal limits from this TOML file instead of the per-user default.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Monitor local IP-interface and route changes in the foreground.
    Monitor {
        /// Maximum monitor duration in minutes (1 to 60).
        #[arg(long)]
        duration_minutes: Option<u32>,
        /// Local snapshot fallback interval in seconds (30 to 3600).
        #[arg(long)]
        interval_seconds: Option<u32>,
    },
    /// Show local Guardian event logs.
    Logs {
        /// Read journal limits from this TOML file instead of the per-user default.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Export an incident report.
    Report {
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Read journal limits from this TOML file instead of the per-user default.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Install or upgrade this verified executable in the current user's profile.
    Install {
        /// SHA-256 copied from a trusted release checksum source.
        #[arg(long)]
        sha256: String,
    },
    /// Remove the per-user runtime executable without deleting local data.
    Uninstall,
    /// Stop active foreground power leases in this Windows user session.
    Stop,
    /// Show the Guardian version.
    Version,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let result = match args.command {
        Command::Version => {
            println!("guardian {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Start {
            pid,
            ttl_minutes,
            allow_battery,
            config: config_path,
        } => {
            let config = match load_config(config_path.as_deref()) {
                Ok(config) => config,
                Err(error) => return report_error(error),
            };
            let ttl_minutes = ttl_minutes.unwrap_or(config.power.default_lease_minutes);
            if !(1..=24 * 60).contains(&ttl_minutes) {
                Err("ttl-minutes must be between 1 and 1440".to_string())
            } else if ttl_minutes > config.power.maximum_lease_minutes {
                Err(format!(
                    "ttl-minutes exceeds the configured maximum of {}",
                    config.power.maximum_lease_minutes
                ))
            } else {
                #[cfg(windows)]
                {
                    windows::start(
                        pid,
                        ttl_minutes,
                        allow_battery || config.power.allow_on_battery,
                        config,
                    )
                }
                #[cfg(not(windows))]
                {
                    let _ = (pid, allow_battery, config);
                    Err("guardian start is supported only on Windows".into())
                }
            }
        }
        Command::Status => platform_diagnostic("requests"),
        Command::Doctor => platform_doctor(args.json),
        Command::Network {
            probe,
            reconnect_saved_wifi,
            config: config_path,
        } => {
            let config = if config_path.is_some() || reconnect_saved_wifi {
                match load_config(config_path.as_deref()) {
                    Ok(config) => config,
                    Err(error) => return report_error(error),
                }
            } else {
                Config::default()
            };
            platform_network(args.json, probe, reconnect_saved_wifi, config)
        }
        Command::Monitor {
            duration_minutes,
            interval_seconds,
        } => {
            let duration_minutes = duration_minutes.unwrap_or(60);
            let interval_seconds = interval_seconds.unwrap_or(300);
            match validate_monitor_options(duration_minutes, interval_seconds) {
                Ok(()) => platform_monitor(args.json, duration_minutes, interval_seconds),
                Err(error) => Err(error),
            }
        }
        Command::Logs { config } => platform_logs(args.json, config),
        Command::Report { output, config } => platform_report(output, args.json, config),
        Command::Install { sha256 } => {
            #[cfg(windows)]
            {
                windows::install(args.json, &sha256)
            }
            #[cfg(not(windows))]
            {
                let _ = (args.json, sha256);
                Err("guardian install is supported only on Windows".into())
            }
        }
        Command::Uninstall => {
            #[cfg(windows)]
            {
                windows::uninstall(args.json)
            }
            #[cfg(not(windows))]
            {
                let _ = args.json;
                Err("guardian uninstall is supported only on Windows".into())
            }
        }
        Command::Stop => {
            #[cfg(windows)]
            {
                windows::stop(args.json)
            }
            #[cfg(not(windows))]
            {
                let _ = args.json;
                Err("guardian stop is supported only on Windows".into())
            }
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(error),
    }
}

fn report_error(error: String) -> ExitCode {
    eprintln!("guardian: {error}");
    ExitCode::from(2)
}

fn validate_monitor_options(duration_minutes: u32, interval_seconds: u32) -> Result<(), String> {
    if !(1..=60).contains(&duration_minutes) {
        return Err("duration-minutes must be between 1 and 60".into());
    }
    if !(30..=3600).contains(&interval_seconds) {
        return Err("interval-seconds must be between 30 and 3600".into());
    }
    Ok(())
}

fn load_config(explicit_path: Option<&Path>) -> Result<Config, String> {
    let path = explicit_path
        .map(Path::to_path_buf)
        .or_else(default_config_path);
    let Some(path) = path else {
        return Ok(Config::default());
    };
    match read_config(&path) {
        Ok(config) => Ok(config),
        Err(error) if explicit_path.is_none() && error.kind() == std::io::ErrorKind::NotFound => {
            Ok(Config::default())
        }
        Err(error) => Err(format!(
            "could not read configuration {}: {error}",
            path.display()
        )),
    }
}

fn default_config_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("AgentGuardian").join("config.toml"))
}

fn read_config(path: &Path) -> Result<Config, std::io::Error> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "configuration path is not a regular file",
        ));
    }
    if file.metadata()?.len() > MAX_CONFIG_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "configuration file exceeds the 64 KiB limit",
        ));
    }
    let mut source = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut source)?;
    if source.len() as u64 > MAX_CONFIG_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "configuration file exceeds the 64 KiB limit",
        ));
    }
    Config::parse(&source)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}

fn platform_diagnostic(mode: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let arguments = if mode == "requests" {
            vec!["/requests"]
        } else {
            vec!["/a"]
        };
        let output = windows::powercfg(&arguments)?;
        print!("{output}");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = mode;
        Err("Windows power diagnostics are available only on Windows".into())
    }
}

fn platform_doctor(json: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::doctor(json)
    }
    #[cfg(not(windows))]
    {
        let _ = json;
        Err("guardian doctor is available only on Windows".into())
    }
}

fn platform_network(
    json: bool,
    probe: bool,
    reconnect_saved_wifi: bool,
    config: Config,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::network(json, probe, reconnect_saved_wifi, config)
    }
    #[cfg(not(windows))]
    {
        let _ = (json, probe, reconnect_saved_wifi, config);
        Err("network diagnostics are available only on Windows".into())
    }
}

fn platform_monitor(
    json: bool,
    duration_minutes: u32,
    interval_seconds: u32,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::monitor(json, duration_minutes, interval_seconds)
    }
    #[cfg(not(windows))]
    {
        let _ = (json, duration_minutes, interval_seconds);
        Err("guardian monitor is available only on Windows".into())
    }
}

fn platform_logs(json: bool, config_path: Option<PathBuf>) -> Result<(), String> {
    #[cfg(windows)]
    {
        let config = load_config(config_path.as_deref())?;
        windows::logs(json, config)
    }
    #[cfg(not(windows))]
    {
        let _ = (json, config_path);
        Err("local Guardian logs are available only on Windows".into())
    }
}

fn platform_report(
    output: Option<std::path::PathBuf>,
    json: bool,
    config_path: Option<PathBuf>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let config = load_config(config_path.as_deref())?;
        windows::report(output, json, config)
    }
    #[cfg(not(windows))]
    {
        let _ = (output, json, config_path);
        Err("Guardian reports are available only on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_path(label: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "guardian-{label}-{}-{stamp}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn explicit_config_is_parsed_and_bounded() {
        let path = unique_path("config");
        std::fs::write(
            &path,
            "[power]\ndefault_lease_minutes = 15\nmaximum_lease_minutes = 30\n",
        )
        .unwrap();
        let config = load_config(Some(&path)).unwrap();
        assert_eq!(config.power.default_lease_minutes, 15);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn explicit_missing_config_is_an_error() {
        let path = unique_path("missing");
        assert!(load_config(Some(&path)).is_err());
    }

    #[test]
    fn oversized_config_is_rejected_before_parsing() {
        let path = unique_path("large");
        std::fs::write(&path, vec![b' '; (MAX_CONFIG_BYTES + 1) as usize]).unwrap();
        assert!(load_config(Some(&path)).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn external_network_probes_require_an_explicit_flag() {
        let local_only = Args::try_parse_from(["guardian", "network"]).unwrap();
        assert!(matches!(
            local_only.command,
            Command::Network {
                probe: false,
                reconnect_saved_wifi: false,
                ..
            }
        ));

        let opt_in = Args::try_parse_from(["guardian", "network", "--probe"]).unwrap();
        assert!(matches!(
            opt_in.command,
            Command::Network {
                probe: true,
                reconnect_saved_wifi: false,
                ..
            }
        ));
        let reconnect =
            Args::try_parse_from(["guardian", "network", "--reconnect-saved-wifi"]).unwrap();
        assert!(matches!(
            reconnect.command,
            Command::Network {
                probe: false,
                reconnect_saved_wifi: true,
                ..
            }
        ));
        let reconnect_with_config = Args::try_parse_from([
            "guardian",
            "network",
            "--reconnect-saved-wifi",
            "--config",
            "guardian.toml",
        ])
        .unwrap();
        assert!(matches!(
            reconnect_with_config.command,
            Command::Network {
                reconnect_saved_wifi: true,
                config: Some(path),
                ..
            } if path == Path::new("guardian.toml")
        ));
        assert!(
            Args::try_parse_from(["guardian", "network", "--probe", "--reconnect-saved-wifi"])
                .is_err()
        );
    }

    #[test]
    fn monitor_defaults_to_a_bounded_foreground_session() {
        let args = Args::try_parse_from(["guardian", "monitor"]).unwrap();
        assert!(matches!(
            args.command,
            Command::Monitor {
                duration_minutes: None,
                interval_seconds: None
            }
        ));
        let bounded = Args::try_parse_from([
            "guardian",
            "monitor",
            "--duration-minutes",
            "60",
            "--interval-seconds",
            "300",
        ])
        .unwrap();
        assert!(matches!(
            bounded.command,
            Command::Monitor {
                duration_minutes: Some(60),
                interval_seconds: Some(300)
            }
        ));
        assert!(validate_monitor_options(1, 30).is_ok());
        assert!(validate_monitor_options(60, 3600).is_ok());
        assert!(validate_monitor_options(0, 300).is_err());
        assert!(validate_monitor_options(61, 300).is_err());
        assert!(validate_monitor_options(60, 29).is_err());
        assert!(validate_monitor_options(60, 3601).is_err());
    }

    #[test]
    fn installer_requires_an_explicit_sha256() {
        assert!(Args::try_parse_from(["guardian", "install"]).is_err());
        let args =
            Args::try_parse_from(["guardian", "--json", "install", "--sha256", &"a".repeat(64)])
                .unwrap();
        assert!(matches!(args.command, Command::Install { .. }));
        assert!(Args::try_parse_from(["guardian", "uninstall"]).is_ok());
        assert!(Args::try_parse_from(["guardian", "stop"]).is_ok());
    }
}
