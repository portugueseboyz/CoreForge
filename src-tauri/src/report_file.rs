//! Grava o relatório de texto em ~/Documentos (ou ~/Documents): sem janela de diálogo e sem dependência nova.
use std::{fs, io::Write, path::PathBuf};

fn docs_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    ["Documentos", "Documents"].iter().map(|d| home.join(d)).find(|p| p.is_dir()).or(Some(home))
}

fn write_unique(dir: &std::path::Path, text: &str) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    for n in 0..100 {
        let name = if n == 0 { format!("coreforge-relatorio-{stamp}.md") } else { format!("coreforge-relatorio-{stamp}-{n}.md") };
        let path = dir.join(name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => { f.write_all(text.as_bytes())?; return Ok(path); }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("sem nome livre"))
}

/// Salva o relatório (já sem dados pessoais, ver report.ts) e devolve o caminho do arquivo.
#[tauri::command]
pub fn report_save(text: String) -> Result<String, String> {
    if text.len() > 1_000_000 { return Err("relatório grande demais".into()); }
    let dir = docs_dir().ok_or("sem pasta pessoal")?;
    write_unique(&dir, &text).map(|p| p.display().to_string()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn never_overwrites() {
        let d = std::env::temp_dir().join(format!("cf-rep-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (super::write_unique(&d, "a").unwrap(), super::write_unique(&d, "b").unwrap());
        assert_ne!(a, b);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a");
        std::fs::remove_dir_all(&d).ok();
    }
}
