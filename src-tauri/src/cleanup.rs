//! Limpeza: tamanhos de caches e lixo. Só apaga o conteúdo de UMA categoria do usuário por vez, de uma lista fechada de pastas
//! dentro do HOME; o que exige root (apt, journal, kernels) é só listado, com o comando para o usuário rodar.
use serde::Serialize;
use std::{fs, path::{Path, PathBuf}, process::Command};

#[derive(Serialize, Debug, PartialEq)]
pub struct Category { id: &'static str, label: &'static str, bytes: u64, cleanable: bool, hint: String }

const MAX_ENTRIES: u32 = 3_000_000; // trava contra árvores gigantes

fn home() -> PathBuf { PathBuf::from(std::env::var("HOME").unwrap_or_default()) }

/// Pastas que podem ser esvaziadas (id -> pastas relativas ao HOME). Lista fechada: nada fora daqui é apagado.
fn allow(id: &str, home: &Path) -> Option<Vec<PathBuf>> {
    let r: &[&str] = match id {
        "thumbs" => &[".cache/thumbnails"],
        "trash" => &[".local/share/Trash/files", ".local/share/Trash/info", ".local/share/Trash/directorysizes"],
        "cache" => &[".cache"],
        _ => return None,
    };
    Some(r.iter().map(|p| home.join(p)).collect())
}

/// Soma tamanhos sem seguir links simbólicos.
pub fn dir_size(p: &Path) -> u64 {
    fn go(p: &Path, n: &mut u32) -> u64 {
        let Ok(md) = fs::symlink_metadata(p) else { return 0 };
        if !md.is_dir() { return md.len(); }
        let Ok(rd) = fs::read_dir(p) else { return 0 };
        rd.flatten().map(|e| { *n += 1; if *n > MAX_ENTRIES { 0 } else { go(&e.path(), n) } }).sum()
    }
    go(p, &mut 0)
}

/// Esvazia a pasta (mantém a própria pasta). Recusa se a raiz for um link simbólico ou não estiver sob `home`.
pub fn empty_dir(root: &Path, home: &Path) -> Result<u64, String> {
    if home.as_os_str().is_empty() || !root.starts_with(home) || root == home { return Err("caminho fora da lista permitida".into()); }
    let md = match fs::symlink_metadata(root) { Ok(m) => m, Err(_) => return Ok(0) };
    if md.file_type().is_symlink() || !md.is_dir() { return Err(format!("{} não é uma pasta comum", root.display())); }
    let before = dir_size(root);
    let mut fail = 0;
    for e in fs::read_dir(root).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        let isdir = fs::symlink_metadata(&p).map(|m| m.is_dir()).unwrap_or(false); // remove_dir_all não segue links
        if (if isdir { fs::remove_dir_all(&p) } else { fs::remove_file(&p) }).is_err() { fail += 1; }
    }
    let freed = before.saturating_sub(dir_size(root));
    if fail > 0 && freed == 0 { return Err(format!("não foi possível apagar {fail} item(ns)")); }
    Ok(freed)
}

/// Pacotes de kernel instalados que não são o kernel em uso nem o metapacote (nome precisa conter versão).
pub fn old_kernels(pkgs: &[&str], running: &str) -> Vec<String> {
    pkgs.iter().filter(|p| p.starts_with("linux-image-") && p.chars().any(|c| c.is_ascii_digit()) && !p.contains(running) && !p.ends_with("-dbg"))
        .map(|p| p.to_string()).collect()
}

fn installed_kernels() -> Vec<String> {
    let Ok(o) = Command::new("dpkg-query").args(["-W", "-f", "${Package} ${db:Status-Abbrev}\\n", "linux-image-*"]).output() else { return vec![] };
    let txt = String::from_utf8_lossy(&o.stdout).into_owned();
    let running = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let names: Vec<&str> = txt.lines().filter(|l| l.split_whitespace().nth(1).is_some_and(|s| s.starts_with("ii"))).filter_map(|l| l.split_whitespace().next()).collect();
    old_kernels(&names, running.trim())
}

