#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSourceClass {
    Ac,
    BatteryOrUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerLeaseDecision {
    Unchanged,
    DeadlineChanged { deadline_ms: u64 },
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerLeasePolicyError {
    ZeroDuration,
    BatteryNotAuthorized,
}

impl std::fmt::Display for PowerLeasePolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroDuration => write!(f, "power lease duration must be greater than zero"),
            Self::BatteryNotAuthorized => write!(f, "battery power was not authorized"),
        }
    }
}

impl std::error::Error for PowerLeasePolicyError {}

/// Bounds a lease against the original TTL and the short battery allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerLeasePolicy {
    overall_deadline_ms: u64,
    active_deadline_ms: u64,
    battery_limit_ms: u64,
    allow_battery: bool,
    source: PowerSourceClass,
}

impl PowerLeasePolicy {
    pub fn new(
        now_ms: u64,
        requested_duration_ms: u64,
        battery_limit_ms: u64,
        source: PowerSourceClass,
        allow_battery: bool,
    ) -> Result<Self, PowerLeasePolicyError> {
        if requested_duration_ms == 0 || battery_limit_ms == 0 {
            return Err(PowerLeasePolicyError::ZeroDuration);
        }
        if source == PowerSourceClass::BatteryOrUnknown && !allow_battery {
            return Err(PowerLeasePolicyError::BatteryNotAuthorized);
        }
        let overall_deadline_ms = now_ms.saturating_add(requested_duration_ms);
        let active_deadline_ms = match source {
            PowerSourceClass::Ac => overall_deadline_ms,
            PowerSourceClass::BatteryOrUnknown => {
                overall_deadline_ms.min(now_ms.saturating_add(battery_limit_ms))
            }
        };
        Ok(Self {
            overall_deadline_ms,
            active_deadline_ms,
            battery_limit_ms,
            allow_battery,
            source,
        })
    }

    pub fn deadline_ms(&self) -> u64 {
        self.active_deadline_ms
    }

    pub fn observe_source(&mut self, now_ms: u64, source: PowerSourceClass) -> PowerLeaseDecision {
        if source == self.source {
            return PowerLeaseDecision::Unchanged;
        }
        if source == PowerSourceClass::BatteryOrUnknown && !self.allow_battery {
            return PowerLeaseDecision::Release;
        }
        self.source = source;
        self.active_deadline_ms = match source {
            PowerSourceClass::Ac => self.overall_deadline_ms,
            PowerSourceClass::BatteryOrUnknown => self
                .overall_deadline_ms
                .min(now_ms.saturating_add(self.battery_limit_ms)),
        };
        PowerLeaseDecision::DeadlineChanged {
            deadline_ms: self.active_deadline_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;

    #[test]
    fn refuses_battery_by_default_and_caps_opted_in_start() {
        assert_eq!(
            PowerLeasePolicy::new(
                100,
                60 * MINUTE,
                5 * MINUTE,
                PowerSourceClass::BatteryOrUnknown,
                false
            ),
            Err(PowerLeasePolicyError::BatteryNotAuthorized)
        );
        let policy = PowerLeasePolicy::new(
            100,
            60 * MINUTE,
            5 * MINUTE,
            PowerSourceClass::BatteryOrUnknown,
            true,
        )
        .unwrap();
        assert_eq!(policy.deadline_ms(), 100 + 5 * MINUTE);
    }

    #[test]
    fn battery_transition_releases_or_shortens_and_ac_restores_original_ttl() {
        let mut denied =
            PowerLeasePolicy::new(0, 60 * MINUTE, 5 * MINUTE, PowerSourceClass::Ac, false).unwrap();
        assert_eq!(
            denied.observe_source(MINUTE, PowerSourceClass::BatteryOrUnknown),
            PowerLeaseDecision::Release
        );

        let mut allowed =
            PowerLeasePolicy::new(0, 60 * MINUTE, 5 * MINUTE, PowerSourceClass::Ac, true).unwrap();
        assert_eq!(
            allowed.observe_source(10 * MINUTE, PowerSourceClass::BatteryOrUnknown),
            PowerLeaseDecision::DeadlineChanged {
                deadline_ms: 15 * MINUTE
            }
        );
        assert_eq!(
            allowed.observe_source(12 * MINUTE, PowerSourceClass::Ac),
            PowerLeaseDecision::DeadlineChanged {
                deadline_ms: 60 * MINUTE
            }
        );
        assert_eq!(
            allowed.observe_source(13 * MINUTE, PowerSourceClass::Ac),
            PowerLeaseDecision::Unchanged
        );
    }

    #[test]
    fn short_original_ttl_stays_the_hard_upper_bound_on_battery() {
        let policy = PowerLeasePolicy::new(
            1_000,
            2 * MINUTE,
            5 * MINUTE,
            PowerSourceClass::BatteryOrUnknown,
            true,
        )
        .unwrap();
        assert_eq!(policy.deadline_ms(), 1_000 + 2 * MINUTE);
    }
}
