//! Curvas de ventoinha: a velocidade segue a temperatura da CPU ou da GPU.
//! Escrever em PWM exige root, então o motor roda como um processo do ajudante, iniciado uma vez via pkexec (uma senha). Ele relê o arquivo de
//! curvas a cada ciclo, só faz as mesmas escritas validadas das ações isoladas, e ao parar (usuário desliga, app fecha ou arquivo some)
//! devolve todos os canais ao automático. Se a temperatura não puder ser lida, o canal vai a 100 %.
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::Path, process::{Child, Command, Stdio}, sync::Mutex, thread, time::Duration};

const MIN_PCT: u32 = 25; // mesmo piso da velocidade manual
const TICK: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Curve {
    pub kind: String, pub hw: String, pub pwm: u32, pub chip: String, pub label: String,
    pub source: String,           // "cpu" ou "gpu"
    pub points: Vec<(f64, u32)>,  // (°C, %), temperaturas crescentes
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct CurveFile { #[serde(default)] pub run: bool, #[serde(default)] pub curves: Vec<Curve> }

pub fn validate(c: &Curve) -> Result<(), String> {
    if !["hwmon", "gpu"].contains(&c.kind.as_str()) { return Err("tipo inválido".into()); }
    if !["cpu", "gpu"].contains(&c.source.as_str()) { return Err("fonte de temperatura inválida".into()); }
    if !(2..=8).contains(&c.points.len()) { return Err("use de 2 a 8 pontos".into()); }
    if c.points.iter().any(|(t, p)| !t.is_finite() || !(0.0..=110.0).contains(t) || !(MIN_PCT..=100).contains(p)) { return Err(format!("pontos devem ter temperatura de 0 a 110 °C e velocidade de {MIN_PCT} a 100 %")); }
    if c.points.windows(2).any(|w| w[1].0 <= w[0].0) { return Err("as temperaturas dos pontos devem ser crescentes".into()); }
    Ok(())
}

/// Velocidade alvo (%) para a temperatura: interpolação linear entre os pontos, constante fora deles.
pub fn target(points: &[(f64, u32)], temp: f64) -> u32 {
    let (first, last) = (points[0], points[points.len() - 1]);
    if temp <= first.0 { return first.1; }
    if temp >= last.0 { return last.1; }
    let w = points.windows(2).find(|w| temp <= w[1].0).expect("temp está entre o primeiro e o último ponto");
    let f = (temp - w[0].0) / (w[1].0 - w[0].0);
    (w[0].1 as f64 + f * (w[1].1 as f64 - w[0].1 as f64)).round() as u32
}

/// Sobe rápido e desce devagar, com faixa morta de 2 %: evita a ventoinha oscilar a cada leitura.
pub fn next_pct(cur: u32, target: u32) -> u32 {
    if target.abs_diff(cur) < 2 { cur } else if target > cur { target.min(cur + 15) } else { target.max(cur.saturating_sub(3)) }
}

// ------------------------------------------------------------------ lado do app

fn file_path() -> Option<std::path::PathBuf> { crate::fans::config_dir().map(|d| d.join("curves.json")) }
fn load() -> CurveFile { file_path().and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() }
fn store(f: &CurveFile) -> Result<(), String> {
    let p = file_path().ok_or("sem pasta de configuração")?;
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(p, serde_json::to_string_pretty(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Pede ao motor que pare (ele devolve as ventoinhas ao automático em até um ciclo).
pub(crate) fn request_stop() { let mut f = load(); if f.run { f.run = false; let _ = store(&f); } }

#[derive(Default)]
pub struct CurveState(Mutex<Option<Child>>);

#[derive(Serialize)]
pub struct EngineStatus { running: bool, error: String }

#[tauri::command(async)]
pub fn curves_get() -> Vec<Curve> { load().curves }

#[tauri::command(async)]
pub fn curves_save(curves: Vec<Curve>) -> Result<(), String> {
    for c in &curves { validate(c).map_err(|e| format!("{}: {e}", c.label))?; }
    let mut f = load();
    f.curves = curves;
    store(&f)
}

#[tauri::command(async)]
pub fn curves_status(state: tauri::State<CurveState>) -> EngineStatus {
    let mut g = state.0.lock().unwrap();
    let Some(child) = g.as_mut() else { return EngineStatus { running: false, error: String::new() } };
    match child.try_wait() {
        Ok(None) => EngineStatus { running: true, error: String::new() },
        Ok(Some(st)) => {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() { let _ = std::io::Read::read_to_string(&mut e, &mut err); }
            let code = st.code();
            *g = None;
            let error = match code { Some(0) => String::new(), Some(126) | Some(127) => "Autenticação cancelada".into(), _ => err.trim().to_string() };
            EngineStatus { running: false, error }
        }
        Err(e) => EngineStatus { running: false, error: e.to_string() },
    }
}

/// Liga o motor: pede a senha uma vez; ele continua rodando enquanto o app estiver aberto.
#[tauri::command(async)]
pub fn curves_start(state: tauri::State<CurveState>) -> Result<(), String> {
    let mut g = state.0.lock().unwrap();
    if let Some(c) = g.as_mut() && c.try_wait().ok().flatten().is_none() { return Ok(()); }
    let mut f = load();
    if f.curves.is_empty() { return Err("Salve ao menos uma curva antes de ligar.".into()); }
    f.run = true;
    store(&f)?;
    let path = file_path().ok_or("sem pasta de configuração")?;
    let exe = crate::fans::helper_path()?;
    let child = Command::new("pkexec").arg(exe).args(["curve-run".to_string(), path.to_string_lossy().into_owned(), std::process::id().to_string()])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| format!("pkexec indisponível: {e}"))?;
    *g = Some(child);
    for c in &f.curves { crate::fans::mark_manual(&c.kind, &c.hw, c.pwm, &c.chip); }
    Ok(())
}

#[tauri::command(async)]
pub fn curves_stop() { request_stop(); }

// ------------------------------------------------------------------ lado do ajudante (root)

fn source_temp(source: &str) -> Option<f64> {
    if source == "gpu" { crate::gpu::first_sample().map(|s| s.0) } else { crate::sysinfo::cpu_temp() }
}

/// Motor: roda como root até o arquivo mandar parar, o app sair ou o arquivo ficar inválido. `path` precisa ser de quem chamou o pkexec.
pub fn daemon(path: &str, app_pid: u32) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;
    let uid: u32 = std::env::var("PKEXEC_UID").ok().and_then(|u| u.parse().ok()).ok_or("sem PKEXEC_UID")?;
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.uid() != uid { return Err("arquivo de curvas inválido".into()); }
    let mut cur: HashMap<String, u32> = HashMap::new();
    let mut touched: HashMap<String, Curve> = HashMap::new();
    while Path::new(&format!("/proc/{app_pid}")).exists() {
        let Some(file) = fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<CurveFile>(&t).ok()).filter(|f| f.run) else { break };
        for c in file.curves.iter().filter(|c| validate(c).is_ok()) {
            let key = format!("{}:{}:{}", c.kind, c.chip, c.pwm);
            let want = source_temp(&c.source).map_or(100, |t| target(&c.points, t));
            let pct = cur.get(&key).map_or(want, |&p| next_pct(p, want));
            if cur.get(&key) == Some(&pct) { continue; }
            let hw = if c.kind == "hwmon" { match crate::fans::find_hwmon(&c.chip) { Some(h) => h, None => continue } } else { c.hw.clone() };
            if crate::fans::run_action(&[format!("{}-set", c.kind), hw, c.pwm.to_string(), c.chip.clone(), pct.to_string()]).is_ok() {
                cur.insert(key.clone(), pct);
                touched.insert(key, c.clone());
            }
        }
        thread::sleep(TICK);
    }
    for c in touched.values() {
        let hw = if c.kind == "hwmon" { crate::fans::find_hwmon(&c.chip).unwrap_or_default() } else { c.hw.clone() };
        let _ = crate::fans::run_action(&[format!("{}-auto", c.kind), hw, c.pwm.to_string(), c.chip.clone()]);
    }
    Ok("motor de curvas parado".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    const P: &[(f64, u32)] = &[(40.0, 30), (60.0, 50), (80.0, 100)];
    fn curve(points: Vec<(f64, u32)>) -> Curve { Curve { kind: "hwmon".into(), hw: "hwmon1".into(), pwm: 1, chip: "x".into(), label: "x".into(), source: "cpu".into(), points } }

    #[test]
    fn interpolates_and_clamps() {
        assert_eq!([target(P, 20.0), target(P, 40.0), target(P, 50.0), target(P, 70.0), target(P, 95.0)], [30, 30, 40, 75, 100]);
    }
    #[test]
    fn rises_fast_and_falls_slow() {
        assert_eq!((next_pct(30, 100), next_pct(100, 30), next_pct(50, 51), next_pct(50, 50)), (45, 97, 50, 50));
    }
    #[test]
    fn validation_rules() {
        assert!(validate(&curve(P.to_vec())).is_ok());
        assert!(validate(&curve(vec![(40.0, 30)])).is_err());                        // um ponto só
        assert!(validate(&curve(vec![(40.0, 10), (60.0, 50)])).is_err());            // abaixo do piso
        assert!(validate(&curve(vec![(60.0, 30), (40.0, 50)])).is_err());            // fora de ordem
        assert!(validate(&curve(vec![(40.0, 30), (f64::NAN, 50)])).is_err());        // NaN
        let mut c = curve(P.to_vec()); c.source = "rm".into();
        assert!(validate(&c).is_err());
    }
    #[test]
    fn daemon_refuses_foreign_files() {
        // sem PKEXEC_UID (ou com arquivo de outro dono) o motor não faz nada
        assert!(daemon("/etc/passwd", std::process::id()).is_err());
    }
}
