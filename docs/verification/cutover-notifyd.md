# Cutover de `comandos-notifyd` (Fase 2f-5)

Procedimiento para el controlador, con Jesús delante. Ningún paso lo ejecuta un subagente. El
binario Rust `comandos-notifyd` sustituye al Python `bin/cc-notifyd` en el puerto 4778 (avisos
`POST /notify` de los hooks, de `cc-dash` y de `cc-app`, y los popups GTK3). Reglas de oro:

- tmux, sus sesiones y los demás servicios no se tocan. El único reinicio es el de
  `cc-notifyd.service`.
- El cambio va en un **drop-in** de systemd. `~/.config/systemd/user/cc-notifyd.service` es un
  enlace simbólico a `systemd/cc-notifyd.service` del checkout principal (un árbol con cambios sin
  commitear): ni ese archivo ni el enlace se editan.
- Las pruebas con ventanas (paso 1) solo de día, en la sesión gráfica de Jesús y con él mirando.
  De noche no se abre ninguna ventana ni se manda ningún aviso de prueba.
- Detenerse y revertir (paso 5) ante cualquier fallo.

## Qué cambia y qué no

| Elemento | Antes | Después |
|---|---|---|
| Puerto 4778 | `python3 bin/cc-notifyd` | `comandos-notifyd`, mismo puerto y mismas respuestas |
| `systemd/cc-notifyd.service` (versionado) y su enlace | `ExecStart=%h/.local/bin/cc-notifyd` | sin tocar |
| `~/.config/systemd/user/cc-notifyd.service.d/10-rust.conf` | no existe | vacía `ExecStart` y pone el del binario Rust |
| `~/.local/bin/cc-notifyd` | enlace a `<repo>/bin/cc-notifyd` | sin tocar (es la reversión) |
| `~/.local/bin/comandos-notifyd` | no existe | enlace a una copia fija en `~/.local/share/comandos/notifyd/` |
| `~/.claude/hooks/cc-notify.conf`, `notifyd-pos.json` | los lee y escribe el Python | los mismos archivos, mismas claves |
| Tablero (4777) | `GET /prefs`, `GET /state`, `POST /focus`, `/prefs-set` | igual; las peticiones salen fuera del hilo de GTK |
| tmux | caída de «Abrir» si el tablero no responde | igual, `tmux` del `PATH` sin socket propio |

## Interfaz

`comandos-notifyd [--port N] [--hooks-dir RUTA] [--repo-root RUTA] [--dash-url URL] [--headless] [--tmux-socket RUTA]`
(`crates/comandos-notifyd/src/main.rs`).

- `--port`: por omisión 4778. Escucha solo en 127.0.0.1.
- `--hooks-dir`: por omisión `$HOME/.claude/hooks`. Sin `HOME` (o vacía) y sin la opción, sale con
  2: nunca usa una ruta relativa al directorio de trabajo.
- `--repo-root`: el checkout del que salen `config/themes.json` y `dash/icons`. Sin la opción:
  `COMANDOS_DASH_REPO`, luego `<hooks>/dash/index.html` resuelto dos niveles arriba y, al final, el
  ejecutable. Si nada resuelve, tema base y viñeta «•» con un aviso en el journal. El cutover la
  pasa siempre.
- `--dash-url`: `http://<host>[:<puerto>]`, por omisión `http://127.0.0.1:4777`.
- `--headless` y `--tmux-socket`: solo pruebas.
- Entorno que lee: `HOME`, `LANG` (idioma si `cc-notify.conf` no trae `CC_LANG`),
  `COMANDOS_DASH_REPO`, y `DISPLAY`/`WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR` a través de GTK. Lo mismo que
  el Python salvo `COMANDOS_DASH_REPO`.
- Arranque correcto: `comandos-notifyd listo en 127.0.0.1:4778 (popups propios v3)` en stdout.
- Salidas: 1 si no puede escuchar, si GTK no arranca (sin pantalla) o si el servidor HTTP termina
  (`Restart=on-failure` lo relanza a los 3 s); 2 con argumentos inválidos.

## Procedimiento

Variables comunes a todos los pasos (una terminal del controlador, fuera de tmux de prueba):

```sh
REPO=$HOME/codebase/0xJesus/ComandOS
SRC=$REPO/.worktrees/notifyd-release          # checkout limpio de lo que se instala
export CARGO_TARGET_DIR=$REPO/.build/target
```

### 0. Previos (de día o de noche: no abren ventanas ni mandan avisos)

Compilar desde un checkout limpio de `main` con la 2f-5 fusionada (el checkout principal tiene
cambios sin commitear) y copiar el binario fuera de `.build/target`, para que una recompilación no
cambie lo que arranca el siguiente reinicio:

