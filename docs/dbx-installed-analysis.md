# Installed DBX evidence and native translation

The reverse-engineer-anything skill was used to inspect `C:\Program Files\DBX\dbx.exe` read-only. Windows version metadata reports **0.6.26**; the executable is 73,333,144 bytes with SHA-256 `3d2f23626335dd237f57310c51e3d3dd8b742d5d818976f63987397d550d6edc`. This installed version differs from the GitHub revision in `dbx-comparison.md`.

## Observations

PE inspection found nine resources. Selected embedded `/assets/` frontend payloads were recovered through Brotli decompression without executing their code. Twelve derived assets and their original byte ranges and decoded hashes are retained in the sibling analysis directory `../dbx-installed-analysis/extraction-manifest.json` relative to the repository root. They are reference artifacts, not Relix dependencies.

- Shipped CSS defines neutral shadcn-style light and dark surfaces and 4/6-pixel corner radii. Compiled theme code supports light/dark/system, palette selection and persistence. Relix translates the neutral light/dark colors into the pinned GPUI Kit's semantic roles in `src/studio_theme.rs`, preserving Kit syntax, chart and status colors. The light focus ring is strengthened from #a1a1a1 to #737373 for visible keyboard focus. System mode and additional palettes are not implemented.
- Quick Open statically indexes connections, schemas, tables and views with several matching strategies. Relix provides a keyboard-searchable native command palette, query-tab switching and saved-query opening; it does not claim the same fuzzy matcher or full object index.
- The compiled grid editor stages new/dirty/deleted rows, checks read-only access and requires preview SQL and confirmation before adapter save. Relix translates single-row INSERT/DELETE into new SQL drafts for explicit review and execution, alongside its existing guarded UPDATE. DELETE requires a complete non-NULL primary key and checks all original values. Unsupported binary/compound values require explicit SQL. Relix does not implement DBX's batch mutation adapter or rollback generator.
- Compiled history, column layout, file and import assets expose search/filtering, persisted layouts and staged import workflows. Relix adds saved-query/history text search and independent query duplication/rename. The broader layout/import workflows remain separate work.

## Evidence records

| Record | REA evidence ID |
| --- | --- |
| PE resources | `ev_a0344f36b11148fe77da0c544c717e9ea850b5e8fa9a57f5aedd2c81668e1095` |
| Derived JavaScript application | `ev_5014413d3f52199e2ea34facc0624f04451472997a58a89462bdeaed87013638` |
| Application summary | `ev_561855635c5673464dce1aa5fd829d95da8c7f9c7e3e2cdd74e0b16bca1a237d` |
| Theme literal provenance | `ev_53997d3a1d29fa0189e19d1baa9a36ce83c192c470e2618e7cace8ca5bd0aeb9` |
| Quick Open module containment | `ev_13c25fdba439b9cb1d531b2d51e617a54b46c08d3aeb8ddc1d5e3d59fe8d2185` |
| Grid editor module containment | `ev_a6e11fe01282fc1312881616d8664f1f98ecb3a0fe01f6af514b082847d8c3ab` |

## Limits of the analysis

These observations are static evidence of shipped code, not proof of runtime reachability or native enforcement. All eleven selected JavaScript files parsed, but semantic expansion for Quick Open, import and grid editing reached resource limits; missing imports and dynamic calls remain unresolved. Ghidra is not configured, so native decompilation was unavailable. DBX and the recovered code were not executed. No installed files, DBX profiles or user databases were changed or inspected.

## Relix validation

Formatting, locked all-target Cargo check, all 67 tests, Clippy with warnings denied,
and the locked native build passed. Tests cover SQLite/DuckDB insert defaults,
NULLs, injection attempts, composite primary keys and stale deletion guards;
native command keyboard confirmation/cancellation, saved connection ownership,
tab limits, rename synchronization and minimum-window controls; and theme contrast.
A native INSERT form test confirms preparation leaves the table unchanged and
rejects stale query provenance. A fresh Relix process remained running for five
seconds before its owned smoke-test process was stopped. This verifies startup,
not a hands-on visual review. Live PostgreSQL/MySQL execution remains untested.
No dependencies were added; no commit, push or deployment was made.
