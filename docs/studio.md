# Database studio

Relix uses GPUI Kit 0.7.1 for a native database workbench. It executes real SQL
through SQLite, DuckDB, PostgreSQL and MySQL drivers. Older cash-register documents
elsewhere in `docs/` are inherited reference material.

The option 1 workspace uses the native neutral shadcn theme, two-line connection
rows, a searchable explorer and underline query/output tabs. Format and Explain
sit beside Run; transactions and row limits remain on a second command row.
At widths of at least 1280 logical pixels, result filtering and Export share the
output tab row. Smaller windows retain them above the grid and in its footer.
Both panes remain resizable and hideable. This layout preserves the existing
connection, execution and data-operation handlers.

## Connections and SQL

Use Connect to choose a supported database from the searchable picker, then Next
to enter a local database path or server details. Categories filter the four
available engines; no additional drivers are advertised. Back returns to the
picker while preserving input values. Saved profiles open their settings directly.
Password fields are
masked. Remember password on this device stores server passwords in the OS
credential store after a successful connection. Existing profiles remain
session-only until you enter the password and enable remembering once. Edit
updates a profile through an explicit reconnect; reconnecting
closes the old session and can roll back an open transaction. Remove deletes only
saved profile settings and its stored password, leaving database files, drafts
and live sessions intact. A credential-removal failure keeps the profile for retry.
Test connection checks the supplied configuration; connecting
opens a retained session and loads its catalog. Profiles do not auto-connect.
Local file profiles store absolute paths so changing the launch directory does not
select another database.

Read-only is the default. Existing SQLite/DuckDB files open in their native
read-only modes. Server sessions enforce read-only transactions and restrict
commands that could escape that protection. The scratch database is explicitly
writable SQLite in memory and disappears when disconnected or the app exits.
TLS defaults on for PostgreSQL/MySQL and validates trust and hostname; disabling
it is an explicit connection choice.

SQL tabs keep separate editors and results. Run executes the selected text when
nonempty, otherwise the complete editor. Scripts expose separate result sets.
Explain accepts a single statement so later statements cannot execute accidentally.
Transaction controls act on the active retained session; transactions are not
automatically committed when switching query tabs. Disconnecting closes the session.

Queries bind to their connection. Choose that connection again or explicitly
retarget a draft before executing it elsewhere. Results indicate changes to the
editor since execution. Generated import/update scripts are drafts: preparation
does not change the database. Review them, then Run to apply them.

Formatting preserves SQL tokens and refuses a transformation that changes them.
Completion uses local keywords, catalog object names and inspected columns;
quoted identifiers follow the connection dialect. It does not contact a service.

## Explorer and data

The explorer filters tables and views. Refresh reloads metadata after schema
changes. Open table query quotes schema and object identifiers; schema inspection
shows column types, nullability, primary keys, indexes and outbound foreign keys.
Composite keys retain column order, and index definitions are shown where available.
Browse rows prepares a query with one typed column filter, optional ordering and
a page size/number. Review and Run it to load that page. Previous/next controls
run adjacent pages in the same tab and original connection. Editing or retargeting
the query removes those controls. Browse pages use their own execution row cap,
without changing the global query limit. Primary keys break ordering ties when
available; offset pages do not provide a snapshot across concurrent changes.

The result grid filters loaded rows and sorts columns. NULL is displayed distinctly.
The grid remains bounded rather than fetching an entire table. Exports use the
loaded result set, including rows hidden by a grid filter. CSV uses standard
escaping but represents both SQL NULL and empty text as an empty field. JSON
exports a column descriptor array and row arrays, preserving nulls and duplicate
column names; non-null values are strings with database types in the descriptors.
View cell opens the selected value in a read-only editor and copies its exact
text; SQL NULL is labeled separately and is not copied as the literal text NULL.
Markdown export escapes markup and represents SQL NULL distinctly. INSERT SQL
export requires an unchanged base-table query and its original live connection;
it prepares a file for review and does not execute it. It rejects ambiguous
binary/compound/unknown values and duplicate column names. Scalar display strings
are converted by target column types; it is not a lossless database dump.

