# GTK semantic layout dump, schema 1

This is the diagnostic contract for the native App and the independently
observed Python GTK/VTE oracle. A unit-test context or exported workspace fixture
is not an accepted capture. Both implementations must collect their own running
widgets, authoritative workspace, applied presentation, and terminal content.

## Activation and publication

Set `COMANDOS_APP_LAYOUT_DUMP=1` explicitly. Other values and an unset variable
leave the diagnostic timer and captures disabled. `COMANDOS_APP_LAYOUT_TIMEOUT_MS`
accepts 100–60000; the default is 15000. Start the real application with a private
HOME, runtime directory and explicit private tmux socket, using the existing
fixture loader. This option never launches an agent or changes the workspace.

The output is the config-derived `layout_dump_path()`: sandbox uses
`sandbox_root()/layout.json`, shadow uses
`runtime_dir()/comandos-app-shadow-layout.json`, and live uses
`runtime_dir()/comandos-app-layout.json`. These are API-relative descriptions,
not user-supplied path overrides. Resolve and report the actual absolute path
from that invocation. All stale removal and atomic publication use WriteGuard;
parent symlinks and paths outside the mode's allowlist are rejected. Publication
is one diagnostic-only local atomic write, independent of the process-job queue.

At 100 ms intervals the owned timer checks restore completion, startup validity,
window mapping, completed successful dashboard load, completed device-focus
restore, first connected preference/session-state/marks fetch, workspace
availability, outstanding workspace POST/resize, dragging, quick-terminal work,
and terminal PTY/closed state plus pending synchronized-output flushes. The entire
semantic snapshot must remain identical for 300 ms before the deadline. A late
timer cannot publish success after the deadline. Unmeasured fonts/allocations or
incomplete terminal grids fail validation. No timestamp or process identity
participates in stability checks.

Success has `readiness: {status: "ready", issues: []}`. Deadline failure has
`schema: 1`, `readiness: {status: "failed", issues: [observed reasons...]}` and
capture metadata only. A failed artifact is evidence of missing readiness, never
a comparator reference. Guard/publication failures go to stderr; no success
artifact is manufactured. The timer is owned by AppRuntime and cancelled at
shutdown. An opted-in capture may include private terminal text and hyperlinks.

## Exact semantic fields

JSON object member order is immaterial. Array order and every semantic value are
significant, including nulls, whitespace, Unicode, hidden text and blank cells.

| Root field | Source and meaning |
| --- | --- |
| `schema` | Integer 1. |
| `capture` | Real `captured_at_unix_ms`, `process_pid`, `window_handle`. These three fields alone may be ignored across processes. |
| `readiness` | Actual diagnostic outcome. Only `ready` captures enter parity comparison. |
| `viewport` | `{width,height,dpr}`: allocated window size in GTK logical pixels, actual GTK scale factor. |
| `window` | `{title}`: current native window title. |
| `dashboard` | `{uri,title,loading}` from the native WebKit widget. No injected script or fabricated page state. This identifies the page; DOM/pixel acceptance is a separate gate. |
| `theme` | `{name,tokens,ansi,button_style}` from the successfully applied App CSS theme and button style. Initial theme includes its actually applied `sutil` button style. Tokens are the entire applied token object; ANSI is the entire applied array. |
| `font` | Actual Pango-resolved window font, `{family,size,size_unit,weight,style}`. `size_unit` is `pt` or `px`; enum strings are lower case. Requested preferences are not a substitute for a resolved font. |
| `tabs` | Logical TabRegistry order, including hidden and closed entries. Each entry has actual `key,label,kind,favorite,selected,state,attached`. `kind` is `local`, `session`, or `web`; `attached` means an owned native TermView exists. |
| `strip` | `{order,selected,layout}` from the actual displayed strip entries and current CSS class. Includes group entries; `layout` is `rows` or `single`. This is independent of logical tab order. |
| `workspace` | `{available,revision,groups,focused_tab,selected_tab,active_group}`. `groups` is the complete authoritative document's groups array, retaining hidden leaves, split ratios and metadata. Unavailable authority has null revision and empty groups. Focus, selected tab and notebook group are independently observed. |
| `widgets` | Stable ordered list of directly owned GTK widgets, described below. |
| `terminals` | Actual native TermView snapshots in logical tab order, described below. |

Tab `label` reads the current label widget, including user edits. `state` is
`{state,mark,editing}` from the TabLabel's active presentation state; no label
is reconstructed from a session key. Inspect the native TabLabel implementation
for state strings rather than assuming an agent name is a state.

## Owned widgets and split identities

