# ComandOS — Design system

Guía canónica de la estética visual y de interacción de ComandOS. Referencia
para cualquier UI nueva (dashboard, cc-app GTK, popups, terminales).

Norte visual: se ve como una **app profesional de terminales** (referencia
mental: Termius). No se ve como una webapp. Nada de emojis, chrome no
seleccionable, tipografía monoespaciada consistente, íconos monolínea.

---

## 1. Paleta

Cinco temas simultáneos — el usuario alterna con el botón luna/sol/llama,
brotes y rayo. Todos
los colores viven en variables CSS (`--bg`, `--panel`, `--brand`, …) y en el
diccionario `THEMES` de `bin/cc-app`. Nunca hardcodear un color en un
componente nuevo: usar la variable.

| Rol             | noche       | dia         | calido      | termius    | bruno      |
| --------------- | ----------- | ----------- | ----------- | ---------- | ---------- |
| Fondo app       | `#0A0D13`   | `#F2F4F8`   | `#161009`   | `#0E1620`  | `#1A1A1A`  |
| Panel           | `#121722`   | `#FBFCFE`   | `#1F1811`   | `#141E2A`  | `#222224`  |
| Panel secundario| `#161C29`   | `#EDF0F6`   | `#261D14`   | `#182432`  | `#26292B`  |
| Text            | `#EAF0FB`   | `#1B2130`   | `#F2E5D0`   | `#D6E0EA`  | `#CCCCCC`  |
| Dim (label)    | `#9AA6BF`   | `#4E5A70`   | `#BCA98C`   | `#8FA0B4`  | `#AAAAAA`  |
| Faint (hint)   | `#5E6980`   | `#8892A6`   | `#8A7A5F`   | `#5C6B80`  | `#999999`  |
| Brand (acento) | `#8B7CFF`   | `#5B4BD6`   | `#E0A458`   | `#4CE07A`  | `#E4AE49`  |
| Línea (border) | `#222A3A`   | `#D9DEE8`   | `#36291A`   | `#1C2733`  | `#333333`  |
| Línea 2         | `#2E3852`   | `#C3CAD8`   | `#4A3823`   | `#25384F`  | `#444444`  |
| Warning         | `#FFAE1A`   | `#B26A00`   | `#FFB454`   | `#FFAE1A`  | `#F6AB79`  |

Bruno es la fuente de verdad para sus roles: fondo `#1A1A1A`, panel
`#222224`, panel secundario `#26292B`, texto `#CCCCCC`, cursor/acento
`#E4AE49`, línea `#333333`, línea 2 `#444444`, dim `#AAAAAA` y faint
`#999999`. Su ANSI, en orden negro a blanco y luego brillantes, es:
`#1A1A1A #DA462F #73E89A #FAD075 #8BC2F9 #D691ED #7DDFF2 #CCCCCC
#666666 #F38172 #73E89A #FAD075 #8BC2F9 #D691ED #7DDFF2 #FFFFFF`.

Uso disciplinado: **brand solo para acentos** (chip activa, foco, marca).
Etiquetas y texto secundario → `dim`. Placeholders y hints → `faint`.

---

## 2. Tipografía

- **Interfaz + monospace**: `JetBrainsMono Nerd Font Mono` (bundleado en
  `assets/fonts/JetBrainsMono/`). El chrome es monoespaciado porque la app es
  de terminales — mantiene coherencia con el contenido.
- Cascada de fallback (por si un usuario borra la carpeta):
  `JetBrainsMono Nerd Font, JetBrains Mono, Cascadia Code, Fira Code,
   DejaVu Sans Mono, monospace`.
- **Tamaños** (px):

  | Rol               | tamaño |
  | ----------------- | ------ |
  | Body / terminal   | 13     |
  | Header buttons    | 12.5   |
  | Chip label        | 11.5   |
  | Section label     | 10.5   |
  | Terminal (VTE)    | 13     |
  | Terminal (xterm)  | 14     |

- Letter-spacing: `.16em` solo en `.brand` (COMANDOS) y section labels — el
  wide-tracking le da el look "terminal art".

---

## 3. Iconos

### Sistema

- **Familia**: Lucide/Feather style — line-only, stroke `currentColor`.
- **Stroke width**: `2.0` (default — se ve tan sólido como los symbolic Adwaita
  del cc-app GTK) / `1.9` para glyphs geométricos con líneas cerradas (`+`, `×`).
  Con menos peso se ven "webby"; con más, cartoon.
- **viewBox**: `0 0 24 24` — permite escalar sin blurriness.
- **Sin fill**: `fill="none"`. Solo líneas.
- **Rounded**: `stroke-linecap="round"`, `stroke-linejoin="round"`.

### Tamaños canónicos

| Contexto                 | tamaño (px) |
| ------------------------ | ----------- |
| Botón del header         | **17**      |
| Chip inline (bell, etc.) | 12          |
| Menú / list item         | 14          |
| Tab close button         | 14          |

Regla: **un solo tamaño por barra de botones**. Si dos íconos son del mismo
componente (ej: header), tienen que tener el mismo tamaño visual y la misma
área clickeable (`28×28` para botones-icon puros).