Edit cell requires an unchanged table query, a writable original connection, a
base table and all primary-key columns with non-NULL values. It prepares an UPDATE
guarded by the primary key and original row values. Zero affected rows indicates
a stale record; review this before retrying. Binary and compound values are not
editable through this text form. Arbitrary inserts/deletes and schema changes are
available through SQL rather than hidden automatic mutations.

CSV import prepares a new table and a bulk INSERT. Headers become quoted column
names; all imported fields are text, including empty fields. SQLite, DuckDB and
PostgreSQL scripts use a transaction. MySQL DDL can commit implicitly, so a failed
import may leave its new table; the script explains this limitation. If a transactional import fails after BEGIN, run
ROLLBACK before retrying; scripts stop at the first error and preserve the session's
transaction state.

## Persistence and limits

Windows workspace data is stored in `%LOCALAPPDATA%/Relix/workspace.json`.
macOS uses `~/Library/Application Support/Relix/workspace.json`; Linux uses
`$XDG_CONFIG_HOME/Relix/workspace.json` or `~/.config/Relix/workspace.json`.
This file contains saved SQL and history, which may contain sensitive user data.
Connection passwords are excluded from serialization. Unsaved SQL tabs are not
restored; explicitly save a query or a SQL file before closing.

Remembered passwords use Windows Credential Manager, macOS Keychain, or Linux
Secret Service through the pinned keyring 3.6.3 native backend. Linux needs D-Bus
development libraries to build and an unlocked Secret Service provider to use
remembering. Other platforms report that secure storage is unavailable; there
is no plaintext file fallback. macOS and Linux runtime behavior has not been
validated on this Windows development machine.

Credential entries use the profile ID and the service namespace
`Relix.DatabaseStudio.Passwords.v1`. The protected record binds the password to
the engine, host, port, database, username and TLS setting. Changing those details
requires re-entering the password; changing the display name does not. Temporary
credential buffers are cleared after use, while active sessions and masked inputs
necessarily retain passwords in memory. OS account protection applies; this is
not isolation from other programs running as the same user. Windows credential
roaming can follow the operating system's policies.

Test connection never saves a password. A successful connection saves profile
metadata before updating the credential store. Unchecking Remember or submitting
an empty password removes its previous credential after a successful connection.
Storage errors are visible and leave remembering disabled, with the connection
usable for the current session. Credential and workspace writes are separate:
if the final metadata write fails, an OS entry can remain without a remembered
profile flag; if profile deletion cannot persist, the old profile can return with
its password already removed. Errors remain visible and re-entering the password
or retrying the operation is required. The protected record has a 2,400-byte limit
including connection metadata; larger credentials remain session-only.

Ordinary tests use an isolated memory store. The explicit native-store probe uses
a unique synthetic credential and removes it, including on assertion failure:
`cargo test --locked native_store_round_trip -- --ignored`.

Workspace writes use a same-directory temporary file, a backup, a write lock
and a conflict check against other windows. Invalid or newer-version files are
preserved and reported instead of overwritten. A missing primary file can be
recovered from its backup. Storage failures remain visible in the workbench.

| Limit | Behavior |
| --- | --- |
| SQL editor execution / saved query | 512 KiB |
| Script | Up to 1,000 statements |
| Results | Configured row cap, maximum 10,000; bounded result memory |
| Columns | Up to 1,024 result columns |
| Execution | 60-second query deadline; cancellation/cleanup may take longer |
| Connection | 10-second connection/socket timeouts per driver step |
| Saved history / queries / profiles | 100 / 256 / 100 |
| CSV input | 1 MiB, 10,000 rows, 128 columns |
| Generated import SQL | 512 KiB |
| INSERT SQL / Markdown export | 512 KiB / 1 MiB; up to 128 columns and 10,000 loaded rows |

MySQL also bounds socket stalls to 10 seconds. A failed or timed-out server session
may require reconnecting before it can be used again.

