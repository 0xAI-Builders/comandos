# Desktop input lag investigation

The live desktop runs commit `4085247`. Its [GTK poller](../../bin/cc-app)
fetches notification counts in a background thread, keeps the latest pending
update and skips identical widget writes. The user's 31 tmux pane identities and
shell PIDs survived installation. Terminal history and agent processes remain
owned by the existing tmux server.

This report records measurements on the user's Linux laptop on September 14,
2026. CPU percentages below represent one logical CPU; the machine has 28.
Short samples establish the observed change, not a long-duration guarantee.

| Measurement | Before intervention | After intervention |
| --- | --- | --- |
| Desktop client CPU, sampled from `/proc/PID/stat` | About 95% with the old client; 12-14% after its first restart | 5.2%, 5.3%, 5.7% in three six-second samples after installing the correction |
| GNOME Shell CPU | About 65% immediately before the last interventions | 25.1%, 35.0%, 32.8% in the same samples |
| Notification daemon CPU | 25-38% during the original accessibility churn | 0% in a five-second sample after restart |
| Available memory | About 27 GiB during the initial investigation | 36.6 GiB |
| Synthetic 19-tab scenario, ten unchanged polls | 210 SVG loads, ten overlay rebuilds, twenty geometry queries | 21 SVG loads, one rebuild, ten queries |

The GNOME change followed both closing an unused Seahorse application service
and installing/restarting the client. Their individual contributions were not
isolated. The keyring daemon was preserved. The SVG decoder alone took about
0.059 ms per load in an isolated test; decoding does not explain the original
95% client CPU.

## Confirmed failures

TranscriptRealTime launched a new native meeting probe every two seconds. Its
AT-SPI connection initialized accessibility caches in unrelated applications.
Pausing that frontend reduced tempmon CPU from 23.1% to 4.3%, keyboard settings
from 15.6% to 1.8%, and media keys from 15.2% to 2.0%. Resuming it restored the
load. Removing the automatic accessibility scan stopped that fan-out. The source
and installed `native_probe.py` both contain the correction. Window/audio
detection remains available; automatic participant enrichment is unavailable.

ComandOS performed synchronous HTTP inside `update_dots`, with a two-second
timeout. An injected 80 ms response blocked that callback for 80.2 ms. Repeated
file replacements and identical size allocations also rebuilt unchanged pills.
The poller accumulated callbacks while GTK was busy. The focused
[regressions](../../tests/test_desktop_performance.py) exercise these actual
callbacks with isolated GTK, HTTP and tmux boundaries.

Verification passed 81 tests covering desktop performance, TUI observations,
pane snapshots, desktop tabs and terminal selection. The ten final performance
regressions passed separately against committed `4085247`. After installation,
`/state` returned HTTP 200 and the saved/current pane identity sets matched.

## Operational state and remaining limits

WhatsApp and Telegram MCP servers, linked accounts, databases, media and voice
recognition were migrated to the Mac. Final SQLite snapshots matched hashes and
row counts; both services returned healthy and paired through the original
laptop endpoints. Their old laptop units and Whisper transcribers are masked.
One SSH tunnel maintains the local HTTP endpoints. The independent ComandOS
Telegram bot still operates local tmux sessions from the laptop.

TranscriptRealTime's saved provider was OpenAI with paid use enabled; it was
changed to Mac with paid use disabled. Its UI and Handy dictation were stopped
and automatic startup disabled. Mac recognition passed the public JFK sample
and the scheduled worker completed an actual queued note with exit status zero.
Deployment and recovery are documented in the sibling TranscriptRealTime
project's `infra/macmini/voice_notes/README.md`.

GNOME animations remain disabled and temporary scheduling weights remain in
place. Accessibility support was restored after disabling it failed to improve
CPU. Notifications are running again. Nineteen unused orphan Xvfb processes were
closed after checking display users and connected sockets. No automated browser
was launched locally.

Memory and I/O pressure were zero during the final samples; swap-in/out were
also zero during diagnosis. Numerous MCP clients and live AI sessions still
consume memory. Their running processes were preserved.

The NVIDIA kernel module is `580.173.02`; NVML reports library version `580.178`
and a driver/library mismatch. Its contribution to input lag remains unproven.
The host and GNOME session were not restarted. Real tmux geometry is still
queried synchronously every two seconds; this correction does not remove every
possible GTK-thread delay.

Recovery snapshots, CPU samples and the temporary-setting restore script are
stored privately in
`~/.local/state/comandos/load-relief-20260914/`. The restore script deliberately
keeps local transcription disabled. The first UI recovery changed the terminal
canvas size while retaining split topology; the final installation preserved
the current 1600 by 880 window size.
