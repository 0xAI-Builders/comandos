# Remote terminal input

`⌫ Borrar`, next to Esc, sends a Backspace byte (`0x7f`) through the active terminal's existing WebSocket. It edits the terminal prompt while preserving the mobile draft. Disconnected presses are not queued for later replay.

The stray `65;6800;1c` was reproduced with the installed tmux 3.2a. Its input parser recognizes the first secondary device-attributes response, but rejects subsequent replies after `TTY_HAVEDA` is set. A one-second startup timer also sets this flag, so a late first reply can reach the application's input. See the [tmux 3.2a input parser](https://github.com/tmux/tmux/blob/3.2a/tty-keys.c) and [startup timer](https://github.com/tmux/tmux/blob/3.2a/tty.c).

`config/terminal-replies.conf` consumes complete replies from VTE 0.68 (`ESC [ > 65;6800;1 c`) and bundled xterm.js (`ESC [ > 0;276;0 c`) with tmux user keys 991 and 990. Empty slots are used; occupied user keys remain intact. Bindings send zero bytes and start no processes. The normal DA handshake still runs before key dispatch. Literal text containing the numeric suffix is preserved.

The installer links this file into `~/.claude/hooks` and adds its source line to tmux configuration. Apply it to an existing server with `tmux source-file ~/.claude/hooks/terminal-replies.conf`; tmux sessions need no restart. When upgrading VTE or the bundled xterm.js, verify their DA2 identity and update these compatibility entries if needed.

Tests use an isolated tmux socket and raw PTY to check late and repeated replies, real Backspace and arrow bytes, literal suffixes, and existing custom bindings. Browser tests run on the Mac mini and check tap delivery, draft preservation, tab switching and disconnected presses.
