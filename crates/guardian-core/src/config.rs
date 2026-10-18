use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub power: PowerConfig,
    pub networking: NetworkConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PowerConfig {
    /// Requiring explicit opt-in avoids holding a keep-awake request on battery by surprise.
    pub allow_on_battery: bool,
    pub default_lease_minutes: u32,
    pub maximum_lease_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    pub enabled: bool,
    pub grace_period_seconds: u32,
    pub maximum_adapter_restarts_per_30_minutes: u8,
    pub redact_network_names: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub maximum_file_size_mib: u16,
    pub retention_days: u16,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            allow_on_battery: false,
            default_lease_minutes: 60,
            maximum_lease_minutes: 24 * 60,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            grace_period_seconds: 12,
            maximum_adapter_restarts_per_30_minutes: 3,
            redact_network_names: true,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            maximum_file_size_mib: 10,
            retention_days: 14,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Parse(String),
    Invalid(&'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid TOML configuration: {error}"),
            Self::Invalid(reason) => write!(f, "invalid configuration: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        let config: Self =
            toml::from_str(source).map_err(|error| ConfigError::Parse(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.power.default_lease_minutes == 0 {
            return Err(ConfigError::Invalid(
                "default_lease_minutes must be greater than zero",
            ));
        }
        if self.power.maximum_lease_minutes == 0 || self.power.maximum_lease_minutes > 24 * 60 {
            return Err(ConfigError::Invalid(
                "maximum_lease_minutes must be between 1 and 1440",
            ));
        }
        if self.power.default_lease_minutes > self.power.maximum_lease_minutes {
            return Err(ConfigError::Invalid(
                "default lease cannot exceed maximum lease",
            ));
        }
        if self.networking.grace_period_seconds > 300 {
            return Err(ConfigError::Invalid(
                "grace_period_seconds must not exceed 300",
            ));
        }
        if self.networking.maximum_adapter_restarts_per_30_minutes > 3 {
            return Err(ConfigError::Invalid(
                "adapter restart budget must not exceed 3 per 30 minutes",
            ));
        }
        if self.logging.maximum_file_size_mib == 0 || self.logging.maximum_file_size_mib > 100 {
            return Err(ConfigError::Invalid(
                "maximum_file_size_mib must be between 1 and 100",
            ));
        }
        if self.logging.retention_days == 0 || self.logging.retention_days > 90 {
            return Err(ConfigError::Invalid(
                "retention_days must be between 1 and 90",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_battery_mode_is_opt_in() {
        let config = Config::default();
        assert!(config.validate().is_ok());
        assert!(!config.power.allow_on_battery);
        assert!(!config.networking.enabled);
        assert_eq!(config.networking.maximum_adapter_restarts_per_30_minutes, 3);
    }

    #[test]
    fn parses_and_validates_configuration() {
        let config = Config::parse(
            "[power]\ndefault_lease_minutes = 30\nmaximum_lease_minutes = 120\nallow_on_battery = false\n",
        )
        .unwrap();
        assert_eq!(config.power.default_lease_minutes, 30);
        assert_eq!(config.logging.retention_days, 14);
    }

    #[test]
    fn rejects_unknown_fields_and_out_of_range_values() {
        assert!(Config::parse("[power]\nsurprise = true").is_err());
        let mut config = Config::default();
        config.networking.maximum_adapter_restarts_per_30_minutes = 4;
        assert!(config.validate().is_err());
    }
}
