# xterm.js (bundled)

Bundle pre-built de [xterm.js](https://xtermjs.org/) + addons, vendorizado en
este repo para que `cc-app` pueda montar un terminal en `WebKit2WebView` sin
depender de `npm install` ni de red al arrancar.

## Contenido

| Archivo                | Versión | Uso                                             |
| ---------------------- | ------- | ----------------------------------------------- |
| `xterm.js`             | 5.5.0   | Core del terminal                               |
| `xterm.css`            | 5.5.0   | Estilos base                                    |
| `addon-fit.js`         | 0.10.0  | Ajusta cols/rows al tamaño del contenedor       |
| `addon-web-links.js`   | 0.11.0  | Detecta y hace clickeables las URLs             |
| `addon-canvas.js`      | 0.7.0   | Renderer Canvas 2D (soporta ligaturas OpenType) |
| `addon-attach.js`      | 0.11.0  | Conecta un `WebSocket` al I/O del terminal      |

## Origen

Descargado de unpkg:

```
https://unpkg.com/@xterm/xterm@5.5.0/lib/xterm.js
https://unpkg.com/@xterm/xterm@5.5.0/css/xterm.css
https://unpkg.com/@xterm/addon-fit@0.10.0/lib/addon-fit.js
https://unpkg.com/@xterm/addon-web-links@0.11.0/lib/addon-web-links.js
https://unpkg.com/@xterm/addon-canvas@0.7.0/lib/addon-canvas.js
https://unpkg.com/@xterm/addon-attach@0.11.0/lib/addon-attach.js
```

## Licencia

**MIT** — © 2017 The xterm.js authors.
Ver <https://github.com/xtermjs/xterm.js/blob/master/LICENSE> para el texto
completo. Redistribución permitida con este aviso.

## Actualizar

Reemplazar los archivos desde el mismo CDN (`@xterm/*@<nueva-version>`) y bumpear
la tabla de arriba. No hay build step.
