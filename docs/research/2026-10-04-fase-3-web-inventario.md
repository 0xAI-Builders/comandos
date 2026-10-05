# Fase 3b — inventario de la interfaz del tablero (B3)

Generado por `cargo run -p xtask -- web-inventory --out xtask/web/inventory.json --doc docs/research/2026-10-04-fase-3-web-inventario.md` sobre el checkout principal (`38c91676b67140ed0df87860810f204f8b7e7f5d`). No se edita a mano: se regenera. Los datos completos están en `xtask/web/inventory.json` (unidades) y `xtask/web/interop.json` (globales, host, iframes y mensajes).

El origen tenía cambios sin comitear en: `?? dash/prototypes/prototype-analytics.html`, `?? dash/prototypes/prototype-usage-cards.html`. El inventario refleja el disco, no el commit.

## Resumen

| Cifra | Valor |
|---|---|
| Unidades | 50 |
| Scripts `dash/*.js` | 18 |
| Regiones de scripts en línea | 32 |
| Líneas de JS | 12315 |
| Globales definidos (nombres distintos) | 379 |
| Globales con consumidores fuera de su unidad (`interop.json`) | 134 |
| Dependencias entre unidades (aristas) | 192 |
| Llamadas del host a la página | 13 |
| …de ellas sin JS literal (dinámicas) | 1 |
| Globales con riesgo anotado | 11 |

## Páginas y orden de carga

- `index.html`: `script:session-config.js` → `script:workspace-layout.js` → `script:workspace-dock.js` → `script:quick-terminal.js` → `script:command-sidebar.js` → `script:chain-builder.js` → `region:prelude` → `region:i18n` → `region:app-nativa` → `region:red` → `region:helpers` → `region:app-combinada` → `region:identidad-de-fila` → `region:barra-de-comandos` → `region:render` → `region:toasts` → `region:registro-local-de-uso` → `region:avisos-de-eventos` → `region:loop` → `region:conmutador-ctrl-k` → `region:iconos` → `region:tema` → `region:favoritos` → `region:prefs-de-terminal` → `region:tabs-internos-de-modales` → `region:solid-range-fill` → `region:preview-de-tipografia` → `region:remoto` → `region:servidores` → `region:notificaciones-del-sistema` → `region:modales-en-medio` → `region:ui-general` → `region:snippets` → `region:funciones-de-ui-globales` → `region:analytics` → `script:analytics-render.js` → `script:analytics.js` → `script:extensions.js` → `script:workspace.js` → `script:work-marks.js` → `script:ui-sounds.js` → `script:pomodoro.js` → `external:/vendor/markdown-it-15.0.2.umd.min.js` → `external:/vendor/purify-3.4.16.min.js` → `script:news-reader.js` → `script:push-settings.js` → `script:notifications.js` → `region:tail`
- `term.html`: `external:../assets/xterm/xterm.js` → `external:../assets/xterm/addon-fit.js` → `external:../assets/xterm/addon-web-links.js` → `external:../assets/xterm/addon-canvas.js` → `external:../assets/opentype/opentype.min.js` → `script:device-drafts.js` → `external:../assets/xterm/addon-ligatures-web.js` → `term:main` → `term:tail`
- Sin página que los cargue (su propio ámbito): `script:sw.js`

## Unidades

«Usa» y «Muta» solo cuentan globales de otras unidades de la misma página. «Host» son los globales de la unidad que llaman `cc-app`, `cc-app-mac` o un iframe.

