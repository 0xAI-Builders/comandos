//! Oráculo Python: carga funciones y constantes de `bin/cc-app` con `ast` (sin
//! ejecutar el módulo, que abriría GTK) y evalúa una expresión. Cero archivos
//! Python nuevos: el programa va como texto a `python3 -c`.
use std::path::PathBuf;
use std::process::Command;

/// `COMANDOS_CC_APP_ORACLE` (checkout principal) o el `bin/cc-app` del worktree.
pub fn cc_app_path() -> PathBuf {
    std::env::var_os("COMANDOS_CC_APP_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-app"))
}

const LOADER: &str = r#"
import ast, json, sys, os, re, shlex
src = open(sys.argv[1]).read()
tree = ast.parse(src)
want = set(json.loads(sys.argv[2]))
ns = {"os": os, "re": re, "json": json, "shlex": shlex, "ES": True}
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.realpath(sys.argv[1]))), "lib"))
for node in tree.body:
    names = []
    if isinstance(node, (ast.FunctionDef, ast.ClassDef)):
        names = [node.name]
    elif isinstance(node, ast.Assign):
        names = [t.id for t in node.targets if isinstance(t, ast.Name)]
    if want & set(names):
        exec(compile(ast.Module([node], []), "cc-app", "exec"), ns)
print(json.dumps(eval(sys.argv[3], ns), ensure_ascii=False))
"#;

/// Define en un espacio de nombres solo los nombres de `defs` (funciones, clases o
/// asignaciones de nivel de módulo, en el orden del archivo) y devuelve el JSON de
/// `expr`. Si `defs` necesita otro nombre, se añade a la lista.
pub fn python_eval(defs: &[&str], expr: &str) -> String {
    let out = Command::new("python3")
        .arg("-c")
        .arg(LOADER)
        .arg(cc_app_path())
        .arg(serde_json::to_string(defs).unwrap())
        .arg(expr)
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}
