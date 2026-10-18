use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

use crate::journal::{ReleaseReason, SessionEvent, SessionEventKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentKind {
    ConnectivityLost,
    ConnectivityRestored,
    RecoveryAttempted,
    RecoveryFailed,
    UserActionRequired,
    PowerLeaseStarted,
    PowerLeaseReleased,
    DiagnosticError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incident {
    pub occurred_at_unix_ms: u64,
    pub kind: IncidentKind,
    pub summary: String,
    pub classification: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionReport {
    pub session_id: String,
    pub started_at_unix_ms: u64,
    pub ended_at_unix_ms: Option<u64>,
    pub protected_duration_ms: Option<u64>,
    pub peak_active_leases: u32,
    pub incidents: Vec<Incident>,
    pub unverified_behavior: Vec<String>,
}

impl SessionReport {
    pub fn from_events(
        events: &[SessionEvent],
        session_id: Uuid,
        generated_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let started_at_unix_ms = events
            .iter()
            .filter(|event| event.session_id == session_id)
            .filter(|event| event.kind == SessionEventKind::LeaseStarted)
            .map(|event| event.occurred_at_unix_ms)
            .min()
            .ok_or_else(|| "no start event exists for this session".to_string())?;
        let release = events
            .iter()
            .filter(|event| {
                event.session_id == session_id
                    && event.occurred_at_unix_ms >= started_at_unix_ms
                    && event.occurred_at_unix_ms <= generated_at_unix_ms
            })
            .filter_map(|event| match event.kind {
                SessionEventKind::LeaseReleased(reason) => {
                    Some((event.occurred_at_unix_ms, reason))
                }
                SessionEventKind::LeaseStarted => None,
            })
            .min_by_key(|(occurred_at_unix_ms, _)| *occurred_at_unix_ms);
        let ended_at_unix_ms = release.map(|(occurred_at_unix_ms, _)| occurred_at_unix_ms);
        let protected_duration_ms =
            ended_at_unix_ms.map(|ended_at| ended_at.saturating_sub(started_at_unix_ms));

        let mut incidents = vec![Incident {
            occurred_at_unix_ms: started_at_unix_ms,
            kind: IncidentKind::PowerLeaseStarted,
            summary: "Power lease started".into(),
            classification: None,
        }];
        if let Some((occurred_at_unix_ms, reason)) = release {
            incidents.push(Incident {
                occurred_at_unix_ms,
                kind: IncidentKind::PowerLeaseReleased,
                summary: release_summary(reason).into(),
                classification: None,
            });
        }

        let report_end = ended_at_unix_ms.unwrap_or(generated_at_unix_ms);
        let mut ordered_events = events.to_vec();
        ordered_events.sort_by_key(|event| event.occurred_at_unix_ms);
        let mut active_sessions = HashSet::new();
        let mut peak_active_leases = 0_u32;
        for event in ordered_events {
            if event.occurred_at_unix_ms > report_end {
                break;
            }
            match event.kind {
                SessionEventKind::LeaseStarted => {
                    active_sessions.insert(event.session_id);
                }
                SessionEventKind::LeaseReleased(_) => {
                    active_sessions.remove(&event.session_id);
                }
            }
            if event.occurred_at_unix_ms >= started_at_unix_ms
                && active_sessions.contains(&session_id)
            {
                peak_active_leases = peak_active_leases.max(active_sessions.len() as u32);
            }
        }
        if peak_active_leases == 0 {
            peak_active_leases = 1;
        }

        let report = Self {
            session_id: session_id.to_string(),
            started_at_unix_ms,
            ended_at_unix_ms,
            protected_duration_ms,
            peak_active_leases,
            incidents,
            unverified_behavior: vec![
                "locked-session behavior".into(),
                "power-request behavior on this device".into(),
                "peak lease count is based on retained local event history".into(),
            ],
        };
        if !report.validate() {
            return Err("session event data does not form a valid report".into());
        }
        Ok(report)
    }

    pub fn validate(&self) -> bool {
        let duration_valid = match (self.ended_at_unix_ms, self.protected_duration_ms) {
            (Some(ended_at), Some(duration)) => {
                ended_at >= self.started_at_unix_ms
                    && duration <= ended_at - self.started_at_unix_ms
            }
            (None, None) => true,
            _ => false,
        };
        duration_valid
            && self.incidents.iter().all(|incident| {
                incident.occurred_at_unix_ms >= self.started_at_unix_ms
                    && self
                        .ended_at_unix_ms
                        .is_none_or(|ended_at| incident.occurred_at_unix_ms <= ended_at)
            })
    }
}

fn release_summary(reason: ReleaseReason) -> &'static str {
    match reason {
        ReleaseReason::TargetExited => "Target process exited; power request released",
        ReleaseReason::TtlExpired => "Power lease reached its configured expiry",
        ReleaseReason::BatteryTransition => "Battery transition ended the power lease",
        ReleaseReason::RuntimeError => "Runtime error ended the power lease",
        ReleaseReason::RuntimeExit => "Guardian runtime exited; power request released",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> SessionReport {
        SessionReport {
            session_id: "session".into(),
            started_at_unix_ms: 100,
            ended_at_unix_ms: Some(300),
            protected_duration_ms: Some(150),
            peak_active_leases: 2,
            incidents: vec![Incident {
                occurred_at_unix_ms: 200,
                kind: IncidentKind::ConnectivityLost,
                summary: "Connectivity check failed".into(),
                classification: Some("UPSTREAM_INTERNET_FAILURE".into()),
            }],
            unverified_behavior: vec!["locked-session behavior".into()],
        }
    }

    #[test]
    fn report_accepts_events_inside_measured_session() {
        assert!(report().validate());
    }

    #[test]
    fn report_rejects_durations_and_events_outside_session() {
        let mut value = report();
        value.protected_duration_ms = Some(201);
        assert!(!value.validate());
        let mut value = report();
        value.incidents[0].occurred_at_unix_ms = 301;
        assert!(!value.validate());
    }

    #[test]
    fn report_counts_concurrent_leases_from_typed_events() {
        let target_session = Uuid::new_v4();
        let other_session = Uuid::new_v4();
        let events = [
            SessionEvent::lease_started(other_session, 50),
            SessionEvent::lease_started(target_session, 100),
            SessionEvent::lease_released(other_session, 150, ReleaseReason::TargetExited),
            SessionEvent::lease_released(target_session, 200, ReleaseReason::TargetExited),
        ];
        let report = SessionReport::from_events(&events, target_session, 250).unwrap();
        assert_eq!(report.peak_active_leases, 2);
        assert_eq!(report.protected_duration_ms, Some(100));
        assert!(report.validate());
    }
}
