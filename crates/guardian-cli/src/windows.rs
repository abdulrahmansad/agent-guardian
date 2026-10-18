use std::ffi::OsString;
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use guardian_core::{
    Config, ConnectivityEvidence, ConnectivityState, EndpointEvidence, EventStore, LeaseBook,
    LeaseId, LocalAdapterObservation, LocalNetworkSummary, PowerLeaseDecision, PowerLeasePolicy,
    PowerLeasePolicyError, PowerSourceClass, ProcessIdentity, ReleaseReason, SessionEvent,
    SessionEventKind, SessionReport, WifiReconnectResult, classify_local_evidence,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
    GAA_FLAG_SKIP_FRIENDLY_NAME, GAA_FLAG_SKIP_MULTICAST, GAA_FLAG_SKIP_UNICAST,
    GetAdaptersAddresses, IF_TYPE_IEEE80211, IF_TYPE_TUNNEL, IP_ADAPTER_ADDRESSES_LH,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;

mod install;
mod monitor;
mod probes;
mod recovery;
mod signals;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x102;
const POWER_REQUEST_SYSTEM_REQUIRED: i32 = 1;
const BATTERY_LEASE_LIMIT: Duration = Duration::from_secs(5 * 60);
const POWER_POLL_INTERVAL: Duration = Duration::from_secs(10);
const ERROR_BUFFER_OVERFLOW: u32 = 111;
const ERROR_NO_DATA: u32 = 232;
const NETWORK_BUFFER_START_BYTES: usize = 15 * 1024;
const NETWORK_BUFFER_MAX_BYTES: usize = 1024 * 1024;
type Handle = *mut c_void;

#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DetailedReason {
    localized_reason_module: Handle,
    localized_reason_id: u32,
    reason_string_count: u32,
    reason_strings: *const *const u16,
}

#[repr(C)]
union Reason {
    detailed: DetailedReason,
    simple_reason: *const u16,
}

#[repr(C)]
struct ReasonContext {
    version: u32,
    flags: u32,
    reason: Reason,
}

#[repr(C)]
#[derive(Default)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn PowerCreateRequest(context: *const ReasonContext) -> Handle;
    fn PowerSetRequest(request: Handle, request_type: i32) -> i32;
    fn PowerClearRequest(request: Handle, request_type: i32) -> i32;
    fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> Handle;
    fn GetProcessTimes(
        process: Handle,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetLastError() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
}