#[tauri::command(async)]
pub fn cleanup_scan() -> Vec<Category> {
    let h = home();
    let sz = |id: &str| allow(id, &h).map(|v| v.iter().map(|p| dir_size(p)).sum()).unwrap_or(0);
    let sys = |ps: &[&str]| ps.iter().map(|p| dir_size(Path::new(p))).sum::<u64>();
    let k = installed_kernels();
    let cmd = |s: &str| format!("Exige root; rode no terminal: {s}");
    vec![
        Category { id: "thumbs", label: "Miniaturas de imagens", bytes: sz("thumbs"), cleanable: true, hint: String::new() },
        Category { id: "trash", label: "Lixeira", bytes: sz("trash"), cleanable: true, hint: String::new() },
        Category { id: "cache", label: "Cache dos programas (~/.cache)", bytes: sz("cache"), cleanable: true, hint: "Os programas recriam o que precisarem; o primeiro uso depois pode ser mais lento.".into() },
        Category { id: "pkg", label: "Cache de pacotes do sistema", bytes: sys(&["/var/cache/apt/archives", "/var/cache/pacman/pkg", "/var/cache/dnf"]), cleanable: false, hint: cmd("sudo apt clean (ou pacman -Sc / dnf clean all)") },
        Category { id: "journal", label: "Logs do sistema (journal)", bytes: sys(&["/var/log/journal"]), cleanable: false, hint: cmd("sudo journalctl --vacuum-size=200M") },
        Category { id: "kernels", label: "Kernels antigos", bytes: 0, cleanable: false,
            hint: if k.is_empty() { "Nenhum kernel antigo encontrado (ou o sistema não usa dpkg).".into() } else { cmd(&format!("sudo apt remove {}", k.join(" "))) } },
    ]
}

#[tauri::command(async)]
pub fn cleanup_run(id: String) -> Result<u64, String> {
    let h = home();
    let dirs = allow(&id, &h).ok_or("categoria não permitida")?;
    let mut total = 0;
    for d in dirs { total += empty_dir(&d, &h)?; }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(n: &str) -> PathBuf { let p = std::env::temp_dir().join(format!("cf-clean-{n}-{}", std::process::id())); let _ = fs::remove_dir_all(&p); fs::create_dir_all(&p).unwrap(); p }

    #[test]
    fn empties_contents_but_not_outside_or_symlink_targets() {
        let h = tmp("a"); let outside = tmp("b"); fs::write(outside.join("keep"), "x").unwrap();
        let c = h.join(".cache"); fs::create_dir_all(c.join("app/sub")).unwrap();
        fs::write(c.join("app/sub/f"), vec![0u8; 100]).unwrap(); fs::write(c.join("g"), vec![0u8; 50]).unwrap();
        std::os::unix::fs::symlink(&outside, c.join("link")).unwrap();
        assert!(empty_dir(&c, &h).unwrap() >= 150);
        assert_eq!(fs::read_dir(&c).unwrap().count(), 0);
        assert!(outside.join("keep").exists());
        let _ = fs::remove_dir_all(&h); let _ = fs::remove_dir_all(&outside);
    }
    #[test]
    fn refuses_unsafe_roots() {
        let h = tmp("c"); let outside = tmp("d");
        assert!(empty_dir(&outside, &h).is_err());
        assert!(empty_dir(&h, &h).is_err());
        assert!(empty_dir(Path::new("/"), Path::new("")).is_err());
        std::os::unix::fs::symlink(&outside, h.join(".cache")).unwrap();
        assert!(empty_dir(&h.join(".cache"), &h).is_err());
        assert_eq!(empty_dir(&h.join("nada"), &h).unwrap(), 0);
        let _ = fs::remove_dir_all(&h); let _ = fs::remove_dir_all(&outside);
    }
    #[test]
    fn allowlist_is_closed() {
        let h = Path::new("/home/u");
        assert!(allow("../etc", h).is_none() && allow("pkg", h).is_none() && allow("cache", h).is_some());
        assert!(cleanup_run("/".into()).is_err());
    }
    #[test]
    fn keeps_running_kernel_and_metapackage() {
        let p = ["linux-image-generic", "linux-image-7.0.0-43-generic", "linux-image-7.0.0-40-generic", "linux-image-6.8.0-1-generic-dbg"];
        assert_eq!(old_kernels(&p, "7.0.0-43-generic"), vec!["linux-image-7.0.0-40-generic"]);
    }
}