| Unidad | Línea | Líneas | Define | Usa de otras | Muta | Rutas | `localStorage` | Intervalos (ms) | Host |
|---|---:|---:|---|---|---|---|---|---|---|
| `script:session-config.js` | 1 | 122 | `SessionConfig` | — | — | — | — | — | — |
| `script:workspace-layout.js` | 1 | 104 | `WorkspaceLayout` | — | — | — | — | — | — |
| `script:workspace-dock.js` | 1 | 412 | `WorkspaceDock` | `WorkMarks`, `WorkspaceLayout`, `activeTerm`, `activeView`, `authToken`, `openTerms` (+3) | — | `/workspace` | — | — | — |
| `script:quick-terminal.js` | 1 | 49 | `ComandosQuickTerminal` | — | — | `/terminal/quick` | — | — | — |
| `script:command-sidebar.js` | 1 | 756 | `ComandosCommandSidebar` | — | — | `/chains`, `/pane/type` | — | — | — |
| `script:chain-builder.js` | 1 | 287 | `ComandosChainBuilder` | `ComandosCommandSidebar` | — | `/chains` | — | — | — |
| `region:prelude` | 1783 | 15 | `S`, `$` | — | — | — | `cc-cfg` | — | — |
| `region:i18n` | 1798 | 84 | `L`, `T_EN`, `t`, `tf`, `applyI18n`, `saveCfg` | `$`, `S` | — | — | `cc-cfg` | — | — |
| `region:app-nativa` | 1882 | 61 | `inApp`, `CENTER_PANELS`, `CENTER_REQ`, `ONLY_PANEL`, `openInApp`, `openSession` | `$`, `WEBTERM`, `activateMtab`, `alertResolve`, `api`, `openAnalytics` (+6) | — | `/ensure`, `/up` | `cc-center-panel` | — | — |
| `region:red` | 1943 | 35 | `authToken`, `webtermTokenPromise`, `webtermAccessToken`, `api` | — | — | `/webterm-token` | `cc_token` | — | — |
| `region:helpers` | 1978 | 1078 | `LABEL`, `agoTxt`, `shortPath`, `mdEsc`, `attrEsc`, `mdInline` (+104) | `$`, `ComandosNotices`, `ICON`, `ONLY_PANEL`, `S`, `SessionConfig` (+15) | — | `/open-path`, `/model-tiers`, `/providers`, `/optimization/plans` (+20) | `cc-model-news`, `cc-notif-read`, `cc-nf-dismiss`, `cc-nf-snooze` (+1) | 120000, 1000, 450 | `nsOpen`, `notifRender`, `nsOpenForPane` |
| `region:app-combinada` | 3056 | 1014 | `openTerms`, `activeTerm`, `activeTermTs`, `remotePaneFocus`, `rememberRemotePaneFocus`, `sidebarActiveTab` (+67) | `$`, `ACTIVE_TAB`, `ComandosQuickTerminal`, `S`, `TERM_BASE`, `TERM_FALLBACK_BASE` (+17) | `termFallbackNotified` | `${}/token`, `/remote-state`, `/tmux-mouse`, `/workspace/sort` (+6) | `cc-split-left`, `comandos.deviceId` | — | `openTerms` |
| `region:identidad-de-fila` | 4070 | 3 | `rowKey` | — | — | — | — | — | — |
| `region:barra-de-comandos` | 4073 | 299 | `isQuickTermSession`, `SBT`, `selectSidebarTerm`, `sidebarTermTarget`, `sbTermFrames`, `sbNativeMsg` (+23) | `$`, `ComandosChainBuilder`, `ComandosCommandSidebar`, `ComandosQuickTerminal`, `ONLY_PANEL`, `S` (+20) | `quickTerminal`, `tabsPollTs` | `/kill`, `/account/switch`, `/analytics/week`, `/commands/catalog` (+1) | `cc-sb-limits3`, `cc-sb-limits`, `cc-sb-limits2` | 60000 | `sidebarTermAction`, `sidebarTermFocused`, `commandSidebar` |
| `region:render` | 4372 | 58 | `render`, `notify`, `favColor`, `favicon` | `$`, `S`, `alertClear`, `inApp`, `notifBadge`, `refreshDesktopTabs` (+5) | — | — | — | — | — |
| `region:toasts` | 4430 | 1 | — | — | — | — | — | — | — |
| `region:registro-local-de-uso` | 4431 | 52 | `ULOG`, `ulog`, `ulogFlush`, `ulogNameOf`, `ulogScreenStart`, `ulogScreenEnd` | `activePaneTarget`, `inApp` | — | `/ui-log` | — | 5000 | — |
| `region:avisos-de-eventos` | 4483 | 28 | `ALERTS`, `alertPush`, `alertLayout`, `alertClear`, `alertResolve`, `ATTENDED` (+1) | `$`, `ComandosNotices`, `notifBadge`, `tf` | — | — | — | — | — |
| `region:loop` | 4511 | 18 | `statePollInFlight`, `tick`, `timer`, `remotePollSeconds`, `arm` | `S`, `api`, `refreshFavorites`, `render`, `tickUsage` | — | `/state` | — | — | — |
| `region:conmutador-ctrl-k` | 4529 | 90 | `swItems`, `swSel`, `swScore`, `SW_ORD`, `loadRecover`, `swOpen` (+4) | `$`, `S`, `api`, `openSession`, `openSwitcherFromKey`, `openTerm` (+3) | — | `/tab-history`, `/recover-tab` | — | — | — |
| `region:iconos` | 4619 | 82 | `ICON`, `svg`, `hydrateIcons`, `hydrateButtonTooltips`, `tooltipObserver` | — | — | — | — | — | — |
| `region:tema` | 4701 | 89 | `THEME_SEQ`, `THEME_META`, `curTheme`, `broadcastTermTheme`, `applyTheme`, `BUTTON_STYLES` (+6) | `$`, `activateMtab`, `api`, `inApp`, `mdEsc`, `openTerms` (+4) | — | `/prefs-set` | — | — | — |
| `region:favoritos` | 4790 | 14 | `loadPrefs` | `api`, `applyButtonStyle`, `applyFavorites`, `applyTabsLayout`, `applyTheme`, `favoriteReadAt` (+2) | `favoriteReadAt` | `/prefs` | — | — | — |
| `region:prefs-de-terminal` | 4804 | 50 | `applyTabsLayout`, `hydrateTerminalPrefs`, `setPref` | `$`, `api`, `attrEsc`, `mdEsc`, `tabRowsHold`, `tf` (+3) | `tabRowsHold` | `/prefs-set` | — | — | — |
| `region:tabs-internos-de-modales` | 4854 | 24 | `activateMtab`, `wireMtabs` | — | — | — | — | — | — |
| `region:solid-range-fill` | 4878 | 26 | `updateRangeFill`, `refreshAllRangeFills`, `wireRangeFills` | — | — | — | — | 250, 250 | — |
| `region:preview-de-tipografia` | 4904 | 52 | `updateFontPreview` | `$`, `setPref`, `tf` | — | — | — | — | — |
| `region:remoto` | 4956 | 131 | `REMOTE`, `REMOTE_BUSY`, `remoteQrPath`, `remoteButtonState`, `setRemoteButton`, `applyRemoteButtonState` (+6) | `$`, `WEBTERM`, `activeTerm`, `addTermTab`, `api`, `arm` (+7) | — | `/remote-state`, `/state` | — | — | — |
| `region:servidores` | 5087 | 230 | `connectHost`, `openSshTab`, `setupSshKey`, `fillSrvForm`, `loadSsh`, `toggleSshManager` (+2) | `$`, `NS`, `WEBTERM`, `activeView`, `api`, `applyTabsLayout` (+9) | — | `/ssh-connect`, `/focus`, `/ssh-new-tab`, `/ssh-key-setup` (+5) | `cc-ssh-open`, `cc-panel-hidden` | — | — |
| `region:notificaciones-del-sistema` | 5317 | 59 | `DESKTOP_POPUPS`, `SYSTEM_POPUP_CATS`, `loadConf` | `$`, `ComandosNotices`, `api`, `svg`, `tf`, `toast` | — | `/conf`, `/conf-set` | — | — | — |
| `region:modales-en-medio` | 5376 | 39 | `openCenterPanel` | `$`, `CENTER_PANELS`, `ONLY_PANEL`, `activateMtab`, `inApp`, `openAnalytics` (+1) | — | — | `cc-center-panel` | — | — |
| `region:ui-general` | 5415 | 127 | `vol`, `bindSwitch`, `poll` | `$`, `L`, `LABEL`, `S`, `api`, `applyI18n` (+17) | `L`, `LABEL` | `/conf-set`, `/test`, `/conf` | — | 1000 | — |
| `region:snippets` | 5542 | 251 | `snipDlg`, `snipState`, `snipSessions`, `pickDefaultSession`, `filterSnippets`, `renderSnipList` (+4) | `api`, `toast` | — | `/paste`, `/snippets/delete`, `/snippets/update`, `/snippets` (+1) | `snippet-last-session` | — | — |
| `region:funciones-de-ui-globales` | 5793 | 23 | `opFavorite`, `setPollSeconds`, `setBrowserNotifications`, `nfDismiss`, `nfPin`, `nfUnpin` (+4) | `$`, `closeModelMenus`, `nsOpen`, `setSessionFavorite`, `swOpen` | — | — | — | — | — |
| `region:analytics` | 5816 | 46 | `ANALYTICS_DEMO`, `analyticsView`, `analyticsPhone`, `analytics`, `openAnalytics`, `openAnalyticsTab` (+2) | `$`, `Analytics`, `activeTerm`, `api`, `applyAppLayout`, `closeSnippets` (+5) | `setSplitLeft` | `/analytics/week` | `cc-analytics-tab`, `cc-split-left` | 60000 | — |
| `script:analytics-render.js` | 1 | 195 | `AnalyticsRender` | — | — | — | — | — | — |
| `script:analytics.js` | 1 | 135 | `Analytics` | `AnalyticsRender` | — | — | — | — | — |
| `script:extensions.js` | 1 | 284 | `openPaneExtensions` | — | — | — | `cc_token`, `cc_pane_shelf_height` | — | `openPaneExtensions` |
| `script:workspace.js` | 1 | 147 | `scOption`, `scSelect`, `scModalReturnFocus`, `scDialog`, `openExtensionUsage`, `scProfilesCache` (+4) | `LABEL`, `NS`, `PROVIDERS`, `S`, `SessionConfig`, `api` (+11) | `PROVIDERS` | `/extension-usage`, `/session-profiles`, `/providers`, `/session-profile-apply` | — | — | — |
| `script:work-marks.js` | 1 | 346 | `WorkMarks` | `S`, `authToken`, `setSessionFavorite`, `tf`, `toast` | — | — | — | 5000 | — |
| `script:ui-sounds.js` | 1 | 184 | `ComandosUISounds`, `uiSounds` | — | — | — | — | — | — |
| `script:pomodoro.js` | 1 | 662 | `ComandosPomodoro`, `ComandosPomodoroProgress`, `pomoRender` | `S`, `authToken`, `mdEsc`, `openAnalyticsTab`, `pickSel`, `svg` (+3) | — | — | `comandos.deviceId` | 1000, 110 | `pomoRender` |
| `script:news-reader.js` | 1 | 1204 | `NewsReader` | `TERM_BASE`, `curTheme`, `resolveTermBase`, `sidebarActiveTab`, `webtermAccessToken` | — | `/news/media/` | `cc_token` | — | — |
| `script:push-settings.js` | 1 | 198 | `PushSettings` | — | — | — | `cc_token` | — | — |
| `script:notifications.js` | 1 | 851 | `ComandosNotices` | `uiSounds` | — | — | — | — | — |
| `region:tail` | 5877 | 79 | `selectPaneInFrame` | `$`, `ComandosNotices`, `DESKTOP_POPUPS`, `NewsReader`, `ONLY_PANEL`, `S` (+9) | — | `/terminal-panes`, `/focus` | — | — | — |
| `script:device-drafts.js` | 1 | 93 | `ComandosDeviceDrafts` | — | — | — | — | — | — |
| `term:main` | 288 | 1712 | `__comandosTerm`, `__comandosOwnsTouchGestures` | `ComandosDeviceDrafts` | — | `/tmux-scroll`, `/workspace/client`, `/terminal-history`, `/terminal-panes` | `comandos.deviceId` | — | `__comandosOwnsTouchGestures` |
| `term:tail` | 2003 | 336 | — | `__comandosTerm` | — | `/terminal-panes`, `/accounts`, `/account/add`, `/model/status` (+2) | — | 100, 2000 | — |
| `script:sw.js` | 1 | 80 | `SHELL`, `eventIdFrom` | — | — | — | — | — | — |