struct ProcessHandle(Handle);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<(Self, u64), String> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                0,
                process_id,
            )
        };
        if handle.is_null() || handle as isize == -1 {
            return Err(format!(
                "could not open process {process_id} (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        let process = Self(handle);
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = FileTime { low: 0, high: 0 };
        let mut kernel = FileTime { low: 0, high: 0 };
        let mut user = FileTime { low: 0, high: 0 };
        let succeeded =
            unsafe { GetProcessTimes(process.0, &mut creation, &mut exit, &mut kernel, &mut user) };
        if succeeded == 0 {
            return Err(format!(
                "could not read process identity (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        let created = (u64::from(creation.high) << 32) | u64::from(creation.low);
        Ok((process, created))
    }

    fn wait(&self, timeout_ms: u32) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.0, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(format!(
                "could not monitor target process (Windows error {})",
                unsafe { GetLastError() }
            )),
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct PowerLease {
    handle: Handle,
    _reason_storage: Vec<u16>,
}

impl PowerLease {
    fn acquire(reason: &str) -> Result<Self, String> {
        let mut reason_storage: Vec<u16> =
            reason.encode_utf16().chain(std::iter::once(0)).collect();
        let context = ReasonContext {
            version: 0,
            flags: 1,
            reason: Reason {
                simple_reason: reason_storage.as_mut_ptr(),
            },
        };
        let handle = unsafe { PowerCreateRequest(&context) };
        if handle.is_null() || handle as isize == -1 {
            return Err(format!(
                "PowerCreateRequest failed (Windows error {})",
                unsafe { GetLastError() }
            ));
        }
        if unsafe { PowerSetRequest(handle, POWER_REQUEST_SYSTEM_REQUIRED) } == 0 {
            let error = unsafe { GetLastError() };
            unsafe {
                CloseHandle(handle);
            }
            return Err(format!("PowerSetRequest failed (Windows error {error})"));
        }
        Ok(Self {
            handle,
            _reason_storage: reason_storage,
        })
    }
}

impl Drop for PowerLease {
    fn drop(&mut self) {
        unsafe {
            let _ = PowerClearRequest(self.handle, POWER_REQUEST_SYSTEM_REQUIRED);
            let _ = CloseHandle(self.handle);
        }
    }
}

struct ActiveLease {
    power: Option<PowerLease>,
    event_store: EventStore,
    session_id: LeaseId,
    finished: bool,
}

impl ActiveLease {
    fn new(power: PowerLease, event_store: EventStore, session_id: LeaseId) -> Self {
        Self {
            power: Some(power),
            event_store,
            session_id,
            finished: false,
        }
    }

    fn finish(mut self, reason: ReleaseReason) -> Result<(), String> {
        self.power.take();
        self.finished = true;
        self.event_store.append(&SessionEvent::lease_released(
            self.session_id,
            unix_time_ms(),
            reason,
        ))
    }
}

impl Drop for ActiveLease {
    fn drop(&mut self) {
        self.power.take();
        if !self.finished {
            let _ = self.event_store.append(&SessionEvent::lease_released(
                self.session_id,
                unix_time_ms(),
                ReleaseReason::RuntimeExit,
            ));
            self.finished = true;
        }
    }
}

pub fn start(
    pid: Option<u32>,
    ttl_minutes: u32,
    allow_battery: bool,
    config: Config,
) -> Result<(), String> {
    let process_id = pid.unwrap_or_else(|| unsafe { GetCurrentProcessId() });
    let (process, creation_time) = ProcessHandle::open(process_id)?;
    if process.wait(0)? {
        return Err(format!("process {process_id} has already exited"));
    }

    let power = power_status()?;
    let on_battery_or_unknown = power.ac_line_status != 1;
    let requested = Duration::from_secs(u64::from(ttl_minutes) * 60);
    let source = if on_battery_or_unknown {
        PowerSourceClass::BatteryOrUnknown
    } else {
        PowerSourceClass::Ac
    };
    let mut policy = PowerLeasePolicy::new(
        0,
        requested.as_millis() as u64,
        BATTERY_LEASE_LIMIT.as_millis() as u64,
        source,
        allow_battery,
    )
    .map_err(|error| match error {
        PowerLeasePolicyError::BatteryNotAuthorized => "AC power could not be confirmed. Connect AC or pass --allow-battery for a bounded lease.".to_string(),
        other => other.to_string(),
    })?;
    let initial_duration = Duration::from_millis(policy.deadline_ms());
    let started = Instant::now();
    let started_at = unix_time_ms();
    let expires_at = started_at.saturating_add(requested.as_millis() as u64);
    let mut leases = LeaseBook::new(32, 24 * 60 * 60 * 1000);
    let identity = ProcessIdentity {
        pid: process_id,
        creation_time_100ns: creation_time,
    };
    let lease_id = leases
        .acquire(identity, started_at, expires_at)
        .map_err(|error| error.to_string())?;

    let event_store = event_store(&config)?;
    event_store.prepare()?;
    let reason = format!("Agent Guardian lease for PID {process_id}");
    let power_lease = PowerLease::acquire(&reason)?;
    let session_id = LeaseId::new_v4();
    event_store.append(&SessionEvent::lease_started(session_id, unix_time_ms()))?;
    let active_lease = ActiveLease::new(power_lease, event_store, session_id);
    println!("Power request active for PID {process_id} (lease {lease_id}).");
    println!(
        "The display remains eligible to turn off. Explicit Sleep, Shutdown, Restart, or critical battery behavior remains under Windows control."
    );
    if on_battery_or_unknown {
        println!(
            "Battery or unknown power source: lease expires in {} minute(s), capped at five minutes; Modern Standby may limit it further.",
            initial_duration.as_secs() / 60
        );
    } else {
        println!(
            "AC power confirmed. Lease expires in {} minute(s). Press Ctrl+C or wait for the target process to exit.",
            initial_duration.as_secs() / 60
        );
    }

    while (started.elapsed().as_millis() as u64) < policy.deadline_ms() {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let remaining = Duration::from_millis(policy.deadline_ms().saturating_sub(elapsed_ms));
        let wait_for = remaining.min(POWER_POLL_INTERVAL);
        if process.wait(wait_for.as_millis().max(1) as u32)? {
            let _ = leases.reap(unix_time_ms(), |_| false);
            println!("Target process exited; releasing the power request.");
            active_lease.finish(ReleaseReason::TargetExited)?;
            return Ok(());
        }

        let power_source = if power_status()?.ac_line_status == 1 {
            PowerSourceClass::Ac
        } else {
            PowerSourceClass::BatteryOrUnknown
        };
        match policy.observe_source(started.elapsed().as_millis() as u64, power_source) {
            PowerLeaseDecision::Release => {
                println!(
                    "Power changed to battery; releasing the power request because --allow-battery was not set."
                );
                active_lease.finish(ReleaseReason::BatteryTransition)?;
                return Ok(());
            }
            PowerLeaseDecision::DeadlineChanged { .. }
                if power_source == PowerSourceClass::BatteryOrUnknown =>
            {
                println!(
                    "Power changed to battery; the remaining lease is capped at five minutes from this transition."
                );
            }
            PowerLeaseDecision::DeadlineChanged { .. } => {
                println!("AC power restored; the original lease duration resumes.");
            }
            PowerLeaseDecision::Unchanged => {}
        }
    }
    let _ = leases.reap(unix_time_ms(), |_| true);
    println!("Lease expired; releasing the power request.");
    active_lease.finish(ReleaseReason::TtlExpired)?;
    Ok(())
}

pub fn install(json: bool, expected_sha256: &str) -> Result<(), String> {
    install::install(json, expected_sha256)
}

pub fn uninstall(json: bool) -> Result<(), String> {
    install::uninstall(json)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn event_store(config: &Config) -> Result<EventStore, String> {
    event_store_at(config, "events.jsonl")
}

fn network_event_store(config: &Config) -> Result<EventStore, String> {
    event_store_at(config, "network-events.jsonl")
}

fn event_store_at(config: &Config, filename: &str) -> Result<EventStore, String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
        "LOCALAPPDATA is unavailable; cannot open the local event log".to_string()
    })?;
    Ok(EventStore::from_logging_config(
        PathBuf::from(local_app_data)
            .join("AgentGuardian")
            .join(filename),
        &config.logging,
    ))
}

pub fn powercfg(arguments: &[&str]) -> Result<String, String> {
    let executable = system_directory()?.join("powercfg.exe");
    let output = Command::new(&executable)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run {}: {error}", executable.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(format!(
            "powercfg {} exited with {}: {}",
            arguments.join(" "),
            output.status,
            detail
        ));
    }
    Ok(stdout.into_owned())
}

