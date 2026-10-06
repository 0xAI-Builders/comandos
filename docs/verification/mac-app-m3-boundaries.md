# M3 AppKit boundary and whole-source ledger

Original: /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app-mac
SHA-256: 79670ba87d6be456c73059dba36f9c358838ca7fb8ce8fb2eb33424f92ad53bb.
This table inventories all 84 function nodes: 83 top-level/class methods and the nested ensure_webterm.probe. M3 supplies its complete window/dashboard/centro consumers. M4 owns menu, close/rename dialogs, persistent tab operations, restore aliases and IPC. Entries marked M3 + M4 deliberately retain those dependencies.

The only unsafe module is /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m3/crates/comandos-app-mac/src/ffi/. Cargo denies unsafe in the crate and each policy/runner module forbids it. objc2 exactly 0.6.4; Foundation/AppKit/WebKit exactly 0.3.2; block2 exactly 0.6.1. Generated protocol signatures are checked for both Apple targets. Neither Linux tests nor cross-checks initialize AppKit.

Ownership: the run stack retains Native; native delegates hold Weak<RefCell<Native>>; buttons retain no owner; the owner retains window, delegates, UCC and views. UCC retains centro, whose owner reference is weak. The single thread-local slot resolves an integer owner id only on main. Background blocks capture Send data, never Rc or AppKit objects. WebKit's shared pool is retained once and terminal views are created on first selection and hidden when switching. SDK deprecations for WKProcessPool/Recessed are allowed locally to preserve the macOS 12 contract.

Each asynchronous delivery carries an instance generation. Closing/replacing the owner invalidates tickets; Jobs adds its own cancellation bit, cancels registered HTTP sockets, drops the bounded job sender and joins its single worker. Input/output queues hold at most eight items and the deferred UI queue at most 128. A pending main-queue notification is coalesced. HTTP and tmux run on the worker. Startup preferences and configuration deliver independently of terminal authentication, so missing credentials cannot suppress the dashboard language/theme; a newer user theme always wins over a late initial preference. State polling waits on its three-second deadline rather than a repeating short idle tick; only current tabs cache known static dot colors. State/theme/rename refreshes never request terminal focus.

Sandbox execution requires an existing owned 0700 root and an explicit loopback dashboard in 7200–7399. Parsing retains the original live default 4777; the shared DashClient refuses that personal port in sandbox before WebKit loads it. Tmux uses a captured explicit private -S socket and drops inherited TMUX only in sandbox. The single Desktop::proc producer preserves Root Shadow run_when and Err(Spawn("cancelled")); the accepted M1 Runtime::procs::child_exited_unreaped helper reserves the leader until group cleanup precedes wait. Desktop depends on Runtime without a cycle or GTK edge. Live argv/environment retain original behavior. D6 loads terminal HTTP pages and lets comandos dash own the terminal service; no standalone legacy cc-webterm daemon or file-access KVC preferences are started.

DOM dumps are opt-in absolute paths. A completion checks the dashboard origin and instance; the worker publishes a complete 0600 file via a private temporary file and no-overwrite hard link. Existing regular files, symlinks and FIFOs are refused without opening them. No output is written if cancellation precedes publication. Each invocation removes its own temporary file. Parent aliases such as Darwin /tmp → /private/tmp are accepted when the actual final directory remains owned and non-writable by other users; a symlink final directory is refused.

