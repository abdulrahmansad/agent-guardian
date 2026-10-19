use std::ffi::c_void;
use std::mem::offset_of;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use guardian_core::{
    ConnectivityState, LocalNetworkSummary, RecoveryAction, RecoveryContext,
    classify_local_observation, decide_recovery,
};
use serde::Serialize;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WiFi::{
    WLAN_CONNECTION_ATTRIBUTES, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO,
    WLAN_INTERFACE_INFO_LIST, WLAN_PROFILE_INFO, WLAN_PROFILE_INFO_LIST, WlanCloseHandle,
    WlanConnect, WlanEnumInterfaces, WlanFreeMemory, WlanGetProfileList, WlanOpenHandle,
    WlanQueryInterface, dot11_BSS_type_infrastructure, wlan_connection_mode_profile,
    wlan_interface_state_connected, wlan_interface_state_disconnected,
    wlan_intf_opcode_current_connection,
};
use windows_sys::core::GUID;

const WLAN_CLIENT_VERSION: u32 = 2;
const MAX_WIFI_INTERFACES: usize = 64;
const MAX_SAVED_PROFILES: usize = 1024;
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(20);
const CONFIRM_INTERVAL: Duration = Duration::from_millis(500);
const MAX_CONFIRM_CHECKS: usize =
    (CONFIRM_TIMEOUT.as_millis() / CONFIRM_INTERVAL.as_millis()) as usize + 1;
const ERROR_INVALID_DATA: u32 = 13;

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct RecoveryReport {
    pub action_attempted: bool,
    pub connection_confirmed: Option<bool>,
    pub disposition: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_error: Option<u32>,
}

impl RecoveryReport {
    fn blocked(disposition: &'static str) -> Self {
        Self {
            action_attempted: false,
            connection_confirmed: None,
            disposition,
            windows_error: None,
        }
    }
}

#[derive(Clone, Copy)]
struct WirelessInterface {
    id: GUID,
    state: i32,
}

trait RecoveryBackend {
    fn default_route_present(&mut self) -> Option<bool>;
    fn interfaces(&mut self) -> Result<Vec<WirelessInterface>, u32>;
    fn preferred_profile(
        &mut self,
        interface: &GUID,
        connected_profile_hint: Option<&[u16]>,
    ) -> Result<Option<Vec<u16>>, u32>;
    fn request_connection(&mut self, interface: &GUID, profile: &[u16]) -> Result<(), u32>;
    fn interface_state(&mut self, interface: &GUID) -> Result<Option<i32>, u32>;
    fn wait(&mut self, duration: Duration);
}

/// Attempt one saved-profile reconnect only after the explicit CLI authorization and local
/// evidence both select the non-destructive reconnect action.
pub(super) fn attempt(summary: &LocalNetworkSummary, authorized: bool) -> RecoveryReport {
    attempt_with_profile_hint(summary, authorized, None)
}

/// Recover the profile that was active when the monitor last observed a connected WLAN.
/// The profile hint remains in memory only and is checked against the interface's saved list.
pub(super) fn attempt_with_profile_hint(
    summary: &LocalNetworkSummary,
    authorized: bool,
    connected_profile_hint: Option<&SavedProfileHint>,
) -> RecoveryReport {
    if !authorized {
        return RecoveryReport::blocked("authorization_required");
    }
    let Some(connected_profile_hint) = connected_profile_hint else {
        return RecoveryReport::blocked("connected_profile_unknown");
    };
    if classify_local_observation(summary, &[], false) != ConnectivityState::WifiDisconnected {
        return RecoveryReport::blocked("wifi_disconnect_not_confirmed");
    }

    let mut backend = match NativeWifiBackend::open() {
        Ok(backend) => backend,
        Err(code) => {
            return RecoveryReport {
                windows_error: Some(code),
                ..RecoveryReport::blocked("wlan_api_unavailable")
            };
        }
    };
    run_with_backend_and_profile_hint(
        summary,
        authorized,
        Some(connected_profile_hint.as_slice()),
        &mut backend,
    )
}

#[cfg(test)]
fn run_with_backend(
    summary: &LocalNetworkSummary,
    authorized: bool,
    backend: &mut impl RecoveryBackend,
) -> RecoveryReport {
    run_with_backend_and_profile_hint(summary, authorized, None, backend)
}