fn system_directory() -> Result<PathBuf, String> {
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err(format!(
            "GetSystemDirectoryW failed or returned an overlong path (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    let directory = OsString::from_wide(&buffer[..length as usize]);
    Ok(PathBuf::from(directory))
}

pub fn doctor(json: bool) -> Result<(), String> {
    let available_sleep_states = powercfg(&["/a"]);
    let active_requests = powercfg(&["/requests"]);
    let power = power_status()?;
    let power_source = match power.ac_line_status {
        1 => "ac",
        0 => "battery",
        _ => "unknown",
    };
    let warnings: Vec<_> = [&available_sleep_states, &active_requests]
        .into_iter()
        .filter_map(|result| result.as_ref().err().cloned())
        .collect();
    if json {
        let result = serde_json::json!({
            "guardian_version": env!("CARGO_PKG_VERSION"),
            "os": "windows",
            "architecture": std::env::consts::ARCH,
            "power_source": power_source,
            "battery_percent": (power.battery_life_percent != 255).then_some(power.battery_life_percent),
            "available_sleep_states": available_sleep_states.as_ref().ok(),
            "active_power_requests": active_requests.as_ref().ok(),
            "diagnostic_warnings": warnings,
            "locked_session_verified": false,
            "network_recovery_enabled": false
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?
        );
    } else {
        println!("Guardian {}", env!("CARGO_PKG_VERSION"));
        println!("OS: Windows ({})", std::env::consts::ARCH);
        println!("Power source: {power_source}");
        if power.battery_life_percent != 255 {
            println!("Battery: {}%", power.battery_life_percent);
        }
        println!();
        println!("Available sleep states (powercfg /a):");
        match &available_sleep_states {
            Ok(output) => print!("{output}"),
            Err(error) => println!("Unavailable: {error}"),
        }
        println!();
        println!("Active power requests (powercfg /requests):");
        match &active_requests {
            Ok(output) => print!("{output}"),
            Err(error) => println!("Unavailable: {error}"),
        }
        println!("Locked-session behavior: not verified on this device.");
        println!("Automatic network recovery: disabled.");
        println!("A one-shot saved-profile reconnect requires an explicit network command option.");
    }
    Ok(())
}

pub fn network(
    json: bool,
    probe: bool,
    reconnect_saved_wifi: bool,
    config: Config,
) -> Result<(), String> {
    let mut snapshot = local_network_snapshot()?;
    if reconnect_saved_wifi {
        let outcome = recovery::attempt(&snapshot.summary, true);
        let recovery_event_logged =
            record_recovery_event(network_event_store(&config), &outcome, unix_time_ms());
        let network_change_requested = matches!(
            outcome.disposition,
            "connected" | "connection_not_confirmed" | "connection_confirmation_failed"
        );
        match local_network_snapshot() {
            Ok(mut refreshed) => {
                refreshed.network_change_requested = network_change_requested;
                refreshed.recovery_event_logged = Some(recovery_event_logged);
                refreshed.post_recovery_snapshot_refreshed = Some(true);
                refreshed.recovery = Some(outcome);
                snapshot = refreshed;
            }
            Err(_) => {
                snapshot.network_change_requested = network_change_requested;
                snapshot.recovery_event_logged = Some(recovery_event_logged);
                snapshot.post_recovery_snapshot_refreshed = Some(false);
                snapshot.recovery = Some(outcome);
            }
        }
    }
    if probe {
        let probes = probes::run_bounded_probes();
        snapshot.external_probes_attempted = probes.network_checks_attempted;
        snapshot.internet_connectivity_tested = probes.network_checks_attempted;
        let endpoints = [
            probes
                .microsoft_connect_test
                .attempted
                .then(|| EndpointEvidence {
                    category: "microsoft_ncsi".to_owned(),
                    reachable: probes.microsoft_connect_test.reachable,
                    user_configured_service: false,
                }),
            probes
                .independent_https
                .attempted
                .then(|| EndpointEvidence {
                    category: "independent_https".to_owned(),
                    reachable: probes.independent_https.reachable,
                    user_configured_service: false,
                }),
        ]
        .into_iter()
        .flatten()
        .collect();
        snapshot.connectivity_classification = Some(classify_local_evidence(
            &snapshot.summary,
            &ConnectivityEvidence {
                default_route_present: probes.default_route_present,
                gateway_reachable: probes.gateway_reachable,
                dns_resolves: probes.dns_resolved_for_both,
                captive_portal_suspected: probes.captive_portal_suspected,
                proxy_restricted: probes.proxy_restricted,
                endpoints,
                ..ConnectivityEvidence::default()
            },
        ));
        snapshot.probes = Some(probes);
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?
        );
    } else {
        println!("Local network snapshot");
        println!(
            "Wi-Fi adapters detected: {}",
            snapshot.summary.wifi_adapters_detected
        );
        println!(
            "Wi-Fi adapter operational: {}",
            snapshot.summary.wifi_adapter_operational
        );
        println!(
            "Operational adapters: {}",
            snapshot.summary.operational_adapter_count
        );
        println!(
            "Gateway address reported: {}",
            snapshot.summary.gateway_address_reported
        );
        println!(
            "Gateway reported on an operational adapter: {}",
            snapshot.summary.operational_gateway_address_reported
        );
        println!(
            "Tunnel-type interface reported: {}",
            snapshot.summary.tunnel_adapter_detected
        );
        println!();
        println!("Adapter metadata is summarized without names or address values.");
        println!("A tunnel-type interface is only a hint; VPN state is not confirmed.");
        if let Some(recovery) = snapshot.recovery {
            println!("Saved-profile reconnect result: {}", recovery.disposition);
            println!(
                "Connection confirmed: {}",
                option_label(recovery.connection_confirmed)
            );
            if let Some(code) = recovery.windows_error {
                println!("Windows error: {code}");
            }
            println!("Profile names and Wi-Fi credentials are not displayed or stored.");
            println!(
                "Post-reconnect snapshot refreshed: {}",
                snapshot.post_recovery_snapshot_refreshed.unwrap_or(false)
            );
            println!(
                "Recovery outcome logged locally: {}",
                snapshot.recovery_event_logged.unwrap_or(false)
            );
        }
        if let Some(probes) = &snapshot.probes {
            println!("One-shot network diagnostics were requested.");
            println!("Web requests attempted: {}", probes.web_requests_attempted);
            println!(
                "Microsoft NCSI name resolved: {}",
                option_label(probes.microsoft_dns_resolved)
            );
            println!(
                "Independent HTTPS name resolved: {}",
                option_label(probes.independent_dns_resolved)
            );
            println!(
                "Default route present: {}",
                option_label(probes.default_route_present)
            );
            println!(
                "Gateway reachability: {}",
                gateway_label(probes.gateway_reachable)
            );
            println!(
                "Microsoft connectivity check: {}",
                probe_label(probes.microsoft_connect_test.reachable)
            );
            println!(
                "Independent HTTPS check: {}",
                probe_label(probes.independent_https.reachable)
            );
            println!(
                "Captive portal suspected: {}",
                probes.captive_portal_suspected
            );
            println!("Proxy authentication required: {}", probes.proxy_restricted);
            if let Some(classification) = snapshot.connectivity_classification {
                println!("Connectivity assessment: {classification:?}");
            }
        } else {
            println!("No external probes were attempted.");
        }
        println!("No adapter or saved-profile configuration was changed.");
        if snapshot.network_change_requested {
            println!("A one-shot request may have changed the active Wi-Fi connection.");
        }
    }
    Ok(())
}

pub fn monitor(json: bool, duration_minutes: u32, interval_seconds: u32) -> Result<(), String> {
    monitor::run(json, duration_minutes, interval_seconds)
}

pub fn logs(json: bool, config: Config) -> Result<(), String> {
    let events = read_all_events(&config)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&events).map_err(|error| error.to_string())?
        );
    } else if events.is_empty() {
        println!("No Guardian events are recorded.");
    } else {
        for event in &events {
            let message = match event.kind {
                SessionEventKind::LeaseStarted => "Power lease started",
                SessionEventKind::LeaseReleased(reason) => release_label(reason),
                SessionEventKind::WifiReconnectOutcome(result) => wifi_reconnect_label(result),
            };
            println!(
                "{}  {}  {}",
                event.occurred_at_unix_ms, event.session_id, message
            );
        }
    }
    Ok(())
}