Safety references: [MainThreadMarker](https://docs.rs/objc2/0.6.4/objc2/struct.MainThreadMarker.html), [define_class](https://docs.rs/objc2/0.6.4/objc2/macro.define_class.html), [NSWindow releasedWhenClosed](https://developer.apple.com/documentation/appkit/nswindow/isreleasedwhenclosed), [NSOperationQueue main](https://developer.apple.com/documentation/foundation/operationqueue/main), [WKWebView evaluateJavaScript](https://developer.apple.com/documentation/webkit/wkwebview/evaluatejavascript(_:completionhandler:)), [script message handler](https://developer.apple.com/documentation/webkit/wkscriptmessagehandler), [WKUIDelegate popup](https://developer.apple.com/documentation/webkit/wkuidelegate).

Native GUI, keyboard/menu M4 parity, native link/runtime, RSS and one-hour acceptance remain pending. C5 Darwin code was propagated only to unblock the cross-check; its separate native runtime review remains pending.

| Original method (immutable SHA above) | Lines | Scope at M3 | Consumer |
|---|---:|---|---|
| `_ui_lang` | 116–125 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `http_get_json` | 134–139 | M3 consumer | jobs::SystemBackend / desktop::DashClient |
| `http_post` | 142–160 | M3 consumer | jobs::SystemBackend / desktop::DashClient |
| `tmuxc` | 163–174 | M3 consumer | tmux::runner / desktop::proc |
| `initial_theme` | 177–182 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `term_url` | 185–196 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `webterm_access_token` | 199–207 | M3 consumer | jobs::execute_task Boot / bounded read_token |
| `ssh_host_from_session` | 210–219 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `load_tab_metadata` | 225–246 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `find_project_dir` | 249–269 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `restore_tab_spec` | 272–300 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `load_saved_tabs` | 306–318 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `merge_tab_labels` | 321–328 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `cancel_restore_snapshot` | 331–340 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `terminal_action_session` | 343–351 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `saved_tab_label` | 354–358 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `archive_tab` | 361–393 | M2 shared | comandos_desktop (accepted M2; exact ports) |
| `ensure_webterm` | 399–422 | D6 superseded | HTTP terminal served by comandos dash; no file preferences/standalone ttyd daemon |
| `DashBridge.initWithController_` | 433–438 | M3 consumer | ffi::Delegate weak owner |
| `DashBridge.userContentController_didReceiveScriptMessage_` | 440–466 | M3 consumer | ffi::Delegate::receive_script -> App::receive_bridge |
| `AppController.applicationDidFinishLaunching_` | 475–497 | M3 + M4 | M3 window/boot/poll lifecycle; M4 menu, restore and IPC |
| `AppController.applicationShouldTerminateAfterLastWindowClosed_` | 499–500 | M3 consumer | ffi::Delegate::last_window_closed |
| `AppController._build_window` | 504–588 | M3 consumer | ffi::Views::new |
| `AppController._enable_local_access` | 591–604 | D6 superseded | HTTP terminal served by comandos dash; no file preferences/standalone ttyd daemon |
| `AppController._make_strip_button` | 608–615 | M3 consumer | ffi::window::button |
| `AppController._relayout_strip` | 618–630 | M3 consumer | ffi::Views::sync |
| `AppController._set_tab_title` | 633–650 | M3 consumer | ffi::window::set_title |
| `AppController._add_tab` | 653–696 | M3 + M4 | M3 button + lazy WK view; M4 metadata + persistence |
| `AppController._tab_context_menu` | 699–712 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._tab_for_button` | 715–719 | M3 consumer | instance tags -> Native::select |
| `AppController._tab_for_key` | 722–726 | M3 consumer | App::tabs keyed lookup |
| `AppController._select_tab` | 729–739 | M3 consumer | App::select_tab -> Views::sync lazy views |
| `AppController.tabClicked_` | 742–743 | M3 consumer | ffi::Delegate::tab_clicked |
| `AppController.renameTabFromMenu_` | 745–748 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.closeTabFromMenu_` | 750–753 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._build_menu` | 757–809 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._mi` | 812–816 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.reloadDashboard_` | 819–820 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.toggleTermPane_` | 822–826 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.zoomIn_` | 828–829 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.zoomOut_` | 831–832 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.zoomReset_` | 834–840 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._zoom_active` | 843–853 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.newLocalTab_` | 855–865 | M3 consumer | Native::action / Task::NewLocal |
| `AppController.closeCurrentTab_` | 867–870 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.nextTab_` | 872–873 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.prevTab_` | 875–876 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._cycle_tab` | 879–884 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._current_cwd` | 888–895 | M3 consumer | Task::NewLocal |
| `AppController._select_win` | 898–912 | M3 consumer | jobs::select_window |
| `AppController.openOrFocus_win_label_` | 914–935 | M3 + M4 | M3 ensure/select/open/raise; M4 saved labels + metadata + persistence |
| `AppController.renameTab_to_` | 937–943 | M3 consumer | App::receive_bridge Rename / Views::sync (save in M4) |
| `AppController._prompt_rename` | 946–963 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._close_tab` | 966–1002 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._save_tabs` | 1005–1018 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._boot_terminals` | 1022–1028 | M3 + M4 | M3 token and authenticated hub; M4 restore |
| `AppController._defer_terminal_action` | 1031–1036 | M3 consumer | App::queue (bounded 128) |
| `AppController._drain_pending_terminal_actions` | 1039–1046 | M3 consumer | App::finish_boot / Native::dispatch |
| `AppController._cancel_pending_terminal_actions` | 1049–1056 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._record_restore_alias` | 1059–1064 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._restore_current_session` | 1067–1070 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._cancel_pending_restore` | 1073–1083 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._restore_is_cancelled` | 1086–1088 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._create_hub_and_restore` | 1091–1110 | M3 + M4 | M3 authenticated hub; M4 snapshot restoration |
| `AppController._restore_saved` | 1113–1172 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._finish_restore` | 1175–1181 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._restore_one` | 1184–1193 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._raise` | 1196–1198 | M3 consumer | Views::show / Opened.raise |
| `AppController._state_poll_loop` | 1202–1209 | M3 consumer | Jobs worker deadline 3s /states delivery |
| `AppController._update_dots` | 1212–1216 | M3 consumer | App::update_states / window::set_title (bounded to tabs) |
| `AppController._start_ipc_timer` | 1220–1226 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._mtime` | 1229–1233 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.checkIPC_` | 1235–1246 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._on_focus` | 1249–1255 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._on_open` | 1258–1273 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController._on_close` | 1276–1289 | M4 pending | M4 checkpoint; not claimed in M3 |
| `AppController.applyTheme_` | 1292–1305 | M3 consumer | App::receive_bridge / Views::sync postMessage |
| `AppController.webView_createWebViewWithConfiguration_forNavigationAction_windowFeatures_` | 1308–1317 | M3 consumer | Delegate::external_action (owned views + schemes) |
| `AppController.webView_didFailProvisionalNavigation_withError_` | 1320–1323 | M3 consumer | Delegate::navigation_failed / shared RetrySchedule |
| `AppController.retryDash_` | 1325–1326 | M3 consumer | Delegate::retry_dashboard (owned coalesced timer) |
| `_nscolor` | 1329–1332 | M3 consumer | window::set_title RGB conversion |
| `_request` | 1335–1337 | M3 consumer | webview::load HTTP |
| `main` | 1340–1344 | M3 consumer | app::parse_args / main / ffi::run |
| `ensure_webterm.probe` | 403–408 | D6 superseded | comandos dash owns HTTP/terminal service |