## Dependencias de globales entre unidades

| Unidad | Depende de | Globales |
|---|---|---|
| `region:analytics` | `region:app-combinada` | `activeTerm`, `applyAppLayout`, `openTerms`, `restoreSplitLeft`, `setSplitLeft` |
| `region:analytics` | `region:prelude` | `$` |
| `region:analytics` | `region:red` | `api` |
| `region:analytics` | `region:snippets` | `closeSnippets`, `openSnippets`, `snipDlg` |
| `region:analytics` | `script:analytics.js` | `Analytics` |
| `region:app-combinada` | `region:app-nativa` | `inApp` |
| `region:app-combinada` | `region:avisos-de-eventos` | `toast` |
| `region:app-combinada` | `region:conmutador-ctrl-k` | `swOpen` |
| `region:app-combinada` | `region:helpers` | `ACTIVE_TAB`, `TERM_BASE`, `TERM_FALLBACK_BASE`, `TERM_PRIMARY_ATTEMPTS`, `TERM_PRIMARY_PROBE_TIMEOUT_MS`, `TERM_PRIMARY_RETRY_MS`, `WEBTERM`, `setSessionFavorite`, `termFallbackNotified`, `updateFavoriteButton` |
| `region:app-combinada` | `region:i18n` | `tf` |
| `region:app-combinada` | `region:prelude` | `$`, `S` |
| `region:app-combinada` | `region:red` | `api`, `authToken`, `webtermAccessToken` |
| `region:app-combinada` | `region:render` | `render` |
| `region:app-combinada` | `region:tema` | `curTheme` |
| `region:app-combinada` | `script:quick-terminal.js` | `ComandosQuickTerminal` |
| `region:app-combinada` | `script:workspace-dock.js` | `WorkspaceDock` |
| `region:app-nativa` | `region:analytics` | `openAnalytics` |
| `region:app-nativa` | `region:app-combinada` | `openTerm` |
| `region:app-nativa` | `region:avisos-de-eventos` | `alertResolve` |
| `region:app-nativa` | `region:helpers` | `WEBTERM` |
| `region:app-nativa` | `region:i18n` | `tf` |
| `region:app-nativa` | `region:modales-en-medio` | `openCenterPanel` |
| `region:app-nativa` | `region:prelude` | `$` |
| `region:app-nativa` | `region:red` | `api` |
| `region:app-nativa` | `region:registro-local-de-uso` | `ulog` |
| `region:app-nativa` | `region:tabs-internos-de-modales` | `activateMtab` |
| `region:app-nativa` | `region:tema` | `renderButtonStyleGallery`, `renderThemeGallery` |
| `region:avisos-de-eventos` | `region:helpers` | `notifBadge` |
| `region:avisos-de-eventos` | `region:i18n` | `tf` |
| `region:avisos-de-eventos` | `region:prelude` | `$` |
| `region:avisos-de-eventos` | `script:notifications.js` | `ComandosNotices` |
| `region:barra-de-comandos` | `region:analytics` | `openAnalytics` |
| `region:barra-de-comandos` | `region:app-combinada` | `openTerm`, `openTerms`, `quickTerminal`, `resolveTermBase`, `sidebarActiveTab`, `tabsPollTs` |
| `region:barra-de-comandos` | `region:app-nativa` | `ONLY_PANEL`, `inApp`, `openInApp` |
| `region:barra-de-comandos` | `region:avisos-de-eventos` | `toast` |
| `region:barra-de-comandos` | `region:helpers` | `TERM_BASE`, `WEBTERM`, `pickSel` |
| `region:barra-de-comandos` | `region:i18n` | `tf` |
| `region:barra-de-comandos` | `region:iconos` | `hydrateIcons` |
| `region:barra-de-comandos` | `region:loop` | `tick` |
| `region:barra-de-comandos` | `region:prelude` | `$`, `S` |
| `region:barra-de-comandos` | `region:red` | `api`, `webtermAccessToken` |
| `region:barra-de-comandos` | `region:servidores` | `loadSsh` |
| `region:barra-de-comandos` | `region:tema` | `curTheme` |
| `region:barra-de-comandos` | `script:chain-builder.js` | `ComandosChainBuilder` |
| `region:barra-de-comandos` | `script:command-sidebar.js` | `ComandosCommandSidebar` |
| `region:barra-de-comandos` | `script:quick-terminal.js` | `ComandosQuickTerminal` |
| `region:conmutador-ctrl-k` | `region:app-combinada` | `openSwitcherFromKey`, `openTerm` |
| `region:conmutador-ctrl-k` | `region:app-nativa` | `openSession` |
| `region:conmutador-ctrl-k` | `region:avisos-de-eventos` | `toast` |
| `region:conmutador-ctrl-k` | `region:i18n` | `t`, `tf` |
| `region:conmutador-ctrl-k` | `region:prelude` | `$`, `S` |
| `region:conmutador-ctrl-k` | `region:red` | `api` |
| `region:favoritos` | `region:helpers` | `applyFavorites`, `favoriteReadAt`, `favoriteVersion` |
| `region:favoritos` | `region:prefs-de-terminal` | `applyTabsLayout`, `hydrateTerminalPrefs` |
| `region:favoritos` | `region:red` | `api` |
| `region:favoritos` | `region:tema` | `applyButtonStyle`, `applyTheme` |
| `region:funciones-de-ui-globales` | `region:conmutador-ctrl-k` | `swOpen` |
| `region:funciones-de-ui-globales` | `region:helpers` | `closeModelMenus`, `nsOpen`, `setSessionFavorite` |
| `region:funciones-de-ui-globales` | `region:prelude` | `$` |
| `region:helpers` | `region:app-combinada` | `activeView`, `openTerm`, `showView`, `sidebarActiveTab` |
| `region:helpers` | `region:app-nativa` | `ONLY_PANEL`, `inApp`, `openInApp`, `openSession` |
| `region:helpers` | `region:avisos-de-eventos` | `toast` |
| `region:helpers` | `region:i18n` | `tf` |
| `region:helpers` | `region:iconos` | `ICON`, `svg` |
| `region:helpers` | `region:identidad-de-fila` | `rowKey` |
| `region:helpers` | `region:loop` | `tick` |
| `region:helpers` | `region:prelude` | `$`, `S` |
| `region:helpers` | `region:red` | `api` |
| `region:helpers` | `region:registro-local-de-uso` | `ulog` |
| `region:helpers` | `region:render` | `render` |
| `region:helpers` | `script:notifications.js` | `ComandosNotices` |
| `region:helpers` | `script:session-config.js` | `SessionConfig` |
| `region:i18n` | `region:prelude` | `$`, `S` |
| `region:loop` | `region:helpers` | `refreshFavorites`, `tickUsage` |
| `region:loop` | `region:prelude` | `S` |
| `region:loop` | `region:red` | `api` |
| `region:loop` | `region:render` | `render` |
| `region:modales-en-medio` | `region:analytics` | `openAnalytics` |
| `region:modales-en-medio` | `region:app-nativa` | `CENTER_PANELS`, `ONLY_PANEL`, `inApp` |
| `region:modales-en-medio` | `region:prelude` | `$` |
| `region:modales-en-medio` | `region:servidores` | `toggleSshManager` |
| `region:modales-en-medio` | `region:tabs-internos-de-modales` | `activateMtab` |
| `region:notificaciones-del-sistema` | `region:avisos-de-eventos` | `toast` |
| `region:notificaciones-del-sistema` | `region:i18n` | `tf` |
| `region:notificaciones-del-sistema` | `region:iconos` | `svg` |
| `region:notificaciones-del-sistema` | `region:prelude` | `$` |
| `region:notificaciones-del-sistema` | `region:red` | `api` |
| `region:notificaciones-del-sistema` | `script:notifications.js` | `ComandosNotices` |
| `region:prefs-de-terminal` | `region:app-combinada` | `tabRowsHold`, `updateTabNavigation` |
| `region:prefs-de-terminal` | `region:avisos-de-eventos` | `toast` |
| `region:prefs-de-terminal` | `region:helpers` | `attrEsc`, `mdEsc` |
| `region:prefs-de-terminal` | `region:i18n` | `tf` |
| `region:prefs-de-terminal` | `region:prelude` | `$` |
| `region:prefs-de-terminal` | `region:red` | `api` |
| `region:prefs-de-terminal` | `region:registro-local-de-uso` | `ulog` |
| `region:preview-de-tipografia` | `region:i18n` | `tf` |
| `region:preview-de-tipografia` | `region:prefs-de-terminal` | `setPref` |
| `region:preview-de-tipografia` | `region:prelude` | `$` |
| `region:registro-local-de-uso` | `region:app-nativa` | `inApp` |
| `region:registro-local-de-uso` | `region:barra-de-comandos` | `activePaneTarget` |
| `region:remoto` | `region:app-combinada` | `activeTerm`, `addTermTab`, `loadDesktopTabs`, `openTerm`, `openTerms` |
| `region:remoto` | `region:avisos-de-eventos` | `toast` |
| `region:remoto` | `region:helpers` | `WEBTERM`, `copyText` |
| `region:remoto` | `region:i18n` | `tf` |
| `region:remoto` | `region:loop` | `arm` |
| `region:remoto` | `region:prelude` | `$` |
| `region:remoto` | `region:red` | `api`, `authToken` |
| `region:render` | `region:app-combinada` | `refreshDesktopTabs`, `rememberRemotePaneFocus`, `renderTabbar` |
| `region:render` | `region:app-nativa` | `inApp` |
| `region:render` | `region:avisos-de-eventos` | `alertClear` |
| `region:render` | `region:barra-de-comandos` | `syncCommandSidebar` |
| `region:render` | `region:helpers` | `notifBadge` |
| `region:render` | `region:identidad-de-fila` | `rowKey` |
| `region:render` | `region:prelude` | `$`, `S` |
| `region:render` | `script:workspace.js` | `renderSessionOverview` |
| `region:servidores` | `region:app-combinada` | `activeView`, `openTerm`, `showView` |
| `region:servidores` | `region:app-nativa` | `inApp`, `openInApp` |
| `region:servidores` | `region:avisos-de-eventos` | `toast` |
| `region:servidores` | `region:barra-de-comandos` | `quickTerminalInstance` |
| `region:servidores` | `region:helpers` | `NS`, `WEBTERM`, `nsOpen` |
| `region:servidores` | `region:i18n` | `t`, `tf` |
| `region:servidores` | `region:prefs-de-terminal` | `applyTabsLayout` |
| `region:servidores` | `region:prelude` | `$` |
| `region:servidores` | `region:red` | `api` |
| `region:snippets` | `region:avisos-de-eventos` | `toast` |
| `region:snippets` | `region:red` | `api` |
| `region:tail` | `region:app-combinada` | `WS_DEVICE`, `appEnabled`, `openTerm`, `openTerms` |
| `region:tail` | `region:app-nativa` | `ONLY_PANEL`, `inApp`, `openInApp` |
| `region:tail` | `region:avisos-de-eventos` | `toast` |
| `region:tail` | `region:notificaciones-del-sistema` | `DESKTOP_POPUPS`, `SYSTEM_POPUP_CATS` |
| `region:tail` | `region:prelude` | `$`, `S` |
| `region:tail` | `region:red` | `api` |
| `region:tail` | `script:news-reader.js` | `NewsReader` |
| `region:tail` | `script:notifications.js` | `ComandosNotices` |
| `region:tema` | `region:app-combinada` | `openTerms`, `styleTermFrame` |
| `region:tema` | `region:app-nativa` | `inApp` |
| `region:tema` | `region:avisos-de-eventos` | `toast` |
| `region:tema` | `region:helpers` | `mdEsc` |
| `region:tema` | `region:i18n` | `tf` |
| `region:tema` | `region:iconos` | `svg` |
| `region:tema` | `region:prelude` | `$` |
| `region:tema` | `region:red` | `api` |
| `region:tema` | `region:tabs-internos-de-modales` | `activateMtab` |
| `region:ui-general` | `region:analytics` | `openAnalytics` |
| `region:ui-general` | `region:app-combinada` | `handleTermFrameMessage`, `handleTermInteractionsPageShow`, `initApp`, `restoreAllTermInteractions` |
| `region:ui-general` | `region:app-nativa` | `inApp` |
| `region:ui-general` | `region:avisos-de-eventos` | `toast` |
| `region:ui-general` | `region:barra-de-comandos` | `mountChainPage`, `mountCommandSidebar` |
| `region:ui-general` | `region:favoritos` | `loadPrefs` |
| `region:ui-general` | `region:helpers` | `LABEL` |
| `region:ui-general` | `region:i18n` | `L`, `applyI18n`, `saveCfg`, `tf` |
| `region:ui-general` | `region:loop` | `arm`, `tick` |
| `region:ui-general` | `region:notificaciones-del-sistema` | `loadConf` |
| `region:ui-general` | `region:prelude` | `$`, `S` |
| `region:ui-general` | `region:red` | `api` |
| `region:ui-general` | `region:remoto` | `loadRemote` |
| `region:ui-general` | `region:servidores` | `loadSsh` |
| `script:analytics.js` | `script:analytics-render.js` | `AnalyticsRender` |
| `script:chain-builder.js` | `script:command-sidebar.js` | `ComandosCommandSidebar` |
| `script:news-reader.js` | `region:app-combinada` | `resolveTermBase`, `sidebarActiveTab` |
| `script:news-reader.js` | `region:helpers` | `TERM_BASE` |
| `script:news-reader.js` | `region:red` | `webtermAccessToken` |
| `script:news-reader.js` | `region:tema` | `curTheme` |
| `script:notifications.js` | `script:ui-sounds.js` | `uiSounds` |
| `script:pomodoro.js` | `region:analytics` | `openAnalyticsTab` |
| `script:pomodoro.js` | `region:avisos-de-eventos` | `toast` |
| `script:pomodoro.js` | `region:helpers` | `mdEsc`, `pickSel` |
| `script:pomodoro.js` | `region:i18n` | `tf` |
| `script:pomodoro.js` | `region:iconos` | `svg` |
| `script:pomodoro.js` | `region:prelude` | `S` |
| `script:pomodoro.js` | `region:red` | `authToken` |
| `script:pomodoro.js` | `script:ui-sounds.js` | `uiSounds` |
| `script:work-marks.js` | `region:avisos-de-eventos` | `toast` |
| `script:work-marks.js` | `region:helpers` | `setSessionFavorite` |
| `script:work-marks.js` | `region:i18n` | `tf` |
| `script:work-marks.js` | `region:prelude` | `S` |
| `script:work-marks.js` | `region:red` | `authToken` |
| `script:workspace-dock.js` | `region:app-combinada` | `activeTerm`, `activeView`, `openTerms`, `showView` |
| `script:workspace-dock.js` | `region:avisos-de-eventos` | `toast` |
| `script:workspace-dock.js` | `region:i18n` | `tf` |
| `script:workspace-dock.js` | `region:red` | `authToken` |
| `script:workspace-dock.js` | `script:work-marks.js` | `WorkMarks` |
| `script:workspace-dock.js` | `script:workspace-layout.js` | `WorkspaceLayout` |
| `script:workspace.js` | `region:app-nativa` | `openSession` |
| `script:workspace.js` | `region:avisos-de-eventos` | `toast` |
| `script:workspace.js` | `region:helpers` | `LABEL`, `NS`, `PROVIDERS`, `attrEsc`, `fmtTokens`, `mdEsc`, `nsOpen`, `nsRender`, `nsSelectDefaults`, `pickSel` (+1) |
| `script:workspace.js` | `region:identidad-de-fila` | `rowKey` |
| `script:workspace.js` | `region:prelude` | `S` |
| `script:workspace.js` | `region:red` | `api` |
| `script:workspace.js` | `script:session-config.js` | `SessionConfig` |
| `term:main` | `script:device-drafts.js` | `ComandosDeviceDrafts` |
| `term:tail` | `term:main` | `__comandosTerm` |

