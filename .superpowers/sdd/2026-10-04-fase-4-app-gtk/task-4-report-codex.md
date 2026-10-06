# GTK T4 — terminal engine adapter and pure paint plan

2026-10-05. Base prerequisite merge 53bd35a joins approved fixed Phase 3 b9eea80 with GTK a4c3797. Implements the approved T4 contract; no GUI is launched.

## Result

Added /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/engine.rs as the single adapter/re-export boundary to comandos-term. TermEngine forwards feed/tick/deadline, modes/events/cursor, resize, palette, scrolling, selection/input APIs and damage. Scrollback constants 10000/400 and VTE options (bold_is_bright=false, min_contrast=1) preserve existing desktop settings. Size reporting matches alacritty's2 × 1 minimum, and synchronized-update damage is retained until end or timeout.

Added /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/paint.rs: strict six-digit RGB/16-color theme palette, xterm indexed cube/grays, VTE 2/3 dim for indexed foregrounds while direct RGB stays unchanged, explicit backgrounds, text/wide/box glyph operations and underline/strike operations. Cell geometry carries DPR and font metrics for the later renderer. No cairo/GTK calls occur in these modules.

## Contract corrections proven by regressions

The plan's sample code needed adaptations to meet its stated desktop contracts:

- comandos-term always reports cursor-line damage even during a synchronized update. Adapter now suppresses consumption while sync is active, then exposes retained damage after end/timeout.
- Resolved xterm runs can merge equal RGB values from indexed and direct RGB sources. Paint splits dim runs by per-cell provenance, preventing direct RGB from receiving indexed dim. Inverse video uses the effective foreground source; scrollback lookup follows displayed coordinates.
- Drawing a box glyph must retain underline/strike rather than continuing before those operations.
- comandos-term suppresses backgrounds equal to the theme. Adapter adds opaque explicit-background runs even on blank cells, preserving opacity distinction from the default surface. No changes to comandos-term itself.
- Theme palette shape is exactly 16 entries; malformed hex including #+12345 is rejected. Adapter size follows the underlying engine minimum.

These are adapter corrections, not new terminal algorithms: keyboard/mouse/selection/URL/glyph engines remain re-exported from the existing crate.

## Evidence and confinement

RED: original T4 tests could not find the term module. Against the plan sample implementation, six concrete regressions failed (sync damage, clamped size, mixed provenance, inverse provenance, box decoration and palette shape); explicit-theme-equal background had a separate RED. GREEN: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/term_paint.rs: 17/17 passes, including wide characters painted once, background coordinates, scrollback/selection, indexed/direct/inverse dim, styles, glyphs and sync timeout. Native prerequisite suites passed 53/53 after the Phase 3 merge. Final comandos-app all-target clippy -D warnings, fmt and diff checks pass; production adapter has no unwrap/expect or unchecked indexing.

Commands use existing /home/someguy/codebase/0xJesus/ComandOS/.build/target-fase4 and -j2. Tests invoke no GTK initialization, display server, browser, PTY, personal agent, service or user tmux socket. gtk_smoke is not executed. Pixel calibration against VTE remains the remote T12 gate and is not claimed by these pure tests.

Next authorized task: T5 private PTY transport and size-settle logic, then T6 integration. Independent T4 review remains pending.

## Ronda de revisión: coste por frame

El reviewer detectó que `fg_is_rgb` buscaba desde el comienzo del viewport por cada celda y que `render_line` recorría el viewport por fila. Se sustituyeron por acceso directo a una fila, validando coordenadas de viewport y límites de almacenamiento antes del único `Index` requerido por Grid. Las columnas usan `get`; la cobertura de fondos se calcula en un pase por fila. La consulta de provenance cuesta O(1); el suplemento de fondos O(columnas), más la ordenación existente O(columnas log columnas), sin búsquedas cuadráticas por celda.

Evidencia acotada en el mismo perfil debug y target: cinco frames 120×40 de texto DIM con provenance RGB/index alternada, 24.000 operaciones de pintura, pasaron de 3,564701825 s antes del cambio a 30,922212 ms después (aproximadamente 115×). El test verifica número de operaciones y coordenadas inválidas; imprime tiempo sin un umbral dependiente de carga de máquina. Los tests existentes cubren scrollback, colores inversos y fondos explícitos.

Validación: term_paint 18/18; clippy comandos-app all-targets con -D warnings, fmt y diff-check pasan. No se lanzó GUI, Xvfb ni navegador. La fidelidad visual sigue pendiente del gate remoto T12.