fn run_with_backend_and_profile_hint(
    summary: &LocalNetworkSummary,
    authorized: bool,
    connected_profile_hint: Option<&[u16]>,
    backend: &mut impl RecoveryBackend,
) -> RecoveryReport {
    if !authorized {
        return RecoveryReport::blocked("authorization_required");
    }
    let route_present = backend.default_route_present();
    let state = classify_local_observation(summary, &[], false);
    let action = decide_recovery(
        state,
        RecoveryContext {
            evidence_sufficient: route_present == Some(false),
            recovery_authorized: authorized,
            adapter_restarts_in_window: 0,
            restart_limit: 0,
        },
    );
    if action != RecoveryAction::ReconnectSavedWifiProfile {
        return RecoveryReport::blocked(match (state, route_present) {
            (ConnectivityState::WifiDisconnected, Some(true)) => "alternate_route_present",
            (ConnectivityState::WifiDisconnected, None) => "route_state_unknown",
            _ if !authorized => "authorization_required",
            _ => "wifi_disconnect_not_confirmed",
        });
    }

    let interfaces = match backend.interfaces() {
        Ok(interfaces) => interfaces,
        Err(code) => return api_failure("interface_enumeration_failed", code),
    };
    let [interface] = interfaces.as_slice() else {
        return RecoveryReport::blocked("wireless_interface_count_not_one");
    };
    if interface.state != wlan_interface_state_disconnected {
        return RecoveryReport::blocked("wireless_interface_not_disconnected");
    }

    let profile = match backend.preferred_profile(&interface.id, connected_profile_hint) {
        Ok(Some(profile)) => TemporaryProfile(profile),
        Ok(None) => return RecoveryReport::blocked("no_saved_profile"),
        Err(code) => return api_failure("saved_profile_lookup_failed", code),
    };
    if profile.0.len() < 2
        || profile.0.len() > 256
        || profile.0.last() != Some(&0)
        || profile.0[..profile.0.len() - 1].contains(&0)
    {
        return api_failure("saved_profile_data_invalid", ERROR_INVALID_DATA);
    }

    match backend.default_route_present() {
        Some(false) => {}
        Some(true) => return RecoveryReport::blocked("alternate_route_present"),
        None => return RecoveryReport::blocked("route_state_unknown"),
    }
    match backend.interface_state(&interface.id) {
        Ok(Some(state)) if state == wlan_interface_state_disconnected => {}
        Ok(Some(state)) if state == wlan_interface_state_connected => {
            return RecoveryReport::blocked("wireless_interface_connected_before_request");
        }
        Ok(Some(_)) => return RecoveryReport::blocked("wireless_interface_state_changed"),
        Ok(None) => return RecoveryReport::blocked("wireless_interface_missing"),
        Err(code) => return api_failure("wireless_state_revalidation_failed", code),
    }

    let connection_request = backend.request_connection(&interface.id, &profile.0);
    drop(profile);
    if let Err(code) = connection_request {
        return RecoveryReport {
            action_attempted: true,
            connection_confirmed: None,
            disposition: "connection_request_failed",
            windows_error: Some(code),
        };
    }

    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    for check in 0..MAX_CONFIRM_CHECKS {
        if check > 0 && Instant::now() >= deadline {
            break;
        }
        match backend.interface_state(&interface.id) {
            Ok(Some(state)) if state == wlan_interface_state_connected => {
                return RecoveryReport {
                    action_attempted: true,
                    connection_confirmed: Some(true),
                    disposition: "connected",
                    windows_error: None,
                };
            }
            Ok(Some(_)) | Ok(None) => {}
            Err(code) => {
                return RecoveryReport {
                    action_attempted: true,
                    connection_confirmed: None,
                    disposition: "connection_confirmation_failed",
                    windows_error: Some(code),
                };
            }
        }
        if check + 1 == MAX_CONFIRM_CHECKS {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        backend.wait(CONFIRM_INTERVAL.min(remaining));
    }
    RecoveryReport {
        action_attempted: true,
        connection_confirmed: Some(false),
        disposition: "connection_not_confirmed",
        windows_error: None,
    }
}

fn api_failure(disposition: &'static str, code: u32) -> RecoveryReport {
    RecoveryReport {
        windows_error: Some(code),
        ..RecoveryReport::blocked(disposition)
    }
}

struct NativeWifiBackend {
    client: HANDLE,
}

impl NativeWifiBackend {
    fn open() -> Result<Self, u32> {
        let mut negotiated_version = 0;
        let mut client = ptr::null_mut();
        let status = unsafe {
            WlanOpenHandle(
                WLAN_CLIENT_VERSION,
                ptr::null(),
                &mut negotiated_version,
                &mut client,
            )
        };
        if status != 0 || client.is_null() {
            if !client.is_null() {
                let _ = unsafe { WlanCloseHandle(client, ptr::null()) };
            }
            if status == 0 {
                return Err(ERROR_INVALID_DATA);
            }
            return Err(status);
        }
        Ok(Self { client })
    }

    fn enumerate_interfaces(&self) -> Result<Vec<WirelessInterface>, u32> {
        let mut list = ptr::null_mut::<WLAN_INTERFACE_INFO_LIST>();
        let status = unsafe { WlanEnumInterfaces(self.client, ptr::null(), &mut list) };
        if status != 0 {
            if !list.is_null() {
                unsafe { WlanFreeMemory(list.cast()) };
            }
            return Err(status);
        }
        if list.is_null() {
            return Err(ERROR_INVALID_DATA);
        }
        let _allocation = WlanAllocation(list.cast());
        let count = unsafe { (*list).dwNumberOfItems as usize };
        if count > MAX_WIFI_INTERFACES {
            return Err(ERROR_INVALID_DATA);
        }
        let first = unsafe {
            list.cast::<u8>()
                .add(offset_of!(WLAN_INTERFACE_INFO_LIST, InterfaceInfo))
                .cast::<WLAN_INTERFACE_INFO>()
        };
        Ok((0..count)
            .map(|index| {
                let info = unsafe { first.add(index).read() };
                WirelessInterface {
                    id: info.InterfaceGuid,
                    state: info.isState,
                }
            })
            .collect())
    }

    fn connected_profile(&self, interface: &GUID) -> Result<Option<SavedProfileHint>, u32> {
        let mut data_size = 0_u32;
        let mut data = ptr::null_mut::<c_void>();
        let mut value_type = 0;
        let status = unsafe {
            WlanQueryInterface(
                self.client,
                interface,
                wlan_intf_opcode_current_connection,
                ptr::null(),
                &mut data_size,
                &mut data,
                &mut value_type,
            )
        };
        if status != 0 || data.is_null() {
            if !data.is_null() {
                unsafe { WlanFreeMemory(data) };
            }
            return if status == 0 {
                Err(ERROR_INVALID_DATA)
            } else {
                Err(status)
            };
        }
        if data_size < std::mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>() as u32 {
            unsafe { WlanFreeMemory(data) };
            return Err(ERROR_INVALID_DATA);
        }
        let allocation = WlanConnectionAttributesAllocation(data.cast());
        let attributes = unsafe { &*allocation.0 };
        if attributes.isState != wlan_interface_state_connected {
            return Ok(None);
        }
        let Some(end) = attributes.strProfileName.iter().position(|unit| *unit == 0) else {
            return Err(ERROR_INVALID_DATA);
        };
        if end == 0 {
            return Err(ERROR_INVALID_DATA);
        }
        let mut profile = attributes.strProfileName[..end].to_vec();
        profile.push(0);
        Ok(Some(SavedProfileHint(profile)))
    }
}

impl RecoveryBackend for NativeWifiBackend {
    fn default_route_present(&mut self) -> Option<bool> {
        super::signals::active_default_route_status()
    }

    fn interfaces(&mut self) -> Result<Vec<WirelessInterface>, u32> {
        self.enumerate_interfaces()
    }

    fn preferred_profile(
        &mut self,
        interface: &GUID,
        connected_profile_hint: Option<&[u16]>,
    ) -> Result<Option<Vec<u16>>, u32> {
        let mut list = ptr::null_mut::<WLAN_PROFILE_INFO_LIST>();
        let status = unsafe { WlanGetProfileList(self.client, interface, ptr::null(), &mut list) };
        if status != 0 {
            if !list.is_null() {
                unsafe { WlanFreeMemory(list.cast()) };
            }
            return Err(status);
        }
        if list.is_null() {
            return Err(ERROR_INVALID_DATA);
        }
        let count = unsafe { (*list).dwNumberOfItems as usize };
        if count > MAX_SAVED_PROFILES {
            unsafe { WlanFreeMemory(list.cast()) };
            return Err(ERROR_INVALID_DATA);
        }
        if count == 0 {
            unsafe { WlanFreeMemory(list.cast()) };
            return Ok(None);
        }
        let allocation = WlanProfileListAllocation { list, count };
        let first = unsafe {
            list.cast::<u8>()
                .add(offset_of!(WLAN_PROFILE_INFO_LIST, ProfileInfo))
                .cast::<WLAN_PROFILE_INFO>()
        };
        let profile = if let Some(hint) = connected_profile_hint {
            (0..count).find_map(|index| {
                let name = unsafe { &(*first.add(index)).strProfileName };
                let end = name.iter().position(|unit| *unit == 0)?;
                (name[..end] == hint[..hint.len().saturating_sub(1)]).then(|| {
                    let mut profile = name[..end].to_vec();
                    profile.push(0);
                    profile
                })
            })
        } else {
            let name = unsafe { &(*first).strProfileName };
            let Some(end) = name.iter().position(|unit| *unit == 0) else {
                return Err(ERROR_INVALID_DATA);
            };
            if end == 0 {
                return Err(ERROR_INVALID_DATA);
            }
            let mut profile = name[..end].to_vec();
            profile.push(0);
            Some(profile)
        };
        drop(allocation);
        Ok(profile)
    }

    fn request_connection(&mut self, interface: &GUID, profile: &[u16]) -> Result<(), u32> {
        let parameters = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: profile.as_ptr(),
            pDot11Ssid: ptr::null_mut(),
            pDesiredBssidList: ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let status = unsafe { WlanConnect(self.client, interface, &parameters, ptr::null()) };
        (status == 0).then_some(()).ok_or(status)
    }

    fn interface_state(&mut self, interface: &GUID) -> Result<Option<i32>, u32> {
        let interfaces = self.enumerate_interfaces()?;
        Ok(interfaces
            .iter()
            .find(|candidate| guid_equal(&candidate.id, interface))
            .map(|candidate| candidate.state))
    }

    fn wait(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

/// Read the connected profile while it is available so automatic recovery can reconnect
/// that exact saved profile after the interface disconnects. The value is zeroed on drop.
pub(super) fn connected_profile_hint() -> Option<SavedProfileHint> {
    let backend = NativeWifiBackend::open().ok()?;
    let interfaces = backend.enumerate_interfaces().ok()?;
    let [interface] = interfaces.as_slice() else {
        return None;
    };
    if interface.state != wlan_interface_state_connected {
        return None;
    }
    backend.connected_profile(&interface.id).ok().flatten()
}

fn guid_equal(left: &GUID, right: &GUID) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

impl Drop for NativeWifiBackend {
    fn drop(&mut self) {
        if !self.client.is_null() {
            let _ = unsafe { WlanCloseHandle(self.client, ptr::null()) };
        }
    }
}

struct WlanAllocation(*mut c_void);

impl Drop for WlanAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WlanFreeMemory(self.0) };
        }
    }
}

