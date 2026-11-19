// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Reaplica as luzes do perfil marcado ao iniciar a sessão, sem abrir janela.
    if std::env::args().any(|a| a == "--apply-profile") { std::process::exit(coreforge_lib::apply_start_profile_headless()); }
    coreforge_lib::run()
}
