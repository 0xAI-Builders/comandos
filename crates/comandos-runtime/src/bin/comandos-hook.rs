//! `comandos-hook <harness>`: los hooks sin el despachador de `comandos` (lo usan
//! las pruebas de paridad). Acepta un `hook` inicial para que el proceso de
//! entrega (`<exe> hook __deliver`) funcione igual que con `comandos`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = if args.first().is_some_and(|a| a == "hook") {
        &args[1..]
    } else {
        &args[..]
    };
    std::process::exit(comandos_runtime::hooks::run(args));
}
