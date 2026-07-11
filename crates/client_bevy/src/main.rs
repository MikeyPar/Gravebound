fn main() {
    if let Err(error) = client_bevy::run_local_lab() {
        eprintln!("Gravebound LocalLab failed to start: {error:#}");
        std::process::exit(1);
    }
}
