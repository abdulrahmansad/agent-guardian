# Safe development demo

This walkthrough demonstrates read-only diagnostics and the bounded monitor. It does not claim that Windows sleep behavior, locked-session behavior, or Wi-Fi recovery has been validated. No external probe or network-changing command is included.

From a PowerShell prompt in the repository:

```powershell
cargo run -p guardian-cli -- doctor
cargo run -p guardian-cli -- network
cargo run -p guardian-cli -- --json monitor --duration-minutes 1 --interval-seconds 30
```

The doctor command reports the power capabilities and any diagnostics Windows allows the current account to read. The default network command reports aggregate local interface state without adapter names, SSIDs, or addresses. The monitor emits an initial aggregate snapshot and a stopped record; it only reports a changed snapshot if an interface or route observation changes during that minute.

To demonstrate the power lease, a person must explicitly choose a bounded duration and understand that Windows may honor the request to prevent idle sleep. The foreground command can be stopped with Ctrl+C. The lease does not keep the display awake and does not override explicit Sleep, Shutdown, lid policy, or critical-battery behavior. Do not present this as verified until the acceptance matrix's hardware scenarios have been performed.

Do not use `guardian network --probe` in a recorded demo unless the operator has reviewed the two fixed web checks and the possible DNS and gateway ICMP traffic. Keep this walkthrough read-only. An explicitly enabled saved-profile reconnect path exists, but a live test did not show Guardian initiating recovery; its classifier fix still needs a live retest. Adapter restart is not implemented.
