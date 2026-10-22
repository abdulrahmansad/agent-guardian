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
    /// Internal same-user event used to confirm a detached worker is ready.
    #[arg(long, hide = true, global = true)]
    startup_event: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Hold a bounded power lease until the target exits, Guardian is stopped, or it expires.
    Start {
        /// Bind the lease to a process in this Windows user and session. Defaults to Guardian itself.
        #[arg(long)]
        pid: Option<u32>,
        /// Internal process-creation identity carried to a detached worker.
        #[arg(long, hide = true)]
        target_creation_time: Option<u64>,
        /// Maximum lease duration (1 to 1440 minutes).
        #[arg(long)]
        ttl_minutes: Option<u32>,
        /// Explicitly permit a short lease while on battery.
        #[arg(long)]
        allow_battery: bool,
        /// Run the lease in a detached same-user process, independent of this terminal.
        #[arg(long)]
        background: bool,
        /// Explicitly enable bounded saved-profile Wi-Fi reconnect attempts during the lease.
        #[arg(long)]
        auto_reconnect_saved_wifi: bool,
        /// Read limits from this TOML file instead of the per-user default.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Report whether Windows power-request status can be queried, with details redacted.
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
    /// Monitor local IP-interface and route changes; reconnect only when explicitly enabled.
    Monitor {
        /// Internal start-process binding used by `guardian start`.
        #[arg(long, hide = true)]
        guardian_pid: Option<u32>,
        /// Maximum monitor duration in minutes (1 to 1440).
        #[arg(long)]
        duration_minutes: Option<u32>,
        /// Local snapshot fallback interval in seconds (30 to 3600).
        #[arg(long)]
        interval_seconds: Option<u32>,
        /// Explicitly enable conservative, bounded saved-profile Wi-Fi reconnect attempts.
        #[arg(long)]
        auto_reconnect_saved_wifi: bool,
        /// Read journal limits from this TOML file instead of the per-user default.
        #[arg(long)]
        config: Option<PathBuf>,
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
        /// Optional SHA-256 from a trusted release checksum source.
        #[arg(long)]
        sha256: Option<String>,
    },
    /// Remove the per-user runtime executable without deleting local data.
    Uninstall,
    /// Stop Guardian power leases and monitors in this Windows user session.
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
            target_creation_time,
            ttl_minutes,
            allow_battery,
            background,
            auto_reconnect_saved_wifi,
            config: config_path,
        } => {
            let mut config = match load_config(config_path.as_deref()) {
                Ok(config) => config,
                Err(error) => return report_error(error),
            };
            config.power.allow_on_battery |= allow_battery;
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
                        windows::TargetProcess {
                            pid,
                            creation_time_100ns: target_creation_time,
                        },
                        ttl_minutes,
                        background,
                        auto_reconnect_saved_wifi,
                        config,
                        config_path,
                        args.startup_event.as_deref(),
                    )
                }
                #[cfg(not(windows))]
                {
                    let _ = (
                        pid,
                        target_creation_time,
                        allow_battery,
                        background,
                        auto_reconnect_saved_wifi,
                        config,
                        config_path,
                        args.startup_event.as_deref(),
                    );
                    Err("guardian start is supported only on Windows".into())
                }
            }
        }
        Command::Status => platform_status(args.json),
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
            guardian_pid,
            duration_minutes,
            interval_seconds,
            auto_reconnect_saved_wifi,
            config: config_path,
        } => {
            let duration_minutes = duration_minutes.unwrap_or(60);
            let interval_seconds = interval_seconds.unwrap_or(300);
            let config = if config_path.is_some() || auto_reconnect_saved_wifi {
                match load_config(config_path.as_deref()) {
                    Ok(config) => config,
                    Err(error) => return report_error(error),
                }
            } else {
                Config::default()
            };
            match validate_monitor_options(duration_minutes, interval_seconds) {
                Ok(()) => platform_monitor(
                    args.json,
                    guardian_pid,
                    duration_minutes,
                    interval_seconds,
                    auto_reconnect_saved_wifi,
                    config,
                    args.startup_event.as_deref(),
                ),
                Err(error) => Err(error),
            }
        }
        Command::Logs { config } => platform_logs(args.json, config),
        Command::Report { output, config } => platform_report(output, args.json, config),
        Command::Install { sha256 } => {
            #[cfg(windows)]
            {
                windows::install(args.json, sha256.as_deref())
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
    if !(1..=1440).contains(&duration_minutes) {
        return Err("duration-minutes must be between 1 and 1440".into());
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

fn status_output(query_available: bool, json: bool) -> String {
    const UNAVAILABLE: &str = "Power request query unavailable; command output was suppressed. An elevated shell may be required.";

    if json {
        let warnings = if query_available {
            Vec::<&str>::new()
        } else {
            vec![UNAVAILABLE]
        };
        let mut output = serde_json::json!({
            "power_request_query_available": query_available,
            "power_request_details_redacted": true,
            "diagnostic_warnings": warnings
        })
        .to_string();
        output.push('\n');
        output
    } else if query_available {
        "Power request query completed; requestor details were redacted.\n".into()
    } else {
        format!("{UNAVAILABLE}\n")
    }
}

fn platform_status(json: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let query_available = windows::powercfg(&["/requests"]).is_ok();
        print!("{}", status_output(query_available, json));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = json;
        Err("guardian status is available only on Windows".into())
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
    guardian_pid: Option<u32>,
    duration_minutes: u32,
    interval_seconds: u32,
    auto_reconnect_saved_wifi: bool,
    config: Config,
    startup_event: Option<&str>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::monitor(
            json,
            guardian_pid,
            duration_minutes,
            interval_seconds,
            auto_reconnect_saved_wifi,
            config,
            startup_event,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (
            json,
            guardian_pid,
            duration_minutes,
            interval_seconds,
            auto_reconnect_saved_wifi,
            config,
            startup_event,
        );
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

    #[test]
    fn status_reports_unavailable_power_query_without_raw_details() {
        let output = status_output(false, false);
        assert!(output.contains("query unavailable"));
        assert!(output.contains("elevated shell may be required"));
        assert!(!output.contains("exit code"));
    }

    #[test]
    fn status_accepts_global_json_flag() {
        let args = Args::try_parse_from(["guardian", "--json", "status"]).unwrap();
        assert!(args.json);
        assert!(matches!(args.command, Command::Status));
    }

    #[test]
    fn status_json_marks_query_availability_and_redaction() {
        let output = status_output(false, true);
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["power_request_query_available"], false);
        assert_eq!(value["power_request_details_redacted"], true);
        assert!(
            value["diagnostic_warnings"][0]
                .as_str()
                .unwrap()
                .contains("elevated shell may be required")
        );
    }

    #[test]
    fn status_json_reports_success_without_diagnostic_warnings() {
        let output = status_output(true, true);
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["power_request_query_available"], true);
        assert_eq!(value["power_request_details_redacted"], true);
        assert!(value["diagnostic_warnings"].as_array().unwrap().is_empty());
    }

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
    fn background_lease_and_automatic_reconnect_are_explicit_options() {
        let background = Args::try_parse_from([
            "guardian",
            "start",
            "--background",
            "--auto-reconnect-saved-wifi",
            "--ttl-minutes",
            "30",
        ])
        .unwrap();
        assert!(matches!(
            background.command,
            Command::Start {
                background: true,
                auto_reconnect_saved_wifi: true,
                ttl_minutes: Some(30),
                ..
            }
        ));

        let monitor = Args::try_parse_from([
            "guardian",
            "monitor",
            "--auto-reconnect-saved-wifi",
            "--duration-minutes",
            "30",
        ])
        .unwrap();
        assert!(matches!(
            monitor.command,
            Command::Monitor {
                auto_reconnect_saved_wifi: true,
                duration_minutes: Some(30),
                ..
            }
        ));
        assert!(
            Args::try_parse_from([
                "guardian",
                "monitor",
                "--auto-reconnect-saved-wifi",
                "--config",
                "guardian.toml",
            ])
            .is_ok()
        );

        let startup = Args::try_parse_from([
            "guardian",
            "--startup-event",
            "Local\\AgentGuardian.Startup.00000000-0000-4000-8000-000000000000",
            "start",
            "--background",
        ])
        .unwrap();
        assert_eq!(
            startup.startup_event.as_deref(),
            Some("Local\\AgentGuardian.Startup.00000000-0000-4000-8000-000000000000")
        );
    }

    #[test]
    fn monitor_defaults_to_a_bounded_foreground_session() {
        let args = Args::try_parse_from(["guardian", "monitor"]).unwrap();
        assert!(matches!(
            args.command,
            Command::Monitor {
                guardian_pid: None,
                duration_minutes: None,
                interval_seconds: None,
                auto_reconnect_saved_wifi: false,
                config: None
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
                guardian_pid: None,
                duration_minutes: Some(60),
                interval_seconds: Some(300),
                auto_reconnect_saved_wifi: false,
                config: None
            }
        ));
        assert!(validate_monitor_options(1, 30).is_ok());
        assert!(validate_monitor_options(1440, 3600).is_ok());
        assert!(validate_monitor_options(0, 300).is_err());
        assert!(validate_monitor_options(1441, 300).is_err());
        assert!(validate_monitor_options(60, 29).is_err());
        assert!(validate_monitor_options(60, 3601).is_err());
    }

    #[test]
    fn installer_supports_self_hash_or_explicit_sha256() {
        let self_hashed = Args::try_parse_from(["guardian", "install"]).unwrap();
        assert!(matches!(
            self_hashed.command,
            Command::Install { sha256: None }
        ));
        let args =
            Args::try_parse_from(["guardian", "--json", "install", "--sha256", &"a".repeat(64)])
                .unwrap();
        assert!(matches!(args.command, Command::Install { sha256: Some(_) }));
        assert!(Args::try_parse_from(["guardian", "uninstall"]).is_ok());
        assert!(Args::try_parse_from(["guardian", "stop"]).is_ok());
    }
}
