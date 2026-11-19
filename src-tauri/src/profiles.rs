//! Perfis: guardam o que o CoreForge ajustou (velocidade das ventoinhas e cores/modos dos LEDs) e o reaplicam depois.
//! Salvo em ~/.config/coreforge/profiles.json. As luzes podem ser reaplicadas ao iniciar a sessão, sem janela e sem senha;
//! as ventoinhas pedem a senha do sistema, então só entram quando o usuário aplica o perfil.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FanSetting { pub kind: String, pub hw: String, pub pwm: u32, pub chip: String, pub label: String, pub percent: Option<u32> }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LedSetting {
    pub device: String, pub zone: Option<String>, pub mode: Option<String>, pub color: Option<String>,
    pub colors: Option<Vec<String>>, pub brightness: Option<u32>, pub size: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Profile { pub name: String, pub fans: Vec<FanSetting>, pub leds: Vec<LedSetting>, #[serde(default)] pub apply_on_start: bool }

#[derive(Serialize)]
pub struct Draft { fans: Vec<FanSetting>, leds: Vec<LedSetting> }

fn file(name: &str) -> Option<PathBuf> { crate::fans::config_dir().map(|d| d.join(name)) }
fn load<T: DeserializeOwned + Default>(name: &str) -> T { file(name).and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() }
fn store<T: Serialize>(name: &str, v: &T) -> Result<(), String> {
    let p = file(name).ok_or("sem pasta de configuração")?;
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(p, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn load_profiles() -> Vec<Profile> { load("profiles.json") }

/// Última configuração de LED aplicada por dispositivo e zona (é o que um perfil novo captura).
pub fn record_led(s: LedSetting) {
    let mut v: Vec<LedSetting> = load("led-state.json");
    v.retain(|x| !(x.device == s.device && x.zone == s.zone));
    v.push(s);
    let _ = store("led-state.json", &v);
}

fn capture() -> Draft { Draft { fans: crate::fans::capture_overrides(), leds: load("led-state.json") } }

#[tauri::command(async)]
pub fn profiles_list() -> Vec<Profile> { load_profiles() }

/// O que um perfil salvo agora conteria (para a tela mostrar antes de salvar).
#[tauri::command(async)]
pub fn profile_capture() -> Draft { capture() }

#[tauri::command(async)]
pub fn profile_save(name: String) -> Result<Profile, String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 40 { return Err("Dê um nome de até 40 caracteres".into()); }
    let d = capture();
    if d.fans.is_empty() && d.leds.is_empty() { return Err("Nada para salvar: ajuste uma ventoinha ou um LED primeiro".into()); }
    let mut all = load_profiles();
    let keep_start = all.iter().find(|p| p.name == name).is_some_and(|p| p.apply_on_start);
    all.retain(|p| p.name != name);
    let p = Profile { name, fans: d.fans, leds: d.leds, apply_on_start: keep_start };
    all.push(p.clone());
    store("profiles.json", &all)?;
    Ok(p)
}

#[tauri::command(async)]
pub fn profile_delete(name: String) -> Result<(), String> {
    let mut all = load_profiles();
    all.retain(|p| p.name != name);
    store("profiles.json", &all)
}

/// Marca o perfil cujas luzes são reaplicadas ao iniciar a sessão (só um; `None` desmarca todos).
#[tauri::command(async)]
pub fn profile_set_start(name: Option<String>) -> Result<(), String> {
    let mut all = load_profiles();
    for p in all.iter_mut() { p.apply_on_start = name.as_deref() == Some(p.name.as_str()); }
    store("profiles.json", &all)
}

/// Aplica as luzes de um perfil: acha cada dispositivo e zona pelo NOME (os números mudam entre boots). Devolve (aplicados, erros).
pub fn apply_leds(settings: &[LedSetting]) -> (usize, Vec<String>) {
    if settings.is_empty() { return (0, vec![]); }
    let st = crate::leds::leds_status();
    let (mut ok, mut errs) = (0, vec![]);
    for s in settings {
        let Some(dev) = st.devices.iter().find(|d| d.name == s.device) else { errs.push(format!("{}: dispositivo não encontrado", s.device)); continue };
        let zone = match &s.zone { Some(z) => match dev.zones.iter().position(|x| x == z) { Some(i) => Some(i), None => { errs.push(format!("{}: zona {z} não encontrada", s.device)); continue } }, None => None };
        match crate::leds::leds_run(dev.index, zone, s.size, s.mode.clone(), s.color.clone(), s.colors.clone(), s.brightness, None) { Ok(_) => ok += 1, Err(e) => errs.push(format!("{}: {e}", s.device)) }
    }
    (ok, errs)
}

#[tauri::command(async)]
pub fn profile_apply(name: String) -> Result<String, String> {
    let p = load_profiles().into_iter().find(|p| p.name == name).ok_or("Perfil não encontrado")?;
    let (ok, mut errs) = apply_leds(&p.leds);
    let mut parts = vec![];
    if !p.leds.is_empty() { parts.push(format!("LEDs: {ok}/{}", p.leds.len())); }
    if !p.fans.is_empty() {
        match crate::fans::apply_settings(&p.fans) { Ok(_) => parts.push(format!("ventoinhas: {}", p.fans.len())), Err(e) => errs.push(e) }
    }
    if errs.is_empty() { Ok(parts.join(" · ")) } else { Err(format!("{} — {}", parts.join(" · "), errs.join("; "))) }
}

// ------------------------------------------------------------------ ao iniciar a sessão (sem janela, sem senha)

fn autostart_path() -> Option<PathBuf> { std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config/autostart/coreforge-profile.desktop")) }

/// Conteúdo do atalho de autostart. Só reaplica as luzes: mudar ventoinhas exige senha e não deve aparecer sozinho no login.
pub fn autostart_entry(exe: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName=CoreForge (reaplicar luzes)\nComment=Reaplica o perfil de luzes do CoreForge ao iniciar a sessão\nExec=\"{exe}\" --apply-profile\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n")
}

#[tauri::command(async)]
pub fn profile_autostart_get() -> bool { autostart_path().is_some_and(|p| p.exists()) }

#[tauri::command(async)]
pub fn profile_autostart_set(enabled: bool) -> Result<(), String> {
    let p = autostart_path().ok_or("HOME indefinido")?;
    if !enabled { let _ = fs::remove_file(&p); return Ok(()); }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(p, autostart_entry(&exe.to_string_lossy())).map_err(|e| e.to_string())
}

/// `coreforge --apply-profile`: reaplica as luzes do perfil marcado e sai. Retorna o código de saída.
pub fn apply_start_profile_headless() -> i32 {
    let Some(p) = load_profiles().into_iter().find(|p| p.apply_on_start) else { println!("nenhum perfil marcado para iniciar"); return 0 };
    let (ok, errs) = apply_leds(&p.leds);
    println!("perfil {}: {ok}/{} configurações de LED aplicadas", p.name, p.leds.len());
    for e in &errs { eprintln!("{e}"); }
    i32::from(!errs.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn led(dev: &str, zone: Option<&str>, color: &str) -> LedSetting { LedSetting { device: dev.into(), zone: zone.map(String::from), mode: Some("Direct".into()), color: Some(color.into()), colors: None, brightness: None, size: None } }
    #[test]
    fn led_state_keeps_the_latest_per_device_and_zone() {
        let tmp = std::env::temp_dir().join(format!("cf_prof_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &tmp) };
        record_led(led("Placa", Some("ARGB 1"), "FF0000"));
        record_led(led("Placa", Some("ARGB 1"), "00FF00"));
        record_led(led("Placa", Some("ARGB 2"), "0000FF"));
        let st: Vec<LedSetting> = load("led-state.json");
        assert_eq!(st.len(), 2);
        assert_eq!(st.iter().find(|s| s.zone.as_deref() == Some("ARGB 1")).unwrap().color.as_deref(), Some("00FF00"));
        // salvar exige algo capturado, valida o nome e guarda uma cópia
        assert!(profile_save("  ".into()).is_err());
        let p = profile_save("Gamer".into()).unwrap();
        assert_eq!(p.leds.len(), 2);
        profile_set_start(Some("Gamer".into())).unwrap();
        assert!(load_profiles()[0].apply_on_start);
        profile_delete("Gamer".into()).unwrap();
        assert!(load_profiles().is_empty());
        assert_eq!(apply_start_profile_headless(), 0); // nenhum marcado: não faz nada
        let _ = fs::remove_dir_all(tmp);
    }
    #[test]
    fn autostart_entry_runs_only_the_headless_mode() {
        let e = autostart_entry("/opt/coreforge/coreforge");
        assert!(e.contains("Exec=\"/opt/coreforge/coreforge\" --apply-profile") && e.contains("NoDisplay=true"));
    }
}
