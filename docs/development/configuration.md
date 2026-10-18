# Configuration

`config.example.toml` documents the typed defaults and validation limits. `guardian start`, `guardian logs`, and `guardian report` read `%LOCALAPPDATA%\AgentGuardian\config.toml` when it exists. Each command accepts `--config PATH` to select another file. The explicitly requested `guardian network --reconnect-saved-wifi` command accepts the same option for the bounded network-event journal limits. The CLI does not create or overwrite configuration files. Files larger than 64 KiB, malformed TOML, unknown fields, and invalid values are rejected.

The Rust core parser rejects unknown fields and validates lease limits, the network observation opt-in, the adapter restart ceiling, and log size/retention bounds. The active runtime applies the power defaults, maximum lease duration, and event-log size/retention; an integration test exercises configured 1 MiB rotation and one-day retention. A command-line TTL overrides the configured default but cannot exceed the configured maximum. Setting `allow_on_battery = true` is an explicit persistent opt-in; battery leases remain capped at five minutes. Network settings remain reserved and do not enable monitoring or recovery.

Do not put Wi-Fi credentials, API keys, tokens, or other secrets in this file. The current options do not need them.
