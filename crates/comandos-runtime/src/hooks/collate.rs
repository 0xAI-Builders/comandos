//! Orden del glob de bash (`strcoll` de glibc) en Rust puro con ICU4X: el locale
//! sale de `LC_ALL` → `LC_COLLATE` → `LANG` (precedencia POSIX; el primero no
//! vacío). `C`, `POSIX`, `C.*` o ninguno → orden de bytes, como glibc.
//!
//! La colación de glibc para `en_US`/`es_MX` (ISO 14651) se parece a la raíz de
//! ICU con la puntuación desplazada: los signos no cuentan en los tres primeros
//! niveles y, al empatar, deciden por posición y por punto de código (`-` < `.` <
//! `_`), antes que un carácter que no es signo. Ese cuarto nivel se reproduce a
//! mano porque el orden de la puntuación de ICU (`_` < `-`) no es el de glibc.
use icu_collator::options::{AlternateHandling, CollatorOptions, MaxVariable, Strength};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use std::cmp::Ordering;

/// El nombre del locale de colación del entorno, o `None` para orden de bytes.
fn collation_locale(get: impl Fn(&str) -> Vec<u8>) -> Option<String> {
    let value = ["LC_ALL", "LC_COLLATE", "LANG"]
        .into_iter()
        .map(get)
        .find(|v| !v.is_empty())?;
    let value = String::from_utf8(value).ok()?;
    // `idioma_TERRITORIO.codificación@modificador` → `idioma-TERRITORIO`.
    let base = value.split(['.', '@']).next().unwrap_or_default();
    if base.is_empty() || base == "C" || base == "POSIX" {
        return None;
    }
    Some(base.replace('_', "-"))
}

/// Comparador del glob: ICU si el locale existe, bytes si no.
pub struct GlobOrder {
    collator: Option<CollatorBorrowed<'static>>,
}

impl GlobOrder {
    pub fn from_env(get: impl Fn(&str) -> Vec<u8>) -> Self {
        let collator = collation_locale(get).and_then(|name| {
            let locale: icu_locale_core::Locale = name.parse().ok()?;
            let mut options = CollatorOptions::default();
            options.strength = Some(Strength::Tertiary);
            options.alternate_handling = Some(AlternateHandling::Shifted);
            // glibc ignora también símbolos como `+`, `=` o `~` (no `$`, que es moneda).
            options.max_variable = Some(MaxVariable::Symbol);
            Collator::try_new(CollatorPreferences::from(&locale), options).ok()
        });
        Self { collator }
    }

    pub fn compare(&self, a: &[u8], b: &[u8]) -> Ordering {
        let Some(collator) = &self.collator else {
            return a.cmp(b);
        };
        collator
            .compare_utf8(a, b)
            .then_with(|| shifted_level(a).cmp(&shifted_level(b)))
            .then_with(|| a.cmp(b))
    }
}

/// ¿Lo desplaza la colación? Espacios y signos ASCII salvo `$` (lo que ICU marca
/// como variable hasta `Symbol` y glibc ignora en los primeros niveles).
fn is_variable(c: char) -> bool {
    c.is_whitespace() || (c.is_ascii_punctuation() && c != '$')
}

/// Cuarto nivel al estilo glibc: cada signo pesa su punto de código; cualquier
/// otro carácter pesa más que todos los signos.
fn shifted_level(text: &[u8]) -> Vec<u32> {
    String::from_utf8_lossy(text)
        .chars()
        .map(|c| if is_variable(c) { c as u32 } else { u32::MAX })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::GlobOrder;
    use std::process::Command;

    const NAMES: &[&str] = &[
        "abc",
        "Abc",
        "ABC",
        "abd",
        "ab-c",
        "ab_c",
        "ab.c",
        "abc-1",
        "abc_1",
        "abc1",
        "abc10",
        "abc2",
        "a-b",
        "a_b",
        "ab",
        "ä",
        "a",
        "á",
        "Á",
        "b",
        "B",
        "éclair",
        "eclair",
        "Eclair",
        "ñandu",
        "nandu",
        "nz",
        "ozzy",
        "Ñu",
        "zeta",
        "Zeta",
        "1abc",
        "10abc",
        "2abc",
        "_x",
        "-x",
        "x-",
        "x_",
        "x",
        "proj--term-r1--2",
        "proj--term-r1--10",
        "proj-term",
        "proj",
        "proj_x",
        "Proj",
        "çà",
        "ca",
        "co",
        "a+b",
        "a$b",
        "a~b",
        "0xJesus--local--0",
        "api-blockapp-2.5",
        "api--term-1932-2--1",
        "app--term-r68823--15",
    ];

    /// El oráculo: el propio glob de bash con el locale dado.
    fn bash_glob(dir: &std::path::Path, env: &[(&str, &str)]) -> Vec<String> {
        let out = Command::new("bash")
            .env_clear()
            .envs(env.iter().copied())
            .current_dir(dir)
            .args(["-c", "for f in *.json; do printf '%s\\n' \"$f\"; done"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn sorted(names: &[&str], env: &[(&str, &str)]) -> Vec<String> {
        let order = GlobOrder::from_env(|k| {
            env.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.as_bytes().to_vec())
                .unwrap_or_default()
        });
        let mut ours: Vec<String> = names.iter().map(|n| format!("{n}.json")).collect();
        ours.sort_by(|a, b| order.compare(a.as_bytes(), b.as_bytes()));
        ours
    }

    fn fixture_dir(tag: &str, names: &[&str]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("comandos-collate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            std::fs::write(dir.join(format!("{name}.json")), b"").unwrap();
        }
        dir
    }

    #[test]
    fn matches_bash_glob_under_each_locale() {
        let dir = fixture_dir("mixed", NAMES);
        let cases: &[&[(&str, &str)]] = &[
            &[("LANG", "en_US.UTF-8")],
            &[("LANG", "es_MX.UTF-8")],
            &[("LANG", "C.UTF-8"), ("LC_ALL", "en_US.UTF-8")],
            &[("LANG", "en_US.UTF-8"), ("LC_COLLATE", "C")],
            &[("LANG", "C.UTF-8"), ("LC_COLLATE", "es_MX.UTF-8")],
            &[("LANG", "en_US.UTF-8"), ("LC_ALL", "")],
            &[("LANG", "C.UTF-8")],
            &[],
        ];
        for env in cases {
            let expected = bash_glob(&dir, env);
            assert_eq!(expected.len(), NAMES.len(), "{env:?}");
            assert_eq!(sorted(NAMES, env), expected, "{env:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Divergencia conocida: `es_MX` de glibc da peso primario al espacio (va antes
    /// que `$`) y ICU lo desplaza. Las claves de estado nunca llevan espacios (se
    /// sanean a `[A-Za-z0-9._-]`); en `en_US` el espacio sí coincide.
    #[test]
    fn space_is_the_known_es_mx_divergence() {
        let names = ["a b", "a$b", "ab", "a-b"];
        let dir = fixture_dir("space", &names);
        let en = [("LANG", "en_US.UTF-8")];
        let es = [("LANG", "es_MX.UTF-8")];
        assert_eq!(sorted(&names, &en), bash_glob(&dir, &en));
        assert_ne!(sorted(&names, &es), bash_glob(&dir, &es));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
