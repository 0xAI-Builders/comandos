#!/usr/bin/env bash
# Un SyntaxError en el <script> del tablero mata TODA la UI (cero sesiones).
# Este test parsea el JS con node antes de que llegue a una pantalla real.
set -e
cd "$(dirname "$0")/.."
command -v node >/dev/null || { echo "SKIP: sin node"; exit 0; }
for f in dash/index.html dash/term.html; do
  node -e "
    const html = require('fs').readFileSync('$f','utf8');
    for (const m of html.matchAll(/<script>([\s\S]*?)<\/script>/g)) new Function(m[1]);
  " || { echo "FALLA parse JS en $f"; exit 1; }
done
node --check assets/xterm/addon-ligatures-web.js
echo OK