struct WlanProfileListAllocation {
    list: *mut WLAN_PROFILE_INFO_LIST,
    count: usize,
}

impl Drop for WlanProfileListAllocation {
    fn drop(&mut self) {
        let first = unsafe {
            self.list
                .cast::<u8>()
                .add(offset_of!(WLAN_PROFILE_INFO_LIST, ProfileInfo))
                .cast::<WLAN_PROFILE_INFO>()
        };
        for index in 0..self.count {
            unsafe { (*first.add(index)).strProfileName.fill(0) };
        }
        unsafe { WlanFreeMemory(self.list.cast()) };
    }
}

struct TemporaryProfile(Vec<u16>);

impl Drop for TemporaryProfile {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SavedProfileHint(Vec<u16>);

impl SavedProfileHint {
    pub(super) fn as_slice(&self) -> &[u16] {
        &self.0
    }

    #[cfg(test)]
    pub(super) fn from_slice(profile: &[u16]) -> Self {
        Self(profile.to_vec())
    }
}

impl Drop for SavedProfileHint {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

struct WlanConnectionAttributesAllocation(*mut WLAN_CONNECTION_ATTRIBUTES);

impl Drop for WlanConnectionAttributesAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                std::ptr::write_bytes(self.0, 0, 1);
                WlanFreeMemory(self.0.cast());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeBackend {
        route: Option<bool>,
        interfaces: Vec<WirelessInterface>,
        profile: Option<Vec<u16>>,
        connected_after_request: bool,
        route_after_initial_check: Option<Option<bool>>,
        state_at_revalidation: Option<i32>,
        connect_requests: usize,
        route_checks: usize,
        read_calls: usize,
        waits: usize,
    }

