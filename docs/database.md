# Cash-register database

The Rust library exposes `cash_counter::database::Database`. The GPUI workspace uses
it through `RegisterService` on the background executor. At startup the UI opens
the user-local database described in [the UI guide](ui.md).
Call `Database::open(path)` with an explicit application-data path to create or
reopen a persistent SQLite file. Its parent directory must already exist.

## Schema and lifecycle

Migration `src/database/migrations/001_cash_register.sql` creates four STRICT tables:

| Table | Purpose |
| --- | --- |
| `registers` | Register name and uppercase three-letter currency code |
| `register_sessions` | Local business date, opening count, cached totals, closing count |
| `cash_in` | Immutable positive incoming amounts, reasons, retry keys, UTC timestamps |
| `cash_out` | Immutable positive outgoing amounts, reasons, retry keys, UTC timestamps |

There can be one open session per register and one session per register/date.
The caller supplies a valid `YYYY-MM-DD` local business date. Opening cash is an
explicit count, not automatically carried forward. Different registers can use
different currencies; their amounts must not be combined without conversion.
Currency minor-unit conventions are the caller's responsibility (CAD uses cents).

All amounts use signed 64-bit integer minor units, never floating point.
Opening and counted amounts must be nonnegative; movements must be positive.
Expected balances may be negative to expose incomplete records. Overflow aborts
the write, including the movement and its summary update.

Entries are append-only. Corrections use compensating entries with explanatory
reasons. Closed sessions cannot accept new entries or be reopened. Exact retries
of already persisted entries can still return their original IDs after closing.
Retry keys are unique within each direction across all sessions; a reused key
with a different session, amount or reason is rejected. Persist and reuse keys
across retries; generating a new key for a retry defeats duplicate protection.

## Efficient running totals

Insert triggers increment the session's incoming or outgoing total in the same
SQLite transaction as each movement. Generated integer columns calculate:

```text
expected = opening + cash_in_total - cash_out_total
difference = counted - expected
```

`balance(session_id)` reads one row by primary key. Its cost is an indexed lookup
(approximately O(log sessions)), independent of the number of movements. Each
write performs one summary update plus indexed inserts; no ledger scan is needed.
`record_entries` batches writes in one immediate transaction, avoiding one disk
commit per entry and serializing retry checks across writers. Any failed entry
rolls back the entire batch. Large imports should use bounded batches to avoid
holding the single SQLite writer lock for long periods.

`entries` uses `(session_id, id)` indexes and keyset pagination (maximum 1,000
rows). Start with cursor 0 and pass the last returned ID. Entry IDs are scoped
by direction. This API returns history, not a balance at an arbitrary past time.
Balance and history query plans are tested for indexed access.

## Durability, migration, and recovery

File databases use WAL, FULL synchronous durability, a five-second busy timeout,
foreign-key enforcement, and recursive triggers. Memory databases use SQLite's
memory journal. A connection is private to its owner. Run this synchronous API
on a worker thread when integrating with GPUI. A timeout returns an error; the
caller can retry using the original request keys.

`PRAGMA user_version` tracks migration version 3. Migration 002 adds the
`(register_id, id)` index for daily-session history; migration 003 adds
`(business_date, register_id)` for dashboard and report date ranges. Schema changes and the version
update commit atomically under an immediate transaction. Opening an existing
version-1 or version-2 database upgrades it without changing records; newer versions are rejected. Only use paths
owned by this application. There is no destructive downgrade operation. Failed
migrations roll back; deployment recovery should restore a verified backup using
a compatible application version, rather than deleting financial records.

SQLite files are not encrypted. Store them in a user-private application-data
directory with appropriate OS permissions. The dashboard's Save verified backup action
uses SQLite online backup, includes committed WAL records, verifies integrity,
and publishes to a new destination without replacing existing files. For manual copies, close all connections
before copying the database and any remaining WAL files as a consistent set, or
use SQLite's online backup tooling. Do not copy only the main file while writers
are active. Backup scheduling and encryption are not implemented in this phase.

## Validation

```sh
cargo test --locked --lib --test database
cargo test --locked --doc
cargo fmt --all -- --check
cargo clippy --locked --lib --test database -- -D warnings
```

Tests cover persistence, concurrent writers, duplicate retries, migration rollback,
closed sessions, constraints, injection-like text, integer overflow, indexed
query plans, integrity checks, and 50,000 movements. The large-history test checks
correctness and pagination; it is not a latency benchmark.

References: [SQLite STRICT tables](https://www.sqlite.org/stricttables.html),
[SQLite durability settings](https://www.sqlite.org/pragma.html#pragma_synchronous),
and [rusqlite connection API](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html).