pub fn report(output: Option<PathBuf>, json: bool, config: Config) -> Result<(), String> {
    let events = read_all_events(&config)?;
    let latest_session = events
        .iter()
        .filter(|event| event.kind == SessionEventKind::LeaseStarted)
        .max_by_key(|event| event.occurred_at_unix_ms)
        .map(|event| event.session_id)
        .ok_or_else(|| "no Guardian session events are recorded".to_string())?;
    let report = SessionReport::from_events(&events, latest_session, unix_time_ms())?;
    if let Some(path) = output {
        write_report(&path, &report)?;
        println!("Wrote report to {}.", path.display());
    } else if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
        );
    } else {
        println!("Guardian session report");
        println!("Session: {}", report.session_id);
        println!("Started (Unix ms): {}", report.started_at_unix_ms);
        match (report.ended_at_unix_ms, report.protected_duration_ms) {
            (Some(ended_at), Some(duration)) => {
                println!("Ended (Unix ms): {ended_at}");
                println!("Protected duration: {}", format_duration(duration));
            }
            _ => println!("Status: incomplete; no release event is recorded."),
        }
        println!("Peak active leases: {}", report.peak_active_leases);
        println!("Incidents:");
        for incident in &report.incidents {
            println!("  {}  {}", incident.occurred_at_unix_ms, incident.summary);
        }
        println!("Unverified behavior:");
        for behavior in &report.unverified_behavior {
            println!("  {behavior}");
        }
    }
    Ok(())
}

