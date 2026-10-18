# Safety boundaries

- Do not request display-required power behavior. Windows should be able to turn off the display and apply its lock policy.
- Respect explicit user sleep, shutdown, restart, lid-close policy, critical battery policy, and Windows power policy.
- Keep leases bounded and bind them to process identity, including process creation time. A PID alone can be reused.
- Default to AC power. The current development CLI requires explicit battery opt-in and caps the lease at five minutes.
- Never export or store Wi-Fi credentials.
- Do not restart an adapter for a failed GitHub, OpenAI, npm, ISP, VPN, or other remote service.
- Do not modify firewall, antivirus, DNS, proxy, network profiles, drivers, TCP/IP configuration, or router settings.
- `guardian network` reads local adapter status and gateway-address presence only. It sends no probes and reveals no network names or addresses.
- DNS, VPN, captive portal, upstream, and remote-service observation are not connected. Classification and recovery decisions in the core do not cause Windows network changes.
- Never claim that a completed/failed Codex stream can be resurrected. Never replay non-idempotent remote operations automatically.
