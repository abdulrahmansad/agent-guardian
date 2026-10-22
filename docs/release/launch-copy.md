# Launch copy draft

These drafts are for later use after release approval. They describe the current experimental feature set and do not claim verified power behavior, broad Wi-Fi recovery, or production readiness.

## GitHub description

Experimental Windows companion for bounded wake leases and privacy-limited connectivity diagnostics during long coding sessions.

Suggested topics: `windows`, `rust`, `agent-skills`, `power-management`, `network-monitoring`, `developer-tools`.

## X / Twitter

I'm building Agent Guardian, an experimental Windows companion for long coding sessions. It has bounded wake leases, local network monitoring, optional one-shot connectivity checks, and an explicitly enabled saved-profile reconnect path. One controlled live monitor test passed on 2026-10-22; broader hardware power behavior remains unverified.

## LinkedIn

Agent Guardian is an experimental open-source project for developers who leave long local coding tasks running. The current build combines bounded Windows power leases with privacy-limited network observation and explicit connectivity diagnostics. Saved-profile Wi-Fi reconnection is an opt-in feature; one controlled live monitor test passed on 2026-10-22, but broader hardware and network coverage is still pending. It does not install a service or restart a network adapter. Power and locked-session behavior also need validation on supported hardware.

## Reddit / developer communities

**Title:** I'm building an experimental Windows companion for long coding sessions

**Post:** Agent Guardian is a small Rust CLI and Agent Skill I'm developing for unattended local coding work. The current Windows build can request a bounded wake lease, summarize local network state, watch interface and route changes, and run optional one-shot connectivity checks. It also has an opt-in saved-profile Wi-Fi reconnect path; one controlled live monitor test passed on 2026-10-22. It does not claim that a model stream survives an outage. I'm looking for feedback on the safety boundaries and setup flow; hardware testing and public release checks are still incomplete.
