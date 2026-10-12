---
name: Relix
description: Plain native database workspace built with GPUI Kit.
---

# Design System: Relix

## Overview

Relix is a desktop workspace in Operate mode. The implemented surface follows
GPUI Kit's native components and standard input/focus behavior. Its plain
appearance supports browsing objects, drafting SQL and inspecting tabular output.
The selected option 1 direction uses a graphite workbench, two-line connection
rows, a searchable explorer and restrained underline tabs. Native Kit components
retain the neutral shadcn treatment in both appearances.

**Key Characteristics:**

- Flat commands grouped by spacing and restrained structural dividers.
- Resizable editor and output panes.
- Visible connection, read-only, execution and result-limit context.

## Colors

The Kit owns semantic theme roles: background, foreground, sidebar, border and
muted foreground. `src/studio_theme.rs` maps the installed DBX neutral shadcn
light/dark tokens into those roles for all native components. The toolbar
switches between the persisted light and dark modes. SQL syntax, chart and
status colors retain the pinned Kit palettes; focus rings remain visible.
Alternate table rows use a near-background tone. Workspace controls and menus
have no elevation; dialogs retain a small shadow to distinguish the modal layer.

## Typography

Interface text uses the theme system font and the small body size. Supporting
labels use the extra-small size; the app name, explorer heading and message
headings use semibold weight. SQL and history query snippets use the theme
monospace font. This surface has no display typography.

## Layout

Hierarchy: application toolbar; left connection/object explorer; main query tabs
and SQL editor; bottom Results/Messages/History; window status bar. The editor
and results receive most of the width. The explorer and result split are
resizable and can be hidden. Keep the primary Run command visible.

The toolbar is 56 logical pixels high and the query command row is 52. The
explorer starts at 280 pixels and resizes between 220 and 380; the output starts
at 44% of the viewport height, clamped between 220 and 440 pixels, with a
140-pixel resize minimum. Compact Kit spacing separates commands,
labels and list entries. The supported minimum window is 900 × 620 logical
pixels. No mobile surface is implemented.

## Elevation & Depth

Workspace panes are flat. Theme borders, sidebar tone, selection and striped
table rows establish separation. The application adds no decorative shadows,
gradients or custom motion; overlays retain the Kit's standard treatment.

## Shapes

The Kit owns control radii and borders. Workspace panes meet at straight
dividers. Lucide icons come from the bundled asset catalog.

## Components

- The toolbar groups connection and scratch actions, query creation and a labeled
  Files menu, then search, pane and appearance controls. At widths of at least
  1280 pixels, command search has a labeled 220-pixel affordance; smaller windows
  use the accessible search icon. Buttons use ghost
  treatments; Run remains the primary action. File opening, workspace save and
  SQL file save remain available through the Files menu and existing shortcuts.
  Unavailable connection and stop actions remain disabled with explanatory
  tooltips. Icon-only buttons have tooltips and accessible labels.
- The explorer uses Kit Sidebar, Input, Tree and ListItem components. Its
  connection area, filter, scrollable object tree and table action stay distinct.
  Connection rows show the engine and saved/live access state beneath the name;
  the selected connection retains its edit and removal commands.
  The active session's navigator follows connection → database → schemas →
  Tables/Views → object. Schema and object names are sorted; category counts
  reflect the current search. Rows use the theme's small sans-serif text,
  28-pixel height, 16-pixel neutral icons and medium-weight parent names.
  Full names remain available on hover. Search expands matching ancestor paths;
  session-only collapse choices return when search clears. Folder selections
  never enable object commands. Only discovered schemas and supported object
  kinds appear; no placeholder DBeaver categories are presented as supported.
- Query tabs use an underline treatment and contain close controls; the SQL editor fills the remaining pane.
  Run is a small primary button with Ctrl/Cmd+Enter support. A compact current
  statement command supports Ctrl/Cmd+Shift+Enter. Format and Explain share the
  command row with live connection/access context. Transaction and row-limit
  controls occupy a wrapping secondary row. A mismatched draft keeps its original
  target label and explicit Retarget command. The live connection owns the dialect.
- Output uses underline tabs, a compact striped DataTable, scrollable messages
  and session history. Messages expose status or alert semantics. The footer
  labels loaded rows and stale results, with Copy TSV disabled for empty output.
  One labeled Export menu offers CSV, JSON, Markdown and SQL INSERT formats.
  Wide Results views place Export and the loaded-row filter beside the output
  tabs. Compact windows put the filter above the grid and Export in the footer.
  NULL cells use muted italic text, preserving their distinct value semantics.
  Paging and record actions remain directly accessible at minimum width.
  View cell uses a native dialog and read-only editor; schema details group
  columns, indexes and foreign keys. Browse controls prepare reviewable SQL.
- The status bar reports connection, access mode and session-only drafts.
- Native dialogs share an eight-pixel frame, small elevation, consistent inset
  and clear section/title/description hierarchy. Connections, table browsing,
  record review, result inspection and query management have purpose-specific
  headings. New connections start with a searchable database picker: category
  navigation, a two-column grid of the four supported engines, and Next/Cancel.
  Connection settings follow with Back, Test connection and Connect. Existing
  profiles open directly in settings; Back preserves entered values.
  Kit owns focus, keyboard dismissal, scrolling and modal lifecycle; footers
  remain reachable at the minimum window size in both appearances.
- Command palette items use one horizontal row containing a two-line label and
  description, with a separate shortcut badge at the trailing edge. Native Kit
  search, selection, keyboard confirmation and disabled-action guards remain in
  place. Each 60-pixel row contains its own text without overlapping its neighbors.

Preserve empty output, execution/error messages, edited drafts with stale results
marked, empty history, filtered objects, disabled unavailable commands,
tab close confirmation and both theme modes. Draft/result
state remains associated with its query tab across tab switches.

## Do's and Don'ts

### Do

- Use GPUI Kit components and semantic theme roles.
- Preserve keyboard navigation, accessible labels and visible focus treatments.
- Keep connection and read-only context visible beside the relevant controls.

### Don't

- Add decorative cards, gradients or custom motion to this plain workspace.
- Present an unavailable driver or integration as supported.
