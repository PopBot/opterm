# Opterm implementation guidance

- Explain changes clearly and briefly. The design interview is closed.
- Follow the accepted scope in `docs/ui-settings-spec.md` and the active sequence in `docs/prototype-plan.md`; later accepted corrections supersede historical plans.
- Keep the native window thin: compact tabs, one terminal by default, no permanent sidebar or IDE panels.
- Keep Opterm independent of other products. Historical reference assets under documentation are not runtime dependencies.
- Keep SQLite, direct-command bookkeeping, public API/CLI/MCP integration, and indexed history/export deferred.
- Mark development slices and unverified platform coverage honestly. Never claim detach/crash survival until the independent service owns the sessions.
- Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` for Rust changes. Use `python3 scripts/smoke_gui.py` for focused Linux GUI checks when the renderer/input changes and prerequisites are available.
- Track Cargo.lock. Do not publish, push, or claim benchmark superiority without supporting work and authorization.