## Puente con el host

`bin/cc-app` ejecuta JS en la página con `run_javascript` (literales, `_dash_js(code)`, `_dash_js_quiet(code)` y `_dash_click(elem_id)`, que envuelven `f"try{{{code}}}…"`); `bin/cc-app-mac` con `evaluateJavaScript`. La página habla con la app por `window.webkit.messageHandlers.<nombre>.postMessage`.

- `bin/cc-app`: 12 llamadas
- `bin/cc-app-mac`: 1 llamadas

| Global | Definido en | Llamado por | Llamadas (archivo:línea vía) |
|---|---|---|---|
| `appLeftPanel` | — | `bin/cc-app` | `bin/cc-app:7901 _dash_js_quiet` |
| `commandSidebar` | `region:barra-de-comandos` | `bin/cc-app` | `bin/cc-app:7208 _dash_js` |
| `notifRender` | `region:helpers` | `bin/cc-app` | `bin/cc-app:6471 run_javascript` |
| `nsOpen` | `region:helpers` | `bin/cc-app` | `bin/cc-app:6409 run_javascript` |
| `nsOpenForPane` | `region:helpers` | `bin/cc-app` | `bin/cc-app:5004 _dash_js` |
| `pomoRender` | `script:pomodoro.js` | `bin/cc-app` | `bin/cc-app:6471 run_javascript` |
| `sidebarTermAction` | `region:barra-de-comandos` | `bin/cc-app` | `bin/cc-app:7415 _dash_js_quiet` |
| `sidebarTermFocused` | `region:barra-de-comandos` | `bin/cc-app` | `bin/cc-app:7777 _dash_js_quiet`, `bin/cc-app:7780 _dash_js_quiet` |