fn read_all_events(config: &Config) -> Result<Vec<SessionEvent>, String> {
    let mut events = event_store(config)?.read_events()?;
    let network_store = network_event_store(config)?;
    match std::fs::symlink_metadata(network_store.path()) {
        Ok(_) => events.extend(network_store.read_events()?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not inspect local network event log: {error}"
            ));
        }
    }
    events.sort_by_key(|event| event.occurred_at_unix_ms);
    Ok(events)
}

fn recovery_event_result(report: &recovery::RecoveryReport) -> WifiReconnectResult {
    if !report.action_attempted {
        return WifiReconnectResult::NotAttempted;
    }
    match report.disposition {
        "connected" => WifiReconnectResult::AssociationConfirmed,
        "connection_not_confirmed" => WifiReconnectResult::AssociationNotConfirmed,
        "connection_confirmation_failed" => WifiReconnectResult::ConfirmationFailed,
        "connection_request_failed" => WifiReconnectResult::RequestFailed,
        _ => WifiReconnectResult::ConfirmationFailed,
    }
}

fn record_recovery_event(
    store: Result<EventStore, String>,
    report: &recovery::RecoveryReport,
    occurred_at_unix_ms: u64,
) -> bool {
    store
        .and_then(|store| {
            store.append(&SessionEvent::wifi_reconnect_outcome(
                occurred_at_unix_ms,
                recovery_event_result(report),
            ))
        })
        .is_ok()
}

