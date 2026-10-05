use comandos_web_dom::bridge::{CallError, Scope, call_in};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fun {
    Double,
    Throw,
}

#[derive(Clone, Debug, PartialEq)]
enum V {
    Num(i32),
    Fun(Fun),
}

struct Fake {
    globals: HashMap<&'static str, V>,
    calls: RefCell<Vec<String>>,
}

impl Scope for Fake {
    type Value = V;
    type Error = String;
    fn lookup(&self, name: &str) -> Option<V> {
        self.globals.get(name).cloned()
    }
    fn is_callable(&self, v: &V) -> bool {
        matches!(v, V::Fun(_))
    }
    fn invoke(&self, name: &str, f: &V, args: &[V]) -> Result<V, String> {
        self.calls.borrow_mut().push(name.to_string());
        match (f, args.first()) {
            (V::Fun(Fun::Double), Some(V::Num(n))) => Ok(V::Num(n * 2)),
            (V::Fun(Fun::Double), _) => Err("sin número".into()),
            (V::Fun(Fun::Throw), _) => Err("boom".into()),
            (V::Num(_), _) => Err("no llamable".into()),
        }
    }
}

fn fake() -> Fake {
    let mut globals = HashMap::new();
    globals.insert("dato", V::Num(7));
    globals.insert("doble", V::Fun(Fun::Double));
    globals.insert("lanza", V::Fun(Fun::Throw));
    Fake {
        globals,
        calls: RefCell::new(Vec::new()),
    }
}

#[test]
fn absent_global_is_an_error_not_a_panic() {
    let s = fake();
    assert_eq!(call_in(&s, "noExiste", &[]), Err(CallError::Missing));
    assert!(s.calls.borrow().is_empty());
}

#[test]
fn non_callable_global_is_an_error() {
    let s = fake();
    assert_eq!(call_in(&s, "dato", &[]), Err(CallError::NotCallable));
}

#[test]
fn callable_global_is_called_and_its_exception_is_returned() {
    let s = fake();
    assert_eq!(call_in(&s, "doble", &[V::Num(4)]), Ok(V::Num(8)));
    assert_eq!(
        call_in(&s, "lanza", &[]),
        Err(CallError::Threw("boom".into()))
    );
    assert_eq!(*s.calls.borrow(), ["doble", "lanza"]);
}

#[test]
fn call_errors_read_in_spanish() {
    assert_eq!(
        CallError::<String>::Missing.describe("openTerms"),
        "window.openTerms no existe (¿es un let/const del script en línea?)"
    );
    assert_eq!(
        CallError::<String>::NotCallable.describe("S"),
        "window.S no es una función"
    );
}

#[test]
fn reexporting_a_name_replaces_and_returns_the_old_value() {
    // Revisión B1, I3: el registro de exportaciones va por nombre, así que
    // reexportar suelta la clausura anterior en lugar de acumularla.
    use comandos_web_dom::bridge::Exports;
    let mut e = Exports::default();
    assert_eq!(e.keep("a", 1), None);
    assert_eq!(e.keep("b", 2), None);
    assert_eq!(e.keep("a", 3), Some(1));
    assert_eq!(e.len(), 2);
}
