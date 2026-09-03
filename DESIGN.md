# ComandOS — Design system

Guía canónica de la estética visual y de interacción de ComandOS. Referencia
para cualquier UI nueva (dashboard, cc-app GTK, popups, terminales).

Norte visual: se ve como una **app profesional de terminales** (referencia
mental: Termius). No se ve como una webapp. Nada de emojis, chrome no
seleccionable, tipografía monoespaciada consistente, íconos monolínea.

---

## 1. Paleta

Nueve temas simultáneos viven en `config/themes.json`, la fuente canónica de
metadata, superficies y estados. **Noche Órbita**, **Día Mineral**, **Cálido
Ámbar**, **Termius** y **Bruno Grafito** preservan las identidades sólidas;
**Super Glass** añade aurora y blur limitado al chrome; **Neón** usa carbono,
cyan y magenta; **Contraste** elimina blur/sombras y refuerza límites; **Ubuntu Terminal**
reproduce la terminal por defecto de Ubuntu (Ptyxis): fondo berenjena `#300A24`,
ANSI Tango y naranja Ubuntu `#E95420` como marca, texto sobre marca oscuro. La
selección se hace en Ajustes → Apariencia mediante radio-cards accesibles.
Dashboard, xterm, GTK/VTE, macOS, tmux y popups deben proyectar el mismo contrato.

IDs estables: `noche`, `dia`, `calido`, `termius`, `bruno`, `superglass`,
`neon`, `contraste`, `ubuntu`.

Los roles obligatorios son `bg`, `panel`, `panel2`, `line`, `line2`, `text`,
`dim`, `faint`, `brand`, `onBrand`, `waiting`, `working`, `done`, `err`, `warn`,
`ok`, `glow`, `code`, `cursor` y `bar`. Nunca hardcodear un color en un
componente nuevo: usar el rol semántico. Día Mineral usa canvas `#BCC7D4`, panel
`#F1F4F7`, texto `#17202B` y acento `#1D55C7`: no usa blanco puro como canvas.

Bruno es la fuente de verdad para sus roles: fondo `#1A1A1A`, panel
`#222224`, panel secundario `#26292B`, texto `#CCCCCC`, cursor/acento
`#E4AE49`, línea `#333333`, línea 2 `#444444`, dim `#AAAAAA` y faint
`#999999`. Su ANSI, en orden negro a blanco y luego brillantes, es:
`#888888 #DA462F #73E89A #FAD075 #8BC2F9 #D691ED #7DDFF2 #CCCCCC
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