fn wifi_reconnect_label(result: WifiReconnectResult) -> &'static str {
    match result {
        WifiReconnectResult::NotAttempted => "Saved-profile reconnect not attempted",
        WifiReconnectResult::RequestFailed => "Saved-profile reconnect request failed",
        WifiReconnectResult::AssociationConfirmed => "Wi-Fi association confirmed",
        WifiReconnectResult::AssociationNotConfirmed => "Wi-Fi association not confirmed",
        WifiReconnectResult::ConfirmationFailed => "Wi-Fi association confirmation failed",
    }
}

fn write_report(path: &PathBuf, report: &SessionReport) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("could not create report {}: {error}", path.display()))?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "could not write report {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

fn format_duration(duration_ms: u64) -> String {
    let seconds = duration_ms / 1000;
    format!("{}m {}s", seconds / 60, seconds % 60)
}

fn release_label(reason: ReleaseReason) -> &'static str {
    match reason {
        ReleaseReason::TargetExited => "Power lease released when target exited",
        ReleaseReason::TtlExpired => "Power lease expired",
        ReleaseReason::BatteryTransition => "Power lease released after battery transition",
        ReleaseReason::RuntimeError => "Power lease released after runtime error",
        ReleaseReason::RuntimeExit => "Power lease released when runtime exited",
    }
}

#[derive(Debug, Default, serde::Serialize)]
struct LocalNetworkSnapshot {
    #[serde(flatten)]
    summary: LocalNetworkSummary,
    internet_connectivity_tested: bool,
    external_probes_attempted: bool,
    network_settings_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    connectivity_classification: Option<ConnectivityState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    probes: Option<probes::ProbeSummary>,
    network_change_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_event_logged: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_recovery_snapshot_refreshed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery: Option<recovery::RecoveryReport>,
}

fn local_network_snapshot() -> Result<LocalNetworkSnapshot, String> {
    let adapters = get_adapter_addresses()?;
    let observations = adapters.iter().map(|adapter| LocalAdapterObservation {
        is_wifi: adapter.IfType == IF_TYPE_IEEE80211,
        is_tunnel: adapter.IfType == IF_TYPE_TUNNEL,
        operational: adapter.OperStatus == IfOperStatusUp,
        default_gateway_reported: !adapter.FirstGatewayAddress.is_null(),
    });
    let snapshot = LocalNetworkSnapshot {
        summary: LocalNetworkSummary::from_adapters(observations),
        ..LocalNetworkSnapshot::default()
    };
    Ok(snapshot)
}