Ids del DOM que toca el host:

| Id | Llamado por | En el marcado |
|---|---|---|
| `btn-notif` | `bin/cc-app` | sí |
| `btn-settings` | `bin/cc-app` | sí |
| `notif-badge` | `bin/cc-app` | sí |
| `notif-panel` | `bin/cc-app` | sí |

Llamadas con JS no literal (archivo, red o variable sin cadena): no se pueden inventariar.

- `bin/cc-app:6983` vía `_dash_js`

Manejadores `messageHandlers`:

| Manejador | Registrado por | Lo usan |
|---|---|---|
| `centro` | `bin/cc-app`, `bin/cc-app-mac` | `region:app-nativa`, `region:barra-de-comandos`, `region:tema`, `region:servidores`, `region:modales-en-medio`, `region:ui-general`, `script:extensions.js`, `region:tail` |
| `extensions` | `bin/cc-app` | `script:extensions.js` |

## Parches entre unidades (`window.X =` sobre un global ajeno)

| Global | Lo define | Lo parchea |
|---|---|---|
| `quickTerminal` | `region:app-combinada` | `region:barra-de-comandos` |
| `setSplitLeft` | `region:app-combinada` | `region:analytics` |

## Contrato padre → iframe

Lo que el tablero lee o escribe en el `window` de un iframe (`frame.contentWindow.X`, `const win = frame.contentWindow; win.X`, `contentDocument` como `document`, `frames[…]`). El port del iframe (A9/A10) debe conservar estos nombres.

