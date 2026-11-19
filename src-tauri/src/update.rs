//! Aviso de nova versão: uma única consulta HTTPS à API pública de releases do GitHub.
//! Só envia o User-Agent `CoreForge/<versão>`: nenhum identificador, dado de hardware ou do usuário.
use serde::{Deserialize, Serialize};
use std::{process::Command, time::Duration};

const API: &str = "https://api.github.com/repos/portugueseboyz/CoreForge/releases/latest";
const PAGE: &str = "https://github.com/portugueseboyz/CoreForge/releases";

#[derive(Serialize, Debug, PartialEq)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub url: String,
    pub notes_excerpt: String,
    pub update_available: bool,
    /// false quando não deu para consultar (offline, limite de requisições, sem release): resultado neutro.
    pub known: bool,
}

#[derive(Deserialize)]
struct Release { tag_name: Option<String>, html_url: Option<String>, body: Option<String> }

/// (números, é_pré-lançamento, sufixo). Aceita `v1.2.3`, `1.2`, `1.2.3-rc1`, `1.2.3+build`; o resto é inválido.
fn parse_version(s: &str) -> Option<([u64; 3], Option<String>)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split('+').next()?;
    let (core, pre) = match s.split_once('-') { Some((c, p)) => (c, Some(p.to_string())), None => (s, None) };
    let mut n = [0u64; 3];
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 3 { return None; }
    for (i, p) in parts.iter().enumerate() { n[i] = p.parse().ok()?; }
    Some((n, pre.filter(|p| !p.is_empty())))
}

/// true se `latest` é estritamente mais nova que `current`. Tag malformada: false.
pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    let (Some((l, lp)), Some((c, cp))) = (parse_version(latest), parse_version(current)) else { return false };
    if l != c { return l > c; }
    match (lp, cp) { (None, Some(_)) => true, (Some(a), Some(b)) => a > b, _ => false }
}

fn unknown(current: &str) -> UpdateInfo {
    UpdateInfo { current: current.into(), latest: String::new(), url: PAGE.into(), notes_excerpt: String::new(), update_available: false, known: false }
}

pub(crate) fn parse_response(json: &str, current: &str) -> UpdateInfo {
    let Ok(r) = serde_json::from_str::<Release>(json) else { return unknown(current) };
    let Some(tag) = r.tag_name.filter(|t| parse_version(t).is_some()) else { return unknown(current) };
    // Só aceita links do próprio repositório: o botão abre essa URL.
    let url = r.html_url.filter(|u| u.starts_with("https://github.com/portugueseboyz/CoreForge/")).unwrap_or_else(|| PAGE.into());
    let notes_excerpt: String = r.body.unwrap_or_default().trim().chars().take(280).collect();
    UpdateInfo { current: current.into(), update_available: is_newer(&tag, current), latest: tag.trim_start_matches(['v', 'V']).into(), url, notes_excerpt, known: true }
}

fn fetch(current: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
    let mut resp = agent.get(API).header("User-Agent", format!("CoreForge/{current}")).header("Accept", "application/vnd.github+json").call().ok()?;
    resp.body_mut().read_to_string().ok()
}

#[tauri::command(async)]
pub fn update_check() -> UpdateInfo {
    let current = env!("CARGO_PKG_VERSION");
    crate::guard(|| Ok(fetch(current).map(|j| parse_response(&j, current)).unwrap_or_else(|| unknown(current)))).unwrap_or_else(|_| unknown(current))
}

/// Abre só a página de releases do projeto (URL fixa, nada vindo da interface).
#[tauri::command(async)]
pub fn update_open() -> Result<(), String> {
    Command::new("xdg-open").arg(PAGE).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("V1.2", "1.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(is_newer("1.0.0", "1.0.0-rc1"));
        assert!(!is_newer("1.0.0-rc1", "1.0.0"));
        assert!(is_newer("1.1.0-rc1", "1.0.0"));
        assert!(is_newer("1.0.0-rc2", "1.0.0-rc1"));
        assert!(!is_newer("1.0.0+build5", "1.0.0"));
    }

    #[test]
    fn malformed_tags_mean_no_update() {
        for t in ["", "latest", "v", "1.2.3.4", "1.x.0", "nightly-2026", "-1.0.0"] { assert!(!is_newer(t, "0.1.0"), "{t}"); }
    }

    #[test]
    fn parses_release_json() {
        let r = parse_response(r#"{"tag_name":"v0.2.0","html_url":"https://github.com/portugueseboyz/CoreForge/releases/tag/v0.2.0","body":"Novidades\n- a"}"#, "0.1.0");
        assert!(r.known && r.update_available);
        assert_eq!((r.latest.as_str(), r.notes_excerpt.as_str()), ("0.2.0", "Novidades\n- a"));
        assert!(r.url.ends_with("/tag/v0.2.0"));
    }

    #[test]
    fn handles_odd_responses() {
        let same = parse_response(r#"{"tag_name":"0.1.0","body":null}"#, "0.1.0");
        assert!(same.known && !same.update_available && same.notes_excerpt.is_empty() && same.url == PAGE);
        let pre = parse_response(r#"{"tag_name":"v0.2.0-beta.1","body":""}"#, "0.1.0");
        assert!(pre.update_available);
        let long = parse_response(&format!(r#"{{"tag_name":"v9.0.0","body":"{}"}}"#, "x".repeat(1000)), "0.1.0");
        assert_eq!(long.notes_excerpt.len(), 280);
        let evil = parse_response(r#"{"tag_name":"v9.0.0","html_url":"https://evil.example/x"}"#, "0.1.0");
        assert_eq!(evil.url, PAGE);
        for bad in ["", "{}", "[]", "not json", r#"{"message":"Not Found"}"#, r#"{"tag_name":"abc"}"#, r#"{"tag_name":5}"#] {
            let r = parse_response(bad, "0.1.0");
            assert!(!r.known && !r.update_available && r.latest.is_empty(), "{bad}");
        }
    }
}
