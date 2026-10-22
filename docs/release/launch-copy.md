# Launch copy draft

These drafts are for later use after release approval. They describe the current experimental feature set and do not claim broad Wi-Fi recovery, display-off power behavior, or production readiness.

## GitHub description

Experimental Windows companion for bounded wake leases and privacy-limited connectivity diagnostics during long coding sessions.

Suggested topics: `windows`, `rust`, `agent-skills`, `power-management`, `network-monitoring`, `developer-tools`.

## X / Twitter

I'm building Agent Guardian, an experimental Windows companion for long coding sessions. It has bounded wake leases, local network monitoring, optional one-shot connectivity checks, and an explicitly enabled saved-profile reconnect path. Two controlled live monitor tests passed on 2026-10-22. A 17-minute AC idle-window test held a lease without sleep, and normal sleep returned on the 15-minute idle timer after release; display-off and broader hardware behavior remain unverified.

## LinkedIn

Agent Guardian is an experimental open-source project for developers who leave long local coding tasks running. The current build combines bounded Windows power leases with privacy-limited network observation and explicit connectivity diagnostics. Saved-profile Wi-Fi reconnection is an opt-in feature; two controlled live monitor tests passed on 2026-10-22, but broader hardware and network coverage is still pending. A 17-minute AC idle-window test held a lease without sleep, and normal sleep returned after release; display-off and locked-session behavior still need validation. It does not install a service or restart a network adapter.

## Reddit / developer communities

**Title:** I'm building an experimental Windows companion for long coding sessions

**Post:** Agent Guardian is a small Rust CLI and Agent Skill I'm developing for unattended local coding work. The current Windows build can request a bounded wake lease, summarize local network state, watch interface and route changes, and run optional one-shot connectivity checks. It also has an opt-in saved-profile Wi-Fi reconnect path; two controlled live monitor tests passed on 2026-10-22, and a 17-minute AC idle-window test held a lease without sleep. It does not claim that a model stream survives an outage. I'm looking for feedback on the safety boundaries and setup flow; display-off and broader hardware testing and public release checks are still incomplete.