| Propiedad | Lee | Escribe | Definida en el iframe por |
|---|---|---|---|
| `WheelEvent` | `region:app-combinada` | — | — |
| `__comandosOwnsTouchGestures` | `region:app-combinada` | — | `term:main` |
| `__comandosScrollWired` | `region:app-combinada` | `region:app-combinada` | — |
| `__comandosSwitchWired` | `region:app-combinada` | `region:app-combinada` | — |
| `addEventListener` | `region:app-combinada` | — | — |
| `document` | `region:app-combinada`, `region:tail` | — | — |
| `location` | `region:tail` | — | — |

## Iframes y mensajes

Llamadas directas `parent.X` desde un iframe (solo ven propiedades de `window`, no `let`/`const` de nivel superior):

- `term:tail` → `openPaneExtensions`, `openTerms`

| Mensaje (`source/type`) | Lo envía | Lo atiende (`.type ===`) |
|---|---|---|
| `*/comandos-extensions-close` | `script:extensions.js` | `script:extensions.js` |
| `*/comandos:open-event` | `script:sw.js` | `script:push-settings.js` |
| `?` | `script:news-reader.js` | — |
| `comandos-term/interaction-request` | `term:main` | `region:app-combinada` |
| `comandos-term/pane-selected` | `term:main` | `region:app-combinada` |
| `comandos-term/ready` | `term:main` | `region:app-combinada` |
| `comandos-term/user-interaction` | `term:main` | `region:tail` |
| `comandos/button-style` | `region:tema` | `term:main` |
| `comandos/interaction-state` | `region:app-combinada` | `term:main` |
| `comandos/select-pane` | `region:tail` | `term:main` |
| `comandos/theme` | `bin/cc-app-mac`, `region:tema` | `term:main` |
| `comandos/toolbar` | `region:analytics` | `term:main` |

