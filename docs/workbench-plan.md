# Four-database workbench

The requested scope is a working native GPUI Kit studio for SQLite, DuckDB,
PostgreSQL and MySQL. The user selected completing these four before advanced
features. Existing GPUI notes remain in `docs/gpui/`; the current studio guide
owns the application's actual behavior.

## Research and scope

Sources inspected on 2026-10-09:

| Project | Relevant ideas for Relix |
| --- | --- |
| [DBX](https://github.com/t8y2/dbx) | Connection profiles, object search, SQL tabs, scripts, explain, query library, persistent history, result inspection and data import/export |
| [Beekeeper Studio](https://github.com/beekeeper-studio/beekeeper-studio) | Straightforward query workflow, filterable/sortable tables, keyboard actions, saved queries and light/dark appearance |
| [DbGate](https://github.com/dbgate/dbgate) | Schema inspection, reviewable SQL changes, table browsing and import/export |
| [DuckDB](https://github.com/duckdb/duckdb) and [Rust client](https://github.com/duckdb/duckdb-rs) | Embedded analytical SQL engine; an engine integration rather than a replacement UI framework |

These references inform behavior. Their application code, branding and assets
are not imported. Relix retains its native Rust/GPUI Kit stack. DBX declares
Apache-2.0; Beekeeper and DbGate include GPL-licensed application code.

The first implementation target is connections, live tables/views and columns,
background SQL execution with cancellation, transactions, bounded results,
query files, saved queries, persistent history, and data import/export. The
studio guide and validation evidence record which paths are implemented and
which remain limited; this plan does not certify feature completion.

AI providers, MCP/CLI services, SSH tunnels, ER diagrams, schema comparison,
database transfer, backup tooling, NoSQL browsers and plugin stores are later
work. There is no claim of complete DBX parity or support for every database.

## Architecture and constraints

- `database`: engine adapters, typed result metadata, NULL-aware cells, catalog
  inspection, quoting, cancellation and driver errors. Sessions retain their
  connection so explicit transactions have a predictable lifecycle.
- `studio`: GPUI Kit controls, interaction state and owned asynchronous tasks.
  Results belong to the submitting draft and connection. Editing invalidates
  stale output; response handling checks identity before applying it.
- `workspace`: profiles without passwords, saved queries, history and layout
  preferences. Writes preserve the previous file and reject concurrent changes.
- Data operations use engine quoting and explicit review before mutations.
  File imports are bounded and do not interpolate unchecked identifiers.

Opening a saved profile does not automatically connect or run its SQL. New
connections default to read-only; a deliberately created local scratch database
is writable. TLS certificates and hostnames are verified for server connections
when TLS is selected. Credentials are neither logged nor persisted.

## Validation and recovery

Run format/check/Clippy, backend integration tests on temporary or in-memory
local databases, workspace persistence tests and headless GPUI workflow tests.
Native smoke tests cover connection setup, SQL/results, failure feedback and
layout. Server tests require known isolated test servers; no existing databases
or credentials are probed automatically.

This repository has no initial commit and all existing files are untracked.
Work stays in the current tree with explicit module ownership. No user work is
staged, committed or discarded. Existing documents remain intact.

Workspace storage uses the user's application data directory. Saves write and
sync a temporary sibling before replacing the primary file. A `.bak` preserves
the prior version, a lock coordinates saves, and a revision check prevents lost
updates across app instances. Invalid files remain untouched and produce an
error. Database schema changes are initiated through the application's explicit
write workflow; there is no automatic migration of user databases.
