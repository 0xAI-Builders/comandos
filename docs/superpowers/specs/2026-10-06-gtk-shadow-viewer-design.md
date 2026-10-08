# GTK Shadow read-only terminal viewer

Root authorized and confirmed this design on 2026-10-06. The hard contract is zero writes and zero geometry changes in the inspected tmux server. tmux 3.2a read-only,ignore-size cannot guarantee it for a lone ordinary PTY client. Existing red receipts remain preserved outside this source checkout.

Use snapshots for Shadow only. Control mode could avoid resize, but it creates a client and would require a new streaming protocol with separate targeting and parser ownership. A normal attach cannot satisfy the contract; anchor clients or option mutation would also violate it.

The Shadow terminal has no PTY, attach, respawn, stdin or SSH input path. Its explicit TmuxCtl is restricted to Shadow. A single owned worker requests one grouped window capture only while mapped, at most twice per second, with one request in flight and one bounded result slot. Hiding cancels in-progress work; closing cancels and releases the worker. No GTK subprocess calls or per-pane timers/jobs.

Capture all panes in the selected current window using =session: targeting for metadata and pinned pane IDs for screen reads. Metadata and socket identity are checked before/after; obsolete, partial, changed-layout and cancelled results are discarded. Commands remain individually read-validated even when grouped into one tmux process. Bounded capture stdout and a whole-capture deadline prevent runaway accumulation.

The local terminal engine renders each pane's captured text and ANSI SGR at its original window coordinates, with local pane borders and active cursor. Widget allocation never becomes a server size request. Local selection and links work through the existing renderer; a selected frame pauses capture so selected text remains stable. Live/Sandbox retain their existing native PTY, input and scheduling path.

This is an audit viewer, not a replacement interactive tmux client: it displays current screen snapshots, not the tmux status line or historical scrollback. ANSI SGR is retained; other terminal control sequences are not replayed. Smaller viewports may clip the immutable source grid. Remote GTK render/selection validation remains a separate Root gate.

Implementation steps: preserve a production-PTY red with an observed real client; add capture/worker/engine tests and real private-server geometry/data/client/environment checks; add cancellation-aware read process seam retaining owned group identity before reap; wire both TermOptions consumers and teardown; run focused terminal/tmux/process suites, App all-target Clippy, fmt, build; freeze a clean commit and supply independent-review provenance. No local GTK initialization, personal tmux, SSH or global settings.
