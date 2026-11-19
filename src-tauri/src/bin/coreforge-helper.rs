//! Ajudante com privilégios do CoreForge. Instalado em /usr/libexec/coreforge/ (dono root) e chamado só pelo app, via pkexec.
//! Aceita um conjunto fechado de ações com argumentos validados (ver `fans::root_helper`); nunca abre janela nem executa texto livre.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(coreforge_lib::root_helper(&args));
}