Every widget entry is `{id,role,label,font,visible,mapped,focused,geometry}`.
Geometry is `{x,y,width,height}` in window-relative logical pixels, obtained by
GTK coordinate translation and allocated size. Unmapped widgets have null
geometry. `visible` and `mapped` are separate actual GTK properties. `focused`
includes an actual focused descendant. For text widgets, `label` and `font`
come from the native GTK label/button and resolved Pango font. Container entries
without text have null label/font.

The owned order starts with window, main pane, dashboard, tabstrip, workspace,
fallback notebook, toolbar, status, drag overlay, content root, header and header
descendants. Toolbar descendants follow. Native header/toolbar descendants use
colon-separated actual GTK child indices and role `chrome-content`; adapters
must enumerate the running widget tree, including actual labels. Strip entries
follow with `strip:<key>` and their actual descendants (including model/sticker,
suggestion and favorite text), then each logical tab's `tab-text:<key>`, owned leaf
if present (including actual leaf/header/placeholder descendants), and
`terminal:<key>` if attached. Surviving split widgets finish
the list.

Split entries additionally have `split: {group,path,axis}`. IDs are
`split:<group>:<compact JSON numeric path>`. The numeric path is through the
**full authoritative tree**, where first=0, second=1. Pruning a missing first
leaf and promoting an inner split never changes that inner split's identity.
The ratio remains exact in `workspace.groups`; no pixel-derived ratio replaces
authority. Python must retain pre-pruning split paths while projecting its own
visible widgets. An adapter unable to identify a split must report that missing
capability, rather than infer an identity from Rust's output.

Only direct owned allocations at `widgets[i].geometry` may receive the agreed
±1 logical-pixel tolerance. Workspace ratios, terminal cell measurements, all
other geometry-like semantic data and all non-geometry fields remain exact.
The legacy `capture(window)` widget-only API is preserved separately; it is not
the semantic capture or evidence of parity.

## Terminal snapshots

Each entry has `key,grid,font,font_scale,cell_metrics,opacity,focused,closed,
selection_text,preedit,cursor`. Font uses the same resolved Pango schema.
`cell_metrics` is `{width,height,origin_x,origin_y,dpr,font_size_px}` from actual
painting/layout state, not an estimated grid or requested preference.
`cursor` is `{row,col,shape,visible,painted,wide}`; `visible` is the application
cursor mode and `painted` includes actual focus/blink paint gating. Shapes use
lower-case native enum strings.

`grid` is `{cols,rows,display_offset,cells}`. Every visible row contains every
column, including trailing blank cells. No content truncation is permitted;
more than 100000 visible cells causes capture failure. Each cell contains:

| Field | Normalization |
| --- | --- |
| `text` | Actual scalar plus combining characters; blank cells are a space. Wide spacer cells have empty text. A tab/NUL displayed as space is normalized to space. Hidden text is retained. |
| `width` | 0 for a wide spacer, 2 for a wide lead cell, otherwise 1. |
| `fg,bg` | Effective RGB triplets (0–255), including OSC palette changes, inverse swapping, and native VTE-compatible dim handling. Bold does not implicitly brighten ANSI colors. |
| `bold,italic,dim,hidden,inverse,strike,wrap` | Actual cell flags. |
| `underline` | `none`, `single`, `double`, `curly`, `dotted`, or `dashed`. |
| `underline_color` | Effective RGB triplet when explicitly set, otherwise null. |
| `hyperlink` | Actual OSC-8 URI or null. |

Live embedded xterm WebViews have no native cell accessor in this slice. If one
exists, capture fails with `web_terminal_content_unavailable:<key>`; its pixels
or guessed text cannot stand in for missing cell content. Sandbox's unavailable
web-backend placeholder is observed as its actual native label. It has no owned
TermView or terminal grid, matching its actual unavailable state.

The Python adapter must obtain visible cell text, dimensions, colors, attributes,
selection, cursor and metrics from the real VTE/widget APIs or an independent
instrumented original implementation. VTE text extraction must not drop blank
cells, wide spacers or combining characters. APIs that do not expose a required
attribute must produce a named capability failure. Copying the candidate dump,
filling unsupported attributes with guessed defaults, deriving expected content
from the fixture alone, or writing a synthetic ready capture is prohibited.

## Evidence limits

Pure tests validate the native terminal snapshot, complete-content checks,
readiness/deadline behavior and guarded atomic IO without GTK initialization.
Compilation validates the GTK API wiring. These checks do not establish that
the displayed Python and Rust applications match. The real remote fixture,
independent oracle capture, semantic comparator, screenshots and remaining T12
runtime/performance checks still require their explicit validation gates.
