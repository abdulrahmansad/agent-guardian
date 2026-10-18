use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

/// Privacy-limited observation of one adapter. It intentionally omits names and addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalAdapterObservation {
    pub is_wifi: bool,
    pub is_tunnel: bool,
    pub operational: bool,
    pub default_gateway_reported: bool,
}

/// Aggregate local connectivity metadata without retaining adapter identifiers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalNetworkSummary {
    pub wifi_adapters_detected: bool,
    pub wifi_adapter_operational: bool,
    pub operational_adapter_count: u32,
    pub gateway_address_reported: bool,
    pub operational_gateway_address_reported: bool,
    pub wifi_gateway_address_reported: bool,
    pub other_gateway_address_reported: bool,
    /// A tunnel-type interface is only a hint; it does not prove a VPN is active.
    pub tunnel_adapter_detected: bool,
    pub tunnel_adapter_operational: bool,
}

impl LocalNetworkSummary {
    pub fn from_adapters(adapters: impl IntoIterator<Item = LocalAdapterObservation>) -> Self {
        let mut summary = Self::default();
        for adapter in adapters {
            if adapter.is_wifi {
                summary.wifi_adapters_detected = true;
                summary.wifi_adapter_operational |= adapter.operational;
                summary.wifi_gateway_address_reported |= adapter.default_gateway_reported;
            }
            if adapter.operational {
                summary.operational_adapter_count =
                    summary.operational_adapter_count.saturating_add(1);
            }
            if adapter.default_gateway_reported {
                summary.gateway_address_reported = true;
                summary.operational_gateway_address_reported |= adapter.operational;
                if !adapter.is_wifi {
                    summary.other_gateway_address_reported = true;
                }
            }
            if adapter.is_tunnel {
                summary.tunnel_adapter_detected = true;
                summary.tunnel_adapter_operational |= adapter.operational;
            }
        }
        summary
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectivityState {
    Healthy,
    Suspect,
    TransientFailure,
    WifiDisconnected,
    WifiConnectedNoRoute,
    LocalNetworkOnly,
    DnsFailure,
    CaptivePortal,
    ProxyRestricted,
    VpnStateChange,
    UpstreamInternetFailure,
    RemoteServiceFailure,
    AdapterFailure,
    UnknownFailure,
    RecoveryInProgress,
    Verifying,
    Backoff,
    UserInteractionRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointEvidence {
    /// Stable category only; do not store URLs, headers, or credentials here.
    pub category: String,
    pub reachable: bool,
    pub user_configured_service: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectivityEvidence {
    pub wifi_connected: Option<bool>,
    pub default_route_present: Option<bool>,
    pub gateway_reachable: Option<bool>,
    pub dns_resolves: Option<bool>,
    pub captive_portal_detected: bool,
    pub captive_portal_suspected: bool,
    pub proxy_restricted: bool,
    pub vpn_state_changed: bool,
    pub adapter_reports_fault: bool,
    pub endpoints: Vec<EndpointEvidence>,
}

/// Classify observations without making network changes. Missing evidence stays conservative.
pub fn classify_connectivity(evidence: &ConnectivityEvidence) -> ConnectivityState {
    if evidence.captive_portal_detected || evidence.captive_portal_suspected {
        return ConnectivityState::CaptivePortal;
    }
    if evidence.proxy_restricted {
        return ConnectivityState::ProxyRestricted;
    }
    if evidence.vpn_state_changed {
        return ConnectivityState::VpnStateChange;
    }
    if evidence.wifi_connected == Some(false) {
        return ConnectivityState::WifiDisconnected;
    }
    if evidence.adapter_reports_fault {
        return ConnectivityState::AdapterFailure;
    }
    if evidence.default_route_present == Some(false) {
        return match evidence.wifi_connected {
            Some(true) => ConnectivityState::WifiConnectedNoRoute,
            Some(false) => ConnectivityState::WifiDisconnected,
            None => ConnectivityState::UnknownFailure,
        };
    }

    if evidence.gateway_reachable == Some(false) {
        return if evidence.wifi_connected == Some(true) {
            ConnectivityState::LocalNetworkOnly
        } else {
            ConnectivityState::UnknownFailure
        };
    }

    let independent: Vec<_> = evidence
        .endpoints
        .iter()
        .filter(|probe| !probe.user_configured_service)
        .collect();
    let unique_categories: HashSet<_> = independent
        .iter()
        .map(|probe| probe.category.as_str())
        .collect();
    if independent.iter().any(|probe| probe.category.is_empty())
        || unique_categories.len() != independent.len()
    {
        return ConnectivityState::Suspect;
    }
    let independent_ok = independent.iter().filter(|probe| probe.reachable).count();
    let independent_failed = independent.iter().filter(|probe| !probe.reachable).count();
    let service_failed = evidence
        .endpoints
        .iter()
        .any(|probe| probe.user_configured_service && !probe.reachable);
    if independent_ok >= 2 {
        if evidence.dns_resolves == Some(false) {
            return ConnectivityState::DnsFailure;
        }
        if evidence.dns_resolves != Some(true)
            || evidence.default_route_present != Some(true)
            || evidence.gateway_reachable != Some(true)
        {
            return ConnectivityState::Suspect;
        }
        if service_failed {
            return ConnectivityState::RemoteServiceFailure;
        }
        return ConnectivityState::Healthy;
    }
    if evidence.gateway_reachable == Some(true) && evidence.dns_resolves == Some(false) {
        return ConnectivityState::DnsFailure;
    }
    if independent_failed >= 2
        && evidence.gateway_reachable == Some(true)
        && evidence.dns_resolves == Some(true)
    {
        return ConnectivityState::UpstreamInternetFailure;
    }
    if independent_ok > 0 || independent_failed > 0 || service_failed {
        return ConnectivityState::Suspect;
    }
    if evidence.wifi_connected == Some(true) && evidence.default_route_present == Some(true) {
        return ConnectivityState::Suspect;
    }
    ConnectivityState::UnknownFailure
}

/// Conservatively combine local adapter metadata with independent probe outcomes.
/// DNS and gateway reachability stay unknown because this snapshot does not test them.
pub fn classify_local_observation(
    summary: &LocalNetworkSummary,
    endpoints: &[EndpointEvidence],
    captive_portal_suspected: bool,
) -> ConnectivityState {
    classify_local_evidence(
        summary,
        &ConnectivityEvidence {
            captive_portal_suspected,
            endpoints: endpoints.to_vec(),
            ..ConnectivityEvidence::default()
        },
    )
}

/// Add privacy-limited local Wi-Fi state to independently collected diagnostics.
pub fn classify_local_evidence(
    summary: &LocalNetworkSummary,
    evidence: &ConnectivityEvidence,
) -> ConnectivityState {
    let wifi_connected = if summary.wifi_adapter_operational {
        Some(true)
    } else if summary.wifi_adapters_detected && summary.operational_adapter_count == 0 {
        Some(false)
    } else {
        None
    };
    let mut evidence = evidence.clone();
    evidence.wifi_connected = wifi_connected;
    classify_connectivity(&evidence)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    NoAction,
    WaitAndRecheck,
    NotifyUser,
    ReconnectSavedWifiProfile,
    RestartAdapter,
}

/// Hard upper bound for automatic adapter resets in any rolling policy window.
pub const MAX_ADAPTER_RESTARTS_PER_WINDOW: u8 = 3;

/// Tracks attempted adapter restarts in a rolling window. A failed restart still consumes budget.
#[derive(Debug, Clone)]
pub struct AdapterRestartBudget {
    attempts_ms: VecDeque<u64>,
    maximum_attempts: u8,
    window_ms: u64,
}

impl AdapterRestartBudget {
    pub fn new(maximum_attempts: u8, window_ms: u64) -> Self {
        Self {
            attempts_ms: VecDeque::new(),
            maximum_attempts: maximum_attempts.min(MAX_ADAPTER_RESTARTS_PER_WINDOW),
            window_ms: window_ms.max(1),
        }
    }

    pub fn attempts_in_window(&mut self, now_ms: u64) -> u8 {
        self.expire_old_attempts(now_ms);
        self.attempts_ms.len().min(u8::MAX as usize) as u8
    }

    pub fn can_attempt(&mut self, now_ms: u64) -> bool {
        self.attempts_in_window(now_ms) < self.maximum_attempts
    }

    pub fn record_attempt(&mut self, now_ms: u64) -> Result<(), RestartBudgetExhausted> {
        self.expire_old_attempts(now_ms);
        if self.attempts_ms.len() >= usize::from(self.maximum_attempts) {
            return Err(RestartBudgetExhausted);
        }
        self.attempts_ms.push_back(now_ms);
        Ok(())
    }

    fn expire_old_attempts(&mut self, now_ms: u64) {
        while self
            .attempts_ms
            .front()
            .is_some_and(|attempt| now_ms.saturating_sub(*attempt) >= self.window_ms)
        {
            self.attempts_ms.pop_front();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartBudgetExhausted;

impl std::fmt::Display for RestartBudgetExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "adapter restart budget is exhausted")
    }
}

impl std::error::Error for RestartBudgetExhausted {}

/// Deterministic exponential retry delay with a caller-supplied monotonic millisecond clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryBackoff {
    base_delay_ms: u64,
    maximum_delay_ms: u64,
    failures: u32,
    next_attempt_ms: Option<u64>,
}

impl RetryBackoff {
    pub fn new(base_delay_ms: u64, maximum_delay_ms: u64) -> Self {
        let base_delay_ms = base_delay_ms.max(1);
        Self {
            base_delay_ms,
            maximum_delay_ms: maximum_delay_ms.max(base_delay_ms),
            failures: 0,
            next_attempt_ms: None,
        }
    }

    pub fn record_failure(&mut self, now_ms: u64) -> u64 {
        self.failures = self.failures.saturating_add(1);
        let shift = self.failures.saturating_sub(1).min(63);
        let multiplier = 1_u64.checked_shl(shift).unwrap_or(u64::MAX);
        let delay = self
            .base_delay_ms
            .saturating_mul(multiplier)
            .min(self.maximum_delay_ms);
        self.next_attempt_ms = Some(now_ms.saturating_add(delay));
        delay
    }

    pub fn record_success(&mut self) {
        self.failures = 0;
        self.next_attempt_ms = None;
    }

    pub fn is_ready(&self, now_ms: u64) -> bool {
        self.next_attempt_ms
            .is_none_or(|deadline| now_ms >= deadline)
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryContext {
    pub evidence_sufficient: bool,
    pub recovery_authorized: bool,
    pub adapter_restarts_in_window: u8,
    pub restart_limit: u8,
}

/// Pure policy decision. Callers must separately execute any permitted action.
pub fn decide_recovery(state: ConnectivityState, context: RecoveryContext) -> RecoveryAction {
    if !context.evidence_sufficient {
        return RecoveryAction::WaitAndRecheck;
    }
    match state {
        ConnectivityState::WifiDisconnected if context.recovery_authorized => {
            RecoveryAction::ReconnectSavedWifiProfile
        }
        ConnectivityState::AdapterFailure if context.recovery_authorized => {
            let effective_limit = context.restart_limit.min(MAX_ADAPTER_RESTARTS_PER_WINDOW);
            if context.adapter_restarts_in_window < effective_limit {
                RecoveryAction::RestartAdapter
            } else {
                RecoveryAction::NotifyUser
            }
        }
        ConnectivityState::AdapterFailure
        | ConnectivityState::WifiConnectedNoRoute
        | ConnectivityState::LocalNetworkOnly
        | ConnectivityState::DnsFailure
        | ConnectivityState::CaptivePortal
        | ConnectivityState::ProxyRestricted
        | ConnectivityState::VpnStateChange
        | ConnectivityState::UpstreamInternetFailure
        | ConnectivityState::RemoteServiceFailure
        | ConnectivityState::UnknownFailure
        | ConnectivityState::UserInteractionRequired
        | ConnectivityState::WifiDisconnected => RecoveryAction::NotifyUser,
        ConnectivityState::Healthy => RecoveryAction::NoAction,
        ConnectivityState::Suspect
        | ConnectivityState::TransientFailure
        | ConnectivityState::RecoveryInProgress
        | ConnectivityState::Verifying
        | ConnectivityState::Backoff => RecoveryAction::WaitAndRecheck,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_independent(reachable: bool) -> Vec<EndpointEvidence> {
        ["independent_probe_a", "independent_probe_b"]
            .into_iter()
            .map(|category| EndpointEvidence {
                category: category.into(),
                reachable,
                user_configured_service: false,
            })
            .collect()
    }

    #[test]
    fn local_adapter_summary_keeps_only_privacy_limited_counts_and_flags() {
        let summary = LocalNetworkSummary::from_adapters([
            LocalAdapterObservation {
                is_wifi: true,
                is_tunnel: false,
                operational: true,
                default_gateway_reported: true,
            },
            LocalAdapterObservation {
                is_wifi: false,
                is_tunnel: true,
                operational: true,
                default_gateway_reported: false,
            },
            LocalAdapterObservation {
                is_wifi: false,
                is_tunnel: false,
                operational: false,
                default_gateway_reported: false,
            },
        ]);

        assert!(summary.wifi_adapters_detected);
        assert!(summary.wifi_adapter_operational);
        assert_eq!(summary.operational_adapter_count, 2);
        assert!(summary.gateway_address_reported);
        assert!(summary.operational_gateway_address_reported);
        assert!(summary.wifi_gateway_address_reported);
        assert!(!summary.other_gateway_address_reported);
        assert!(summary.tunnel_adapter_detected);
        assert!(summary.tunnel_adapter_operational);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("name"));
        assert!(!serialized.contains("ssid"));
        assert!(!serialized.contains("192.168."));
        assert!(!serialized.contains("10.0."));
    }

    #[test]
    fn local_adapter_summary_does_not_infer_tunnel_or_route_when_missing() {
        let summary = LocalNetworkSummary::from_adapters([LocalAdapterObservation {
            is_wifi: true,
            is_tunnel: false,
            operational: false,
            default_gateway_reported: false,
        }]);

        assert!(summary.wifi_adapters_detected);
        assert!(!summary.wifi_adapter_operational);
        assert_eq!(summary.operational_adapter_count, 0);
        assert!(!summary.gateway_address_reported);
        assert!(!summary.operational_gateway_address_reported);
        assert!(!summary.tunnel_adapter_detected);
    }

    #[test]
    fn gateway_on_inactive_adapter_does_not_imply_an_operational_route() {
        let summary = LocalNetworkSummary::from_adapters([
            LocalAdapterObservation {
                is_wifi: true,
                is_tunnel: false,
                operational: true,
                default_gateway_reported: false,
            },
            LocalAdapterObservation {
                is_wifi: false,
                is_tunnel: false,
                operational: false,
                default_gateway_reported: true,
            },
        ]);

        assert!(summary.gateway_address_reported);
        assert!(!summary.operational_gateway_address_reported);
        assert_eq!(
            classify_local_observation(&summary, &[], false),
            ConnectivityState::UnknownFailure
        );
    }

    #[test]
    fn endpoint_success_without_dns_and_gateway_tests_stays_suspect() {
        let summary = LocalNetworkSummary {
            wifi_adapters_detected: true,
            wifi_adapter_operational: true,
            operational_adapter_count: 1,
            gateway_address_reported: true,
            wifi_gateway_address_reported: true,
            ..LocalNetworkSummary::default()
        };
        assert_eq!(
            classify_local_observation(&summary, &two_independent(true), false),
            ConnectivityState::Suspect
        );
    }

    #[test]
    fn healthy_requires_dns_route_gateway_and_two_independent_endpoints() {
        let evidence = ConnectivityEvidence {
            wifi_connected: Some(true),
            default_route_present: Some(true),
            gateway_reachable: Some(true),
            dns_resolves: Some(true),
            endpoints: two_independent(true),
            ..ConnectivityEvidence::default()
        };
        assert_eq!(classify_connectivity(&evidence), ConnectivityState::Healthy);

        for missing in [
            ConnectivityEvidence {
                gateway_reachable: None,
                ..evidence.clone()
            },
            ConnectivityEvidence {
                default_route_present: None,
                ..evidence.clone()
            },
            ConnectivityEvidence {
                dns_resolves: None,
                ..evidence.clone()
            },
        ] {
            assert_eq!(classify_connectivity(&missing), ConnectivityState::Suspect);
        }
    }

    #[test]
    fn missing_route_is_not_mislabeled_as_a_wifi_fault_without_wifi_evidence() {
        let evidence = ConnectivityEvidence {
            default_route_present: Some(false),
            ..ConnectivityEvidence::default()
        };
        assert_eq!(
            classify_connectivity(&evidence),
            ConnectivityState::UnknownFailure
        );
    }

    #[test]
    fn proxy_restriction_takes_precedence_over_remote_endpoint_failures() {
        let evidence = ConnectivityEvidence {
            wifi_connected: Some(true),
            default_route_present: Some(true),
            gateway_reachable: Some(true),
            dns_resolves: Some(true),
            proxy_restricted: true,
            endpoints: two_independent(false),
            ..ConnectivityEvidence::default()
        };
        assert_eq!(
            classify_connectivity(&evidence),
            ConnectivityState::ProxyRestricted
        );
        assert_eq!(
            decide_recovery(ConnectivityState::ProxyRestricted, authorized()),
            RecoveryAction::NotifyUser
        );
    }

    #[test]
    fn local_observation_does_not_infer_route_from_gateway_absence() {
        let connected_without_route = LocalNetworkSummary {
            wifi_adapters_detected: true,
            wifi_adapter_operational: true,
            operational_adapter_count: 1,
            ..LocalNetworkSummary::default()
        };
        assert_eq!(
            classify_local_observation(&connected_without_route, &[], false),
            ConnectivityState::UnknownFailure
        );

        let wifi_down = LocalNetworkSummary {
            wifi_adapters_detected: true,
            operational_adapter_count: 0,
            ..LocalNetworkSummary::default()
        };
        assert_eq!(
            classify_local_observation(&wifi_down, &[], false),
            ConnectivityState::WifiDisconnected
        );
    }

    #[test]
    fn inactive_wifi_does_not_override_an_operational_wired_route() {
        let summary = LocalNetworkSummary {
            wifi_adapters_detected: true,
            operational_adapter_count: 1,
            gateway_address_reported: true,
            operational_gateway_address_reported: true,
            other_gateway_address_reported: true,
            ..LocalNetworkSummary::default()
        };
        assert_eq!(
            classify_local_observation(&summary, &two_independent(true), false),
            ConnectivityState::Suspect
        );
    }

    #[test]
    fn portal_suspicion_is_classified_without_proving_the_portal_identity() {
        let summary = LocalNetworkSummary {
            wifi_adapters_detected: true,
            wifi_adapter_operational: true,
            operational_adapter_count: 1,
            gateway_address_reported: true,
            ..LocalNetworkSummary::default()
        };
        assert_eq!(
            classify_local_observation(&summary, &[], true),
            ConnectivityState::CaptivePortal
        );
    }

    fn authorized() -> RecoveryContext {
        RecoveryContext {
            evidence_sufficient: true,
            recovery_authorized: true,
            adapter_restarts_in_window: 0,
            restart_limit: 3,
        }
    }

    #[test]
    fn remote_service_failure_does_not_authorize_adapter_reset() {
        let mut evidence = ConnectivityEvidence {
            wifi_connected: Some(true),
            default_route_present: Some(true),
            gateway_reachable: Some(true),
            dns_resolves: Some(true),
            endpoints: two_independent(true),
            ..Default::default()
        };
        evidence.endpoints.push(EndpointEvidence {
            category: "configured_service".into(),
            reachable: false,
            user_configured_service: true,
        });
        let state = classify_connectivity(&evidence);
        assert_eq!(state, ConnectivityState::RemoteServiceFailure);
        assert_eq!(
            decide_recovery(state, authorized()),
            RecoveryAction::NotifyUser
        );
    }

    #[test]
    fn multiple_failed_probes_with_healthy_local_signals_mean_upstream_failure() {
        let evidence = ConnectivityEvidence {
            wifi_connected: Some(true),
            default_route_present: Some(true),
            gateway_reachable: Some(true),
            dns_resolves: Some(true),
            endpoints: two_independent(false),
            ..Default::default()
        };
        assert_eq!(
            classify_connectivity(&evidence),
            ConnectivityState::UpstreamInternetFailure
        );
        assert_eq!(
            decide_recovery(ConnectivityState::UpstreamInternetFailure, authorized()),
            RecoveryAction::NotifyUser
        );
    }

    #[test]
    fn dns_only_failure_never_restarts_adapter() {
        let evidence = ConnectivityEvidence {
            wifi_connected: Some(true),
            default_route_present: Some(true),
            gateway_reachable: Some(true),
            dns_resolves: Some(false),
            endpoints: two_independent(true),
            ..Default::default()
        };
        let state = classify_connectivity(&evidence);
        assert_eq!(state, ConnectivityState::DnsFailure);
        assert_eq!(
            decide_recovery(state, authorized()),
            RecoveryAction::NotifyUser
        );
    }

    #[test]
    fn wifi_reconnect_requires_evidence_and_adapter_restart_is_bounded_and_authorized() {
        let waiting = RecoveryContext {
            evidence_sufficient: false,
            ..authorized()
        };
        assert_eq!(
            decide_recovery(ConnectivityState::WifiDisconnected, waiting),
            RecoveryAction::WaitAndRecheck
        );
        assert_eq!(
            decide_recovery(ConnectivityState::WifiDisconnected, authorized()),
            RecoveryAction::ReconnectSavedWifiProfile
        );
        let unauthorized = RecoveryContext {
            recovery_authorized: false,
            ..authorized()
        };
        assert_eq!(
            decide_recovery(ConnectivityState::WifiDisconnected, unauthorized),
            RecoveryAction::NotifyUser
        );
        let exhausted = RecoveryContext {
            adapter_restarts_in_window: 3,
            ..authorized()
        };
        assert_eq!(
            decide_recovery(ConnectivityState::AdapterFailure, exhausted),
            RecoveryAction::NotifyUser
        );
        assert_eq!(
            decide_recovery(ConnectivityState::AdapterFailure, unauthorized),
            RecoveryAction::NotifyUser
        );
        let over_configured = RecoveryContext {
            adapter_restarts_in_window: 3,
            restart_limit: u8::MAX,
            ..authorized()
        };
        assert_eq!(
            decide_recovery(ConnectivityState::AdapterFailure, over_configured),
            RecoveryAction::NotifyUser
        );
    }

    #[test]
    fn captive_portal_and_vpn_change_take_precedence_over_adapter_faults() {
        let portal = ConnectivityEvidence {
            captive_portal_detected: true,
            wifi_connected: Some(false),
            ..Default::default()
        };
        let portal_state = classify_connectivity(&portal);
        assert_eq!(portal_state, ConnectivityState::CaptivePortal);
        assert_eq!(
            decide_recovery(portal_state, authorized()),
            RecoveryAction::NotifyUser
        );
        let vpn = ConnectivityEvidence {
            vpn_state_changed: true,
            adapter_reports_fault: true,
            ..Default::default()
        };
        let vpn_state = classify_connectivity(&vpn);
        assert_eq!(vpn_state, ConnectivityState::VpnStateChange);
        assert_eq!(
            decide_recovery(vpn_state, authorized()),
            RecoveryAction::NotifyUser
        );
    }

    #[test]
    fn duplicate_probe_categories_do_not_count_as_independent_evidence() {
        let evidence = ConnectivityEvidence {
            wifi_connected: Some(true),
            default_route_present: Some(true),
            gateway_reachable: Some(true),
            dns_resolves: Some(true),
            endpoints: vec![
                EndpointEvidence {
                    category: "same_probe".into(),
                    reachable: true,
                    user_configured_service: false,
                },
                EndpointEvidence {
                    category: "same_probe".into(),
                    reachable: true,
                    user_configured_service: false,
                },
            ],
            ..Default::default()
        };
        assert_eq!(classify_connectivity(&evidence), ConnectivityState::Suspect);
    }

    #[test]
    fn configured_github_and_openai_failures_never_authorize_adapter_restarts() {
        for service in ["github", "openai"] {
            let evidence = ConnectivityEvidence {
                wifi_connected: Some(true),
                default_route_present: Some(true),
                gateway_reachable: Some(true),
                dns_resolves: Some(true),
                endpoints: {
                    let mut endpoints = two_independent(true);
                    endpoints.push(EndpointEvidence {
                        category: service.into(),
                        reachable: false,
                        user_configured_service: true,
                    });
                    endpoints
                },
                ..Default::default()
            };
            let state = classify_connectivity(&evidence);
            assert_eq!(state, ConnectivityState::RemoteServiceFailure);
            assert_eq!(
                decide_recovery(state, authorized()),
                RecoveryAction::NotifyUser
            );
        }
    }

    #[test]
    fn adapter_restart_budget_is_rolling_and_hard_capped() {
        let mut budget = AdapterRestartBudget::new(u8::MAX, 30 * 60 * 1000);
        for attempt in 0..3 {
            assert!(budget.can_attempt(attempt * 1000));
            budget.record_attempt(attempt * 1000).unwrap();
        }
        assert!(!budget.can_attempt(30 * 60 * 1000 - 1));
        assert_eq!(
            budget.record_attempt(30 * 60 * 1000 - 1),
            Err(RestartBudgetExhausted)
        );
        assert!(budget.can_attempt(30 * 60 * 1000));
        assert_eq!(budget.attempts_in_window(30 * 60 * 1000), 2);
    }

    #[test]
    fn retry_backoff_grows_to_a_cap_and_resets_after_success() {
        let mut backoff = RetryBackoff::new(1_000, 8_000);
        assert!(backoff.is_ready(0));
        assert_eq!(backoff.record_failure(0), 1_000);
        assert!(!backoff.is_ready(999));
        assert!(backoff.is_ready(1_000));
        assert_eq!(backoff.record_failure(1_000), 2_000);
        assert_eq!(backoff.record_failure(3_000), 4_000);
        assert_eq!(backoff.record_failure(7_000), 8_000);
        assert_eq!(backoff.record_failure(15_000), 8_000);
        backoff.record_success();
        assert_eq!(backoff.failures(), 0);
        assert!(backoff.is_ready(15_000));
        assert_eq!(backoff.record_failure(15_000), 1_000);
    }
}