## Globales de riesgo

- `L` (`region:i18n`): mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza
- `LABEL` (`region:helpers`): mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza
- `PROVIDERS` (`region:helpers`): mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza
- `appLeftPanel` (—): nadie lo define: la llamada falla o depende de algo fuera del inventario
- `favoriteReadAt` (`region:helpers`): mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza
- `openTerms` (`region:app-combinada`): leído como propiedad (window.openTerms/parent.openTerms) pero declarado con let/const/class: por esa vía vale undefined, sin excepción; el lector sigue con su valor por defecto
- `quickTerminal` (`region:app-combinada`): parcheado con window.quickTerminal = desde region:barra-de-comandos: portar el definidor sin seguir exportando quickTerminal por window rompe el parche, y portar el parche exige que el definidor ya lo haya creado; mutado desde otra unidad: debe seguir siendo propiedad de window mientras ambos lados vivan
- `setSplitLeft` (`region:app-combinada`): parcheado con window.setSplitLeft = desde region:analytics: portar el definidor sin seguir exportando setSplitLeft por window rompe el parche, y portar el parche exige que el definidor ya lo haya creado; mutado desde otra unidad: debe seguir siendo propiedad de window mientras ambos lados vivan
- `tabRowsHold` (`region:app-combinada`): mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza
- `tabsPollTs` (`region:app-combinada`): mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza
- `termFallbackNotified` (`region:helpers`): mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza

## Orden de port (hojas primero)

