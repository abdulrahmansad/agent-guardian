# Safety boundaries

- Do not request display-required power behavior. Windows should be able to turn off the display and apply its lock policy.
- Respect explicit user sleep, shutdown, restart, lid-close policy, critical battery policy, and Windows power policy.
- Keep leases bounded and bind them to process identity, including process creation time. A PID alone can be reused.
- Default to AC power. The current development CLI requires explicit battery opt-in and caps the lease at five minutes.
- Never export or store Wi-Fi credentials.
- Do not restart an adapter for a failed GitHub, OpenAI, npm, ISP, VPN, or other remote service.
- Do not modify firewall, antivirus, DNS, proxy, network profiles, drivers, TCP/IP configuration, or router settings.
- `guardian network` reads local adapter state and gateway-address presence only. It sends no probes and reveals no network names or address values.
- Use `guardian monitor` only when the user asks for ongoing observation. It listens for local interface and route changes for at most 60 minutes, refreshes aggregate state at a bounded interval, and makes no network or power changes. It does not establish internet health.
- Use `guardian network --probe` only when the user asks for a connectivity check. It runs two fixed DNS lookups, reads active route metadata, may send one bounded ICMP echo to a selected gateway per address family, and sends two fixed web requests. It displays summary values only; transient gateway addresses and DNS answers are not displayed or persisted. It writes no probe results to the journal and does not authorize recovery.
- There is no continuous external connectivity monitor, VPN-state detection, configured remote-service test, or network recovery executor. The foreground monitor only observes local interface/route metadata. ICMP filtering can leave gateway reachability unknown. Classification and recovery decisions in the core do not cause Windows network changes.
- Never claim that a completed/failed Codex stream can be resurrected. Never replay non-idempotent remote operations automatically.