### Fuentes

- **HTML / dashboard**: SVG inline via helper `svg(name, size)` — el registry
  vive en `dash/index.html` en el objeto `ICON = {…}`. Para HTML estático
  (sin JS), usar `<span data-icon="name" data-size="16"></span>` — `hydrateIcons()`
  lo reemplaza al `DOMContentLoaded`.
- **cc-app GTK**: **symbolic icons de Adwaita** (`edit-copy-symbolic`,
  `edit-find-symbolic`, `list-add-symbolic`, `window-close-symbolic`,
  `view-grid-symbolic`, `help-about-symbolic`). Se colorean automáticamente
  con el theme del sistema. Cargar con
  `Gtk.Image.new_from_icon_name(name, Gtk.IconSize.SMALL_TOOLBAR)`. **No**
  volver a poner símbolos Unicode (`⧉ ⌕ ▦ + ?`) — se ven inconsistentes.

### Prohibido

- ❌ Emojis (🔔 🔊 🌙 ☀ 🕯 🔇). Reemplazar por SVG.
- ❌ Íconos rellenos multicolor / gradientes.
- ❌ Mixear varias familias de iconos en la misma pantalla.

---

## 4. Selección de texto

**El chrome de la UI no es seleccionable.** Regla en el body:

```css
body { user-select: none }
input, textarea, .code, pre, code, .selectable,
.row-title, .rpath, .preview, .send input, .log, .msg-body { user-select: text }
```

Contenido legítimamente copiable (nombres de sesión, paths, mensajes,
snippets, terminal output) rehabilita la selección.

---

## 5. Espaciado

- Grid base: **4px**. Todo múltiplo de 4 (padding, gap, margin).
- Header: `padding: 11px 18px`, `gap: 8px 14px`.
- Chips: `padding: 3px 10px`, `border-radius: 20px`.
- Botones icon-only: `28×28px`, ícono `16×16px` centrado.
- Modales / drawers: `padding: 20px 24px`.

Radios de esquina:

| Elemento             | radio |
| -------------------- | ----- |
| Botón / input        | 6-8px |
| Chip                 | 20px  |
| Modal / card         | 10px  |
| Scrollbar thumb      | 10px  |

---

## 6. Componentes

### Botones

- **Primary**: fondo `bg`, borde `line`, hover `brand`. Padding `3px 9px`.
- **Icon-only** (`aria-label` presente): 28×28px, sin borde, hover brand.
- Siempre `aria-label` en icon-only para accesibilidad.

### Chips (`.pill`)

- `padding: 3px 10px`, `border-radius: 20px`, `font-size: 11.5px`.
- 3 sabores por estado: `.hot` (esperando), `.ok` (listo), `.run` (trabajando).

### Modales

- Overlay semitransparente sobre `body`.
- Content: `.modal` centrado, `max-width: 640px`, `padding: 20px 24px`.
- Cierre: **ESC** o click fuera. Botón `×` en top-right (icono `close`,
  no el carácter `×`).
- Foco atrapado dentro del modal mientras esté abierto.

### Drawer lateral (Settings)

- **Deprecado**. Migrar a modal. Ver Task #16.

### Tabs (cc-app)

- Barra tipo Termius: pestañas horizontales con dot de estado + label + close.
- Alto: `28px`. Font: 11px, monospace.
- Tab activa: subrayado 2px con brand, texto en color text.

---

## 7. Terminal (xterm.js + VTE)

