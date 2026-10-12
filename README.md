# Relix

A native database studio built in Rust with GPUI Kit. Relix connects to SQLite
and DuckDB files or PostgreSQL and MySQL servers. A writable in-memory SQLite
scratch database lets you try SQL without a server.

```sh
cargo run --locked
```

The workbench includes connection profiles, a searchable object explorer, schema
inspection, independent SQL tabs, local metadata completion, SQL formatting,
selected/current-statement SQL execution, Explain, cancellation, transaction controls
and multiple result sets. Table browsing supports database-side filters, ordering
and pages. Schema inspection includes indexes and foreign keys. Results support
filtering, sorting, full cell inspection, copying, and CSV/JSON/Markdown/INSERT export.
CSV imports and primary-key record edits generate SQL for review before execution.
Single-row INSERT/DELETE forms also prepare reviewable drafts. Ctrl/Cmd+K opens
a searchable native command palette. Queries can be renamed or duplicated;
saved queries and history have text search. Neutral shadcn-style light/dark
themes use GPUI Kit components and global semantic colors.
Saved queries, connection profiles, history and appearance preferences persist;
server passwords can be remembered in the OS credential store. Passwords never
enter workspace files; unsaved drafts remain session-only.

Connections default to read-only. Enable writes explicitly when needed. SQL run
on a writable connection can change data; review generated scripts before running
them. TLS is enabled by default for server connections and verifies certificates.

Read [the studio guide](docs/studio.md) for behavior, limits and storage.
The [DBX comparison](docs/dbx-comparison.md) records reviewed features and remaining gaps.
The local framework reference lives in [docs/gpui](docs/gpui/README.md).

```sh
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
```

Windows builds need the MSVC C++ tools and Windows SDK. DuckDB and SQLite are
bundled: the first DuckDB build compiles its native library and can take time.
GPUI Kit is pinned to 0.7.1 with its compatible runtime and SQL Tree-sitter grammar.
Other platforms require GPUI Kit's native prerequisites. Linux credential storage
also requires the D-Bus development libraries at build time and an unlocked
Secret Service keyring at runtime.

The current scope is the four-database workbench. AI providers, MCP/CLI, SSH
tunnels, ER diagrams, schema comparison and additional engines are later work.

Read [the release process](docs/releases.md) for PR checks, native packages,
version changes and automatic publication after merge. Download packages from
[GitHub Releases](https://github.com/Zain-Repo/relix/releases).
Read [the update guide](docs/updates.md) for automatic updates and upgrading
older versions. Relix 0.1.0 and 0.1.1 require one manual package replacement;
workspace data and remembered credentials remain separate from the app files.
