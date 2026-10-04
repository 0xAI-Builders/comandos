fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(
        comandos_runtime::events_cli::run(&args).unwrap_or_else(|error| {
            eprintln!("event_intake: {error}");
            1
        }),
    );
}