    impl RecoveryBackend for FakeBackend {
        fn default_route_present(&mut self) -> Option<bool> {
            self.read_calls += 1;
            let route = if self.route_checks == 0 {
                self.route
            } else {
                self.route_after_initial_check.unwrap_or(self.route)
            };
            self.route_checks += 1;
            route
        }

        fn interfaces(&mut self) -> Result<Vec<WirelessInterface>, u32> {
            self.read_calls += 1;
            Ok(self.interfaces.clone())
        }

        fn preferred_profile(
            &mut self,
            _interface: &GUID,
            connected_profile_hint: Option<&[u16]>,
        ) -> Result<Option<Vec<u16>>, u32> {
            self.read_calls += 1;
            Ok(match connected_profile_hint {
                Some(hint) => self
                    .profile
                    .as_ref()
                    .filter(|profile| profile.as_slice() == hint)
                    .cloned(),
                None => self.profile.clone(),
            })
        }

        fn request_connection(&mut self, _interface: &GUID, _profile: &[u16]) -> Result<(), u32> {
            self.connect_requests += 1;
            Ok(())
        }

        fn interface_state(&mut self, _interface: &GUID) -> Result<Option<i32>, u32> {
            self.read_calls += 1;
            if self.connect_requests == 0 {
                Ok(Some(self.state_at_revalidation.unwrap_or_else(|| {
                    self.interfaces
                        .first()
                        .map_or(-1, |interface| interface.state)
                })))
            } else if self.connected_after_request {
                Ok(Some(wlan_interface_state_connected))
            } else {
                Ok(Some(wlan_interface_state_disconnected))
            }
        }

