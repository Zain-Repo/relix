# Relix project notes

- This is a native Rust database studio intended for different database servers.
- GPUI framework documentation is in `docs/gpui/`. Current Relix behavior and
  component integration are in `docs/studio.md`; older cash-register docs are
  inherited references, not current implementation requirements.
- Use GPUI Kit components and UI skills for UI guidance. Preserve the simple
  workspace contract in `DESIGN.md` and the user intent in `PRODUCT.md`.
- Check exact APIs against the pinned `gpui-kit` source. Website snippets may
  describe newer APIs. Keep the Kit's GPUI dependency stack compatible.
- The current scope is a real four-database workbench: SQLite, DuckDB,
  PostgreSQL and MySQL. Advanced DBX-like features are later work, as selected
  by the user. Do not introduce fake controls or unsupported-driver claims.
- Database calls belong in `src/database/` and run off the UI thread. Preserve
  session ownership, explicit transactions, cancellation and read-only checks.
- Workspace persistence is in `src/workspace.rs`; passwords must never be
  serialized or logged. Reviewable data-operation SQL is in `src/data_ops.rs`.
- Drafts remain session-only; saved queries and history persist explicitly.
  Do not discard unrelated local changes or overwrite damaged workspace files.
- Validate with formatting, Cargo check/test/Clippy and a native smoke test when
  available. Use RTK prefixes on shell commands in this environment.
