// Barra de comandos (S1): acordeón por CLI con los comandos del catálogo,
// cadenas guardadas que se corren paso a paso con «Siguiente» y la lista de
// terminales rápidas. Un clic teclea el texto letra a letra en el pane de
// destino vía POST /pane/type y NUNCA envía Enter: el Enter lo da el usuario.
// Sin temporizadores: una cadena avanza solo cuando el servidor respondió 200.
//
// rowHTML/cliHTML son puros y se exportan para el constructor de cadenas (S3):
// mode 'run' (por defecto) pone data-cmd en filas y chips; mode 'build' pone
// data-add y un botón «+ cadena».
(function (root) {
  'use strict';
  const KEY_TERMS_HIDDEN = 'comandos.commands.termsHidden';
  const KEY_OPEN = 'comandos.commands.open.v2', KEY_PREF = 'comandos.commands.preferred.';
  const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const STATUS_TEXT = { ok: '', drift: 'sin verificar · el CLI confirma', missing: 'no instalado', unverified: 'sin verificar' };
  const CONTROL_RE = /[\x00-\x1f\x7f]/;
  const WAIT = 'Espera a que termine de escribir';
  const chev = '<span class="chev" data-icon="chevron" data-size="12"></span>';
  // Los encabezados plegables son <div> (sin el estilo 3D de los botones) pero se
  // operan con teclado: role=button, foco con Tab, Enter/Espacio (isToggleKey).
  const toggleAttrs = open => ` role="button" tabindex="0" aria-expanded="${open ? 'true' : 'false'}"`;
  const isToggleKey = e => !!e && (e.key === 'Enter' || e.key === ' ' || e.key === 'Spacebar') && !e.isComposing;

  // La consulta llega cruda (con espacios); aquí se parte en palabras y cada
  // una debe aparecer en el texto, la descripción o algún argumento.
  function hits(cmd, q) {
    const words = String(q || '').toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return true;
    const hay = [cmd.text, cmd.description, ...(Array.isArray(cmd.args) ? cmd.args : [])].map(x => String(x || '')).join(' ').toLowerCase();
    return words.every(w => hay.includes(w));
  }

  // Fila de dos líneas: comando (con «…» si espera argumento) y descripción;
  // los args son chips que teclean «texto arg».
  function rowHTML(cmd, opts = {}) {
    const mode = opts.mode === 'build' ? 'build' : 'run';
    const kind = opts.kind === 'shell' ? 'shell' : 'pane';
    const attr = mode === 'build' ? 'data-add' : 'data-cmd';
    const text = String(cmd.text ?? '');
    const withArg = a => (text.endsWith(' ') ? text : text + ' ') + a;
    const fresh = new Set(Array.isArray(cmd.newArgs) ? cmd.newArgs : []);
    const chips = (Array.isArray(cmd.args) ? cmd.args : [])
      .map(a => fresh.has(a)
        ? `<button type="button" data-flat class="new" aria-label="${esc(a)} (nuevo)" title="nuevo" ${attr}="${esc(withArg(a))}" data-kind="${kind}">${esc(a)}</button>`
        : `<button type="button" data-flat ${attr}="${esc(withArg(a))}" data-kind="${kind}">${esc(a)}</button>`).join('');
    const add = mode === 'build' ? `<button type="button" data-flat class="add" data-add="${esc(text)}" data-kind="${kind}">+ cadena</button>` : '';
    const dis = (opts.dis ? ' dis' : '') + (opts.cls ? ' ' + opts.cls : '');
    const hidden = hits(cmd, opts.q) ? '' : ' hidden';
    return `<div class="cmd${dis}" ${attr}="${esc(text)}" data-kind="${kind}"${hidden}>`
      + `<code>${esc(text)}${text.endsWith(' ') ? '<em>…</em>' : ''}</code>`
      + (cmd.description ? `<small title="${esc(cmd.description)}">${esc(cmd.description)}</small>` : '')
      + (chips ? `<span class="opts">${chips}</span>` : '') + add + '</div>';
  }

  // Arranques como píldoras monoespaciadas (mockup): yolo en ámbar, normal neutro.
  // Siguen siendo .cmd[data-cmd|data-add] para que clic/teclado y las pruebas no cambien.
  // Monograma por CLI: colores del mockup aprobado; el pictograma es el icono de
  // proveedor del producto (data-icon) y, sin icono, la inicial.
  // Iconos pixel del mockup aprobado (pack shikashi, tabla I del prototipo), mismo
  // marcado <i class="px ic s20|s24"> y mismas posiciones de sprite.
  const IC = { book: [7, 4], lens: [8, 10], chain: [2, 11], bolt: [8, 0], fire: [2, 4], cast: [0, 21], spark: [5, 0], bomb: [12, 10] };
  function ic(k, s = '') {
    const p = IC[k];
    return p ? `<i class="px ic ${s}" style="background-position:${-p[0] * 32}px ${-p[1] * 32}px"></i>` : '';
  }
  // Logo oficial de cada CLI (LobeHub Icons, MIT; ver assets/brand-logos/CREDITS.md):
  // a color donde la marca lo tiene (Claude, Codex, Antigravity); Grok es monocromo y
  // toma el color del texto del tema. OpenCode es su favicon oficial, el que trae el
  // propio binario (`opencode web`), con su fondo #131010. Baldosa del propio tema (diseño 3
  // aprobado, «Oficial a color»). Sin logo, la inicial.
  const LOGO = {
    "claude": "<svg class=\"logo\" aria-hidden=\"true\" focusable=\"false\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z\" fill=\"#D97757\" fill-rule=\"nonzero\"></path></svg>",
    "codex": "<svg class=\"logo\" aria-hidden=\"true\" focusable=\"false\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M19.503 0H4.496A4.496 4.496 0 000 4.496v15.007A4.496 4.496 0 004.496 24h15.007A4.496 4.496 0 0024 19.503V4.496A4.496 4.496 0 0019.503 0z\" fill=\"#fff\"></path><path d=\"M9.064 3.344a4.578 4.578 0 012.285-.312c1 .115 1.891.54 2.673 1.275.01.01.024.017.037.021a.09.09 0 00.043 0 4.55 4.55 0 013.046.275l.047.022.116.057a4.581 4.581 0 012.188 2.399c.209.51.313 1.041.315 1.595a4.24 4.24 0 01-.134 1.223.123.123 0 00.03.115c.594.607.988 1.33 1.183 2.17.289 1.425-.007 2.71-.887 3.854l-.136.166a4.548 4.548 0 01-2.201 1.388.123.123 0 00-.081.076c-.191.551-.383 1.023-.74 1.494-.9 1.187-2.222 1.846-3.711 1.838-1.187-.006-2.239-.44-3.157-1.302a.107.107 0 00-.105-.024c-.388.125-.78.143-1.204.138a4.441 4.441 0 01-1.945-.466 4.544 4.544 0 01-1.61-1.335c-.152-.202-.303-.392-.414-.617a5.81 5.81 0 01-.37-.961 4.582 4.582 0 01-.014-2.298.124.124 0 00.006-.056.085.085 0 00-.027-.048 4.467 4.467 0 01-1.034-1.651 3.896 3.896 0 01-.251-1.192 5.189 5.189 0 01.141-1.6c.337-1.112.982-1.985 1.933-2.618.212-.141.413-.251.601-.33.215-.089.43-.164.646-.227a.098.098 0 00.065-.066 4.51 4.51 0 01.829-1.615 4.535 4.535 0 011.837-1.388zm3.482 10.565a.637.637 0 000 1.272h3.636a.637.637 0 100-1.272h-3.636zM8.462 9.23a.637.637 0 00-1.106.631l1.272 2.224-1.266 2.136a.636.636 0 101.095.649l1.454-2.455a.636.636 0 00.005-.64L8.462 9.23z\" fill=\"url(#cs-logo-codex-_R_0_)\"></path><defs><linearGradient gradientUnits=\"userSpaceOnUse\" id=\"cs-logo-codex-_R_0_\" x1=\"12\" x2=\"12\" y1=\"3\" y2=\"21\"><stop stop-color=\"#B1A7FF\"></stop><stop offset=\".5\" stop-color=\"#7A9DFF\"></stop><stop offset=\"1\" stop-color=\"#3941FF\"></stop></linearGradient></defs></svg>",
    "grok": "<svg class=\"logo\" aria-hidden=\"true\" focusable=\"false\" fill=\"currentColor\" fill-rule=\"evenodd\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815\"></path></svg>",
    "opencode": "<svg class=\"logo\" aria-hidden=\"true\" focusable=\"false\" viewBox=\"0 0 512 512\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"512\" height=\"512\" rx=\"96\" fill=\"#131010\"/><path d=\"M320 224V352H192V224H320Z\" fill=\"#5A5858\"/><path fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"M384 416H128V96H384V416ZM320 160H192V352H320V160Z\" fill=\"white\"/></svg>",
    "agy": "<svg class=\"logo\" aria-hidden=\"true\" focusable=\"false\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><mask height=\"23\" id=\"cs-logo-antigravity-0-_R_0_\" maskUnits=\"userSpaceOnUse\" width=\"24\" x=\"0\" y=\"1\"><path d=\"M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z\" fill=\"#fff\"></path></mask><g mask=\"url(#cs-logo-antigravity-0-_R_0_)\"><g filter=\"url(#cs-logo-antigravity-1-_R_0_)\"><path d=\"M-1.018-3.992c-.408 3.591 2.686 6.89 6.91 7.37 4.225.48 7.98-2.043 8.387-5.633.408-3.59-2.686-6.89-6.91-7.37-4.225-.479-7.98 2.043-8.387 5.633z\" fill=\"#FFE432\"></path></g><g filter=\"url(#cs-logo-antigravity-2-_R_0_)\"><path d=\"M15.269 7.747c1.058 4.557 5.691 7.374 10.348 6.293 4.657-1.082 7.575-5.653 6.516-10.21-1.058-4.556-5.691-7.374-10.348-6.292-4.657 1.082-7.575 5.653-6.516 10.21z\" fill=\"#FC413D\"></path></g><g filter=\"url(#cs-logo-antigravity-3-_R_0_)\"><path d=\"M-12.443 10.804c1.338 4.703 7.36 7.11 13.453 5.378 6.092-1.733 9.947-6.95 8.61-11.652C8.282-.173 2.26-2.58-3.833-.848-9.925.884-13.78 6.1-12.443 10.804z\" fill=\"#00B95C\"></path></g><g filter=\"url(#cs-logo-antigravity-4-_R_0_)\"><path d=\"M-12.443 10.804c1.338 4.703 7.36 7.11 13.453 5.378 6.092-1.733 9.947-6.95 8.61-11.652C8.282-.173 2.26-2.58-3.833-.848-9.925.884-13.78 6.1-12.443 10.804z\" fill=\"#00B95C\"></path></g><g filter=\"url(#cs-logo-antigravity-5-_R_0_)\"><path d=\"M-7.608 14.703c3.352 3.424 9.126 3.208 12.896-.483 3.77-3.69 4.108-9.459.756-12.883C2.69-2.087-3.083-1.871-6.853 1.82c-3.77 3.69-4.108 9.458-.755 12.883z\" fill=\"#00B95C\"></path></g><g filter=\"url(#cs-logo-antigravity-6-_R_0_)\"><path d=\"M9.932 27.617c1.04 4.482 5.384 7.303 9.7 6.3 4.316-1.002 6.971-5.448 5.93-9.93-1.04-4.483-5.384-7.304-9.7-6.301-4.316 1.002-6.971 5.448-5.93 9.93z\" fill=\"#3186FF\"></path></g><g filter=\"url(#cs-logo-antigravity-7-_R_0_)\"><path d=\"M2.572-8.185C.392-3.329 2.778 2.472 7.9 4.771c5.122 2.3 11.042.227 13.222-4.63 2.18-4.855-.205-10.656-5.327-12.955-5.122-2.3-11.042-.227-13.222 4.63z\" fill=\"#FBBC04\"></path></g><g filter=\"url(#cs-logo-antigravity-8-_R_0_)\"><path d=\"M-3.267 38.686c-5.277-2.072 3.742-19.117 5.984-24.83 2.243-5.712 8.34-8.664 13.616-6.592 5.278 2.071 11.533 13.482 9.29 19.195-2.242 5.713-23.613 14.298-28.89 12.227z\" fill=\"#3186FF\"></path></g><g filter=\"url(#cs-logo-antigravity-9-_R_0_)\"><path d=\"M28.71 17.471c-1.413 1.649-5.1.808-8.236-1.878-3.135-2.687-4.531-6.201-3.118-7.85 1.412-1.649 5.1-.808 8.235 1.878s4.532 6.2 3.119 7.85z\" fill=\"#749BFF\"></path></g><g filter=\"url(#cs-logo-antigravity-10-_R_0_)\"><path d=\"M18.163 9.077c5.81 3.93 12.502 4.19 14.946.577 2.443-3.612-.287-9.727-6.098-13.658-5.81-3.931-12.502-4.19-14.946-.577-2.443 3.612.287 9.727 6.098 13.658z\" fill=\"#FC413D\"></path></g><g filter=\"url(#cs-logo-antigravity-11-_R_0_)\"><path d=\"M-.915 2.684c-1.44 3.473-.97 6.967 1.05 7.804 2.02.837 4.824-1.3 6.264-4.772 1.44-3.473.97-6.967-1.05-7.804-2.02-.837-4.824 1.3-6.264 4.772z\" fill=\"#FFEE48\"></path></g></g><defs><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"17.587\" id=\"cs-logo-antigravity-1-_R_0_\" width=\"19.838\" x=\"-3.288\" y=\"-11.917\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"1.117\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"38.565\" id=\"cs-logo-antigravity-2-_R_0_\" width=\"38.9\" x=\"4.251\" y=\"-13.493\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"5.4\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"36.517\" id=\"cs-logo-antigravity-3-_R_0_\" width=\"40.955\" x=\"-21.889\" y=\"-10.592\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"4.591\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"36.517\" id=\"cs-logo-antigravity-4-_R_0_\" width=\"40.955\" x=\"-21.889\" y=\"-10.592\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"4.591\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"36.595\" id=\"cs-logo-antigravity-5-_R_0_\" width=\"36.632\" x=\"-19.099\" y=\"-10.278\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"4.591\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"34.087\" id=\"cs-logo-antigravity-6-_R_0_\" width=\"33.533\" x=\".981\" y=\"8.758\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"4.363\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"35.276\" id=\"cs-logo-antigravity-7-_R_0_\" width=\"35.978\" x=\"-6.143\" y=\"-21.659\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"3.954\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"46.523\" id=\"cs-logo-antigravity-8-_R_0_\" width=\"45.114\" x=\"-11.96\" y=\"-.46\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"3.531\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"24.054\" id=\"cs-logo-antigravity-9-_R_0_\" width=\"25.094\" x=\"10.485\" y=\".58\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"3.159\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"30.007\" id=\"cs-logo-antigravity-10-_R_0_\" width=\"33.508\" x=\"5.833\" y=\"-12.467\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"2.669\"></feGaussianBlur></filter><filter color-interpolation-filters=\"sRGB\" filterUnits=\"userSpaceOnUse\" height=\"26.151\" id=\"cs-logo-antigravity-11-_R_0_\" width=\"22.194\" x=\"-8.355\" y=\"-8.876\"><feFlood flood-opacity=\"0\" result=\"BackgroundImageFix\"></feFlood><feBlend in=\"SourceGraphic\" in2=\"BackgroundImageFix\" result=\"shape\"></feBlend><feGaussianBlur result=\"effect1_foregroundBlur_977_115\" stdDeviation=\"3.303\"></feGaussianBlur></filter></defs></svg>"
  };
  function monoHTML(id) {
    const svgLogo = LOGO[id];
    return svgLogo ? `<span class="mono logo-tile" data-cli="${esc(id)}">${svgLogo}</span>`
      : `<span class="mono logo-tile">${esc(String(id || '?').slice(0, 1).toUpperCase())}</span>`;
  }

  // Arranques leídos de `<cli> --help` (cli.start): la fila del binario con el primer
  // párrafo de su ayuda, las cuentas reales, los flags que el propio CLI describe como
  // saltarse permisos (ámbar) y cada sección de la ayuda con su título original,
  // plegada. Ningún texto de aquí es nuestro: título y descripciones son del CLI.
  function startHTML(cli, o) {
    const st = cli.start || { rows: [], yolo: [], sections: [] };
    const dis = (cli.version && cli.version.status === 'missing') || o.noTarget;
    const row = (c, cls) => rowHTML(c, { mode: o.mode, kind: 'shell', dis, q: o.q, cls });
    const top = (st.rows || []).map(c => row(c, 'bin')).join('') + (st.yolo || []).map(c => row(c, 'y')).join('');
    const secs = (st.sections || []).map(sec => {
      const key = `${cli.id}:help:${sec.title}`, open = o.open.has(key);
      const items = sec.items || [];
      const empty = o.q && !items.some(c => hits(c, o.q));
      return `<div class="hsec${open ? ' open' : ' closed'}"${empty ? ' hidden' : ''}>`
        + `<div class="hsec-h" data-toggle="${esc(key)}"${toggleAttrs(open)}><span class="t">${esc(sec.title)}</span>`
        + `<small class="n">${items.length}</small><code class="src">${esc(st.command || '')}</code>`
        + `<span class="chev">${open ? '▾' : '▸'}</span></div>`
        + `<div class="srows">${items.map(c => row(c, '')).join('')}</div></div>`;
    }).join('');
    return `<div class="start"><div class="srows top">${top}</div>${secs}</div>`;
  }
  const startTexts = cli => {
    const st = cli.start || {};
    return [...(st.rows || []), ...(st.yolo || []), ...(st.sections || []).flatMap(x => x.items || [])];
  };

  // Lista plana (ronda 6 A aprobada): los grupos del catálogo solo ordenan; no
  // se pintan subtítulos ni se pliegan. Solo llegan los comandos detectados en
  // el binario (cli.detected); si nada quedó, se dice.
  function commandsHTML(cli, o) {
    const cmds = (Array.isArray(cli.groups) ? cli.groups : []).flatMap(g => Array.isArray(g.commands) ? g.commands : []);
    const rows = cmds.map(c => rowHTML(c, { mode: o.mode, kind: 'pane', dis: o.noCli, q: o.q })).join('');
    const d = cli.detected;
    const none = !cmds.length && d && d.total
      ? `<div class="note"${o.q ? ' hidden' : ''}>ninguno de los ${d.total} comandos del catálogo está en este binario</div>` : '';
    return `<div class="cmds">${rows}${none}</div>`;
  }

  // o: {mode, open: Set, here, noCli, noTarget, q}. Todo CLI y arranque se pinta
  // siempre; la clase .open solo decide la visibilidad por CSS.
  function cliHTML(cli, opts = {}) {
    const o = { mode: opts.mode === 'build' ? 'build' : 'run', open: opts.open || new Set(), q: String(opts.q || '').trim(),
      noCli: opts.mode === 'build' ? false : !!opts.noCli, noTarget: opts.mode === 'build' ? false : !!opts.noTarget };
    const v = cli.version || {};
    // Versión distinta a la del catálogo pero todos los comandos presentes en
    // el binario: verificado de verdad, sin ámbar. Si falta alguno, sigue ámbar.
    const d = cli.detected && cli.detected.total ? cli.detected : null;
    const st0 = STATUS_TEXT[v.status] !== undefined ? v.status : 'ok';
    const st = (st0 === 'drift' || st0 === 'unverified') && d && d.found === d.total ? 'ok' : st0;
    const ver = v.installed ? `${v.installed}` : (st === 'missing' ? 'no instalado' : '');
    const groups = Array.isArray(cli.groups) ? cli.groups : [];
    const empty = o.q && !startTexts(cli).some(c => hits(c, o.q))
      && !groups.some(g => (g.commands || []).some(c => hits(c, o.q)));
    const det = '';   // el conteo detectado va en el title; la fila muestra solo la versión (mockup)
    const cls = ['cs-cli', o.open.has(cli.id) ? 'open' : '', opts.here ? 'here' : '', st !== 'ok' ? st : ''].filter(Boolean).join(' ');
    const badge = opts.here ? '<span class="badge">en este pane</span>'
      : st === 'missing' ? '<span class="badge off">no instalado</span>'
      : st === 'ok' ? '<span class="badge off">instalado</span>'
      : '<span class="badge warn">sin verificar</span>';
    const title = [`catálogo v${v.pinned}`, STATUS_TEXT[st0], d ? `${d.found} de ${d.total} comandos presentes en el binario` : ''].filter(Boolean).join(' · ');
    return `<div class="${cls}" data-cli="${esc(cli.id)}"${empty ? ' hidden' : ''}>`
      + `<div class="cli-h" data-toggle="${esc(cli.id)}"${toggleAttrs(o.open.has(cli.id))} title="${esc(title)}">${monoHTML(cli.id)}<span class="nm">${esc(cli.label || cli.id)}</span>`
      + `<small class="ver">${esc(ver)}</small>${badge}<span class="chev">${o.open.has(cli.id) ? '▾' : '▸'}</span></div>`
      + startHTML(cli, o) + commandsHTML(cli, o) + '</div>';
  }

  function createCommandSidebar(opts) {
    const { api, root: el, storage, makeId, getTarget, focusTarget = () => {}, openBuilder = () => {}, mountTerm = null,
      toast = () => {}, terminals = () => [], newTerm = () => {}, killTerm = null, hydrate = () => {},
      mountServers = () => {} } = opts;
    const state = { catalog: null, cliInPane: '', catalogTarget: null, chains: [], open: new Set(), run: null, curTerm: '',
      typing: null, q: '', firstRender: true, appliedTarget: null };

    // móvil (≤900): el bloque «arrancar normal» nace plegado; en escritorio, abierto
    // (la barra del escritorio es angosta pero no es móvil: se decide por el puntero táctil)
    const read = k => { try { return storage ? storage.getItem(k) : null; } catch (_) { return null; } };
    const write = (k, v) => { try { if (storage) storage.setItem(k, v); } catch (_) {} };
    (function readOpen() {
      try {
        const saved = JSON.parse(read(KEY_OPEN) || 'null');
        if (Array.isArray(saved)) { state.open = new Set(saved.map(String)); state.firstRender = false; }
      } catch (_) {}
    })();
    const writeOpen = () => write(KEY_OPEN, JSON.stringify([...state.open]));
    state.termsHidden = read(KEY_TERMS_HIDDEN) === '1';
    // Barra de herramientas (grill 1-oct, diseño 1): Comandos, Cadenas y Servidores son un
    // panel con pestañas que se abre a demanda encima de la terminal ('' = cerrado).
    state.sheet = '';

    function target() { try { return getTarget() || null; } catch (_) { return null; } }
    const prefKey = t => KEY_PREF + (t.paneKey || t.pane);
    const sameTarget = (a, b) => !!a && !!b && a.session === b.session && a.pane === b.pane;
    // El CLI del catálogo solo vale para el destino con el que se pidió: al
    // cambiar de pane, hasta el próximo refresh() no se afirma ningún CLI.
    function hereCli() {
      const t = target();
      if (!t) return '';
      if (state.catalogTarget && !sameTarget(state.catalogTarget, t)) return '';
      return state.cliInPane;
    }
    function launchCli(text) {
      const exe = String(text).replace(/^(?:[A-Z_]+=\S+\s+)+/, '').split(' ')[0];
      return ((state.catalog && state.catalog.clis) || []).find(c => c.binary === exe)?.id || '';
    }
    const clis = () => (state.catalog && Array.isArray(state.catalog.clis)) ? state.catalog.clis : [];

    function applyCatalog(payload) {
      state.catalog = payload.catalog || null;
      state.cliInPane = payload.cliInPane || '';
      state.catalogTarget = payload.target || null;
      let changed = false;
      // Todos los CLI arrancan cerrados (pedido de Jesús, 30-sep): solo
      // «Cadenas guardadas» se abre la primera vez; el CLI del pane se marca
      // con su badge, pero no se despliega solo.
      if (state.firstRender && state.catalog) {
        state.open.add('saved');
        state.firstRender = false;
        changed = true;
      }
      const t = target();
      const applied = state.catalogTarget || (t ? { session: t.session, pane: t.pane } : null);
      state.appliedTarget = applied ? { session: applied.session, pane: applied.pane } : null;
      if (changed) writeOpen();
      render();
    }

    async function refresh() {
      // Sin destino se pide el catálogo sin pane: se ve, pero todo queda .dis.
      const t = target();
      const [cat, ch] = await Promise.all([
        api(t && t.session && t.pane
          ? `/commands/catalog?session=${encodeURIComponent(t.session)}&pane=${encodeURIComponent(t.pane)}`
          : '/commands/catalog'),
        api('/chains'),
      ]);
      state.chains = Array.isArray(ch && ch.chains) ? ch.chains : [];
      applyCatalog(cat || {});
    }

    const setBusy = on => { try { el.classList && el.classList.toggle('typing', on); } catch (_) {} };

    // Único punto que llama a /pane/type. Devuelve una promesa que nunca
    // rechaza: {ok, result} o {ok: false, error}. `after` corre antes de liberar
    // state.typing para que quien espere la promesa vea el estado final.
    function typeInto(tgt, text, kind, after) {
      const body = { session: tgt.session, pane: tgt.pane, text, requestId: makeId() };
      const p = (async () => {
        await null;
        let res;
        try {
          res = { ok: true, result: await api('/pane/type', body) };
          if (kind === 'shell') { const cli = launchCli(text); if (cli) write(prefKey(tgt), cli); }
        } catch (err) {
          res = { ok: false, error: (err && err.message) || 'No se pudo escribir en el pane' };
          toast(res.error, true);
        }
        try { if (after) after(res); } finally {
          if (state.typing === p) { state.typing = null; setBusy(false); }
        }
        return res;
      })();
      state.typing = p;
      setBusy(true);
      return p;
    }

    function insert(text, kind = 'pane') {
      if (state.typing) { toast(WAIT); return null; }
      const tgt = target();
      if (!tgt || !tgt.session || !tgt.pane) { toast('Selecciona un pane primero', true); return null; }
      text = String(text ?? '');
      if (!text || CONTROL_RE.test(text)) { toast('El comando no puede llevar saltos de línea', true); return null; }
      if (kind === 'pane' && !hereCli()) return null;   // sin CLI en el destino: /… no aplica
      return typeInto({ ...tgt }, text, kind === 'shell' ? 'shell' : 'pane');
    }

    // Los archivos .md se editan a mano: antes de correr se releen del disco
    // (si falla la lectura se usa la copia en memoria).
    async function reloadChains() {
      try {
        const ch = await api('/chains');
        if (ch && Array.isArray(ch.chains)) state.chains = ch.chains;
      } catch (_) {}
      return state.chains;
    }
    function beginRun(chain, tgt) {
      state.run = { slug: chain.slug, name: chain.name || chain.slug, steps: chain.steps.map(s => ({ ...s })),
        step: 0, target: { ...tgt }, error: '' };
      // el avance de la cadena vive en la pestaña Cadenas: se abre para verlo
      state.sheet = 'chains';
      render();
      return state.run;
    }
    function startChain(slug) {
      const tgt = target();
      if (!tgt || !tgt.session || !tgt.pane) { toast('Selecciona un pane primero', true); return null; }
      const runnable = c => c && !c.error && Array.isArray(c.steps) && c.steps.length;
      return reloadChains().then(chains => {
        const chain = chains.find(c => c.slug === slug);
        if (!runnable(chain)) { toast('Esa cadena no se puede correr', true); render(); return null; }
        return beginRun(chain, tgt);
      });
    }

    function next() {
      const run = state.run;
      if (!run || run.step >= run.steps.length) return null;
      if (state.typing) { toast(WAIT); return null; }
      const step = run.steps[run.step];
      if (CONTROL_RE.test(String(step.text ?? ''))) { toast('El paso tiene saltos de línea', true); return null; }
      return typeInto(run.target, String(step.text), step.kind === 'shell' ? 'shell' : 'pane', res => {
        if (state.run !== run) return;
        if (res.ok) { run.step += 1; run.error = ''; } else { run.error = res.error; }
        render();
      });
    }

    function stop() { state.run = null; render(); }

    function toggle(key, keepFocus) {
      if (state.open.has(key)) state.open.delete(key);
      else state.open.add(key);
      writeOpen();
      render();
      if (keepFocus) {   // el repintado del cuerpo suelta el foco: se devuelve al mismo encabezado
        const h = [...el.querySelectorAll('[data-toggle]')].find(n => n.dataset.toggle === key);
        if (h && h.focus) h.focus();
      }
    }

    function targetTitle() {
      const t = target();
      return t ? (t.title || `${t.session} ${t.pane}`) : 'Sin destino';
    }
    // Píldora del mockup: estado del catálogo del CLI del destino (ámbar si su
    // binario cambió y no están todos sus comandos; «catálogo ok» si no).
    function catalogPill() {
      const here = hereCli();
      const c = clis().find(x => x.id === here);
      const v = (c && c.version) || {}, d = c && c.detected;
      const bad = !!c && v.status !== 'ok' && v.status !== 'missing' && !(d && d.total && d.found === d.total);
      return bad ? `<span class="pill warn">${ic('bomb', 's20')}cambio de versión</span>`
                 : `<span class="pill">${ic('spark', 's20')}catálogo ok</span>`;
    }
    const SHEETS = [['cmds', 'Comandos'], ['chains', 'Cadenas'], ['srv', 'Servidores']];
    const sheetIcon = k => k === 'srv' ? '<span class="sic" data-icon="server" data-size="15" aria-hidden="true"></span>' : ic(k === 'cmds' ? 'book' : 'chain', 's20');
    // Fila de la barra: abre/cierra cada pestaña del panel; el número de Cadenas es el de guardadas.
    function toolsHTML() {
      return '<div class="cs-tools" role="toolbar" aria-label="Comandos, cadenas y servidores">' + SHEETS.map(([k, l]) =>
        `<button type="button" data-flat class="tb" data-sheet="${k}" aria-pressed="false">${sheetIcon(k)}<span class="lb">${l}</span>`
        + (k === 'chains' ? '<small class="n"></small>' : '') + '</button>').join('') + '</div>';
    }
    function headHTML() {
      return `<div class="cs-head"><div class="sh-h"><span class="sh-t"></span>`
        + `<button type="button" data-flat class="cls" data-sheet-close title="Cerrar (Esc)">Cerrar <kbd>Esc</kbd></button></div>`
        + `<div class="cs-dest">destino: <span class="cs-target">${esc(targetTitle())}</span><span class="cs-pill">${catalogPill()}</span></div>`
        + `<div class="search">${ic('lens')}<input class="cs-search" type="search" placeholder="Buscar en todos los CLI…" value="${esc(state.q)}"></div></div>`;
    }

    function savedHTML() {
      const items = state.chains.map(c => c.error
        ? `<div class="cs-saved-item it error"><span class="nm">${esc(c.name || c.slug)}</span><small>${esc(c.error)}</small></div>`
        : `<div class="cs-saved-item it" data-run="${esc(c.slug)}"><span class="nm">${esc(c.name || c.slug)}</span>`
          + `<small>${(c.steps || []).length} pasos</small><button type="button" data-flat class="run">${ic('cast', 's20')}Correr</button></div>`).join('');
      const add = `<button type="button" data-flat class="cs-chains chains-btn" data-open-builder>${ic('chain', 's20')}Nueva cadena</button>`;
      return items
        ? `<div class="cs-saved saved open">${items}</div><div class="cs-chain-add">${add}</div>`
        : `<div class="cs-empty-chains"><b>Aún no hay cadenas guardadas</b>Una cadena escribe varios pasos seguidos en un pane, por ejemplo «/compact» y luego «continúa».${add}</div>`;
    }

    function runnerHTML() {
      const run = state.run;
      if (!run) return '';
      const n = run.steps.length, done = run.step >= n;
      return `<div class="cs-runner runner${done ? ' done' : ''}">`
        + `<div class="r-h">${done ? '<i class="px chest"></i>' : '<i class="px hour sm"></i>'}${esc(run.name)}<small>${done ? 'completa' : `paso ${run.step + 1} de ${n}`}</small>`
        + `<span class="right"><button type="button" data-flat class="ghost" data-run-stop>${done ? 'Cerrar' : 'Parar'}</button></span></div>`
        + `<div class="r-t">en ${esc(run.target.title || `${run.target.session} ${run.target.pane}`)}</div>`
        + '<div class="steps">' + run.steps.map((s, i) =>
          `<div class="step${i < run.step ? ' done' : ''}${i === run.step ? ' cur' : ''}" data-kind="${esc(s.kind)}"><span class="k">${esc(s.kind)}</span><code>${esc(s.text)}</code></div>`).join('') + '</div>'
        + (run.error ? `<div class="r-err">${esc(run.error)}</div>` : '')
        + (done ? '' : `<div class="acts"><button type="button" data-flat class="cs-next primary" data-run-next>${ic('cast', 's20')}${run.step === 0 ? 'Escribir paso 1' : 'Siguiente'}</button><small>Se escribe sin Enter; tú das Enter.</small></div>`) + '</div>';
    }

    function termList() { try { return terminals() || []; } catch (_) { return []; } }
    // Pila aprobada (ronda 5 A): comandos arriba, terminales rápidas abajo con
    // separador. La tira de pestañas se repinta; .mini es estable y aloja el iframe
    // de la terminal elegida (mountTerm), así el repintado no la reinicia.
    // Mockup: la pestaña de una terminal rápida se llama por su hora («14:32»);
    // la carpeta T-AAAA-MM-DD-HH-MM-SS queda en el title.
    const shortTerm = l => { const m = /^T-\d{4}-\d{2}-\d{2}-(\d{2})-(\d{2})-\d{2}$/.exec(String(l)); return m ? `${m[1]}:${m[2]}` : String(l); };
    // Persiana (elegida en el grill del 1-oct): chevron en su propio hueco a la derecha;
    // gira al esconder y la cabecera se ve como persiana bajada (CSS .terms-hidden).
    const CHEVRON = '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>';
    function termsHTML() {
      const t = target();
      const cur = x => !!t && (t.paneKey && x.paneKey ? t.paneKey === x.paneKey : sameTarget(t, x));
      const list = termList();
      const shown = state.curTerm && list.some(x => x.tabId === state.curTerm) ? state.curTerm : (list[0] ? list[0].tabId : '');
      if (shown !== state.curTerm) state.curTerm = shown;
      const armed = id => state.closeArm === String(id);
      // Píldoras con flechas (grill ronda 2, 1-oct): punto de estado + nombre + ✕.
      return list.map(x => `<span class="tw${x.tabId === shown ? ' on' : ''}${cur(x) ? ' sel' : ''}">`
          + `<button type="button" data-flat class="t${x.tabId === shown ? ' on' : ''}${cur(x) ? ' sel' : ''}" data-focus-term="${esc(x.tabId)}" title="${esc(x.label || x.tabId)}">`
          + `<span class="dot" aria-hidden="true"></span>${esc(shortTerm(x.label || x.tabId))}${cur(x) ? ' · destino' : ''}</button>`
          + `<button type="button" data-flat class="tx${armed(x.tabId) ? ' armed' : ''}" data-close-term="${esc(x.tabId)}" aria-label="Cerrar ${esc(shortTerm(x.label || x.tabId))}" title="${armed(x.tabId) ? 'Otro clic la cierra' : 'Cerrar esta terminal'}">${armed(x.tabId) ? '¿Cerrar?' : '✕'}</button></span>`).join('');
    }
    function togHTML() {
      if (!termList().length) return '';
      const h = !!state.termsHidden;
      return `<button type="button" data-flat class="tog" data-terms-toggle aria-pressed="${h ? 'false' : 'true'}" aria-label="${h ? 'Mostrar las terminales' : 'Esconder las terminales'}" title="${h ? 'Mostrar las terminales' : 'Esconder las terminales'}">${CHEVRON}</button>`;
    }
    // Las mismas pestañas como datos: el escritorio pinta esta cabecera en GTK, encima
    // de su terminal nativa (mountTerm recibe esto en su tercer argumento).
    function termTabs() {
      const t = target();
      const cur = x => !!t && (t.paneKey && x.paneKey ? t.paneKey === x.paneKey : sameTarget(t, x));
      return termList().map(x => ({ id: String(x.tabId), label: shortTerm(x.label || x.tabId) + (cur(x) ? ' · destino' : ''),
        title: String(x.label || x.tabId), on: x.tabId === state.curTerm, sel: cur(x), closing: state.closeArm === String(x.tabId) }));
    }

    function bodyHTML() {
      const here = hereCli(), t = target(), noTarget = !t || !t.session || !t.pane;
      return clis().map(c => cliHTML(c, { mode: 'run', open: state.open, here: !!here && c.id === here, noCli: !here, noTarget, q: state.q })).join('');
    }

    // La cabecera (con el input de búsqueda) se pinta una sola vez; después solo
    // se actualiza el título del destino y se repinta .cs-body, así el input
    // conserva foco, selección y composición (IME, teclas muertas).
    function render() {
      const head = el.querySelector('.cs-head'), body = el.querySelector('.cs-body');
      if (head && body) {
        const tgt = head.querySelector('.cs-target');
        if (tgt) tgt.textContent = targetTitle();
        const pill = head.querySelector('.cs-pill');
        if (pill) pill.innerHTML = catalogPill();   // hydrate(el) al final del render pinta su icono
        body.innerHTML = bodyHTML();
        const cb = el.querySelector('.cs-chains-body');
        if (cb) cb.innerHTML = runnerHTML() + savedHTML();
      } else {
        // Una sola cabecera para las terminales: agarre + pestañas + «+ Terminal» + ▾.
        // Arrastrarla (fuera de los botones) cambia la altura; ya no hay separador aparte.
        el.innerHTML = toolsHTML()
          + `<div class="sec-cmds cs-sheet" role="dialog" aria-label="Comandos, cadenas y servidores">${headHTML()}<div class="cs-body">${bodyHTML()}</div>`
          + `<div class="cs-chains-body">${runnerHTML() + savedHTML()}</div><div class="cs-srv"><div class="cs-srv-slot"></div></div></div>`
          + '<div class="cs-empty-terms"><div><b class="et-t"></b><span class="et-d"></span>'
          + '<button type="button" data-flat class="et-go"></button></div></div>'
          + '<div class="sec-terms"><div class="cs-terms tt" role="group" aria-label="Terminales de la barra">'
          + '<span class="grip" aria-hidden="true"></span>'
          + '<button type="button" data-flat class="arr" data-tscroll="-1" aria-label="Terminales anteriores" hidden>‹</button>'
          + '<div class="tabs"></div>'
          + '<button type="button" data-flat class="arr" data-tscroll="1" aria-label="Más terminales" hidden>›</button>'
          + '<button type="button" data-flat class="t plus" data-new-term aria-label="Nueva terminal" title="Nueva terminal">+</button>'
          + '<div class="tog-slot"></div></div><div class="mini"></div></div>';
        wireTrack();
      }
      const tt = el.querySelector('.cs-terms .tabs') || el.querySelector('.cs-terms');
      if (tt) {
        // repintar no debe devolver la pista al inicio; si cambió la activa, se lleva a la vista
        const keep = tt.scrollLeft || 0, was = state.shownTerm;
        tt.innerHTML = termsHTML();
        tt.scrollLeft = keep;
        state.shownTerm = state.curTerm;
        if (was !== state.curTerm) { const on = tt.querySelector && tt.querySelector('.tw.on'); try { on && on.scrollIntoView({ block: 'nearest', inline: 'nearest' }); } catch (_) {} }
        syncArrows();
      }
      const slot = el.querySelector('.cs-terms .tog-slot');
      if (slot) slot.innerHTML = togHTML();
      const n = termList().length;
      paintSheet(n);
      try { el.classList && el.classList.toggle('no-terms', !n); } catch (_) {}
      try { el.classList && el.classList.toggle('terms-hidden', !!state.termsHidden); } catch (_) {}
      applyHeights();
      const mini = el.querySelector('.mini');
      if (mini && typeof mountTerm === 'function') {
        try { mountTerm(state.termsHidden ? '' : (state.curTerm || ''), mini, { session: state.curTerm || '', hidden: !!state.termsHidden, tabs: termTabs(), cmds: cmdsInfo() }); } catch (_) {}
      }
      try { el.classList && el.classList.toggle('searching', !!state.q.trim()); } catch (_) {}
      try { hydrate(el); } catch (_) {}   // pinta los data-icon del marcado recién puesto
    }

    // Flechas ‹ › de la pista: solo se ven si las píldoras no caben y se apagan en
    // cada extremo; el borde se desvanece donde hay más (.at-start/.at-end). La rueda
    // vertical también desplaza en horizontal.
    function syncArrows() {
      const tr = el.querySelector('.cs-terms .tabs');
      if (!tr || tr.scrollWidth == null) return;
      // barra muy angosta: sin flechas, la pista se queda con el espacio (rueda y toque siguen)
      const head = tr.parentNode, narrow = !!head && head.clientWidth > 0 && head.clientWidth < 250;
      const over = !narrow && tr.scrollWidth > tr.clientWidth + 2;
      const atS = tr.scrollLeft <= 2, atE = tr.scrollLeft + tr.clientWidth >= tr.scrollWidth - 2;
      try { tr.classList.toggle('at-start', atS); tr.classList.toggle('at-end', atE); } catch (_) {}
      for (const b of el.querySelectorAll('.cs-terms [data-tscroll]')) {
        b.hidden = !over;
        b.disabled = b.dataset.tscroll === '-1' ? atS : atE;
      }
    }
    function wireTrack() {
      const tr = el.querySelector('.cs-terms .tabs');
      if (!tr || !tr.addEventListener) return;
      tr.addEventListener('scroll', syncArrows);
      tr.addEventListener('wheel', e => {
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX) && tr.scrollWidth > tr.clientWidth) { tr.scrollLeft += e.deltaY; e.preventDefault(); }
      }, { passive: false });
      const win = el.ownerDocument && el.ownerDocument.defaultView;
      try { if (win && win.ResizeObserver) new win.ResizeObserver(syncArrows).observe(tr); } catch (_) {}
    }

    // La altura arrastrada va en línea y le ganaría a las reglas .terms-hidden/.no-terms:
    // escondidas (o sin terminales) se quita y los comandos toman todo; al mostrar vuelve.
    // Restos de la altura arrastrada (antes del panel flotante): fuera.
    function applyHeights() {
      const cmds = el.querySelector('.sec-cmds'), terms = el.querySelector('.sec-terms');
      if (!cmds || !terms || !cmds.style) return;
      cmds.style.flex = ''; terms.style.flex = '';
    }

    // Acciones de las pestañas, compartidas por los clics de aquí y por la cabecera
    // nativa del escritorio (termAction).
    function setHidden(v) { state.termsHidden = !!v; write(KEY_TERMS_HIDDEN, state.termsHidden ? '1' : '0'); }
    function focusTerm(id) {
      state.curTerm = String(id);
      if (state.termsHidden) setHidden(false);
      const x = termList().find(q => String(q.tabId) === String(id));
      if (x) focusTarget({ kind: 'term', tabId: x.tabId, paneKey: x.paneKey, session: x.session, pane: x.pane, title: x.label || x.tabId });
      render();
    }
    // ✕ de una pestaña: el primer clic pide confirmación («¿Cerrar?» 3 s), el segundo
    // termina esa terminal (su sesión tmux, por nombre exacto).
    let armTimer = 0;
    function closeTerm(id) {
      id = String(id);
      clearTimeout(armTimer);
      if (state.closeArm !== id) {
        state.closeArm = id;
        armTimer = setTimeout(() => { if (state.closeArm === id) { state.closeArm = null; render(); } }, 3000);
        return void render();
      }
      state.closeArm = null;
      if (state.curTerm === id) state.curTerm = '';
      render();
      if (typeof killTerm === 'function') { try { Promise.resolve(killTerm(id)).catch(err => toast((err && err.message) || String(err), true)); } catch (err) { toast(err.message, true); } }
    }
    // Estado del panel para la web y para el escritorio. Cerrado, el WebView del escritorio
    // mide hasta el borde inferior de la fila de la barra (h) y la terminal nativa toma el
    // resto; abierto, el WebView ocupa la columna y la terminal nativa se oculta.
    function cmdsInfo() {
      let h = 0;
      try {
        const row = el.querySelector('.cs-tools');
        const r = row && row.getBoundingClientRect ? row.getBoundingClientRect() : null;
        const win = el.ownerDocument && el.ownerDocument.defaultView;
        if (r && r.height) h = Math.ceil(r.bottom + ((win && win.scrollY) || 0));
      } catch (_) {}
      return { open: !!state.sheet, sheet: state.sheet, h, empty: !!emptyTerms() };
    }
    // Sin terminal que mostrar (escondidas o ninguna): la columna no se queda vacía.
    function emptyTerms() {
      if (!termList().length) return { t: 'No hay terminales en la barra', d: 'Abre una para tener una shell a mano junto a tus sesiones.', go: '+ Nueva terminal', act: 'new' };
      if (state.termsHidden) return { t: 'Terminales escondidas', d: 'Siguen abiertas; vuelve a mostrarlas cuando las necesites.', go: 'Mostrar terminal', act: 'toggle' };
      return null;
    }
    function paintSheet(n) {
      const cur = state.sheet;
      try { el.classList.toggle('sheet-open', !!cur); el.setAttribute('data-panel', cur); } catch (_) {}
      for (const b of el.querySelectorAll('[data-sheet]')) {
        const on = b.dataset.sheet === cur;
        try { b.classList.toggle('on', on); b.setAttribute('aria-pressed', on ? 'true' : 'false'); } catch (_) {}
      }
      const ttl = el.querySelector('.cs-head .sh-t');
      if (ttl) ttl.textContent = ({ cmds: 'Comandos del pane', chains: 'Cadenas guardadas', srv: 'Servidores SSH' })[cur] || '';
      const cnt = el.querySelector('.cs-tools .n');
      if (cnt) cnt.textContent = state.chains.length ? String(state.chains.length) : '';
      const sheet = el.querySelector('.cs-sheet');
      if (sheet) sheet.hidden = !cur;
      const e = emptyTerms(), box = el.querySelector('.cs-empty-terms');
      if (box) {
        box.hidden = !e || !!cur;
        if (e) {
          box.querySelector('.et-t').textContent = e.t; box.querySelector('.et-d').textContent = e.d;
          const go = box.querySelector('.et-go'); go.textContent = e.go; go.setAttribute('data-term-act', e.act);
        }
      }
      if (cur === 'srv') { state.srvMounted = true; try { mountServers(el.querySelector('.cs-srv-slot')); } catch (_) {} }
      else if (state.srvMounted) { state.srvMounted = false; try { mountServers(null); } catch (_) {} }
      const tools = el.querySelector('.cs-tools');
      if (tools && tools.offsetHeight && el.style && el.style.setProperty) el.style.setProperty('--cs-tools-h', tools.offsetHeight + 'px');
    }
    function setSheet(k) {
      const next = SHEETS.some(x => x[0] === k) ? k : '';
      if (next === state.sheet) return;
      state.sheet = next;
      render();
      if (next === 'cmds') { const q = el.querySelector('.cs-search'); try { q && q.focus && q.focus(); } catch (_) {} }
    }
    const setCmdsOpen = v => setSheet(v ? (state.sheet || 'cmds') : '');
    function termAction(kind, id) {
      if (kind === 'close' && id) return void closeTerm(id);
      if (kind === 'new') { if (state.termsHidden) setHidden(false); return void newTerm(); }
      if (kind === 'toggle') { setHidden(!state.termsHidden); return void render(); }
      if (kind === 'focus' && id) return void focusTerm(id);
    }

    el.addEventListener('click', e => {
      const t = e.target;
      if (!t || typeof t.closest !== 'function') return;
      let n;
      if (t.closest('[data-run-next]')) return void next();
      if (t.closest('[data-run-stop]')) return void stop();
      if (t.closest('[data-open-builder]')) return void openBuilder();
      if ((n = t.closest('[data-sheet]'))) return void setSheet(state.sheet === n.dataset.sheet ? '' : n.dataset.sheet);
      if (t.closest('[data-sheet-close]')) return void setSheet('');
      if ((n = t.closest('[data-term-act]'))) return void termAction(n.dataset.termAct);
      if ((n = t.closest('[data-close-term]'))) return void closeTerm(n.dataset.closeTerm);
      if ((n = t.closest('[data-tscroll]'))) {
        const tr = el.querySelector('.cs-terms .tabs');
        if (tr) { tr.scrollLeft += Number(n.dataset.tscroll) * 150; syncArrows(); }
        return;
      }
      if (t.closest('[data-new-term]')) return void termAction('new');
      if (t.closest('[data-terms-toggle]')) return void termAction('toggle');
      if ((n = t.closest('[data-focus-term]'))) return void focusTerm(n.dataset.focusTerm);
      if ((n = t.closest('[data-cmd]'))) {
        const row = n.closest('.cmd');
        if (row && row.classList.contains('dis')) {
          const t = target();
          if (!t || !t.session || !t.pane) toast('Selecciona un pane primero', true);
          return;
        }
        const r = insert(n.dataset.cmd, n.dataset.kind === 'shell' ? 'shell' : 'pane');
        if (r) setSheet('');   // escrito en el pane (sin Enter): el panel se aparta
        return;
      }
      if (t.closest('button') && (n = t.closest('.cs-saved-item[data-run]'))) return void startChain(n.dataset.run);
      if ((n = t.closest('[data-toggle]'))) return void toggle(n.dataset.toggle);
    });
    el.addEventListener('keydown', e => {
      if (e.key === 'Escape' && state.sheet) { if (e.preventDefault) e.preventDefault(); return void setSheet(''); }
      const t = e.target, n = t && typeof t.closest === 'function' ? t.closest('[data-toggle]') : null;
      if (!n || n !== t || !isToggleKey(e)) return;
      if (e.preventDefault) e.preventDefault();   // Espacio no debe desplazar la lista
      toggle(n.dataset.toggle, true);
    });
    const isSearch = t => !!(t && t.classList && t.classList.contains('cs-search'));
    const search = t => { state.q = String(t.value ?? ''); render(); };
    el.addEventListener('input', e => { if (isSearch(e.target) && !e.isComposing) search(e.target); });
    el.addEventListener('compositionend', e => { if (isSearch(e.target)) search(e.target); });

    return { refresh, render, insert, startChain, next, stop, applyCatalog, termAction, setCmdsOpen, setSheet, get state() { return state; } };
  }

  root.ComandosCommandSidebar = { createCommandSidebar, rowHTML, cliHTML, esc, isToggleKey };
  if (typeof module !== 'undefined' && module.exports) module.exports = { createCommandSidebar, rowHTML, cliHTML, esc, isToggleKey };
})(typeof window !== 'undefined' ? window : globalThis);