        fn wait(&mut self, _duration: Duration) {
            self.waits += 1;
        }
    }

    fn disconnected_summary() -> LocalNetworkSummary {
        LocalNetworkSummary {
            wifi_adapters_detected: true,
            ..LocalNetworkSummary::default()
        }
    }

    fn interface(state: i32) -> WirelessInterface {
        WirelessInterface {
            id: GUID::from_u128(1),
            state,
        }
    }

    fn ready_backend() -> FakeBackend {
        FakeBackend {
            route: Some(false),
            interfaces: vec![interface(wlan_interface_state_disconnected)],
            profile: Some(vec![b'p' as u16, 0]),
            connected_after_request: true,
            ..FakeBackend::default()
        }
    }

    #[test]
    fn no_network_apis_are_called_without_explicit_authorization() {
        let mut backend = ready_backend();
        let report = run_with_backend(&disconnected_summary(), false, &mut backend);
        assert_eq!(report.disposition, "authorization_required");
        assert_eq!(backend.read_calls, 0);
        assert_eq!(backend.connect_requests, 0);
    }

    #[test]
    fn recovery_requires_confirmed_no_route_and_wifi_disconnect() {
        for route in [Some(true), None] {
            let mut backend = ready_backend();
            backend.route = route;
            let report = run_with_backend(&disconnected_summary(), true, &mut backend);
            assert!(!report.action_attempted);
            assert_eq!(backend.connect_requests, 0);
        }

        let mut backend = ready_backend();
        let mut connected = disconnected_summary();
        connected.wifi_adapter_operational = true;
        connected.operational_adapter_count = 1;
        let report = run_with_backend(&connected, true, &mut backend);
        assert_eq!(report.disposition, "wifi_disconnect_not_confirmed");
        assert_eq!(backend.connect_requests, 0);
    }

