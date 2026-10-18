//! Agent-independent configuration, lease, classification, and recovery policy.

pub mod config;
pub mod journal;
pub mod lease;
pub mod network;
pub mod power;
pub mod report;

pub use config::{Config, ConfigError};
pub use journal::{EventStore, ReleaseReason, SessionEvent, SessionEventKind};
pub use lease::{Lease, LeaseBook, LeaseError, LeaseId, ProcessIdentity};
pub use network::{
    AdapterRestartBudget, ConnectivityEvidence, ConnectivityState, EndpointEvidence,
    LocalAdapterObservation, LocalNetworkSummary, MAX_ADAPTER_RESTARTS_PER_WINDOW, RecoveryAction,
    RecoveryContext, RestartBudgetExhausted, RetryBackoff, classify_connectivity, decide_recovery,
};
pub use power::{PowerLeaseDecision, PowerLeasePolicy, PowerLeasePolicyError, PowerSourceClass};
pub use report::{Incident, IncidentKind, SessionReport};