Nivel 0 = no toma globales de ninguna otra unidad. Un grupo con varias unidades es un ciclo: se portan juntas o una exporta por `window` lo que la otra aún lee de JS.

1. Nivel 0: `script:session-config.js`, `script:workspace-layout.js`, `script:quick-terminal.js`, `script:command-sidebar.js`, `region:prelude`, `region:red`, `region:identidad-de-fila`, `region:toasts`, `region:iconos`, `region:tabs-internos-de-modales`, `region:solid-range-fill`, `script:analytics-render.js`, `script:extensions.js`, `script:ui-sounds.js`, `script:push-settings.js`, `script:device-drafts.js`, `script:sw.js`
2. Nivel 1: `script:chain-builder.js`, `region:i18n`, `script:analytics.js`, `script:notifications.js`, `term:main`
3. Nivel 2: [ciclo: `script:workspace-dock.js`, `region:app-nativa`, `region:helpers`, `region:app-combinada`, `region:barra-de-comandos`, `region:render`, `region:registro-local-de-uso`, `region:avisos-de-eventos`, `region:loop`, `region:conmutador-ctrl-k`, `region:tema`, `region:prefs-de-terminal`, `region:servidores`, `region:modales-en-medio`, `region:snippets`, `region:analytics`, `script:workspace.js`, `script:work-marks.js`], `term:tail`
4. Nivel 3: `region:favoritos`, `region:preview-de-tipografia`, `region:remoto`, `region:notificaciones-del-sistema`, `region:funciones-de-ui-globales`, `script:pomodoro.js`, `script:news-reader.js`
5. Nivel 4: `region:ui-general`, `region:tail`

## Límites del inventario léxico

- No ejecuta JS: un tokenizador separa cadenas, plantillas, comentarios y expresiones regulares del código; `/` es regex o división según el token anterior, y un `}` seguido de `/regex/` en otra línea se leería como división.
- Ámbitos aproximados: bloques `{}`; parámetros de funciones, métodos y `catch` se declaran en su cuerpo; los de una flecha sin llaves, solo en la expresión del cuerpo; la cabecera de un `for` y su cuerpo forman un ámbito. `var` se trata como `let` (no se eleva a la función). Un nombre declarado en un ámbito tapa el global en ese ámbito y sus hijos.
- Globales: `function`, `class`, `const`/`let`/`var` fuera de todo paréntesis, corchete o llave, y `window.X =`, `globalThis.X =`, `self.X =` en cualquier sitio; `root.X =` solo si `root` es un parámetro (envoltura UMD), no un `root` local. Un `window.X =` sobre un global que otra unidad de la misma página ya define (con declaración propia o antes en el orden de carga) es un parche, no una definición. No ve `Object.assign(window, …)`, `window["X"] =` ni `defineProperty` (hoy no hay ninguno en `dash/`).
- Usos: identificadores libres (no tras `.`, no claves de objeto, no nombres de método) y `window.X`/`root.X`; se cruzan solo con unidades de la misma página. No ve llamadas `window["X"]()` ni manejadores en línea (`onclick="X()"` en el marcado o en plantillas): hoy no hay ninguno.
- Páginas: solo `index.html` y `term.html`. `extensions.html` (que `cc-app` carga sola, `bin/cc-app:5583`, y que va también en un iframe) y `prototype-*.html` no se modelan; `extensions.js` figura solo en el ámbito de `index.html`.
- Rutas: primer argumento literal (o `const` de la unidad) de `api(`, `fetch(`, `sendBeacon(` y `new EventSource(`, sin la consulta; los huecos de plantilla quedan como `${}`. Las rutas construidas en variables o pasadas por parámetro no se ven.
- `localStorage`: `getItem/setItem/removeItem` con literal o `const`, `localStorage.clave` y `localStorage["clave"]`; las claves de `sessionStorage` llevan `session:`. Una clave guardada en una propiedad (`storage.getItem(this.key)`) no se resuelve.
- Intervalos: segundo argumento de `setInterval` si es número, `const` numérica o producto/suma de ellos.
- Mensajes: `postMessage` con objeto literal (`source/type`) o `JSON.stringify({…})`. Los tipos atendidos salen de `x.type`/`x?.type` comparados con `===`, `==`, `!==` o `!=` (en cualquier orden) y de los `case` de un `switch (x.type)`; también recogen tipos de eventos del DOM, y un manejador que compara el tipo guardado en otra variable no se ve.
- Padre → iframe: `….contentWindow.X`, `win.X` cuando `win` se asignó desde `….contentWindow`, `contentDocument` y `frames[…]`. No sigue el `window` del iframe si pasa por una función o un objeto.
- Host: tokenizador mínimo de Python. Los envoltorios se descubren cuando un `def` pasa su parámetro a `run_javascript`/`evaluateJavaScript` u otro envoltorio; un argumento variable se resuelve por su última asignación en el mismo `def`; código leído de archivo o de red queda como dinámico. Los huecos `{expr}` de las f-strings se sustituyen por `__py__`. No lee las 19 llamadas `ui_call(…)` de `lib/operator_catalog.py`: eran del chat de CommandOS, retirado (`bin/cc-dash` responde 410 en `/operator*`).
- Regiones: marcadores `// ---------- … ----------` en columna 0 del primer script en línea; los marcadores sangrados son subsecciones y no cortan. El hash de una región es el del texto desde `marker_start` hasta `marker_end` (excluido), igual que el corte del compositor (B2). Un marcador repetido dentro del script se avisa en la tabla.
- El DOM que construye el JS (plantillas, `innerHTML`) no se inventaría: para eso está `shots dom-dump` (B4).