    #[test]
    fn automatic_recovery_requires_the_exact_previously_connected_saved_profile() {
        let connected_profile = [b'p' as u16, 0];
        let mut backend = ready_backend();
        let report = run_with_backend_and_profile_hint(
            &disconnected_summary(),
            true,
            Some(&connected_profile),
            &mut backend,
        );
        assert!(report.action_attempted);
        assert_eq!(report.connection_confirmed, Some(true));
        assert_eq!(backend.connect_requests, 1);

        let wrong_profile = [b'x' as u16, 0];
        let mut backend = ready_backend();
        let report = run_with_backend_and_profile_hint(
            &disconnected_summary(),
            true,
            Some(&wrong_profile),
            &mut backend,
        );
        assert!(!report.action_attempted);
        assert_eq!(report.disposition, "no_saved_profile");
        assert_eq!(backend.connect_requests, 0);
    }

    #[test]
    fn automatic_recovery_does_not_guess_when_connected_profile_is_unknown() {
        let report = attempt_with_profile_hint(&disconnected_summary(), true, None);
        assert!(!report.action_attempted);
        assert_eq!(report.disposition, "connected_profile_unknown");
    }

    #[test]
    fn alternate_routes_and_ambiguous_interfaces_never_trigger_connect() {
        let mut alternate = ready_backend();
        alternate
            .interfaces
            .push(interface(wlan_interface_state_disconnected));
        let report = run_with_backend(&disconnected_summary(), true, &mut alternate);
        assert_eq!(report.disposition, "wireless_interface_count_not_one");
        assert_eq!(alternate.connect_requests, 0);

        let mut route = ready_backend();
        route.route = Some(true);
        let report = run_with_backend(&disconnected_summary(), true, &mut route);
        assert_eq!(report.disposition, "alternate_route_present");
        assert_eq!(route.connect_requests, 0);

        let mut route_appeared_during_preparation = ready_backend();
        route_appeared_during_preparation.route_after_initial_check = Some(Some(true));
        let report = run_with_backend(
            &disconnected_summary(),
            true,
            &mut route_appeared_during_preparation,
        );
        assert_eq!(report.disposition, "alternate_route_present");
        assert_eq!(route_appeared_during_preparation.connect_requests, 0);

        let mut wifi_reconnected_during_preparation = ready_backend();
        wifi_reconnected_during_preparation.state_at_revalidation =
            Some(wlan_interface_state_connected);
        let report = run_with_backend(
            &disconnected_summary(),
            true,
            &mut wifi_reconnected_during_preparation,
        );
        assert_eq!(
            report.disposition,
            "wireless_interface_connected_before_request"
        );
        assert_eq!(wifi_reconnected_during_preparation.connect_requests, 0);
    }

    #[test]
    fn only_a_single_disconnected_interface_with_a_saved_profile_is_requested_once() {
        let mut connected_interface = ready_backend();
        connected_interface.interfaces = vec![interface(wlan_interface_state_connected)];
        let report = run_with_backend(&disconnected_summary(), true, &mut connected_interface);
        assert_eq!(report.disposition, "wireless_interface_not_disconnected");
        assert_eq!(connected_interface.connect_requests, 0);

        let mut no_profile = ready_backend();
        no_profile.profile = None;
        let report = run_with_backend(&disconnected_summary(), true, &mut no_profile);
        assert_eq!(report.disposition, "no_saved_profile");
        assert_eq!(no_profile.connect_requests, 0);

        let mut backend = ready_backend();
        let report = run_with_backend(&disconnected_summary(), true, &mut backend);
        assert_eq!(report.disposition, "connected");
        assert_eq!(report.connection_confirmed, Some(true));
        assert_eq!(backend.connect_requests, 1);
    }

    #[test]
    fn connection_confirmation_times_out_without_repeating_the_request() {
        let mut backend = ready_backend();
        backend.connected_after_request = false;
        let report = run_with_backend(&disconnected_summary(), true, &mut backend);
        assert_eq!(report.disposition, "connection_not_confirmed");
        assert_eq!(report.connection_confirmed, Some(false));
        assert_eq!(backend.connect_requests, 1);
        assert_eq!(backend.waits, 40);
    }
}
