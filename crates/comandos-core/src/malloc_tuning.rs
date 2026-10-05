//! Ajuste de glibc malloc del frente (`comandos dash` bajo `cc-dash.service`).
//!
//! Sin él, cada liberación de un bloque grande (el memo de GET `/usage/state`,
//! el cuerpo de GET `/state`) sube el umbral dinámico de `mmap` de glibc y las
//! arenas de los hilos dejan de devolver memoria: el frente retiene ≈ 61 MiB
//! bajo la carga realista de `xtask poll --shadow`. Con el umbral fijo en
//! 128 KiB y dos arenas, ≈ 45 MiB. Va en el drop-in de la unidad (no hay
//! dependencia nueva ni código `unsafe`) y los hijos del frente no lo heredan.

/// La variable que lee glibc al arrancar el proceso.
pub const GLIBC_TUNABLES_ENV: &str = "GLIBC_TUNABLES";

/// El valor de producción del drop-in de `cc-dash.service` y el que
/// `xtask poll --shadow` da al frente que mide.
pub const PRODUCTION_GLIBC_TUNABLES: &str =
    "glibc.malloc.mmap_threshold=131072:glibc.malloc.arena_max=2";
