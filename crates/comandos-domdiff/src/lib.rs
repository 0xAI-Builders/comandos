//! DOM normalizado: etiquetas, atributos ordenados por nombre (el valor de
//! `class` conserva su orden, que sí cambia el CSS), texto con espacios
//! colapsados; los nodos de solo espacios desaparecen. Una línea por nodo.
//!
//! Vive fuera de `xtask` (preflight R12) para que las pruebas de vista, que
//! compilan a wasm32, lo usen sin arrastrar las dependencias del arnés.
use scraper::{Html, Node};
use std::fmt::Write as _;

/// Forma canónica de un fragmento HTML, una línea por nodo con sangría.
pub fn normalize(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    let mut out = String::new();
    walk(doc.tree.root(), 0, &mut out);
    out
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn walk(node: ego_tree::NodeRef<'_, Node>, depth: usize, out: &mut String) {
    for child in node.children() {
        match child.value() {
            Node::Element(el) => {
                let mut attrs: Vec<(String, String)> = el
                    .attrs()
                    .map(|(k, v)| (k.to_string(), collapse(v)))
                    .collect();
                attrs.sort();
                out.push_str(&"  ".repeat(depth));
                out.push('<');
                out.push_str(el.name());
                for (k, v) in attrs {
                    // Escribir en un String no falla.
                    let _ = write!(out, " {k}={v:?}");
                }
                out.push_str(">\n");
                walk(child, depth + 1, out);
            }
            Node::Text(t) => {
                let text = collapse(t);
                if !text.is_empty() {
                    out.push_str(&"  ".repeat(depth));
                    let _ = writeln!(out, "#text {text:?}");
                }
            }
            // Comentarios, doctype e instrucciones no cuentan; sus hijos sí.
            _ => walk(child, depth, out),
        }
    }
}

/// Primera línea distinta de las dos formas normalizadas, con su número de
/// nodo; `None` si son iguales.
pub fn first_difference(a: &str, b: &str) -> Option<String> {
    let (na, nb) = (normalize(a), normalize(b));
    let mut la = na.lines();
    let mut lb = nb.lines();
    let mut n = 0usize;
    loop {
        n += 1;
        match (la.next(), lb.next()) {
            (None, None) => return None,
            (x, y) if x == y => {}
            (x, y) => {
                return Some(format!(
                    "nodo {n}: {:?} ≠ {:?}",
                    x.unwrap_or("<fin>"),
                    y.unwrap_or("<fin>")
                ));
            }
        }
    }
}
