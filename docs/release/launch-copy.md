# Launch copy draft

These drafts are for later use after release approval. They describe the current experimental feature set and do not claim verified power behavior, automatic Wi-Fi recovery, or production readiness.

## GitHub description

Experimental Windows companion for bounded wake leases and privacy-limited connectivity diagnostics during long coding sessions.

Suggested topics: `windows`, `rust`, `agent-skills`, `power-management`, `network-monitoring`, `developer-tools`.

## X / Twitter

I’m building Agent Guardian, an experimental Windows companion for long coding sessions. It has a bounded foreground wake lease, local network monitoring, and optional one-shot connectivity checks. Automatic Wi-Fi recovery and hardware power validation are still open work.

## LinkedIn

Agent Guardian is an experimental open-source project for developers who leave long local coding tasks running. The current build combines a bounded Windows foreground power lease with privacy-limited network observation and explicit connectivity diagnostics. It does not install a service or automatically restart a network adapter. Power and locked-session behavior still need validation on supported hardware.

## Reddit / developer communities

**Title:** I’m building an experimental Windows companion for long coding sessions

**Post:** Agent Guardian is a small Rust CLI and Agent Skill I’m developing for unattended local coding work. The current Windows build can request a bounded foreground wake lease, summarize local network state, watch interface and route changes, and run optional one-shot connectivity checks. It does not currently recover Wi-Fi or claim that a model stream survives an outage. I’m looking for feedback on the safety boundaries and setup flow; hardware testing and public release checks are still incomplete.
