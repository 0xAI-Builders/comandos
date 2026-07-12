# ComandOS — Design system

Guía canónica de la estética visual y de interacción de ComandOS. Referencia
para cualquier UI nueva (dashboard, cc-app GTK, popups, terminales).

Norte visual: se ve como una **app profesional de terminales** (referencia
mental: Termius). No se ve como una webapp. Nada de emojis, chrome no
seleccionable, tipografía monoespaciada consistente, íconos monolínea.

---

## 1. Paleta

Tres temas simultáneos — el usuario alterna con el botón luna/sol/llama. Todos
los colores viven en variables CSS (`--bg`, `--panel`, `--brand`, …) y en el
diccionario `THEMES` de `bin/cc-app`. Nunca hardcodear un color en un
componente nuevo: usar la variable.

| Rol             | noche       | dia         | calido      |
| --------------- | ----------- | ----------- | ----------- |
| Fondo app       | `#0A0D13`   | `#F2F4F8`   | `#161009`   |
| Panel           | `#121722`   | `#FBFCFE`   | `#1F1811`   |
| Text            | `#EAF0FB`   | `#1B2130`   | `#F2E5D0`   |
| Dim (label)    | `#9AA6BF`   | `#4E5A70`   | `#BCA98C`   |
| Faint (hint)   | `#5E6980`   | `#8892A6`   | `#8A7A5F`   |
| Brand (acento) | `#8B7CFF`   | `#5B4BD6`   | `#E0A458`   |
| Línea (border) | `#222A3A`   | `#D9DEE8`   | `#36291A`   |
| Warning         | `#FFAE1A`   | `#B26A00`   | `#FFB454`   |

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
5. **Diagnóstico** — probar con los 3 temas antes de mergear.
