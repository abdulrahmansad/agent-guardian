use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

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
use super::{LocalNetworkSnapshot, local_network_snapshot};

const EVENT_DEBOUNCE: Duration = Duration::from_millis(250);

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

pub(super) fn run(json: bool, duration_minutes: u32, interval_seconds: u32) -> Result<(), String> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(u64::from(duration_minutes) * 60);
    let interval = Duration::from_secs(u64::from(interval_seconds));
    let mut next_poll = started + interval;
    STOP_REQUESTED.store(false, Ordering::Release);
    let (mut notifications, receiver) = NetworkChangeNotifications::register()?;
    let console_handler = ConsoleHandler::install()?;
    let mut observation = observe_local_network()?;
    if json {
        print_record(
            "snapshot",
            Some("initial"),
            duration_minutes,
            Some(&observation),
        )?;
    } else {
        println!(
            "Monitoring local IP-interface and route changes for up to {duration_minutes} minute(s)."
        );
        println!(
            "No DNS, HTTP, or ICMP probes, network changes, recovery, or event logging are performed."
        );
        println!("Press Ctrl+C to stop.");
        print_human_observation(&observation, "Initial local snapshot");
    }

    loop {
        if STOP_REQUESTED.load(Ordering::Acquire) {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let until_poll = next_poll.saturating_duration_since(now);
        let until_deadline = deadline.saturating_duration_since(now);
        match receiver.recv_timeout(until_poll.min(until_deadline)) {
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
                refresh_observation(&mut observation, json, "notification", duration_minutes)?;
            }
            Err(RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    break;
                }
                if Instant::now() >= next_poll {
                    refresh_observation(
                        &mut observation,
                        json,
                        "periodic_refresh",
                        duration_minutes,
                    )?;
                    next_poll = Instant::now() + interval;
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
    } else {
        println!("Monitor stopped. No network settings were changed.");
    }
    Ok(())
}

fn observe_local_network() -> Result<NetworkObservation, String> {
    let LocalNetworkSnapshot { summary, .. } = local_network_snapshot()?;
    Ok(NetworkObservation {
        summary,
        default_route_present: active_default_route_status(),
    })
}

fn refresh_observation(
    previous: &mut NetworkObservation,
    json: bool,
    source: &'static str,
    duration_minutes: u32,
) -> Result<(), String> {
    let current = observe_local_network()?;
    if !observation_changed(previous, &current) {
        return Ok(());
    }
    *previous = current;
    if json {
        print_record("changed", Some(source), duration_minutes, Some(previous))?;
    } else {
        print_human_observation(previous, "Local network metadata changed");
    }
    Ok(())
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
        };
        let json = serde_json::to_string(&observation).unwrap();
        assert!(json.contains("\"wifi_adapter_operational\":true"));
        assert!(json.contains("\"default_route_present\":true"));
        assert!(!json.contains("\"address\":"));
        assert!(!json.contains("1.1.1.1"));
        assert!(!json.contains("2606:"));
        assert!(!json.contains("SSID"));
        assert!(!json.contains("adapter_name"));
    }

    #[test]
    fn unchanged_observations_are_equal_and_do_not_require_an_event_record() {
        let observation = NetworkObservation {
            summary: LocalNetworkSummary::default(),
            default_route_present: None,
        };
        let same = observation.clone();
        let changed = NetworkObservation {
            default_route_present: Some(false),
            ..observation.clone()
        };
        assert!(!observation_changed(&observation, &same));
        assert!(observation_changed(&observation, &changed));
    }
}
