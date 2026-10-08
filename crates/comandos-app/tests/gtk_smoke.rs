//! Pruebas GTK con pantalla (`harness = false`). Solo corren si el controlador fija
//! `COMANDOS_GTK_TEST_DISPLAY`; sin esa variable se saltan con aviso y pasan. Nunca
//! se abre una ventana en la pantalla del usuario por omisión.
use std::process::ExitCode;

fn main() -> ExitCode {
    // Comprueba que GTK enlaza sin inicializarlo (no toca ninguna pantalla).
    let version = (
        gtk::major_version(),
        gtk::minor_version(),
        gtk::micro_version(),
    );
    let Some(display) = std::env::var_os("COMANDOS_GTK_TEST_DISPLAY").filter(|d| !d.is_empty())
    else {
        println!(
            "gtk_smoke: GTK {}.{}.{} enlazado; COMANDOS_GTK_TEST_DISPLAY sin definir, pruebas con pantalla saltadas",
            version.0, version.1, version.2
        );
        return ExitCode::SUCCESS;
    };
    // Las pruebas con pantalla llegan con la Tarea 3 en adelante.
    println!("gtk_smoke: pantalla {display:?}; aún no hay pruebas con pantalla (T1)");
    ExitCode::SUCCESS
}