```sh
git -C "$REPO" worktree add --detach "$SRC" main
git -C "$SRC" log -1 --oneline                                  # debe incluir la 2f-5
nice -n 10 cargo build --manifest-path "$SRC/Cargo.toml" --release --locked -p comandos-notifyd -j 4
SHA=$(git -C "$SRC" rev-parse --short=12 HEAD)
mkdir -p ~/.local/share/comandos/notifyd
cp "$CARGO_TARGET_DIR/release/comandos-notifyd" ~/.local/share/comandos/notifyd/comandos-notifyd-$SHA
ln -sfn ~/.local/share/comandos/notifyd/comandos-notifyd-$SHA ~/.local/bin/comandos-notifyd
ldd ~/.local/bin/comandos-notifyd | grep 'not found' && echo "FALTAN BIBLIOTECAS: no seguir"
```

Comprobaciones, anotando cada salida:

```sh
readlink -f ~/.local/bin/cc-notifyd                       # $REPO/bin/cc-notifyd (la reversión)
readlink -f ~/.claude/hooks/dash/index.html               # $REPO/dash/index.html → --repo-root=$REPO
ls "$REPO/config/themes.json" "$REPO/dash/icons/bell.svg" "$REPO/dash/icons/check.svg"
readlink -f ~/.config/systemd/user/cc-notifyd.service     # enlace al checkout: NO editarlo
ls ~/.config/systemd/user/cc-notifyd.service.d 2>/dev/null # hoy no existe; si existe, leerlo antes
systemctl --user show cc-notifyd -p ExecStart -p NRestarts -p ActiveState -p MainPID
systemctl --user show-environment | grep -E '^(HOME|LANG|DISPLAY|WAYLAND_DISPLAY|XDG_RUNTIME_DIR|DBUS_SESSION_BUS_ADDRESS|PATH|COMANDOS_DASH_REPO)='
ss -ltnp 'sport = :4778'                                  # hoy: python3 (cc-notifyd)
grep -E '^(POPUPS|CC_LANG|NATIVE_NOTIFY)=' ~/.claude/hooks/cc-notify.conf
```

- `show-environment` debe traer `DISPLAY` (o `WAYLAND_DISPLAY`) y `XDG_RUNTIME_DIR`: sin ellos GTK
  no arranca y el servicio entra en bucle de reinicios. `PATH` debe incluir `tmux` y `wmctrl` (la
  caída de «Abrir»). `COMANDOS_DASH_REPO` no debería estar; si está, `--repo-root` manda igual.
- `POPUPS=1` es lo que hace pintar popups (sin él solo se aceptan los avisos, como en el Python).

### 1. Pruebas de día (sesión gráfica de Jesús, con él delante)

1. Pruebas GTK reales (abren y cierran ventanas solas, más una ventanita vacía para el caso del
   grab). Incluyen la comprobación de fugas: tras cada escenario, `gtk::Window::list_toplevels()`
   no debe conservar ninguna ventana de popup, también tras 60 ciclos de abrir y cerrar y con un
   `gtk_grab_add` de otra ventana activo durante el cierre:

   ```sh
   COMANDOS_GTK_TESTS=1 nice -n 10 cargo test --manifest-path "$SRC/Cargo.toml" -p comandos-notifyd --test gtk -- --nocapture
   ```

2. Instancia de prueba en el puerto 7391 con hooks temporales. Lee `/prefs` y `/state` del tablero
   vivo (solo GET). **En esta instancia no pulsar «Abrir» ni arrastrar** salvo a propósito: hacen
   `POST /focus` y `POST /prefs-set {"notif_pos":"free"}` contra el tablero real.

   ```sh
   T=$(mktemp -d)
   printf 'POPUPS=1\n' > "$T/cc-notify.conf"     # + la línea CC_LANG= del vivo si la hay
   ~/.local/bin/comandos-notifyd --port 7391 --hooks-dir "$T" --repo-root "$REPO" --dash-url http://127.0.0.1:4777 &
   NPID=$!
   ```

