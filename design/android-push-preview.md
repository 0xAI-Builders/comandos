# Brief Android notification with preview

Jesús chose a brief notice and requested a preview. The mockup proposes a collapsed notice with project, pane name and event title. Expanding reveals a short source excerpt; tapping the notice opens its source pane. The expansion treatment is presented for review, not recorded as an additional approved interaction.

Review: https://nodo-01.tail63a117.ts.net:8444/prototypes/prototype-v1-grill.html?round=notification-detail&variant=B&preview=android

Append `&expanded=1` to open the expanded example. The existing project-grouped notification layout remains the host. Its five earlier layout alternatives are preserved; this preview refines notice content without reopening that layout choice.

The review dialog offers completed-turn, permission-request and error examples. Expanding changes only the preview. Reading or opening a permission notice does not approve it. The existing fixture handles an unavailable source pane or offline state; it does not create a replacement session. Closing the dialog exposes the application and the Preview push control reopens it. The dialog is a laboratory control, not proposed production navigation.

This is an HTML simulation, not an Android notification or a push-delivery test. Android and the browser control the actual system layout and expansion. Production must verify how the short title, status and excerpt fit on the user's phone. The supporting platform findings are in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/mobile-push-research.md. Web Push architecture is documented at https://web.dev/articles/push-notifications-overview. The preview sends no push, requests no browser permission, plays no sound and connects to no real terminal.

## Verification

Chrome on the remote Mac mini checked 390×844, 320×568, 667×375 and 1440×1000. The preview fits the viewport and has no horizontal dialog or document overflow. On short screens its contents scroll. The first check exposed a hidden reopen control on mobile; the corrected control was visible and reopened the dialog in the repeated narrow, landscape and desktop checks.

Interaction checks confirmed expansion, collapse on event change, unchanged pane/event state while previewing, retained drafts, correct source-pane opening, unresolved permission retention, unavailable-pane handling and blocked navigation while offline. The checked console contained no warnings or errors. The approved news reader still renders with its terminal and without the push overlay. The three inline scripts pass Node syntax checking.

Screenshots were captured on the Mac mini and transferred for inspection:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/android-push/comandos-android-push-brief.png
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/android-push/comandos-android-push-expanded.png
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/android-push/comandos-android-push-desktop.png

The owned remote page and temporary port forward were closed. The persistent Tailscale prototype remains available. Physical-device push delivery remains unimplemented and untested.
