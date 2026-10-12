# Relix product context

Relix is a native Rust database studio for working with different databases.
The user selected SQLite and DuckDB locally, plus PostgreSQL and MySQL servers.
Complete this four-database workbench before advanced features. DBX, Beekeeper
Studio and DbGate inform familiar workflows; Relix retains GPUI Kit components.

The primary workflow is browsing database objects, drafting SQL and inspecting
results/messages/history. This is a desktop working tool, so usable workspace
proportions, keyboard controls, resizable panes and predictable feedback matter.
The local GPUI reference is in `docs/gpui/`; current project behavior is in
`docs/studio.md`. Older cash-register docs are inherited material.

Current scope: real connections and SQL execution, table/view browsing, schema
inspection, transactions, results, saved queries, persistent history and local
file import/export. Improvements should make connection context, write access,
busy state, errors and result limits clear without crowding the editor.

AI, MCP/CLI, SSH tunneling, ER diagrams, schema comparison, additional drivers,
plugins and administration tooling are subsequent work. Do not represent
incomplete integrations as supported features. Remembered server passwords use
the operating system's credential store; local workspace persistence stores
profiles without passwords. Session-only passwords remain available in memory.
