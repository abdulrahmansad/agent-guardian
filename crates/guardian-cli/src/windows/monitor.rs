use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use guardian_core::{ConnectivityState, RetryBackoff, classify_local_observation};
use serde::Serialize;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    CancelMibChangeNotify2, MIB_IPFORWARD_ROW2, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE,
    NotifyIpInterfaceChange, NotifyRouteChange2,
};
use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;
use windows_sys::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, PHANDLER_ROUTINE, SetConsoleCtrlHandler,
};

use super::signals::active_default_route_status;
use super::{
    Config, LocalNetworkSnapshot, ProcessHandle, StopEvent, local_network_snapshot,
    network_event_store, record_recovery_event, unix_time_ms,
};

const EVENT_DEBOUNCE: Duration = Duration::from_millis(250);
const AUTO_RECOVERY_GRACE: u64 = 10_000;
const AUTO_RECOVERY_INTERVAL: Duration = Duration::from_secs(5);
const AUTO_RECOVERY_MAX_FAILURES: u8 = 3;

static WAKE_SENDER: OnceLock<SyncSender<MonitorSignal>> = OnceLock::new();
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
enum MonitorSignal {
    NetworkChanged,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NetworkObservation {
    #[serde(flatten)]
    summary: guardian_core::LocalNetworkSummary,
    default_route_present: Option<bool>,
    connected_profile_available: bool,
    #[serde(skip)]
    connected_profile_hint: Option<super::recovery::SavedProfileHint>,
}

#[derive(Serialize)]
struct MonitorRecord<'a> {
    event: &'static str,
    source: Option<&'static str>,
    duration_minutes: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    observation: Option<&'a NetworkObservation>,
}

struct NetworkChangeNotifications {
    handles: Vec<HANDLE>,
}

impl NetworkChangeNotifications {
    fn register() -> Result<(Self, Receiver<MonitorSignal>), String> {
        if WAKE_SENDER.get().is_some() {
            return Err("the local network monitor can run only once per process".into());
        }
        let (sender, receiver) = sync_channel(1);
        WAKE_SENDER
            .set(sender)
            .map_err(|_| "the local network monitor is already initialized".to_string())?;

        let mut notifications = Self {
            handles: Vec::with_capacity(2),
        };
        let mut interface_handle = std::ptr::null_mut();
        let interface_status = unsafe {
            NotifyIpInterfaceChange(
                AF_UNSPEC,
                Some(on_ip_interface_change),
                std::ptr::null(),
                false,
                &mut interface_handle,
            )
        };
        if interface_status != 0 || interface_handle.is_null() {
            return Err(format!(
                "could not register for IP-interface changes (Windows error {interface_status})"
            ));
        }
        notifications.handles.push(interface_handle);

        let mut route_handle = std::ptr::null_mut();
        let route_status = unsafe {
            NotifyRouteChange2(
                AF_UNSPEC,
                Some(on_route_change),
                std::ptr::null(),
                false,
                &mut route_handle,
            )
        };
        if route_status != 0 || route_handle.is_null() {
            return Err(format!(
                "could not register for IP-route changes (Windows error {route_status})"
            ));
        }
        notifications.handles.push(route_handle);
        Ok((notifications, receiver))
    }

    fn cancel(&mut self) -> Option<u32> {
        let mut first_error = None;
        for handle in self.handles.drain(..).rev() {
            let status = unsafe { CancelMibChangeNotify2(handle) };
            if status != 0 && first_error.is_none() {
                first_error = Some(status);
            }
        }
        first_error
    }
}

impl Drop for NetworkChangeNotifications {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}

struct ConsoleHandler;

impl ConsoleHandler {
    fn install() -> Result<Self, String> {
        let handler: PHANDLER_ROUTINE = Some(on_console_control);
        if unsafe { SetConsoleCtrlHandler(handler, 1) } == 0 {
            return Err("could not register the monitor Ctrl+C handler".into());
        }
        Ok(Self)
    }
}

