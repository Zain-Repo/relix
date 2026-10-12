# DBX feature comparison

Reference: [t8y2/dbx](https://github.com/t8y2/dbx), commit
[`ea8c2da594857531fad111475c56bc034a04f9f8`](https://github.com/t8y2/dbx/tree/ea8c2da594857531fad111475c56bc034a04f9f8),
desktop version 0.6.40. The review covered the repository's top-level architecture
and representative implementations across its desktop, core, drivers, web, CLI,
MCP, agents, plugin SDK, packaging and tests. It was not a line-by-line review of
all 3,274 indexed source files or a security audit.

DBX uses Vue, CodeMirror and shadcn-vue on a Tauri desktop shell. Its shared Rust
workspace includes 28 crates plus independent web, CLI, MCP and driver processes.
Relix implements the familiar workbench workflows independently in Rust using
GPUI Kit 0.7.1, GPUI Base and the Kit's pinned GPUI framework. No DBX source,
frontend dependencies, platform patches or services are incorporated. DBX's
Apache-2.0 license applies to its source; future source reuse must retain the
applicable notices and document modifications.

## Core workbench coverage

| DBX workflow | Relix implementation |
| --- | --- |
| Connection manager and connection testing | SQLite, DuckDB, PostgreSQL and MySQL retained sessions; editable saved profiles; TLS; explicit read-only default; passwords remain in memory |
| Query editor | Independent session-only drafts; SQL highlighting; local completion; formatting; selection/script execution; current statement at cursor; SQL files |
| Execution control | Multiple result sets; explicit transactions; stop/cancellation; bounded rows/memory; deadlines; explain output |
| Saved SQL and history | Explicit workspace saves and bounded persistent execution history; no automatic draft restore |
| Table/view explorer | Search, catalog refresh, quoted query creation and schema inspection |
| Table browsing | Typed single-column filters, database-side ordering, primary-key tie breakers, bounded pages and previous/next navigation |
| Result grid | Kit virtual table; resize/sort; loaded-row filtering; visible/loaded counts; selected-cell details and raw value copy; explicit NULL display |
| Schema metadata | Columns, types, nullability, primary keys, indexes/definitions and outbound composite foreign keys/actions |
| Data editing | Primary-key/original-value guarded UPDATE drafts for unchanged base-table queries; review and Run explicitly |
| Import/export | CSV import drafts; SQL file load/save; CSV, JSON, Markdown and loaded-row INSERT SQL export |
| Appearance and layout | GPUI Kit semantic light/dark theme, compact commands, resizable panes, native dialogs, persistent visibility preferences |

## Features added by this comparison

- `src/table_browse.rs` generates typed predicates and quoted SELECT statements.
  `src/studio/browse.rs` inspects real columns before accepting a filter or sort,
  prepares a draft, and keeps page state with its query and original connection.
  Paging executes read queries; editor changes or explicit retargeting invalidate
  browse navigation. Its execution cap matches its page size. Offset paging does
  not hold a snapshot across concurrent database changes; ordering without a
  unique key can still be unstable.
- `src/database/schema.rs` reads indexes and foreign keys through retained
  sessions on background jobs. Metadata is bounded; it reports unsupported or
  oversized results rather than silently hiding them. SQLite expression keys
  retain their definition; implicit referenced primary keys are labeled.
- `src/sql_tools.rs` selects the exact source statement at the UTF-8 cursor using
  dialect-aware tokens. Compound unquoted procedural bodies require explicit
  selection or script execution rather than a guessed boundary.
- `src/studio/results.rs` and `view.rs` inspect complete raw cell values, including
  empty strings, multiline values and SQL NULL. Display truncation does not alter
  copied values.
- `src/data_ops.rs` exports escaped Markdown and reviewable INSERT statements.
  INSERT export requires an unchanged base-table query and its original live
  connection. It rejects duplicate columns, binary/compound/unknown types and
  ambiguous hex displays. Scalar display strings are converted by target column
  types; this is not a lossless database dump. Only loaded rows are exported,
  including rows hidden by the local grid filter.

## Source implementation references

The following paths were inspected in the pinned DBX checkout to understand
behavior, rather than using its feature list alone:

- `apps/desktop/src/composables/useDataGridActions.ts` and
  `useDataGridColumnFilters.ts`: remote table predicates and paging.
- `crates/dbx-core/src/data/transfer.rs`: dialect-specific paging/filter/order SQL.
- `apps/desktop/src/lib/table/tableStructureMetadataLoading.ts` and
  `crates/dbx-driver-postgres/src/postgres.rs`: schema loading and index/FK queries.
- `apps/desktop/src/lib/sql/sqlStatementRanges.ts`: executable statement ranges.
- `apps/desktop/src/composables/useDataGridEditor.ts` and `useDataGridExport.ts`:
  reviewed mutations and export behavior.
- `apps/desktop/src/components`: result details, table panels and timing UI.
- `crates/dbx-core/src/{connection,query,schema,data,persistence,safety}`,
  `crates/dbx-web/src/routes`, `crates/dbx-cli/src`, `crates/dbx-mcp/src`,
  `plugins/sdk`, `agents`, `deploy`, `scripts`, `tests`, `docs`, `vendor`:
  architecture, integration boundaries and distribution.

## Remaining differences

Relix does not claim full DBX parity. Its four-engine workbench does not include
DBX's additional native/JDBC drivers, middleware browsers, AI assistant, MCP/CLI,
SSH tunnels/proxies, plugins, scheduled backups, ER diagrams, visual explain,
schema/data comparison, table structure editing, triggers/procedures/functions,
batch row mutation, cross-database transfer, full dumps, Excel import/export,
DuckDB file preview, connection import, localization or updater. Those require
separate lifecycle, security and compatibility work. This feature translation
preserves the agreed four-engine core and adds no unsupported feature controls.

SQLite and DuckDB metadata/browse/export behavior is exercised locally. Live
PostgreSQL/MySQL schema and browsing verification requires dedicated test servers;
the queries have been checked against their official catalog/function documentation.

## Validation of this implementation

The installed-app follow-up and its evidence limits are documented in
`dbx-installed-analysis.md`. It adds reviewed single-row INSERT/DELETE forms,
native command search, query rename/duplicate, saved/history text filters and
translated light/dark tokens. Validation below describes the preceding pass.

- `cargo fmt --all -- --check` and `cargo check --locked --all-targets` passed.
- `cargo test --locked`: 57 tests passed, including SQLite/DuckDB metadata and
  quoting, read-only file preservation, export roundtrips, current-statement
  execution, page-size/global-cap regression, incomplete-page protection and
  essential controls at the 900 × 620 minimum window.
- `cargo clippy --locked --all-targets -- -D warnings` passed.
- `cargo build --locked` passed. A fresh native process remained running for five
  seconds before the smoke-test process was stopped. This confirms startup, not
  a hands-on visual review of the native window.
- No dependency or lockfile changes; no commit, push or deployment was made.
