# Fase 1 · pendientes de cierre en el escritorio (30-sep-2026)

Decisiones de Jesús tras ver el escritorio real (captura 30-sep 08:39) y el
mockup `dash/prototypes/prototype-desktop-unificado.html` (rama del grill).
No están programadas: otro agente trabaja la fase 2 en paralelo y estos
fixes esperan su turno para no chocar.

## 1. Quitar «Sí» y «No» de la píldora de cada pane

La píldora `fable-5-1 ✓ · ⚙ · Sí · No · MCPs · Skills` no debe llevar Sí/No.
Quitarlos de `_pane_pill` (`bin/cc-app`, `_answer_button`). Los Sí/No de la
barra táctil de la terminal remota (`dash/term.html`) se quedan: ahí sí
tienen sentido porque en el celular no hay Enter/Esc a la mano.

## 2. Botones de acción a la izquierda, iguales al remoto

Variante **A** del mockup: la barra de ventana queda solo con la marca y los
controles de ventana; `>_ + ⇅ Analytics Remoto reloj campana ✨ Ajustes` van
arriba de la barra lateral, en el mismo orden que en remoto (hoy están arriba
a la derecha en `_headerbar.pack_end`). Mismo tamaño para todos, mismo estilo
3D elegido en Ajustes.

## 3. Misma calidad de componentes en escritorio y remoto

Por qué hoy el escritorio se ve peor: **la barra lateral y los paneles son
web (WebKit, mismo CSS que el remoto), pero las pestañas, la barra de ventana,
las píldoras de pane y el arrastre están dibujados con widgets GTK a mano**
(`bin/cc-app` usa 286 llamadas Gtk.*; CSS GTK propio con `monospace 9px`,
`font-size: 11px`, sin Inter/Ubuntu Sans, sin transiciones). GTK3 no tiene
`transform`, así que el reacomodo de pestañas no puede animarse como el mockup
aprobado (levantar, resorte, encajar): hoy solo atenúa la pestaña.

Camino recomendado (decidir al programar):
- **a)** Mover la tira de pestañas y las píldoras a la vista web que ya tiene
  la app (`?app=1`), como en remoto: un solo CSS, mismas fuentes, mismas
  animaciones (`workspace-dock.js` ya hace levantar/resorte/bandejas/
  auto-desplazamiento). GTK conserva solo la ventana, las terminales VTE y la
  barra de ventana. Es lo que iguala de verdad ambas versiones.
- **b)** Quedarse en GTK y aproximar: tipografía Ubuntu Sans/Inter en el CSS
  GTK, mismos tamaños y radios que `workspace.css`, `gtk-enable-animations`
  con transiciones de `opacity`/`margin` (GTK3 sí anima esas propiedades),
  y un «fantasma» de la pestaña en la capa `_ws_layer` durante el arrastre.
  Más barato, pero nunca queda idéntico.

Jesús quiere: flex y espaciados impecables por botón, transiciones suaves,
arrastre de pestañas exactamente como el mockup aprobado, ligero y rápido.

## Estado del mockup

`prototype-desktop-unificado.html` (sin commit, rama `prototype/comandos-v1-grill`):
variantes Actual / A / B; se eligió **A**. Falta verlo en pantalla (chrome-bg
desconectado al momento de hacerlo).

## 4. Semáforos de pestaña: «ComandOS bot» por default (30-sep-2026)

Decidido en el grill `prototype-iguales.html` (rama del grill): los iconos
actuales de `lib/work_marks.py` / `dash/work-marks.js` se sustituyen por el
personaje propio **ComandOS bot** (robot-terminal con cara CRT `>_`),
5 estados × 2 cuadros a 48 px: `dash/prototypes/assets/semaforos/comandos/`
(`<estado>.png` tira 96×48, `sheet-48.png`, `README.md` con la receta).
Kicked-in-Teeth (CC-BY-SA) queda como opción elegible en Ajustes; el resto
de sets de terceros y los personajes de prueba (Satoshi, Taquito, Michi,
Pato) se descartaron.

Al programar: mover la tira a `dash/icons/semaforos/comandos/`, servirla en
`installer`/`install.sh`, y en escritorio y remoto pintar el estado como
`background-image` con `animation: steps(2)` (idle 1.4 fps, work 5, need 4,
done 3, error 3), en vez de SVG inline. En GTK el `_paint_tab_sticker` ya
dibuja imágenes en la pestaña; usar la misma tira.

## 5. Reloj de arena propio (30-sep-2026)

Sustituir la tira de Davitheoles (`assets/pomodoro/davitheoles/hourglass.png`)
por la nuestra: `dash/prototypes/assets/hourglass/comandos/hourglass.png`
(864×32, 27 cuadros, **mismo contrato** 0–20 llenado / 21–26 giro; ver su
`README.md`). Es drop-in: `P.hourglassFrame` y `hourglass_frame` no cambian.
Al programar: mover a `assets/pomodoro/comandos/hourglass.png`, apuntar
`dash/pomodoro.js` (`anim(...)`/ART_SOURCES) y `bin/cc-app` (`_HOURGLASS`)
a la nueva ruta, quitar la entrada de Davitheoles del README/CREDITS y
borrar su carpeta. Latón + vidrio pálido + ámbar: combina con temas claros y
oscuros (probado sobre `#12091f` y `#eceef3`).
