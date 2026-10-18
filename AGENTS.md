# Repository guidance

- Keep README and product docs aligned with behavior that is actually implemented and verified.
- Treat power and network operations as user-visible side effects. Keep power leases bounded, respect explicit user actions, and do not add network mutation without a separate authorization path and controlled tests.
- Never collect or persist Wi-Fi credentials, SSIDs, adapter names, IP addresses, tokens, or arbitrary command output unless a documented feature strictly requires it and redacts it by default.
- Keep classification and recovery policy deterministic and separate from Windows API execution.
- Record hardware behavior as `NOT TESTED` until the exact test is performed. Unit tests do not prove Windows power or adapter behavior.
- Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`, `python scripts/check-doc-links.py`, and the Agent Skills validator before proposing a code change as ready.
- Do not publish, push, tag, or release without explicit user authorization.