fn get_adapter_addresses() -> Result<Vec<IP_ADAPTER_ADDRESSES_LH>, String> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_SKIP_FRIENDLY_NAME
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_UNICAST;
    let mut capacity_bytes = NETWORK_BUFFER_START_BYTES;
    for _ in 0..3 {
        if capacity_bytes > NETWORK_BUFFER_MAX_BYTES {
            return Err("network adapter metadata exceeds the one MiB safety limit".into());
        }
        let adapter_capacity =
            capacity_bytes.div_ceil(std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>());
        let mut adapters = vec![IP_ADAPTER_ADDRESSES_LH::default(); adapter_capacity];
        let mut reported_size =
            (adapters.len() * std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>()) as u32;
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                std::ptr::null(),
                adapters.as_mut_ptr(),
                &mut reported_size,
            )
        };
        if result == 0 {
            return Ok(adapters);
        }
        if result == ERROR_NO_DATA {
            return Ok(Vec::new());
        }
        if result != ERROR_BUFFER_OVERFLOW {
            return Err(format!(
                "GetAdaptersAddresses failed with Windows error {result}"
            ));
        }
        capacity_bytes = (reported_size as usize).max(capacity_bytes.saturating_mul(2));
    }
    Err("network adapter list changed repeatedly while it was being read".into())
}

fn power_status() -> Result<SystemPowerStatus, String> {
    let mut status = SystemPowerStatus::default();
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return Err(format!(
            "GetSystemPowerStatus failed (Windows error {})",
            unsafe { GetLastError() }
        ));
    }
    Ok(status)
}

fn probe_label(reachable: bool) -> &'static str {
    if reachable {
        "reachable"
    } else {
        "not confirmed"
    }
}

fn option_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn gateway_label(reachable: Option<bool>) -> &'static str {
    match reachable {
        Some(true) => "confirmed",
        Some(false) => "unreachable",
        None => "not confirmed (ICMP may be filtered)",
    }
}

#[cfg(test)]
mod recovery_event_tests {
    use super::*;
    use std::fs;

    #[test]
    fn recovery_result_is_logged_redacted_and_survives_log_failure() {
        let directory =
            std::env::temp_dir().join(format!("guardian-network-event-test-{}", LeaseId::new_v4()));
        fs::create_dir(&directory).unwrap();
        let store = EventStore::new(directory.join("network-events.jsonl"), 4096, 14);
        let outcome = recovery::RecoveryReport {
            action_attempted: true,
            connection_confirmed: Some(true),
            disposition: "connected",
            windows_error: None,
        };

        assert!(record_recovery_event(Ok(store.clone()), &outcome, 123));
        let events = store.read_events().unwrap();
        assert!(matches!(
            events[0].kind,
            SessionEventKind::WifiReconnectOutcome(WifiReconnectResult::AssociationConfirmed)
        ));
        let serialized = fs::read_to_string(store.path()).unwrap();
        assert!(!serialized.to_ascii_lowercase().contains("ssid"));
        assert!(!serialized.contains("profile"));

        let blocker = directory.join("not-a-directory");
        fs::write(&blocker, b"existing file").unwrap();
        let unavailable_store = EventStore::new(blocker.join("network-events.jsonl"), 4096, 14);
        assert!(!record_recovery_event(Ok(unavailable_store), &outcome, 456));
        assert_eq!(outcome.disposition, "connected");

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn recovery_dispositions_map_to_typed_outcomes() {
        let cases = [
            (
                false,
                "wifi_disconnect_not_confirmed",
                None,
                WifiReconnectResult::NotAttempted,
            ),
            (
                true,
                "connection_request_failed",
                None,
                WifiReconnectResult::RequestFailed,
            ),
            (
                true,
                "connected",
                Some(true),
                WifiReconnectResult::AssociationConfirmed,
            ),
            (
                true,
                "connection_not_confirmed",
                Some(false),
                WifiReconnectResult::AssociationNotConfirmed,
            ),
            (
                true,
                "connection_confirmation_failed",
                None,
                WifiReconnectResult::ConfirmationFailed,
            ),
        ];

        for (action_attempted, disposition, connection_confirmed, expected) in cases {
            let report = recovery::RecoveryReport {
                action_attempted,
                connection_confirmed,
                disposition,
                windows_error: None,
            };
            assert_eq!(recovery_event_result(&report), expected);
        }
    }
}
