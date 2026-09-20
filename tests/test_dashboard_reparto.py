"""La pestaña Reparto: markup, assets, copy y que el JS parsea."""
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash/index.html").read_text()
JS = (ROOT / "dash/reparto.js").read_text()
CSS = (ROOT / "dash/reparto.css").read_text()


def test_la_pestana_reparto_sustituye_a_optimizar():
    assert 'data-mtab="reparto"' in HTML and 'data-mpane="reparto"' in HTML
    assert 'id="reparto"' in HTML
    assert 'data-mtab="optimizar"' not in HTML and 'data-mpane="optimizar"' not in HTML


def test_no_queda_codigo_muerto_de_optimizar():
    for muerto in ("loadOptimize", "renderOptimize", "applyOptimization", "optimizeSessions", "OPTIMIZE"):
        assert muerto not in HTML, muerto


def test_los_assets_se_cargan():
    assert '/reparto.css' in HTML and '/reparto.js' in HTML
    assert re.search(r'MTAB_GROUPS\s*=\s*\{[^}]*reparto:\s*\["reparto"\]', HTML)
    assert 'if(name === "reparto") window.Reparto?.load();' in HTML


def test_el_js_parsea_y_expone_su_api():
    subprocess.run(["node", "--check", str(ROOT / "dash/reparto.js")], check=True)
    assert "window.Reparto" in JS
    assert "load:" in JS and "render:" in JS


def test_habla_en_cristiano():
    for prohibido in ("aguanta", ">F<", ">M<", ">L<"):
        assert prohibido not in JS, prohibido
    # copia que sí es del navegador
    for esperado in ("ritmo", "Analizar", "Aplicar", "Propuesta original",
                     "Revertir todo", "reintentar", "conservando la conversación"):
        assert esperado in JS, esperado


def test_el_veredicto_de_la_cuota_lo_dicta_el_servidor():
    """Una sola fuente para la copia: el navegador pinta lo que manda allocation.py,
    no vuelve a redactarlo. Si se duplicara, las dos podrían divergir."""
    motor = (ROOT / "lib/allocation.py").read_text()
    assert "llega al reset" in motor and "se acaba en" in motor
    assert "llega al reset" not in JS and "se acaba en " not in JS
    assert "data-verdict" in JS, "el navegador debe pintar el veredicto del servidor"


def test_el_tanque_tiene_sus_dos_capas():
    for pieza in ("rp-base", "rp-liq", "rp-pre", "rp-foam", "rp-full"):
        assert pieza in CSS and pieza in JS, pieza


def test_la_ficha_muestra_modelo_y_effort_y_deja_cambiarlos():
    assert "rp-old" in JS and "rp-arrow" in JS, "falta el diff antes → después"
    assert 'seg(item, "model"' in JS and 'seg(item, "effort"' in JS, "faltan los selectores"
    assert "data-set-key=" in JS and "data-lock=" in JS
    # item.key ya es "sesion|pane": un atributo compuesto con delimitador trunca la
    # clave al leerla y el selector se vuelve un no-op silencioso. Campo y valor
    # viajan en atributos propios.
    assert "data-set-field=" in JS and "data-set-value=" in JS
    assert 'dataset.set.split' not in JS


def test_el_candado_se_puede_soltar():
    """Fijar una ficha no puede esconder el botón que la suelta."""
    assert 'S.phase === "curar"\n        ? \'<button type="button" class="rp-lk ' in JS


def test_aplicando_siempre_ofrece_salida():
    """Si el sondeo se corta a mitad del lote, la pestaña no puede quedarse sin botones."""
    assert "data-repoll" in JS and JS.count("data-repoll") >= 2, "el boton necesita su manejador"
    assert "data-again" in JS


def test_el_angosto_usa_container_queries_y_barra_de_destinos():
    assert "container-type" in CSS and "@container" in CSS
    assert "rp-dropbar" in CSS and "rp-dropbar" in JS


def test_agrupa_por_la_cuota_que_paga_no_por_la_cuenta_del_harness():
    """Ruling de la tarea 3: la cuota se identifica por (motor, motorAccount)."""
    assert "poolKey(i.to.motor, i.to.motorAccount)" in JS
    assert "poolKey(i.to.motor, i.to.harnessAccount)" not in JS


def test_el_plan_caducado_se_explica_en_vez_de_reventar():
    assert "plan_stale" in JS
    assert "Vuelve a analizar" in JS


