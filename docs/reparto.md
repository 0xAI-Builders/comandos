# Reparto de cuota

Analytics → **Reparto**. Sustituye a la vieja pestaña Optimizar.

## Qué ves

Un tanque por cuota con fuente conocida: Claude main, Claude relotto, Codex, Grok.
La **capa oscura** es lo que ya gastaste. La **capa clara** encima es lo que tus
sesiones vivas añadirán antes de que la cuota se renueve. Si la suma pasa del
borde, el tanque se marca en rojo.

Cada tanque dice una de dos cosas, sin códigos que descifrar:

- **llega al reset** — con el ritmo actual, esa cuota aguanta hasta renovarse.
- **se acaba en 1d 22h** — se agota antes, y cuándo.

Y su **ritmo**: `1.0x` es gastar justo lo que llega al reset; `1.3x` es ir un 30 %
por encima. El color sale del ritmo, no del porcentaje: 98 % de Grok con ritmo
0.2x está verde, 25 % de Codex con ritmo 2.5x está rojo.

## Los tres pasos

1. **Analizar.** Reparte tus sesiones vivas entre cuentas y motores para que
   ninguna cuota se acabe antes de tiempo. Es determinista: las mismas sesiones y
   las mismas cuotas dan siempre el mismo reparto.
2. **Curar.** Cada sesión aparece como ficha dentro del tanque de la cuota que
   paga, mostrando lo que tiene tachado, la flecha, y lo nuevo:
   - toca los selectores para cambiar **modelo** o **effort**; solo se ofrecen los
     válidos para ese motor, y se marca en color lo que difiere de lo actual;
   - **arrastra** la ficha a otro tanque para cambiarla de cuenta o de motor. El
     tanque destino dibuja en punteado cómo quedaría antes de que sueltes;
   - el **candado** fija una sesión para que el reparto no la toque;
   - en pantallas estrechas, al arrastrar aparece abajo una barra con los destinos.
3. **Aplicar N.** Reinicia esas sesiones con la nueva configuración conservando la
   conversación. El pie se vuelve un panel de progreso con cada sesión como paso.
   Si una se detiene, dice por qué y puedes reintentar solo esa. **Revertir todo**
   aparece al terminar y deshace únicamente lo que llegó a confirmarse.

## Lo que hace por debajo para no romperte una sesión

- **El plan se congela.** Si entre analizar y aplicar cambias el modelo de una
  sesión, o nace o muere alguna, aplicar responde que el plan caducó y te dice
  cuáles cambiaron, en vez de ejecutar algo que ya no corresponde.
- **Cada sesión va por separado.** Un fallo en una no tumba el lote: esa queda
  detenida con su motivo y las demás siguen.
- **El trust se hereda.** Al cambiar de cuenta, si la carpeta ya estaba aceptada en
  la cuenta de origen, ComandOS la marca aceptada en la de destino antes de lanzar,
  para que no te quedes parado en "¿Confías en los archivos de esta carpeta?".
  Nunca acepta una carpeta que no hubieras aceptado antes en ninguna cuenta, y lo
  anota en el registro de cambios como `trust_inherited`.
- **Más margen cuando hace falta.** Una sesión que carga MCPs tarda más en
  arrancar, así que la verificación pasa de 30 a 90 segundos en ese caso.
- **Las rutas que no admiten cambio en caliente** (OpenCode, Antigravity y todos
  los ACP salvo Claude) aparecen fijadas: su consumo cuenta para la cuota, pero el
  reparto no las mueve.

## Límites conocidos

- Una sesión marcada como la más importante se reparte entre las cuentas de su
  propio motor, nunca salta a otro motor por su cuenta. Si quieres bajarla de
  motor, arrástrala tú.
- Cambiar de cuenta no está disponible en rutas pasarela (`claude:codex`,
  `claude:grok`) ni en ACP: no hay exportador de conversación para eso.
- Sin fuente de cuota no hay tanque. Gemini, Antigravity y OpenCode no publican
  cuota hoy, así que sus sesiones salen en un grupo aparte.