3. Comparación visual, un aviso cada vez, primero en el Rust (7391) y luego en el Python (4778,
   solo si la conf viva tiene `POPUPS=1`), con captura de cada uno:
   - «te espera» con pane `%N` y markdown con tabla, código, negrita, cursiva y enlace;
   - «listo» (se cierra a los 10 s; desplegado con «Ver TODO» no se cierra);
   - `full` de más de 520 caracteres (aparece «Ver TODO») y de más de 16 000 (se pinta recortado:
     ver «Decisiones pendientes»);
   - markup roto (`**a `b** c`` cruzados): el Rust pinta el texto crudo, el Python deja la etiqueta
     en blanco (diferencia aceptada);
   - aviso sin sesión al estilo `desktop_popup` de `cc-dash`;
   - `CC_LANG=en` en `$T/cc-notify.conf` (requiere reiniciar la instancia: el idioma se lee al
     arrancar);
   - los cinco `notif_pos` (`tl`, `tr`, `bl`, `br` y libre con ancla) y un arrastre que escribe
     `$T/notifyd-pos.json`.

   Un aviso de ejemplo:

   ```sh
   curl -s -XPOST 127.0.0.1:7391/notify -d '{"title":"Claude Code","kind":"waiting","session":"prueba","pane":"%1","full":"**hola** `x`"}'; echo
   ```

4. Ráfaga de 20 a la vez: 20 respuestas `{"ok": true}`, 8 popups visibles, «Cerrar todas · 8», y
   el botón los cierra todos.

   ```sh
   seq 20 | xargs -P20 -I{} curl -s -XPOST 127.0.0.1:7391/notify -d '{"title":"t","kind":"done","session":"s{}"}'; echo
   ```

5. Resistencia: RSS, hilos y ventanas antes, tras 200 avisos a uno por segundo y tras cerrarlos
   todos. Criterio: los hilos vuelven a la cifra base y el RSS se estabiliza (anotar las cifras;
   el Python vivo ronda 41 MB, el Rust sin pantalla 10,7 MB).

   ```sh
   ps -o rss= -p $NPID; ls /proc/$NPID/task | wc -l
   for i in $(seq 200); do curl -s -XPOST 127.0.0.1:7391/notify -d "{\"title\":\"r$i\",\"kind\":\"done\",\"session\":\"r$((i % 30))\"}" >/dev/null; sleep 1; done
   ps -o rss= -p $NPID; ls /proc/$NPID/task | wc -l
   # «Cerrar todas», esperar 5 s
   ps -o rss= -p $NPID; ls /proc/$NPID/task | wc -l
   kill $NPID; rm -rf "$T"
   ```

### 2. Nada en vuelo

Justo antes del cutover:

- Ningún popup del Python visible esperando acción (un «te espera» abierto desaparece con el
  reinicio; el tablero conserva el aviso, pero el popup no vuelve).
- Ningún agente a mitad de un permiso: un aviso que llegue durante el reinicio (~1 s) no se pinta.
- `systemctl --user show cc-notifyd -p NRestarts` igual que en el paso 0 (el Python estable).

### 3. Cutover

```sh
mkdir -p ~/.config/systemd/user/cc-notifyd.service.d
cat > ~/.config/systemd/user/cc-notifyd.service.d/10-rust.conf <<'EOF'
[Service]
ExecStart=
ExecStart=%h/.local/bin/comandos-notifyd --port 4778 --hooks-dir %h/.claude/hooks --repo-root %h/codebase/0xJesus/ComandOS --dash-url http://127.0.0.1:4777
EOF
systemctl --user daemon-reload
systemctl --user cat cc-notifyd | tail -4          # el drop-in con las dos líneas ExecStart
systemctl --user restart cc-notifyd.service
```

La primera línea `ExecStart=` vacía borra la del archivo versionado; sin ella systemd rechaza la
unidad por tener dos `ExecStart` en un servicio `simple`.

### 4. Verificación

Inmediata (válida de día y de noche, sin avisos de prueba):

```sh
systemctl --user show cc-notifyd -p ActiveState -p NRestarts -p ExecStart -p MainPID
ss -ltnp 'sport = :4778'                            # comandos-notify(d), no python3
journalctl --user -u cc-notifyd -n 20 --no-pager    # «comandos-notifyd listo …», sin «aviso: no existe» ni «no se encontró el checkout»
```

Solo de día, con Jesús mirando (abre un popup en su pantalla):

```sh
curl -s -XPOST 127.0.0.1:4778/notify -d '{"title":"prueba","kind":"done","session":"x"}'; echo   # {"ok": true}
```

Con agentes reales: un «te espera» aparece y se cierra solo a los 3–7 s de responder en la
terminal; «Abrir» enfoca la pestaña correcta; un arrastre escribe
`~/.claude/hooks/notifyd-pos.json` y `/prefs` pasa a `notif_pos: free`.

A las 24 h: `NRestarts` sin cambios, y RSS e hilos del `MainPID`:

```sh
P=$(systemctl --user show -p MainPID --value cc-notifyd.service)
ps -o rss= -p $P; ls /proc/$P/task | wc -l
journalctl --user -u cc-notifyd --since '-24h' --no-pager | grep -E 'aviso|no se pudo|sigue viva'
```