def test_el_sondeo_se_suelta_al_cerrar_el_panel():
    """Un intervalo huérfano sigue pegándole al servidor con el modal cerrado."""
    assert "function visible()" in JS and "offsetParent" in JS
    assert "if (!visible()) { clearInterval(S.poll)" in JS
    assert 'S.phase === "aplicando" && S.batch && !S.poll) pollBatch()' in JS, \
        "al reabrir hay que reenganchar el lote en vuelo"


def test_dos_analisis_a_la_vez_no_se_pisan():
    """El más lento no puede sobrescribir un plan más nuevo."""
    assert "var mine = ++seq" in JS and "if (mine !== seq) return" in JS


def test_el_tanque_avisa_de_las_otras_cuotas_del_grupo():
    """claude main sale al 95 % de su semana con el tope por modelo al 100 %: si el
    cilindro solo enseñara la semana, esa segunda cuota agotada quedaría invisible.
    Se muestran todas las mediciones del grupo, no solo la peor: estar al 1 % en la
    ventana de 5 h y al 100 % por modelo son situaciones distintas."""
    assert "function others(" in JS, "falta el calculo de las otras mediciones"
    assert "rp-tight" in JS and "rp-tight" in CSS
    assert "weekly_scoped" in JS


def test_el_tactil_puede_hacer_scroll_sobre_las_fichas():
    """touch-action:none deja el panel inmóvil en el móvil con muchas sesiones."""
    assert "touch-action: pan-y" in CSS and "touch-action: none" not in CSS


def test_los_tanques_siguen_visibles_al_arrastrar():
    assert "#reparto.rp-dragging .rp-tanks" in CSS and "position: sticky" in CSS


def test_la_copia_es_bilingue():
    """El panel y el motor hablan los dos idiomas del tablero, no medio y medio."""
    motor = (ROOT / "lib/allocation.py").read_text()
    assert "def _t(lang" in motor, "el motor necesita su puente de idioma"
    assert "lasts to reset" in motor and "runs out in" in motor
    assert "function T(es, en)" in JS
    for es in ("Analizar", "Aplicar ", "Revertir todo", "Volver a analizar"):
        assert 'T("' + es in JS, es


def test_la_ficha_no_ensena_el_nombre_crudo_de_tmux():
    """term-3692-1 no dice nada: la ficha usa la etiqueta del tablero, y la
    carpeta como respaldo antes que el nombre interno."""
    motor = (ROOT / "lib/allocation.py").read_text()
    assert 'project=s.get("project")' in motor, "el motor debe arrastrar la etiqueta"
    assert "function nameOf(item)" in JS
    assert "if (item.project) return item.project" in JS
    assert "esc(nameOf(item))" in JS, "la ficha debe pintar nameOf, no item.session"
    assert "rp-name\">' + esc(item.session)" not in JS


def test_las_rejillas_no_se_desbordan():
    """`1fr` es minmax(auto,1fr) y `auto` no baja del ancho minimo del contenido.
    Los selectores de modelo y effort son anchos, asi que las columnas de fichas
    crecian mas que las de tanques y la rejilla se salia por la derecha: los
    tanques quedaban desalineados respecto a sus propias fichas."""
    import re
    for sel in (r"\.rp-tanks", r"\.rp-bins"):
        for m in re.finditer(sel + r"[^{]*\{[^}]*grid-template-columns:\s*([^;]+);", CSS):
            cols = m.group(1).strip()
            if "repeat" not in cols:
                continue
            assert "minmax(0" in cols, f"{sel} usa {cols}: debe ser minmax(0, 1fr)"
    tk = re.search(r"\.rp-tk \{[^}]*\}", CSS).group(0)
    assert "min-width: 0" in tk, "la ficha tiene que poder encogerse"
    seg = re.search(r"\.rp-seg \{[^}]*\}", CSS).group(0)
    assert "flex-wrap: wrap" in seg, "los botones deben partir en varias lineas, no empujar"


def test_la_capsula_del_tanque_no_se_solapa():
    """El distintivo flotaba y su borde cruzaba el veredicto por la mitad."""
    assert "float: right" not in CSS
    assert "rp-top" in CSS and "rp-top" in JS
    assert "linear-gradient" in CSS, "el texto va sobre el liquido y necesita velo"


def test_el_candado_no_es_un_emoji():
    """Un emoji se pinta con su propio color e ignora `color`, asi que fijada y
    libre se veian identicas."""
    assert "\U0001F512" not in JS
    assert "function lockIcon()" in JS and "lockIcon()" in JS