Stop requests cancellation and reports when the operation finishes. A cancelled
script may have completed earlier statements; use explicit transactions when
atomic behavior is required. Database operations and SQL/CSV/JSON file reads and writes run off the UI thread.
Workspace saves use bounded local file replacements.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl/Cmd+N | New query |
| Ctrl/Cmd+Enter | Run selected SQL or the full script |
| Ctrl/Cmd+Shift+Enter | Run the statement at the cursor |
| Ctrl/Cmd+S | Save query to workspace |
| Ctrl/Cmd+O | Open SQL file |
| Ctrl/Cmd+Shift+F | Format SQL |
| Ctrl/Cmd+B | Toggle explorer |
| Ctrl/Cmd+J | Toggle results |
| Ctrl/Cmd+F | Find in the focused editor |

Current-statement execution preserves original SQL and recognizes quoted
semicolons/comments using the connection dialect. Compound procedural bodies
require selection or full-script execution rather than a guessed boundary.

## Components and architecture

The database navigator groups the **selected connection** into database,
schemas, Tables/Views and sorted object names. Schemas are derived from the
session's catalog, so empty schemas and unsupported DBeaver object categories
are not listed. Select a connection profile to switch its catalog. Selecting a
folder cannot query or inspect an object. Search matches schema/object names,
retains the matching parent path and temporarily expands it. Collapse choices
are remembered for the session. Small native tree rows preserve keyboard
navigation and expose complete names through hover tooltips.

The flat application toolbar groups secondary SQL actions in **Files**.
The result footer groups CSV/JSON/Markdown/INSERT options in **Export**.
Run and the record/paging actions stay visible. Native dialogs share a compact
frame and purpose-specific heading hierarchy; connection forms have a flat
engine selector. Kit continues to own focus, dismissal and scroll behavior.

Ctrl/Cmd+K opens the Kit command palette: search commands, open tabs and saved
queries; use arrow keys, Enter and Escape. Rename updates the captured query
tab and its saved title. Duplicate creates an independent unsaved SQL draft
with the original connection, without results or mutation provenance. Saved
queries and history support case-insensitive text search.

For unchanged table results on the original writable connection, Insert row
prepares literal values, SQL NULL or omitted defaults. Delete row requires a
selected record, a complete non-NULL primary key, original-value guards and
confirmation. Both create SQL drafts and perform no writes until Run. Review
triggers and cascading foreign keys before running DELETE. Binary, compound
or unknown values require explicit SQL. Forms support at most 128 columns.

GPUI Kit owns theme colors, typography, focus, dialogs and component styling.
Toolbar, Sidebar, Tree, Input, Editor, Select, Checkbox, TabBar, resizable panels,
DataTable, Dialog, Tooltip and StatusBar compose the workbench. The editor has
SQL highlighting, line numbers, find and undo. Light/dark appearance and panel
visibility are saved preferences. Window minimum size is 900 by 620 logical pixels.
`src/studio_theme.rs` translates the installed DBX neutral light/dark tokens
through Kit theme roles while preserving syntax, status and chart colors.

- `src/main.rs`: assets, startup, window lifecycle and module registration.
- `src/studio/`: component composition, connection forms, query lifecycle, results,
  file operations and reviewed cell editing.
- `src/database/`: engine adapters, retained sessions, scripts, metadata and limits.
- `src/workspace.rs`: saved profiles, queries, history and recoverable persistence.
- `src/data_ops.rs`: import/export and safely quoted update previews.
- `src/table_browse.rs`: metadata-validated filter/order/page SELECT generation.
- `src/sql_tools.rs` and `src/sql_completion.rs`: formatting and local suggestions.

See the maintained general GPUI reference in [gpui/](gpui/README.md). Verify
examples against the pinned Kit source; some framework notes describe an older
application. GPUI Kit's [component catalog](https://gpui-kit.com/component/)
was reviewed through Browser for this implementation.

## Scope

The workbench draws feature ideas from [DBX](https://github.com/t8y2/dbx),
[Beekeeper Studio](https://github.com/beekeeper-studio/beekeeper-studio) and
[DbGate](https://github.com/dbgate/dbgate), with an original implementation.
The agreed next stage includes advanced features such as SSH tunnels, ER diagrams,
schema comparison, AI integration and MCP/CLI. Those features are not implemented
in this four-database stage.
See [DBX feature comparison](dbx-comparison.md) for repository review coverage,
implementation references and remaining differences.