`una ventana de popup sigue viva tras 6 cierres` en el journal es una fuga (I2): anotarla con la
hora y revisar qué ventana tenía un grab.

### 5. Reversión

```sh
rm ~/.config/systemd/user/cc-notifyd.service.d/10-rust.conf
rmdir ~/.config/systemd/user/cc-notifyd.service.d 2>/dev/null
systemctl --user daemon-reload && systemctl --user restart cc-notifyd.service
ss -ltnp 'sport = :4778'                            # python3 otra vez
```

El enlace `~/.local/bin/comandos-notifyd` y la copia en `~/.local/share/comandos/notifyd/` pueden
quedarse (nada los usa sin el drop-in). Para quitarlos: `rm ~/.local/bin/comandos-notifyd` y
`rm -r ~/.local/share/comandos/notifyd`. El worktree de compilación se retira con
`git -C "$REPO" worktree remove "$SRC"`.

## Diferencias aceptadas

Respecto al Python (`bin/cc-notifyd`):

- `POST /notify` con escapes sustitutos sueltos (`"\ud800"`) en el JSON: 400 (el Python los
  aceptaba). Ningún llamador real los manda.
- JSON con más de 1000 contenedores anidados: 400 por el límite del parser (el Python cortaba por
  `RecursionError` hacia ~980).
- `Content-Length` negativo: 413, con tope de 200 000 bytes.
- Sin cabeceras `Server` ni `Date` en las respuestas (ruling 1).
- Línea de arranque distinta (N1): `comandos-notifyd listo en 127.0.0.1:4778 (popups propios v3)`.
- `NATIVE_NOTIFY=1` sin libnotify: popup propio, como el Python cuando `HAVE_NOTIFY` es falso (N2).
- Markup que Pango rechaza: el popup pinta el texto crudo; el Python dejaba la etiqueta en blanco.
- `/prefs` fuera del hilo de GTK: un cambio de `notif_pos` hecho en el tablero se aplica un aviso
  (un `reposition`) más tarde que en el Python; una sola petición `/prefs` sirve para el tema y para
  `notif_pos`.
- Acciones fuera del hilo de GTK: el popup desaparece al pulsar «Abrir» sin esperar al tablero.
- POST al tablero en HTTP/1.0 (`urlopen` usaba HTTP/1.1). Como `urlopen`, el POST decide por la
  línea de estado y no espera a que el tablero cierre la conexión, y el GET lee `Content-Length`
  bytes.
- `valid_pane` solo acepta dígitos ASCII (`%123`); `\d` de Python aceptaba otros dígitos de Unicode,
  que tmux nunca usa.
- Excepción de dependencias: gtk3-rs 0.18 (ver abajo).

## Excepción de gtk3-rs (avisos RustSec)

`crates/comandos-notifyd/Cargo.toml` fija `gtk = "=0.18.2"`, la última serie de gtk3-rs. `Cargo.lock`
trae crates con aviso:

- RUSTSEC-2024-0411 a RUSTSEC-2024-0420: gtk3-rs sin mantenimiento (`gtk`, `gdk`, `atk`, `gtk-sys`,
  `gdk-sys`, `atk-sys`, `gtk3-macros`…). Archivado a favor de gtk4-rs; no es una vulnerabilidad.
- RUSTSEC-2024-0429 (`glib 0.18.5`, `VariantStrIter`): no alcanzable; el crate no usa
  `glib::Variant` y el workspace prohíbe `unsafe`.
- RUSTSEC-2024-0370 (`proc-macro-error 1.0.4`, sin mantenimiento): solo en compilación, vía
  `gtk3-macros`; no llega al binario.

Alternativa descartada: GTK4 no ofrece mover ventanas, `keep_above` ni el tipo `Notification`, de
los que dependen la pila de popups y el «encima de todo», y la máquina no tiene sus cabeceras.
**Decisión final pendiente de Jesús.**

Nota para la Fase 5 (Mac): con `comandos-notifyd` en el workspace, `cargo … --workspace` necesita
las cabeceras de GTK3 (`pkg-config gtk+-3.0`). Las releases usan `-p comandos-cli` y no se ven
afectadas.

## Decisiones pendientes de Jesús

- **«Ver TODO» recorta a 16 000 caracteres**, como el Python (`full[:16000]`), sin marca visible
  del recorte; el aviso guarda hasta 60 000 y «Copiar» lleva el texto entero guardado. La memoria
  de feedback pide «texto completo SIEMPRE». Se queda en 16 000 por paridad; las opciones son subir
  el tope o añadir una línea «… recortado: Copiar lleva N caracteres».
- gtk3-rs frente a GTK4 (sección anterior).