def test_el_numero_grande_del_tanque_es_lo_gastado_no_la_proyeccion():
    """usedAfter es la proyeccion al reset TOPADA a 100: con codex al 37 % real el
    tanque mostraba 100 % y se leia como el nivel actual. El numero grande pasa a
    ser el hecho (lo gastado) y la proyeccion va aparte y marcada con flecha."""
    assert "Math.round(p.used) + \"%</b>\"" in JS, "el numero grande debe ser p.used"
    assert 'class="rp-lvl rp-num"><b>' in JS
    assert "rp-proj" in JS and "rp-proj" in CSS, "la proyeccion necesita su propio hueco"
    assert 'T("usado", "used")' in JS, "hay que decir de que es el porcentaje"
    # La proyeccion solo aparece cuando difiere de lo gastado.
    assert "Math.abs(usedAfter - p.used) >= 1" in JS


def test_la_vista_previa_no_toca_lo_gastado():
    """Arrastrar cambia la proyeccion, nunca lo que ya se gasto."""
    import re
    paint = re.search(r"function paintImpact\(impact\) \{.*?\n  \}", JS, re.S).group(0)
    assert ".rp-lvl b" in paint, "la previa lee lo gastado para comparar"
    assert 'lvl.textContent = Math.round(used) + "%"' not in paint, \
        "la previa no puede sobrescribir lo gastado con la proyeccion"


def test_el_cilindro_dice_cuando_se_renueva_la_cuota():
    """"reset en 34 h" dice cuanto falta; para planear hace falta cuando."""
    assert "function fmtWhen(" in JS
    assert "toLocaleString" in JS and "weekday" in JS
    assert 'T("se renueva el ", "renews on ")' in JS, "tambien en el globo de ayuda"
    assert "rp-reset em" in CSS, "la fecha va en su propia linea"


def test_cada_medicion_del_cilindro_lleva_su_nombre():
    """Claude mide tres limites por cuenta (semana, tope por modelo, ventana de
    5 h) y Codex y Grok solo uno. Sin nombrarlos el numero es indescifrable."""
    assert "function windowName(" in JS
    for es, en in (("semana", "week"), ("modelo", "model"), ("5 h", "5 h")):
        assert 'T("' + es + '", "' + en + '")' in JS, es
    # El numero grande se etiqueta con lo que mide, no con un "usado" generico.
    assert "esc(p.unit || " in JS
    # Se muestran todas las mediciones del grupo, no solo la peor.
    assert "function others(" in JS and "p.others" in JS


def test_hay_glosario_de_las_mediciones():
    assert "rp-gloss" in JS and "rp-gloss" in CSS
    assert "presupuesto semanal de la cuenta" in JS
    assert "tope semanal de un modelo concreto" in JS
    assert "ventana corta que te frena ahora" in JS


def test_la_ficha_dice_su_estado_y_cuanto_lleva_quieta():
    """Dos paneles del mismo proyecto (uno terminado hace 17 h, otro trabajando)
    se veian identicos salvo por un punto de color, y pasaban por duplicados
    muertos. Ahora el estado va en palabras y con la antiguedad."""
    assert "function stateHtml(" in JS and "rp-state" in JS and "rp-state" in CSS
    for st in ("working", "waiting", "done", "idle"):
        assert st in JS, st
    assert "item.seenAt" in JS, "hace falta cuando se vio por ultima vez"
    assert "rp-quiet" in JS and "rp-quiet" in CSS, "las paradas se apagan un poco"


def test_el_plan_arrastra_cuando_se_vio_cada_sesion():
    dash = (ROOT / "bin/cc-dash").read_text()
    assert 'item["seenAt"]' in dash
    assert "float(s.get(\"ts\") or 0)" in dash
    assert "except (TypeError, ValueError)" in dash, "un ts corrupto no puede tumbar el plan"


def test_analytics_dice_cuando_se_renueva_cada_cuota():
    """El hero de Resumen solo decia "12h 5m al reset": cuanto falta, no cuando.
    Para planear el dia hace falta la fecha, y por cuenta."""
    dash = (ROOT / "dash/index.html").read_text()
    assert "function fmtStamp(" in dash
    assert 'weekday:"short", day:"numeric", month:"short"' in dash
    assert 'hour12:false' in dash, "con la UI en espanol no puede salir reloj de 12 h"
    assert 'L === "en" ? "en" : "es"' in dash, "la fecha sigue al idioma del tablero"
    # En la fila y en la cabecera de cada cuenta.
    assert 'al reset","to reset")}<em>${mdEsc(fmtStamp(l.resets_at))}</em>' in dash
    assert "qh-gwhen" in dash and 'tf("se renueva","renews")' in dash
    assert "qh-gsum" in dash, "cada cuenta resume su ventana mas apretada"