impl Drop for ConsoleHandler {
    fn drop(&mut self) {
        let handler: PHANDLER_ROUTINE = Some(on_console_control);
        let _ = unsafe { SetConsoleCtrlHandler(handler, 0) };
    }
}

pub(super) fn run(
    json: bool,
    guardian_pid: Option<u32>,
    duration_minutes: u32,
    interval_seconds: u32,
    auto_reconnect_saved_wifi: bool,
    config: Config,
    startup_event: Option<&str>,
) -> Result<(), String> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(u64::from(duration_minutes) * 60);
    let _auto_reconnect_lease = auto_reconnect_saved_wifi
        .then(super::acquire_auto_reconnect_monitor_lease)
        .transpose()?;
    let interval = Duration::from_secs(u64::from(interval_seconds));
    let poll_interval = if auto_reconnect_saved_wifi {
        interval.min(AUTO_RECOVERY_INTERVAL)
    } else {
        interval
    };
    let guardian_process = guardian_pid
        .map(ProcessHandle::open)
        .transpose()?
        .map(|(process, _)| process);
    STOP_REQUESTED.store(false, Ordering::Release);
    let stop_event = StopEvent::create()?;
    let (mut notifications, receiver) = NetworkChangeNotifications::register()?;
    let console_handler = ConsoleHandler::install()?;
    let mut observation = unknown_observation();
    let mut snapshot_retry = SnapshotRetrySchedule::new();
    let mut next_poll = started + poll_interval;
    let initial_snapshot_fresh = match observe_local_network(auto_reconnect_saved_wifi) {
        Ok(initial) => {
            observation = initial;
            snapshot_retry.record_success();
            if json {
                print_record(
                    "snapshot",
                    Some("initial"),
                    duration_minutes,
                    Some(&observation),
                )?;
            }
            true
        }
        Err(_) => {
            let retry_after = snapshot_retry.record_failure(0);
            next_poll = started + retry_after;
            report_snapshot_unavailable(json, "initial", retry_after);
            false
        }
    };
    let mut reconnect_schedule = AutoReconnectSchedule::new();
    super::signal_startup_event(startup_event)?;
    if !json {
        println!(
            "Monitoring local IP-interface and route changes for up to {duration_minutes} minute(s)."
        );
        if auto_reconnect_saved_wifi {
            println!(
                "Saved-profile Wi-Fi reconnect is enabled explicitly; attempts are limited to three with backoff."
            );
            println!(
                "Recovery requires Wi-Fi to remain disconnected for 10 seconds with no active default route."
            );
        } else {
            println!(
                "No DNS, HTTP, or ICMP probes, network changes, recovery, or event logging are performed."
            );
        }
        println!("Press Ctrl+C to stop.");
        if initial_snapshot_fresh {
            print_human_observation(&observation, "Initial local snapshot");
        }
    }
    if initial_snapshot_fresh {
        maybe_auto_reconnect(
            &observation,
            auto_reconnect_saved_wifi,
            &mut reconnect_schedule,
            started,
            &config,
            json,
        );
    }

    loop {
        if STOP_REQUESTED.load(Ordering::Acquire) || stop_event.is_signaled()? {
            break;
        }
        if let Some(process) = guardian_process.as_ref()
            && process.wait(0)?
        {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let until_poll = next_poll.saturating_duration_since(now);
        let until_deadline = deadline.saturating_duration_since(now);
        match receiver.recv_timeout(until_poll.min(until_deadline).min(Duration::from_secs(1))) {
            Ok(MonitorSignal::Stop) => break,
            Ok(MonitorSignal::NetworkChanged) => {
                let debounce_deadline = (Instant::now() + EVENT_DEBOUNCE).min(deadline);
                while !STOP_REQUESTED.load(Ordering::Acquire) {
                    let remaining = debounce_deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match receiver.recv_timeout(remaining) {
                        Ok(MonitorSignal::Stop) => break,
                        Ok(MonitorSignal::NetworkChanged) => {}
                        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
                    }
                }
                if STOP_REQUESTED.load(Ordering::Acquire) {
                    break;
                }
                match refresh_with_backoff(
                    &mut observation,
                    &mut snapshot_retry,
                    json,
                    "notification",
                    duration_minutes,
                    auto_reconnect_saved_wifi,
                    started,
                )? {
                    SnapshotRefresh::Fresh => maybe_auto_reconnect(
                        &observation,
                        auto_reconnect_saved_wifi,
                        &mut reconnect_schedule,
                        started,
                        &config,
                        json,
                    ),
                    SnapshotRefresh::Unavailable(retry_after) => {
                        reconnect_schedule.reset_disconnection_context();
                        next_poll = Instant::now() + retry_after;
                    }
                    SnapshotRefresh::Deferred => {}
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    break;
                }
                if Instant::now() >= next_poll {
                    match refresh_with_backoff(
                        &mut observation,
                        &mut snapshot_retry,
                        json,
                        "periodic_refresh",
                        duration_minutes,
                        auto_reconnect_saved_wifi,
                        started,
                    )? {
                        SnapshotRefresh::Fresh => {
                            maybe_auto_reconnect(
                                &observation,
                                auto_reconnect_saved_wifi,
                                &mut reconnect_schedule,
                                started,
                                &config,
                                json,
                            );
                            next_poll = Instant::now() + poll_interval;
                        }
                        SnapshotRefresh::Unavailable(retry_after) => {
                            reconnect_schedule.reset_disconnection_context();
                            next_poll = Instant::now() + retry_after;
                        }
                        SnapshotRefresh::Deferred => {
                            next_poll = Instant::now() + Duration::from_secs(1);
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(console_handler);
    if let Some(error) = notifications.cancel() {
        return Err(format!(
            "could not cleanly unregister a network notification (Windows error {error})"
        ));
    }
    if json {
        print_record("stopped", None, duration_minutes, None)?;
    } else if auto_reconnect_saved_wifi {
        println!(
            "Monitor stopped. No adapter settings were changed; saved-profile reconnect attempts may have changed the active Wi-Fi connection."
        );
    } else {
        println!("Monitor stopped. No network settings were changed.");
    }
    Ok(())
}

struct AutoReconnectSchedule {
    disconnected_since_ms: Option<u64>,
    consecutive_failures: u8,
    awaiting_network_change: bool,
    backoff: RetryBackoff,
}

struct SnapshotRetrySchedule {
    backoff: RetryBackoff,
}

impl SnapshotRetrySchedule {
    fn new() -> Self {
        Self {
            backoff: RetryBackoff::new(1_000, 30_000),
        }
    }

    fn is_ready(&self, now_ms: u64) -> bool {
        self.backoff.is_ready(now_ms)
    }

    fn record_failure(&mut self, now_ms: u64) -> Duration {
        Duration::from_millis(self.backoff.record_failure(now_ms))
    }

    fn record_success(&mut self) {
        self.backoff.record_success();
    }
}

enum SnapshotRefresh {
    Fresh,
    Unavailable(Duration),
    Deferred,
}

impl AutoReconnectSchedule {
    fn new() -> Self {
        Self {
            disconnected_since_ms: None,
            consecutive_failures: 0,
            awaiting_network_change: false,
            backoff: RetryBackoff::new(30_000, 5 * 60_000),
        }
    }

    fn should_attempt(&mut self, observation: &NetworkObservation, now_ms: u64) -> bool {
        let confirmed_wifi_loss = observation.default_route_present == Some(false)
            && classify_local_observation(&observation.summary, &[], false)
                == ConnectivityState::WifiDisconnected;
        if !confirmed_wifi_loss {
            self.disconnected_since_ms = None;
            self.awaiting_network_change = false;
            return false;
        }
        if self.awaiting_network_change {
            return false;
        }
        let since = *self.disconnected_since_ms.get_or_insert(now_ms);
        self.consecutive_failures < AUTO_RECOVERY_MAX_FAILURES
            && now_ms.saturating_sub(since) >= AUTO_RECOVERY_GRACE
            && self.backoff.is_ready(now_ms)
    }

    fn record_result(&mut self, connection_confirmed: Option<bool>, now_ms: u64) {
        if connection_confirmed == Some(true) {
            self.consecutive_failures = 0;
            self.backoff.record_success();
            self.disconnected_since_ms = None;
            self.awaiting_network_change = true;
        } else {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            self.backoff.record_failure(now_ms);
        }
    }

    fn reset_disconnection_context(&mut self) {
        self.disconnected_since_ms = None;
        self.awaiting_network_change = false;
    }
}

fn maybe_auto_reconnect(
    observation: &NetworkObservation,
    enabled: bool,
    schedule: &mut AutoReconnectSchedule,
    started: Instant,
    config: &Config,
    json: bool,
) {
    if !enabled {
        return;
    }
    let now_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    if !schedule.should_attempt(observation, now_ms) {
        return;
    }

    let outcome = super::recovery::attempt_with_profile_hint(
        &observation.summary,
        true,
        observation.connected_profile_hint.as_ref(),
    );
    schedule.record_result(outcome.connection_confirmed, now_ms);
    let logged = if outcome.action_attempted {
        record_recovery_event(network_event_store(config), &outcome, unix_time_ms())
    } else {
        false
    };
    if json {
        let record = serde_json::json!({
            "event": "recovery",
            "source": "automatic_saved_profile",
            "action_attempted": outcome.action_attempted,
            "connection_confirmed": outcome.connection_confirmed,
            "disposition": outcome.disposition,
            "event_logged": logged,
            "attempts_remaining": AUTO_RECOVERY_MAX_FAILURES.saturating_sub(schedule.consecutive_failures),
        });
        println!("{record}");
    } else {
        println!(
            "Automatic saved-profile reconnect: {} (attempted: {}, connection confirmed: {}).",
            outcome.disposition,
            outcome.action_attempted,
            option_label(outcome.connection_confirmed),
        );
    }
}

fn observe_local_network(capture_connected_profile: bool) -> Result<NetworkObservation, String> {
    let LocalNetworkSnapshot { summary, .. } = local_network_snapshot()?;
    let connected_profile_hint = (capture_connected_profile && summary.wifi_adapter_operational)
        .then(super::recovery::connected_profile_hint)
        .flatten();
    Ok(NetworkObservation {
        summary,
        default_route_present: active_default_route_status(),
        connected_profile_available: connected_profile_hint.is_some(),
        connected_profile_hint,
    })
}

fn unknown_observation() -> NetworkObservation {
    NetworkObservation {
        summary: guardian_core::LocalNetworkSummary::default(),
        default_route_present: None,
        connected_profile_available: false,
        connected_profile_hint: None,
    }
}

fn refresh_with_backoff(
    previous: &mut NetworkObservation,
    retry: &mut SnapshotRetrySchedule,
    json: bool,
    source: &'static str,
    duration_minutes: u32,
    capture_connected_profile: bool,
    started: Instant,
) -> Result<SnapshotRefresh, String> {
    let now_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    if !retry.is_ready(now_ms) {
        return Ok(SnapshotRefresh::Deferred);
    }
    match refresh_observation(
        previous,
        json,
        source,
        duration_minutes,
        capture_connected_profile,
    ) {
        Ok(()) => {
            retry.record_success();
            Ok(SnapshotRefresh::Fresh)
        }
        Err(ObservationRefreshError::Unavailable) => {
            let retry_after = retry.record_failure(now_ms);
            report_snapshot_unavailable(json, source, retry_after);
            Ok(SnapshotRefresh::Unavailable(retry_after))
        }
        Err(ObservationRefreshError::Output(error)) => Err(error),
    }
}

fn report_snapshot_unavailable(json: bool, source: &'static str, retry_after: Duration) {
    let retry_after_ms = retry_after.as_millis().min(u64::MAX as u128) as u64;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "event": "snapshot_unavailable",
                "source": source,
                "retry_after_ms": retry_after_ms,
            })
        );
    } else {
        eprintln!(
            "Local network snapshot unavailable; retrying in {} second(s).",
            retry_after.as_secs()
        );
    }
}

fn refresh_observation(
    previous: &mut NetworkObservation,
    json: bool,
    source: &'static str,
    duration_minutes: u32,
    capture_connected_profile: bool,
) -> Result<(), ObservationRefreshError> {
    refresh_observation_with(previous, json, source, duration_minutes, || {
        observe_local_network(capture_connected_profile)
    })
}

fn refresh_observation_with(
    previous: &mut NetworkObservation,
    json: bool,
    source: &'static str,
    duration_minutes: u32,
    observe: impl FnOnce() -> Result<NetworkObservation, String>,
) -> Result<(), ObservationRefreshError> {
    let mut current = observe().map_err(|_| ObservationRefreshError::Unavailable)?;
    if !current.summary.wifi_adapter_operational {
        current.connected_profile_hint = previous.connected_profile_hint.clone();
        current.connected_profile_available = previous.connected_profile_available;
    }
    if !observation_changed(previous, &current) {
        return Ok(());
    }
    *previous = current;
    if json {
        print_record("changed", Some(source), duration_minutes, Some(previous))
            .map_err(ObservationRefreshError::Output)?;
    } else {
        print_human_observation(previous, "Local network metadata changed");
    }
    Ok(())
}

enum ObservationRefreshError {
    Unavailable,
    Output(String),
}

fn observation_changed(previous: &NetworkObservation, current: &NetworkObservation) -> bool {
    previous != current
}

fn print_record(
    event: &'static str,
    source: Option<&'static str>,
    duration_minutes: u32,
    observation: Option<&NetworkObservation>,
) -> Result<(), String> {
    let record = MonitorRecord {
        event,
        source,
        duration_minutes,
        observation,
    };
    println!(
        "{}",
        serde_json::to_string(&record).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn print_human_observation(observation: &NetworkObservation, title: &str) {
    println!("{title}:");
    println!(
        "  Wi-Fi adapter operational: {}",
        observation.summary.wifi_adapter_operational
    );
    println!(
        "  Operational adapters: {}",
        observation.summary.operational_adapter_count
    );
    println!(
        "  Active default route: {}",
        option_label(observation.default_route_present)
    );
    println!(
        "  Gateway address reported: {}",
        observation.summary.operational_gateway_address_reported
    );
    println!(
        "  Tunnel-type interface reported: {}",
        observation.summary.tunnel_adapter_detected
    );
}

fn option_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn signal_monitor(signal: MonitorSignal) {
    if let Some(sender) = WAKE_SENDER.get() {
        let _ = sender.try_send(signal);
    }
}

unsafe extern "system" fn on_ip_interface_change(
    _caller_context: *const std::ffi::c_void,
    _row: *const MIB_IPINTERFACE_ROW,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    signal_monitor(MonitorSignal::NetworkChanged);
}

unsafe extern "system" fn on_route_change(
    _caller_context: *const std::ffi::c_void,
    _row: *const MIB_IPFORWARD_ROW2,
    _notification_type: MIB_NOTIFICATION_TYPE,
) {
    signal_monitor(MonitorSignal::NetworkChanged);
}

unsafe extern "system" fn on_console_control(control_type: u32) -> i32 {
    if control_type == CTRL_C_EVENT || control_type == CTRL_BREAK_EVENT {
        STOP_REQUESTED.store(true, Ordering::Release);
        signal_monitor(MonitorSignal::Stop);
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guardian_core::LocalNetworkSummary;

    #[test]
    fn local_observation_contains_only_aggregate_flags_and_counts() {
        let observation = NetworkObservation {
            summary: LocalNetworkSummary {
                wifi_adapters_detected: true,
                wifi_adapter_operational: true,
                operational_adapter_count: 1,
                gateway_address_reported: true,
                operational_gateway_address_reported: true,
                wifi_gateway_address_reported: true,
                other_gateway_address_reported: false,
                tunnel_adapter_detected: false,
                tunnel_adapter_operational: false,
            },
            default_route_present: Some(true),
            connected_profile_available: true,
            connected_profile_hint: Some(super::super::recovery::SavedProfileHint::from_slice(&[
                112, 114, 105, 118, 97, 116, 101, 45, 116, 101, 115, 116, 0,
            ])),
        };
        let json = serde_json::to_string(&observation).unwrap();
        assert!(json.contains("\"wifi_adapter_operational\":true"));
        assert!(json.contains("\"default_route_present\":true"));
        assert!(!json.contains("\"address\":"));
        assert!(!json.contains("1.1.1.1"));
        assert!(!json.contains("2606:"));
        assert!(!json.contains("SSID"));
        assert!(!json.contains("adapter_name"));
        assert!(!json.contains("private-test"));
    }

    #[test]
    fn unchanged_observations_are_equal_and_do_not_require_an_event_record() {
        let observation = NetworkObservation {
            summary: LocalNetworkSummary::default(),
            default_route_present: None,
            connected_profile_available: false,
            connected_profile_hint: None,
        };
        let same = observation.clone();
        let changed = NetworkObservation {
            default_route_present: Some(false),
            ..observation.clone()
        };
        assert!(!observation_changed(&observation, &same));
        assert!(observation_changed(&observation, &changed));
    }

    #[test]
    fn snapshot_failures_use_bounded_backoff_and_success_resets_it() {
        let mut retry = SnapshotRetrySchedule::new();
        assert!(retry.is_ready(0));
        assert_eq!(retry.record_failure(0), Duration::from_secs(1));
        assert!(!retry.is_ready(999));
        assert!(retry.is_ready(1_000));
        assert_eq!(retry.record_failure(1_000), Duration::from_secs(2));
        assert_eq!(retry.record_failure(3_000), Duration::from_secs(4));
        assert_eq!(retry.record_failure(7_000), Duration::from_secs(8));
        assert_eq!(retry.record_failure(15_000), Duration::from_secs(16));
        assert_eq!(retry.record_failure(31_000), Duration::from_secs(30));
        assert_eq!(retry.record_failure(61_000), Duration::from_secs(30));
        retry.record_success();
        assert!(retry.is_ready(61_000));
        assert_eq!(retry.record_failure(61_000), Duration::from_secs(1));
    }

    #[test]
    fn failed_snapshot_preserves_last_observation_and_discards_raw_error() {
        let mut previous = NetworkObservation {
            summary: LocalNetworkSummary {
                wifi_adapters_detected: true,
                operational_adapter_count: 0,
                ..LocalNetworkSummary::default()
            },
            default_route_present: Some(false),
            connected_profile_available: false,
            connected_profile_hint: None,
        };
        let before = previous.clone();
        let result = refresh_observation_with(&mut previous, false, "notification", 5, || {
            Err("untrusted Windows detail and adapter identifier".into())
        });

        assert!(matches!(result, Err(ObservationRefreshError::Unavailable)));
        assert_eq!(previous, before);
    }

    #[test]
    fn unknown_snapshot_is_not_classified_as_wifi_disconnection() {
        let observation = unknown_observation();
        let mut schedule = AutoReconnectSchedule::new();
        assert!(!schedule.should_attempt(&observation, AUTO_RECOVERY_GRACE + 1));
    }

    #[test]
    fn snapshot_gap_restarts_disconnection_grace_without_resetting_failure_backoff() {
        let disconnected = NetworkObservation {
            summary: LocalNetworkSummary {
                wifi_adapters_detected: true,
                operational_adapter_count: 0,
                ..LocalNetworkSummary::default()
            },
            default_route_present: Some(false),
            connected_profile_available: false,
            connected_profile_hint: None,
        };
        let mut schedule = AutoReconnectSchedule::new();
        assert!(!schedule.should_attempt(&disconnected, 0));
        assert!(schedule.should_attempt(&disconnected, AUTO_RECOVERY_GRACE));
        schedule.record_result(Some(false), AUTO_RECOVERY_GRACE);
        schedule.reset_disconnection_context();
        assert!(!schedule.should_attempt(&disconnected, AUTO_RECOVERY_GRACE + 30_000));
        assert!(schedule.should_attempt(&disconnected, AUTO_RECOVERY_GRACE + 40_000));
    }

    #[test]
    fn automatic_reconnect_is_delayed_backed_off_and_limited() {
        let disconnected = NetworkObservation {
            summary: LocalNetworkSummary {
                wifi_adapters_detected: true,
                operational_adapter_count: 0,
                ..LocalNetworkSummary::default()
            },
            default_route_present: Some(false),
            connected_profile_available: false,
            connected_profile_hint: None,
        };
        let mut schedule = AutoReconnectSchedule::new();

        assert!(!schedule.should_attempt(&disconnected, 0));
        assert!(!schedule.should_attempt(&disconnected, AUTO_RECOVERY_GRACE - 1));
        assert!(schedule.should_attempt(&disconnected, AUTO_RECOVERY_GRACE));

        schedule.record_result(Some(false), AUTO_RECOVERY_GRACE);
        assert!(!schedule.should_attempt(&disconnected, AUTO_RECOVERY_GRACE + 29_999));
        assert!(schedule.should_attempt(&disconnected, AUTO_RECOVERY_GRACE + 30_000));
        schedule.record_result(None, AUTO_RECOVERY_GRACE + 30_000);

        let second_retry = AUTO_RECOVERY_GRACE + 90_000;
        assert!(schedule.should_attempt(&disconnected, second_retry));
        schedule.record_result(Some(false), second_retry);
        assert!(!schedule.should_attempt(&disconnected, second_retry + 600_000));
        assert_eq!(schedule.consecutive_failures, AUTO_RECOVERY_MAX_FAILURES);
    }

    #[test]
    fn other_operational_adapters_do_not_suppress_wifi_recovery_and_routes_still_block_it() {
        let disconnected_wifi = NetworkObservation {
            summary: LocalNetworkSummary {
                wifi_adapters_detected: true,
                operational_adapter_count: 1,
                ..LocalNetworkSummary::default()
            },
            default_route_present: Some(false),
            connected_profile_available: false,
            connected_profile_hint: None,
        };
        let mut schedule = AutoReconnectSchedule::new();
        assert!(!schedule.should_attempt(&disconnected_wifi, 0));
        assert!(schedule.should_attempt(&disconnected_wifi, AUTO_RECOVERY_GRACE));

        let route_still_present = NetworkObservation {
            default_route_present: Some(true),
            ..disconnected_wifi
        };
        let mut schedule = AutoReconnectSchedule::new();
        assert!(!schedule.should_attempt(&route_still_present, AUTO_RECOVERY_GRACE));
    }

    #[test]
    fn confirmed_reconnect_waits_for_fresh_local_network_state() {
        let disconnected = NetworkObservation {
            summary: LocalNetworkSummary {
                wifi_adapters_detected: true,
                operational_adapter_count: 0,
                ..LocalNetworkSummary::default()
            },
            default_route_present: Some(false),
            connected_profile_available: false,
            connected_profile_hint: None,
        };
        let connected = NetworkObservation {
            summary: LocalNetworkSummary {
                wifi_adapters_detected: true,
                wifi_adapter_operational: true,
                operational_adapter_count: 1,
                ..LocalNetworkSummary::default()
            },
            default_route_present: Some(true),
            connected_profile_available: false,
            connected_profile_hint: None,
        };
        let mut schedule = AutoReconnectSchedule::new();
        assert!(!schedule.should_attempt(&disconnected, 0));
        schedule.record_result(Some(true), AUTO_RECOVERY_GRACE);
        assert!(!schedule.should_attempt(&disconnected, 60_000));
        assert!(!schedule.should_attempt(&connected, 60_001));
        assert!(!schedule.should_attempt(&disconnected, 60_002));
        assert_eq!(schedule.consecutive_failures, 0);
    }
}