- Fondo = `--bg` del theme activo.
- Cursor: `--cursor` (warning). Blink on.
- Selección: `--line2`.
- Padding interno: `8px 6px 6px 10px`.
- Ligaduras: activas via `assets/xterm/addon-ligatures-web.js` para xterm.js;
  no disponibles en VTE (limitación upstream — ver arco Task #13).

---

## 8. Reglas de contribución

Antes de agregar un componente nuevo:

1. **Reusar** — buscar si ya existe una variante (`.pill`, `.hdr-btn`, etc.).
2. **Variables** — nunca hardcodear color/tamaño; usar `var(--xxx)`.
3. **Icons** — usar el registry (`ICON` o symbolic Adwaita). No introducir
   glifos Unicode ni emojis en UI.
4. **Selección** — el elemento debe respetar la regla de user-select. Si su
   contenido es copiable, agregar `.selectable` o listarlo en el CSS del
   `<body>`.
5. **Diagnóstico** — probar con los 5 temas antes de mergear.

---

## 9. Snippets

Snippets guardan comandos shell reutilizables (one-liners y scripts multilínea)
que se pegan en la terminal activa sin ejecutar hasta que el usuario aprieta
Enter. Nunca corren automáticamente — esa promesa es load-bearing.

### Store

- Archivo: `~/.claude/hooks/snippets.json`
- Formato: array de `{"id","name","body","tags","updated_at"}`.
- `id`: hex de 16 caracteres, generado por el servidor (`secrets.token_hex(8)`).
- Escritura atómica: `write_json_file` (tmp + `os.replace`).

### API (cc-dash)

- `GET  /snippets`               → lista completa
- `POST /snippets`               `{name, body, tags?}` → crea, devuelve `{item}`
- `POST /snippets/update`        `{id, name, body, tags?}` → actualiza
- `POST /snippets/delete`        `{id}` → borra
- `POST /paste`                  `{session, text, pane?}` → bracketed paste, **sin ejecución**

### Send-vs-Paste

- `POST /send` (existente): `tmux send-keys ... Enter` — ejecuta.
- `POST /paste` (nuevo, para snippets): `tmux load-buffer` + `paste-buffer -p`
  + `delete-buffer`. El shell buffea la entrada; el usuario decide cuándo
  ejecutar. Es la única forma de mantener la promesa de "no auto-run" con
  bodies multilínea.

### UI

- **Dashboard**: botón `snippet` en el header → modal `#dlg-snippets`
  (720×520). Layout split: lista izq (search + items + "nuevo"), pane der
  (preview con tags + `<pre>` selectable + acciones Send/Copy/Edit/Delete
  + dropdown de sesión destino). `Ctrl+Shift+K` abre/cierra. Última sesión
  destino persistida en `localStorage["snippet-last-session"]`.

- **cc-app GTK**: botón `snippet` en el header (mismo SVG que dash) →
  `SnippetsDialog` (`Gtk.Dialog` con `Gtk.Paned` de 260 + resto). Mismo
  shortcut, misma paridad funcional. `snip_load`/`snip_save`/`snip_paste`
  leen y escriben el mismo `snippets.json`.

### Búsqueda

Contains case-insensitive sobre `name + tags + body`. Ordenamiento: matches en
`name` primero (score 3), luego `tags` (2), luego `body` (1), luego por
`updated_at` desc.

---

## 10. PerezOS

PerezOS es el operador perezoso de pixel art del Centro de Control de la
sesión seleccionada. Cuelga del cable de ejecución cuyo anclaje pertenece al
borde superior del panel: cable, cuerpo, sombras de contacto, nodo y borde
forman una sola composición. La mascota permanece dentro de su escenario y
nunca cubre controles ni modifica el ancho mínimo del Centro de Control.

### Identidad visual y composición

- Su identidad corporal siempre usa pelaje café cálido y crema, máscara facial
  oscura y tres garras por extremidad. Los temas sólo ajustan el rim light, el
  sesgo de sombras profundas, el nodo del cable y props pequeños; nunca
  recolorean todo el cuerpo.
- Todo elemento deformable —cable, anatomía, cara, garras, pelaje, luz y
  sombras— se pinta en un solo canvas transparente de 224×192 píxeles lógicos.
  No se permiten nodos DOM por parte del cuerpo ni actualizaciones DOM por
  frame. Los pseudo-elementos CSS se reservan para el anclaje fijo del panel.
- El render usa nearest-neighbor y escala únicamente por píxeles enteros; si el
  espacio es menor, usa la cámara compacta y letterboxing. Nunca distorsiona o
  escala fraccionalmente el atlas.
- La postura es de cuerpo completo, colgante y estable en tres cuartos. Las
  piezas anatómicas y sus máscaras mantienen contacto y oclusión. Se prohíben
  transforms de novedad aplicados al sprite completo —giros, saltos, flips o
  sacudidas— porque el movimiento debe tener causa anatómica y continuidad.

### Estado, interacción y movimiento

- `idle` mantiene respiración, parpadeo, mirada y cambios de soporte tranquilos;
  `working` se vuelve atento y se afirma; `waiting` mira el aviso y extiende una
  garra libre; `done` libera esfuerzo y se acomoda; `dead` comprueba la señal,
  se acurruca de forma segura en el cable y duerme. El texto de estado sigue
  siendo la fuente autoritativa: PerezOS no comunica estado sólo con color o
  movimiento.
- El escenario es un botón alcanzable por teclado. Click, Enter y Space
  solicitan una reacción segura, limitada por cooldown, y muestran una frase
  localizada; la interacción nunca captura el puntero ni activa controles
  vecinos. El nombre accesible se localiza y las microacciones no se anuncian.
- `prefers-reduced-motion: reduce` selecciona inmediatamente el modo Static:
  un cuadro seguro por cambio relevante, sin tracking continuo de ojos,
  respiración en loop, balanceo del cable ni movimiento secundario del pelaje.
- Hay cero trabajo cuando está oculto por preferencia, fuera del viewport o con
  `document.hidden`: no quedan actualizaciones ni renders programados y al
  volver no se reproduce un backlog. Destruir el controlador también elimina
  callbacks, observers y listeners.

### Rendimiento y degradación

Hay un solo controlador para la sesión seleccionada y se conserva al cambiar
estado, tema, rol o costume. El gobernador adapta Full → Balanced → Economy →
Static con histéresis antes de afectar la respuesta del dashboard. Al degradar
prioriza, en este orden, seguridad de contactos, silueta y anatomía, lectura de
la cara y continuidad; después conserva pelaje medio, y descarta primero
detalle fino, dithering dinámico y luces secundarias. Cambiar de calidad nunca
reinicia pose, contactos, fase de comportamiento ni secuencia determinista.