def test_las_tarjetas_de_limites_ensenan_la_fecha_del_reset():
    """El hero y las tarjetas son dos bloques distintos de Resumen. La tarjeta
    solo decia "en 11 h" y escondia la fecha en el globo de ayuda, asi que la
    fecha seguia sin verse aunque el hero ya la tuviera."""
    dash = (ROOT / "dash/index.html").read_text()
    i = dash.index("const resetHtml")
    chip = dash[i:i + 400]
    assert "fmtStamp(l.resets_at)" in chip, "la fecha tiene que estar en el chip, no solo en el title"
    assert "<em>" in chip
    assert ".uw-reset em{" in dash


def test_codex_declara_su_cuenta():
    """El tablero agrupa por (proveedor, cuenta); sin el campo, la tarjeta de
    Codex salia sin cuenta mientras Claude y Grok si la traen."""
    usage = (ROOT / "bin/cc_usage.py").read_text()
    i = usage.index('"provider": "codex"')
    assert '"account": "main"' in usage[i:i + 400]


def test_codex_muestra_sus_tokens_medidos():
    """Codex no reporta tokens en su fila de limite, pero SI se miden aqui
    (codex_weekly_tokens / codex_daily_tokens). Sin engancharlos, su tarjeta
    salia mas pobre que la de Grok pese a existir el dato."""
    usage = (ROOT / "bin/cc_usage.py").read_text()
    dash = (ROOT / "bin/cc-dash").read_text()
    assert "def attach_token_counts(" in usage
    assert "cc_usage.attach_token_counts(" in dash, "hay que llamarlo donde conviven limites y windows"
    # Nunca pisar lo que el proveedor ya reporto (Grok trae los suyos).
    i = usage.index("def attach_token_counts(")
    assert "if value and not row.get(key)" in usage[i:i + 1400]


def test_el_subtexto_no_inventa_ceros():
    """Codex mide tokens pero no turnos: "0 turnos en 7d" es ruido, no un dato."""
    dash = (ROOT / "dash/index.html").read_text()
    i = dash.index("function limitSubText(")
    body = dash[i:i + 700]
    assert "l.turns_7d ?" in body and "filter(Boolean)" in body
    assert "${l.turns_7d || 0}" not in body


def test_la_proyeccion_de_agotamiento_no_se_confunde_con_el_pasado():
    """"se agota Fri 6:15pm" se lee como que YA se agoto. Es una proyeccion al
    ritmo actual, y lo util es compararla con la fecha de renovacion."""
    dash = (ROOT / "dash/index.html").read_text()
    assert 'tf("se agotaría","would run dry")' in dash, "condicional: es una proyeccion"
    assert 'tf("se agota","dry")' not in dash
    assert 'tf("tocaría el 100%","would hit 100%")' in dash
    # La renovacion va junto a la proyeccion, que es la comparacion que decide.
    i = dash.index('se agotaría')
    assert 'tf("Se renueva","Renews")' in dash[i - 400:i + 200]
    # Y con fecha completa, no solo el dia de la semana.
    assert "fmtStamp(fc.projectedExhaustionAt)" in dash


def test_fmtwhen_tambien_sigue_el_idioma_del_tablero():
    dash = (ROOT / "dash/index.html").read_text()
    i = dash.index("function fmtWhen(")
    body = dash[i:i + 400]
    assert 'L === "en" ? "en" : "es"' in body and "hour12:false" in body


def test_la_pestana_explica_su_propio_criterio():
    """Si no se puede auditar como decide, la propuesta es un oraculo y no una
    herramienta. El metodo va en la pestana, no solo en la documentacion."""
    assert "function metodoHtml(" in JS and "rp-how" in JS and "rp-how" in CSS
    assert "metodoHtml()" in JS, "hay que pintarlo, no solo definirlo"
    # Los tres pasos reales del motor, con sus numeros.
    for pieza in ("capa 3", "capa 1", "model-tiers", "bajo 0.5", "1.6", "2.2",
                  "Ritmo 1.0", "reset más lejano", "te dure hasta el reset"):
        assert pieza in JS, pieza
    assert "determinista" in JS
    # Y los pesos citados tienen que ser los que usa el motor de verdad.
    motor = (ROOT / "lib/allocation.py").read_text()
    assert '"low": 0.5, "medium": 1.0, "high": 1.6, "xhigh": 2.2, "max": 3.0' in motor
