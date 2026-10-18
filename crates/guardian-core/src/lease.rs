use std::collections::HashMap;
use uuid::Uuid;

pub type LeaseId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Windows process creation FILETIME captured from an opened process handle.
    pub creation_time_100ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub id: LeaseId,
    pub process: ProcessIdentity,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    InvalidPid,
    InvalidExpiry,
    ExpiryTooLong,
    DuplicateProcess,
    CapacityReached,
    UnknownLease,
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPid => write!(f, "process ID must be greater than zero"),
            Self::InvalidExpiry => write!(f, "lease expiry must be later than its creation time"),
            Self::ExpiryTooLong => write!(f, "lease duration exceeds the configured maximum"),
            Self::DuplicateProcess => write!(f, "this process already has an active lease"),
            Self::CapacityReached => write!(f, "maximum number of active leases reached"),
            Self::UnknownLease => write!(f, "lease ID was not found"),
        }
    }
}

impl std::error::Error for LeaseError {}

#[derive(Debug)]
pub struct LeaseBook {
    active: HashMap<LeaseId, Lease>,
    maximum_active: usize,
    maximum_duration_ms: u64,
}

impl Default for LeaseBook {
    fn default() -> Self {
        Self::new(32, 24 * 60 * 60 * 1000)
    }
}

impl LeaseBook {
    pub fn new(maximum_active: usize, maximum_duration_ms: u64) -> Self {
        Self {
            active: HashMap::new(),
            maximum_active,
            maximum_duration_ms,
        }
    }

    pub fn acquire(
        &mut self,
        process: ProcessIdentity,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<LeaseId, LeaseError> {
        if process.pid == 0 {
            return Err(LeaseError::InvalidPid);
        }
        if expires_at_ms <= now_ms {
            return Err(LeaseError::InvalidExpiry);
        }
        if expires_at_ms - now_ms > self.maximum_duration_ms {
            return Err(LeaseError::ExpiryTooLong);
        }
        if self.active.values().any(|lease| lease.process == process) {
            return Err(LeaseError::DuplicateProcess);
        }
        if self.active.len() >= self.maximum_active {
            return Err(LeaseError::CapacityReached);
        }
        let id = Uuid::new_v4();
        self.active.insert(
            id,
            Lease {
                id,
                process,
                created_at_unix_ms: now_ms,
                expires_at_unix_ms: expires_at_ms,
            },
        );
        Ok(id)
    }

    pub fn release(&mut self, id: LeaseId) -> Result<Lease, LeaseError> {
        self.active.remove(&id).ok_or(LeaseError::UnknownLease)
    }

    /// Remove leases whose TTL elapsed or whose PID no longer maps to the captured process identity.
    pub fn reap(
        &mut self,
        now_ms: u64,
        mut process_matches: impl FnMut(ProcessIdentity) -> bool,
    ) -> Vec<Lease> {
        let stale_ids: Vec<_> = self
            .active
            .iter()
            .filter(|(_, lease)| {
                lease.expires_at_unix_ms <= now_ms || !process_matches(lease.process)
            })
            .map(|(id, _)| *id)
            .collect();
        stale_ids
            .into_iter()
            .filter_map(|id| self.active.remove(&id))
            .collect()
    }

    pub fn active(&self) -> impl Iterator<Item = &Lease> {
        self.active.values()
    }
    pub fn len(&self) -> usize {
        self.active.len()
    }
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, creation: u64) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            creation_time_100ns: creation,
        }
    }

    #[test]
    fn leases_bind_creation_identity_and_detect_pid_reuse() {
        let mut leases = LeaseBook::new(4, 60_000);
        let id = leases.acquire(process(17, 100), 1_000, 30_000).unwrap();
        assert!(leases.acquire(process(17, 100), 1_000, 30_000).is_err());
        let removed = leases.reap(2_000, |identity| identity == process(17, 200));
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, id);
        assert!(leases.is_empty());
    }

    #[test]
    fn expiry_and_capacity_are_enforced() {
        let mut leases = LeaseBook::new(1, 10_000);
        assert_eq!(
            leases.acquire(process(0, 1), 0, 1),
            Err(LeaseError::InvalidPid)
        );
        assert_eq!(
            leases.acquire(process(1, 1), 10, 10),
            Err(LeaseError::InvalidExpiry)
        );
        assert_eq!(
            leases.acquire(process(1, 1), 0, 10_001),
            Err(LeaseError::ExpiryTooLong)
        );
        let id = leases.acquire(process(1, 1), 0, 10_000).unwrap();
        assert_eq!(
            leases.acquire(process(2, 2), 0, 10_000),
            Err(LeaseError::CapacityReached)
        );
        assert_eq!(leases.reap(10_000, |_| true).len(), 1);
        assert!(leases.release(id).is_err());
    }

    #[test]
    fn active_processes_are_retained_until_exit_or_expiry() {
        let mut leases = LeaseBook::new(2, 100_000);
        let identity = process(42, 123);
        leases.acquire(identity, 0, 50_000).unwrap();
        assert!(leases.reap(10_000, |actual| actual == identity).is_empty());
        assert_eq!(leases.reap(10_000, |_| false).len(), 1);
    }

    #[test]
    fn independent_processes_can_hold_concurrent_leases() {
        let mut leases = LeaseBook::new(3, 100_000);
        let first = leases.acquire(process(10, 1), 0, 50_000).unwrap();
        let second = leases.acquire(process(11, 2), 0, 50_000).unwrap();
        assert_ne!(first, second);
        assert_eq!(leases.len(), 2);
        assert!(leases.release(first).is_ok());
        assert_eq!(leases.len(), 1);
    }
}
