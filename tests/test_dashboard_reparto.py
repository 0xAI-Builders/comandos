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


def test_el_tanque_avisa_de_la_cuota_mas_apretada():
    """claude main sale al 85 % con su semanal por modelo al 100 %: hay que verlo."""
    assert "function tightest(" in JS, "falta el cálculo de la fila más tensa"
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
