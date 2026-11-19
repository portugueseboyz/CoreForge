//! Ventoinhas: leitura (RPM e %) de hwmon e da GPU NVIDIA, e controle de velocidade.
//! Descoberta automática: acha os controladores de ventoinha da placa (tenta o driver certo pelo fabricante) e descobre qual PWM move qual
//! ventoinha (teste de mapeamento), guardando o resultado por chip. Escrever em PWM, carregar drivers e testar exige root: o app chama o
//! ajudante instalado via pkexec, que só aceita ações fixas com argumentos validados. Nada é gravado sem o usuário confirmar.
use crate::sysinfo::read;
use nvml_wrapper::{enums::device::FanControlPolicy, Nvml};
use serde::Serialize;
use std::{collections::{BTreeMap, HashMap}, fs, path::{Path, PathBuf}, process::Command, thread, time::Duration};

const HWMON_MIN: u32 = 25; // piso de segurança: nunca deixa o usuário parar uma ventoinha por aqui
/// Drivers de sensores/PWM de placa-mãe que o app pode tentar carregar (lista fechada; o ajudante só aceita estes).
const MODULES: &[&str] = &["nct6775", "nct6683", "it87", "f71882fg", "asus_wmi_sensors", "asus_ec_sensors", "dell_smm_hwmon"];

/// Ordem de tentativa por fabricante (DMI): o mais provável primeiro.
fn candidates(vendor: &str) -> Vec<&'static str> {
    let v = vendor.to_lowercase();
    let list: &[&str] = if v.contains("asus") { &["nct6775", "asus_wmi_sensors", "asus_ec_sensors", "it87", "nct6683", "f71882fg"] }
        else if v.contains("micro-star") || v.contains("msi") { &["nct6775", "nct6683", "it87", "f71882fg"] }
        else if v.contains("gigabyte") { &["it87", "nct6775", "f71882fg", "nct6683"] }
        else if v.contains("asrock") { &["nct6775", "it87", "f71882fg", "nct6683"] }
        else if v.contains("dell") { &["dell_smm_hwmon", "nct6775"] }
        else { &["nct6775", "it87", "nct6683", "f71882fg"] };
    list.to_vec()
}

#[derive(Serialize, Default)]
pub struct Channel {
    id: String, kind: &'static str, label: String, chip: String,
    hw: String, pwm: u32, // hwmon: "hwmon3" + número do PWM; gpu: índice da GPU + índice da ventoinha
    rpm: Option<u32>, percent: Option<f64>, mode: String, controllable: bool, min: u32, max: u32,
    fan: Option<u32>,     // hwmon: qual entrada de tacômetro este PWM move (do teste de mapeamento; sem teste, a de mesmo número)
    tested: bool, responsive: bool, // tested: houve teste de mapeamento; responsive: alguma ventoinha reagiu a este PWM
}

impl Channel {
    pub(crate) fn controllable_pub(&self) -> bool { self.controllable && self.kind == "hwmon" }
    pub(crate) fn has_rpm(&self) -> bool { self.kind == "hwmon" && self.rpm.is_some() }
}

/// Mapeamento PWM→ventoinha por chip, guardado em ~/.config/coreforge/fan-map.json (o número do hwmon muda entre boots; o nome do chip não).
type FanMap = HashMap<String, HashMap<String, Option<u32>>>;

/// Pasta de configuração do CoreForge (~/.config/coreforge).
pub(crate) fn config_dir() -> Option<PathBuf> { map_path().and_then(|p| p.parent().map(PathBuf::from)) }

fn map_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME").ok().map(PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("coreforge/fan-map.json"))
}
fn load_map() -> FanMap { map_path().and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() }
fn save_map(m: &FanMap) { if let Some(p) = map_path() { let _ = fs::create_dir_all(p.parent().unwrap()); let _ = fs::write(p, serde_json::to_string_pretty(m).unwrap_or_default()); } }

/// Canais que o CoreForge colocou em velocidade manual (para poder devolvê-los ao automático). Guardado em ~/.config/coreforge/fan-overrides.json.
#[derive(Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
pub struct Override { kind: String, hw: String, pwm: u32, chip: String }

fn overrides_path() -> Option<PathBuf> { map_path().map(|p| p.with_file_name("fan-overrides.json")) }
fn load_overrides() -> Vec<Override> { overrides_path().and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() }
fn save_overrides(v: &[Override]) { if let Some(p) = overrides_path() { let _ = fs::create_dir_all(p.parent().unwrap()); let _ = fs::write(p, serde_json::to_string(v).unwrap_or_default()); } }

fn track_override(kind: &str, hw: &str, pwm: u32, chip: &str, manual: bool) {
    let mut v = load_overrides();
    v.retain(|o| !(o.kind == kind && o.hw == hw && o.pwm == pwm && o.chip == chip));
    if manual { v.push(Override { kind: kind.into(), hw: hw.into(), pwm, chip: chip.into() }); }
    save_overrides(&v);
}

/// Overrides que ainda valem: o chip continua no mesmo lugar e o canal continua em manual (um reboot ou a BIOS os desfaz).
pub(crate) fn active_overrides() -> Vec<Override> {
    load_overrides().into_iter().filter(|o| match o.kind.as_str() {
        "hwmon" => read(&format!("/sys/class/hwmon/{}/name", o.hw)).as_deref() == Some(o.chip.as_str())
            && read(&format!("/sys/class/hwmon/{}/pwm{}_enable", o.hw, o.pwm)).as_deref() == Some("1"),
        "gpu" => Nvml::init().ok().and_then(|n| n.device_by_index(o.hw.parse().ok()?).ok().and_then(|d| d.fan_control_policy(o.pwm).ok())).is_some_and(|p| matches!(p, FanControlPolicy::Manual)),
        _ => false,
    }).collect()
}

/// Ventoinhas que o CoreForge deixou em manual, com a velocidade de agora: é o que um perfil novo guarda.
pub(crate) fn capture_overrides() -> Vec<crate::profiles::FanSetting> {
    let mut chans = scan_hwmon();
    chans.extend(scan_gpu());
    active_overrides().into_iter().filter_map(|o| {
        let c = chans.iter().find(|c| c.kind == o.kind && c.hw == o.hw && c.pwm == o.pwm && c.chip == o.chip)?;
        Some(crate::profiles::FanSetting { kind: o.kind, hw: o.hw, pwm: o.pwm, chip: o.chip, label: c.label.clone(), percent: Some(c.percent?.round().clamp(HWMON_MIN as f64, 100.0) as u32) })
    }).collect()
}

/// "tipo:hw:pwm:chip:percentual|auto" — formato que o ajudante lê.
pub(crate) fn fan_spec(f: &crate::profiles::FanSetting, hw: &str) -> String {
    format!("{}:{}:{}:{}:{}", f.kind, hw, f.pwm, f.chip, f.percent.map_or("auto".to_string(), |p| p.to_string()))
}

/// Aplica as velocidades de um perfil numa só chamada (uma senha). O número do hwmon é reencontrado pelo nome do chip.
/// Pasta hwmon ("hwmon3") do chip com esse nome: o número muda entre boots, o nome não.
pub(crate) fn find_hwmon(chip: &str) -> Option<String> {
    let mut dirs: Vec<_> = fs::read_dir("/sys/class/hwmon").into_iter().flatten().flatten().map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter().find(|d| read(&format!("{}/name", d.display())).as_deref() == Some(chip)).and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// Registra o canal como "em manual" (para o app poder devolvê-lo ao automático depois).
pub(crate) fn mark_manual(kind: &str, hw: &str, pwm: u32, chip: &str) { track_override(kind, hw, pwm, chip, true); }

pub(crate) fn apply_settings(list: &[crate::profiles::FanSetting]) -> Result<String, String> {
    let mut specs = vec!["fans-apply".to_string()];
    for f in list {
        let hw = if f.kind == "hwmon" {
            find_hwmon(&f.chip).ok_or_else(|| format!("{}: chip {} não encontrado", f.label, f.chip))?
        } else { f.hw.clone() };
        specs.push(fan_spec(f, &hw));
    }
    let out = run_helper(&specs)?;
    for f in list { track_override(&f.kind, &f.hw, f.pwm, &f.chip, f.percent.is_some()); }
    Ok(out)
}

#[tauri::command(async)]
pub fn fan_overrides() -> Vec<Override> { active_overrides() }

/// Devolve ao automático todos os canais que o CoreForge deixou em manual. Uma só senha.
#[tauri::command]
pub async fn fan_restore_all() -> Result<String, String> {
    crate::curves::request_stop(); // o motor de curvas não pode reaplicar velocidades por cima
    let list = active_overrides();
    if list.is_empty() { return Ok("Nada para restaurar.".into()); }
    let mut args = vec!["restore-all".to_string()];
    args.extend(list.iter().map(|o| format!("{}:{}:{}:{}", o.kind, o.hw, o.pwm, o.chip)));
    let out = blocking(move || run_helper(&args)).await??;
    save_overrides(&[]);
    Ok(out)
}

fn friendly_chip(chip: &str) -> String {
    match chip { "amdgpu" => "GPU AMD".into(), "nouveau" => "GPU NVIDIA (nouveau)".into(), "i915" | "xe" => "GPU Intel".into(),
        "thinkpad" => "ThinkPad".into(), "dell_smm" => "Dell".into(), c if c.starts_with("nct") || c.starts_with("it8") || c.starts_with("f718") => format!("Placa-mãe ({c})"), c => c.into() }
}

#[derive(Serialize)]
pub struct FanReport { channels: Vec<Channel>, hints: Vec<String>, modules: Vec<&'static str> }

fn mode_text(enable: Option<u32>) -> String {
    match enable { Some(0) => "Sem controle (100 %)".into(), Some(1) => "Manual".into(), Some(_) => "Automático".into(), None => "—".into() }
}

pub(crate) fn scan_hwmon() -> Vec<Channel> {
    let (mut out, maps) = (vec![], load_map());
    let mut dirs: Vec<_> = fs::read_dir("/sys/class/hwmon").into_iter().flatten().flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let hw = d.file_name().unwrap().to_string_lossy().into_owned();
        let chip = read(&format!("{}/name", d.display())).unwrap_or_default();
        let num = |f: &str| read(&format!("{}/{f}", d.display())).and_then(|s| s.parse::<u32>().ok());
        let fans = fan_rpms(&d.to_string_lossy());
        let pwms: Vec<u32> = (1..=16u32).filter(|n| num(&format!("pwm{n}")).is_some()).collect();
        let tested_map = maps.get(&chip);
        let label_of = |f: u32| read(&format!("{}/fan{f}_label", d.display())).unwrap_or_else(|| format!("Ventoinha {f}"));
        let mut used = std::collections::HashSet::new();
        for &p in &pwms {
            let tested = tested_map.is_some_and(|m| m.contains_key(&p.to_string()));
            let fan = if tested { tested_map.and_then(|m| m.get(&p.to_string()).copied().flatten()) } else { fans.contains_key(&p).then_some(p) };
            if let Some(f) = fan { used.insert(f); }
            let label = match (tested, fan) { (true, None) => format!("Controle PWM {p} (sem ventoinha)"), (_, Some(f)) => label_of(f), _ => format!("Controle PWM {p}") };
            let writable = Path::new(&format!("{}/pwm{p}_enable", d.display())).exists();
            out.push(Channel { id: format!("hwmon/{hw}/{p}"), kind: "hwmon", label: format!("{} · {label}", friendly_chip(&chip)), chip: chip.clone(), hw: hw.clone(), pwm: p,
                rpm: fan.and_then(|f| fans.get(&f).copied()), percent: num(&format!("pwm{p}")).map(|v| v as f64 * 100.0 / 255.0),
                mode: mode_text(num(&format!("pwm{p}_enable"))), controllable: writable, min: HWMON_MIN, max: 100, fan, tested, responsive: fan.is_some() });
        }
        for (&f, &rpm) in &fans { // tacômetros sem PWM associado: só leitura
            if used.contains(&f) { continue; }
            out.push(Channel { id: format!("hwmon/{hw}/fan{f}"), kind: "hwmon", label: format!("{} · {}", friendly_chip(&chip), label_of(f)), chip: chip.clone(), hw: hw.clone(), pwm: 0,
                rpm: Some(rpm), percent: None, mode: "Só leitura".into(), controllable: false, min: HWMON_MIN, max: 100, fan: Some(f), tested: false, responsive: true });
        }
    }
    out
}

fn fan_rpms(dir: &str) -> BTreeMap<u32, u32> {
    (1..=16u32).filter_map(|n| read(&format!("{dir}/fan{n}_input")).and_then(|s| s.parse::<u32>().ok()).map(|r| (n, r))).collect()
}

fn scan_gpu() -> Vec<Channel> {
    let mut out = vec![];
    let Ok(nv) = Nvml::init() else { return out };
    for i in 0..nv.device_count().unwrap_or(0) {
        let Ok(d) = nv.device_by_index(i) else { continue };
        let name = d.name().unwrap_or("GPU NVIDIA".into());
        let (lo, hi) = d.min_max_fan_speed().unwrap_or((30, 100));
        for f in 0..d.num_fans().unwrap_or(0) {
            let policy = d.fan_control_policy(f).ok().map(|p| if matches!(p, FanControlPolicy::Manual) { "Manual" } else { "Automático" }).unwrap_or("—");
            out.push(Channel { id: format!("gpu/{i}/{f}"), kind: "gpu", label: format!("{name} · ventoinha {}", f + 1), chip: "nvidia".into(), hw: i.to_string(), pwm: f,
                rpm: d.fan_speed_rpm(f).ok(), percent: d.fan_speed(f).ok().map(|p| p as f64), mode: policy.into(), controllable: true, min: lo.max(20), max: hi.min(100), fan: None, tested: false, responsive: true });
        }
    }
    out
}

#[tauri::command(async)]
pub fn fans_report() -> FanReport {
    let mut channels = scan_hwmon();
    channels.extend(scan_gpu());
    let mut hints = vec![];
    if !channels.iter().any(|c| c.kind == "hwmon") {
        hints.push("O kernel não expõe nenhuma ventoinha da placa-mãe. Se ela usa um chip Nuvoton (NCT67xx) ou ITE, carregar o driver correspondente costuma liberar RPM e PWM. Se nada aparecer depois, a placa só controla pela BIOS.".into());
    }
    if channels.iter().any(|c| c.kind == "gpu" && c.percent == Some(0.0)) {
        hints.push("A ventoinha da GPU está em 0 %: é o modo silencioso (zero RPM), que a placa usa até esquentar. É normal.".into());
    }
    FanReport { channels, hints, modules: MODULES.to_vec() }
}

const HELPER_PATH: &str = "/usr/libexec/coreforge/coreforge-helper";

/// Onde está o ajudante com root: o instalado pelo pacote (dono root) ou, só em build de desenvolvimento, o que fica ao lado do executável.
/// Nunca se usa um binário de pasta gravável pelo usuário em versão de produção: o pkexec o rodaria como root.
pub(crate) fn helper_path() -> Result<std::path::PathBuf, String> {
    if Path::new(HELPER_PATH).exists() { return Ok(HELPER_PATH.into()); }
    if cfg!(debug_assertions)
        && let Some(p) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("coreforge-helper"))).filter(|p| p.exists()) { return Ok(p); }
    Err("O ajudante de privilégios do CoreForge não está instalado. Instale o pacote .deb (o AppImage não consegue controlar hardware que exige senha).".into())
}

/// Roda uma ação do ajudante via pkexec (o sistema mostra a janela de senha).
pub(crate) fn run_helper(args: &[String]) -> Result<String, String> {
    let exe = helper_path()?;
    let out = Command::new("pkexec").arg(exe).args(args).output().map_err(|e| format!("pkexec indisponível: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    match out.status.code() {
        Some(0) => Ok(text),
        Some(126) | Some(127) => Err("Autenticação cancelada".into()),
        _ => Err(if text.is_empty() { "Falhou".into() } else { text }),
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// Define a velocidade (`percent`) ou devolve o controle ao automático (`None`). Pede a senha do sistema.
#[tauri::command]
pub async fn fan_apply(kind: String, hw: String, pwm: u32, chip: String, percent: Option<u32>) -> Result<String, String> {
    if !["hwmon", "gpu"].contains(&kind.as_str()) { return Err("Tipo inválido".into()); }
    if let Some(p) = percent && !(HWMON_MIN..=100).contains(&p) { return Err(format!("Use um valor entre {HWMON_MIN} % e 100 %")); }
    let mut args = vec![if percent.is_some() { format!("{kind}-set") } else { format!("{kind}-auto") }, hw.clone(), pwm.to_string(), chip.clone()];
    if let Some(p) = percent { args.push(p.to_string()); }
    let out = blocking(move || run_helper(&args)).await??;
    track_override(&kind, &hw, pwm, &chip, percent.is_some());
    Ok(out)
}

#[tauri::command]
pub async fn fan_load_module(name: String) -> Result<String, String> {
    if !MODULES.contains(&name.as_str()) { return Err("Módulo fora da lista".into()); }
    let n = name.clone();
    blocking(move || run_helper(&["modprobe".into(), n])).await?.map(|_| format!("{name} carregado. Reabra a página para ver as ventoinhas novas."))
}

/// Faz o driver (já carregado) valer também depois de reiniciar.
#[tauri::command]
pub async fn fan_persist_module(name: String) -> Result<String, String> {
    if !module_allowed(&name) { return Err("Módulo fora da lista".into()); }
    blocking(move || run_helper(&["persist-module".into(), name])).await?
}

/// Procura sozinho o driver de ventoinhas da placa: tenta os candidatos do fabricante e mantém só o que faz aparecer ventoinhas.
#[tauri::command]
pub async fn fan_scan_modules() -> Result<String, String> { blocking(|| run_helper(&["scan-modules".into()])).await? }

/// Descobre qual PWM move qual ventoinha neste chip e guarda o resultado. Dura cerca de 8 s por PWM.
#[tauri::command]
pub async fn fan_map_test(hw: String, chip: String) -> Result<String, String> {
    let out = blocking(move || run_helper(&["map-test".into(), hw, chip])).await??;
    let v: serde_json::Value = serde_json::from_str(&out).map_err(|_| format!("resposta inesperada do ajudante: {out}"))?;
    let (chip, map) = (v["chip"].as_str().ok_or("sem chip")?.to_string(), v["map"].as_object().ok_or("sem mapa")?);
    let mut all = load_map();
    all.insert(chip, map.iter().map(|(k, f)| (k.clone(), f.as_u64().map(|n| n as u32))).collect());
    save_map(&all);
    let moved = map.values().filter(|f| !f.is_null()).count();
    Ok(format!("{moved} de {} controles movem uma ventoinha; os outros não têm ventoinha ligada.", map.len()))
}

// ------------------------------------------------------------------ ajudante com root (chamado só via pkexec)

fn valid_hw(s: &str) -> bool { s.strip_prefix("hwmon").is_some_and(|n| !n.is_empty() && n.len() < 4 && n.chars().all(|c| c.is_ascii_digit())) }

fn hwmon_write(dir: &str, file: &str, value: u32) -> Result<(), String> { fs::write(format!("{dir}/{file}"), value.to_string()).map_err(|e| format!("{file}: {e}")) }

/// Módulos de leitura de temperatura (CPU e discos) que o app pode carregar além dos de ventoinha.
pub(crate) const SENSOR_MODULES: &[&str] = &["k10temp", "coretemp", "drivetemp"];

fn module_allowed(name: &str) -> bool { MODULES.contains(&name) || SENSOR_MODULES.contains(&name) || name == "i2c-dev" }

fn helper_modprobe(name: &str) -> Result<String, String> {
    if !module_allowed(name) { return Err("módulo fora da lista".into()); }
    let o = Command::new("modprobe").arg(name).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(format!("{name} carregado")) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

/// Nome de usuário de um uid (lido de /etc/passwd).
fn user_of_uid(uid: u32) -> Option<String> {
    read("/etc/passwd")?.lines().find_map(|l| { let f: Vec<&str> = l.split(':').collect(); (f.get(2)?.parse::<u32>().ok()? == uid).then(|| f[0].to_string()) })
}

/// Adiciona ao grupo de portas seriais (dialout, ou uucp no Arch) só quem chamou o pkexec (PKEXEC_UID); nunca um nome vindo de argumento.
fn helper_dialout() -> Result<String, String> {
    let uid: u32 = std::env::var("PKEXEC_UID").ok().and_then(|u| u.parse().ok()).ok_or("sem PKEXEC_UID")?;
    let user = user_of_uid(uid).filter(|u| u != "root").ok_or("usuário inválido")?;
    let groups = read("/etc/group").unwrap_or_default();
    let group = ["dialout", "uucp"].into_iter().find(|g| groups.lines().any(|l| l.split(':').next() == Some(*g))).ok_or("nenhum grupo de portas seriais encontrado")?;
    let o = Command::new("usermod").args(["-aG", group, &user]).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(format!("{user} adicionado ao grupo {group}")) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

fn helper_remove_brltty() -> Result<String, String> {
    let (cmd, args): (&str, &[&str]) = if Path::new("/usr/bin/apt-get").exists() { ("apt-get", &["remove", "-y", "brltty"]) }
        else if Path::new("/usr/bin/dnf").exists() { ("dnf", &["remove", "-y", "brltty"]) }
        else if Path::new("/usr/bin/pacman").exists() { ("pacman", &["-R", "--noconfirm", "brltty"]) }
        else { return Err("gerenciador de pacotes não reconhecido; remova o brltty manualmente".into()) };
    let o = Command::new(cmd).args(args).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok("brltty removido".into()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
}

/// Pacotes que o app pode instalar (nome igual em apt, dnf e pacman).
pub(crate) const INSTALLABLE: &[&str] = &["smartmontools"];

/// Comando de instalação do gerenciador de pacotes da distribuição, ou None se não reconhecido.
pub(crate) fn install_command(pkg: &str, has: impl Fn(&str) -> bool) -> Option<(&'static str, Vec<String>)> {
    if !INSTALLABLE.contains(&pkg) { return None; }
    if has("/usr/bin/apt-get") { Some(("apt-get", vec!["install".into(), "-y".into(), pkg.into()])) }
    else if has("/usr/bin/dnf") { Some(("dnf", vec!["install".into(), "-y".into(), pkg.into()])) }
    else if has("/usr/bin/pacman") { Some(("pacman", vec!["-S".into(), "--noconfirm".into(), pkg.into()])) }
    else if has("/usr/bin/zypper") { Some(("zypper", vec!["--non-interactive".into(), "install".into(), pkg.into()])) }
    else { None }
}

/// Comando que o usuário digitaria para instalar `pkg` na distribuição dele (só para mostrar numa dica).
pub(crate) fn install_hint(pkg: &str) -> String {
    let has = |p: &str| Path::new(p).exists();
    let cmd = if has("/usr/bin/dnf") { "dnf install" } else if has("/usr/bin/pacman") { "pacman -S" }
        else if has("/usr/bin/zypper") { "zypper install" } else { "apt install" };
    format!("sudo {cmd} {pkg}")
}

fn helper_install_package(pkg: &str) -> Result<String, String> {
    let (cmd, args) = install_command(pkg, |p| Path::new(p).exists()).ok_or("pacote fora da lista ou gerenciador de pacotes não reconhecido")?;
    let o = Command::new(cmd).args(&args).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(format!("{pkg} instalado")) } else { Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("falhou").to_string()) }
}

/// Faz um módulo carregar em todo boot (arquivo em /etc/modules-load.d). Só módulos da lista; só quando o usuário pede.
fn helper_persist_module(name: &str) -> Result<String, String> {
    if !module_allowed(name) { return Err("módulo fora da lista".into()); }
    fs::write(format!("/etc/modules-load.d/coreforge-{name}.conf"), format!("{name}\n")).map_err(|e| e.to_string())?;
    Ok(format!("{name} vai carregar em todo boot"))
}

fn fan_files_count() -> usize {
    fs::read_dir("/sys/class/hwmon").into_iter().flatten().flatten().map(|e| {
        fs::read_dir(e.path()).into_iter().flatten().flatten().filter(|f| { let n = f.file_name().to_string_lossy().into_owned(); (n.starts_with("fan") && n.ends_with("_input")) || (n.starts_with("pwm") && n[3..].chars().all(|c| c.is_ascii_digit())) }).count()
    }).sum()
}

/// Tenta os drivers candidatos do fabricante. Linhas de saída: vendor:, already:, missing:, kept:, none:, error:.
fn helper_scan_modules() -> Result<String, String> {
    let vendor = read("/sys/class/dmi/id/board_vendor").or_else(|| read("/sys/class/dmi/id/sys_vendor")).unwrap_or_default();
    let mut lines = vec![format!("vendor:{vendor}")];
    for m in candidates(&vendor) {
        if Path::new(&format!("/sys/module/{m}")).exists() { lines.push(format!("already:{m}")); continue; }
        if !Command::new("modinfo").args(["-n", m]).output().is_ok_and(|o| o.status.success()) { lines.push(format!("missing:{m}")); continue; }
        let before = fan_files_count();
        match Command::new("modprobe").arg(m).output() {
            Ok(o) if o.status.success() => {
                thread::sleep(Duration::from_millis(700));
                if fan_files_count() > before { lines.push(format!("kept:{m}")); break; }
                let _ = Command::new("modprobe").args(["-r", m]).output(); // não ajudou: descarrega
                lines.push(format!("none:{m}"));
            }
            Ok(o) => lines.push(format!("error:{m}:{}", String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("falhou").trim())),
            Err(e) => lines.push(format!("error:{m}:{e}")),
        }
    }
    Ok(lines.join("\n"))
}

/// Decide qual ventoinha reagiu: a que mais perdeu RPM entre 100 % e o nível baixo (mínimo de 20 % e 100 RPM, e que estivesse girando).
pub fn decide_map(base: &BTreeMap<u32, u32>, low: &BTreeMap<u32, u32>) -> Option<u32> {
    base.iter().filter_map(|(f, &b)| { let l = *low.get(f)?; let d = b.checked_sub(l)?; (b >= 300 && d >= 100 && d * 100 >= b * 20).then_some((*f, d)) }).max_by_key(|&(_, d)| d).map(|(f, _)| f)
}

/// Restaura o estado original do PWM mesmo se o teste falhar no meio.
struct Restore { dir: String, pwm: u32, enable: u32, value: u32 }
impl Drop for Restore {
    fn drop(&mut self) {
        let w = |f: String, v: u32| { let _ = fs::write(format!("{}/{f}", self.dir), v.to_string()); };
        w(format!("pwm{}_enable", self.pwm), 1); w(format!("pwm{}", self.pwm), self.value); w(format!("pwm{}_enable", self.pwm), self.enable);
    }
}

fn helper_map_test(hw: &str, chip: &str) -> Result<String, String> {
    if !valid_hw(hw) { return Err("canal inválido".into()); }
    let dir = format!("/sys/class/hwmon/{hw}");
    if read(&format!("{dir}/name")).as_deref() != Some(chip) { return Err("o chip mudou de lugar; atualize a página".into()); }
    if crate::sysinfo::cpu_temp().is_some_and(|t| t >= 70.0) { return Err("A CPU está quente (70 °C ou mais). Espere esfriar e deixe o PC ocioso antes do teste.".into()); }
    let num = |f: &str| read(&format!("{dir}/{f}")).and_then(|s| s.parse::<u32>().ok());
    let mut map = serde_json::Map::new();
    for p in 1..=16u32 {
        let (Some(value), Some(enable)) = (num(&format!("pwm{p}")), num(&format!("pwm{p}_enable"))) else { continue };
        let _guard = Restore { dir: dir.clone(), pwm: p, enable, value };
        hwmon_write(&dir, &format!("pwm{p}_enable"), 1)?;
        hwmon_write(&dir, &format!("pwm{p}"), 255)?;
        thread::sleep(Duration::from_millis(3500));
        let base = fan_rpms(&dir);
        hwmon_write(&dir, &format!("pwm{p}"), 90)?; // ~35 %
        thread::sleep(Duration::from_millis(4500));
        let low = fan_rpms(&dir);
        map.insert(p.to_string(), decide_map(&base, &low).map_or(serde_json::Value::Null, |f| f.into()));
    }
    Ok(serde_json::json!({ "chip": chip, "map": map }).to_string())
}

/// Executa uma ação fixa como root. Retorna o código de saída do processo.
/// Devolve ao automático todos os canais pedidos ("tipo:hw:pwm:chip"), numa só chamada (uma só senha).
fn helper_restore_all(specs: &[String]) -> Result<String, String> {
    let mut done = 0;
    let mut errs = vec![];
    for sp in specs {
        let f: Vec<&str> = sp.splitn(4, ':').collect();
        let [kind, hw, pwm, chip] = f[..] else { errs.push(format!("{sp}: formato inválido")); continue };
        if !["hwmon", "gpu"].contains(&kind) { errs.push(format!("{sp}: tipo inválido")); continue; }
        match run_action(&[format!("{kind}-auto"), hw.into(), pwm.into(), chip.into()]) { Ok(_) => done += 1, Err(e) => errs.push(format!("{hw}/{pwm}: {e}")) }
    }
    if errs.is_empty() { Ok(format!("{done} canal(is) devolvido(s) ao automático")) } else { Err(format!("{done} ok; falhas: {}", errs.join("; "))) }
}

/// Aplica várias velocidades ("tipo:hw:pwm:chip:percentual|auto") numa só chamada; cada uma passa pelas mesmas validações das ações isoladas.
fn helper_fans_apply(specs: &[String]) -> Result<String, String> {
    let (mut done, mut errs) = (0, vec![]);
    for sp in specs {
        let f: Vec<&str> = sp.splitn(5, ':').collect();
        let [kind, hw, pwm, chip, pct] = f[..] else { errs.push(format!("{sp}: formato inválido")); continue };
        if !["hwmon", "gpu"].contains(&kind) { errs.push(format!("{sp}: tipo inválido")); continue; }
        let mut args = vec![format!("{kind}-{}", if pct == "auto" { "auto" } else { "set" }), hw.into(), pwm.into(), chip.into()];
        if pct != "auto" { args.push(pct.into()); }
        match run_action(&args) { Ok(_) => done += 1, Err(e) => errs.push(format!("{hw}/{pwm}: {e}")) }
    }
    if errs.is_empty() { Ok(format!("{done} ventoinha(s) ajustada(s)")) } else { Err(format!("{done} ok; falhas: {}", errs.join("; "))) }
}

pub(crate) fn run_action(a: &[String]) -> Result<String, String> {
        match a.first().map(String::as_str) {
            Some("modprobe") => return helper_modprobe(a.get(1).map(String::as_str).unwrap_or("")),
            Some("dialout") => return helper_dialout(),
            Some("remove-brltty") => return helper_remove_brltty(),
            Some("scan-modules") => return helper_scan_modules(),
            Some("install-package") => return helper_install_package(a.get(1).map(String::as_str).unwrap_or("")),
            Some("persist-module") => return helper_persist_module(a.get(1).map(String::as_str).unwrap_or("")),
            Some("install-rgb-rules") => return crate::leds::helper_install_rules(&a[1..]),
            Some("restore-all") => return helper_restore_all(&a[1..]),
            Some("fans-apply") => return helper_fans_apply(&a[1..]),
            Some("curve-run") => return match (a.get(1), a.get(2).and_then(|p| p.parse().ok())) { (Some(path), Some(pid)) => crate::curves::daemon(path, pid), _ => Err("argumentos faltando".into()) },
            Some("map-test") => return helper_map_test(a.get(1).map(String::as_str).unwrap_or(""), a.get(2).map(String::as_str).unwrap_or("")),
            _ => {}
        }
        let [action, hw, pwm, chip, rest @ ..] = a else { return Err("argumentos faltando".into()) };
        let pwm: u32 = pwm.parse().map_err(|_| "pwm inválido")?;
        let percent: Option<u32> = rest.first().map(|p| p.parse().map_err(|_| "percentual inválido")).transpose()?;
        if let Some(p) = percent && !(HWMON_MIN..=100).contains(&p) { return Err("percentual fora do limite".into()); }
        match action.as_str() {
            "hwmon-set" | "hwmon-auto" => {
                if !valid_hw(hw) || !(1..=16).contains(&pwm) { return Err("canal inválido".into()); }
                let dir = format!("/sys/class/hwmon/{hw}");
                if read(&format!("{dir}/name")).as_deref() != Some(chip.as_str()) { return Err("o chip mudou de lugar; atualize a página".into()); }
                if action == "hwmon-set" {
                    let p = percent.ok_or("percentual faltando")?;
                    hwmon_write(&dir, &format!("pwm{pwm}_enable"), 1)?;
                    hwmon_write(&dir, &format!("pwm{pwm}"), (p * 255 + 50) / 100)?;
                    Ok(format!("pwm{pwm} em {p} %"))
                } else {
                    // 5 = SmartFan (NCT67xx), 2 = automático por temperatura (ITE e outros)
                    hwmon_write(&dir, &format!("pwm{pwm}_enable"), 5).or_else(|_| hwmon_write(&dir, &format!("pwm{pwm}_enable"), 2))?;
                    Ok("controle automático".into())
                }
            }
            "gpu-set" | "gpu-auto" => {
                let idx: u32 = hw.parse().map_err(|_| "GPU inválida")?;
                let nv = Nvml::init().map_err(|e| e.to_string())?;
                let mut d = nv.device_by_index(idx).map_err(|e| e.to_string())?;
                if action == "gpu-set" {
                    let p = percent.ok_or("percentual faltando")?;
                    d.set_fan_control_policy(pwm, FanControlPolicy::Manual).map_err(|e| e.to_string())?;
                    d.set_fan_speed(pwm, p).map_err(|e| e.to_string())?;
                    Ok(format!("ventoinha {} em {p} %", pwm + 1))
                } else { d.set_default_fan_speed(pwm).map_err(|e| e.to_string())?; Ok("controle automático".into()) }
            }
            _ => Err("ação desconhecida".into()),
        }
}

/// Executa uma ação fixa como root. Retorna o código de saída do processo.
pub fn root_helper(a: &[String]) -> i32 {
    match run_action(a) { Ok(m) => { println!("{m}"); 0 } Err(e) => { eprintln!("{e}"); 1 } }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }
    #[test]
    fn helper_rejects_bad_input_without_touching_anything() {
        assert_eq!(root_helper(&s(&["hwmon-set", "../../etc", "1", "x", "50"])), 1);        // caminho fora do hwmon
        assert_eq!(root_helper(&s(&["hwmon-set", "hwmon1", "1", "k10temp", "5"])), 1);       // abaixo do piso
        assert_eq!(root_helper(&s(&["hwmon-set", "hwmon1", "99", "k10temp", "50"])), 1);     // pwm fora da faixa
        assert_eq!(root_helper(&s(&["hwmon-set", "hwmon1", "1", "errado", "50"])), 1);       // nome do chip não confere
        assert_eq!(root_helper(&s(&["rm", "hwmon1", "1", "x"])), 1);                         // ação desconhecida
        assert_eq!(root_helper(&s(&["restore-all", "hwmon1:../x:1:k10temp", "disk:hwmon1:1:x"])), 1); // specs inválidas
        assert_eq!(root_helper(&s(&["fans-apply", "hwmon:../x:1:k10temp:50", "hwmon:hwmon1:1:k10temp:5", "x:hwmon1:1:c:50", "lixo"])), 1); // caminho, abaixo do piso, tipo e formato
        assert_eq!(root_helper(&s(&["modprobe", "rm -rf"])), 1);                             // módulo fora da lista
        assert_eq!(root_helper(&s(&["dialout"])), 1);                                        // sem PKEXEC_UID não faz nada
        assert_eq!(root_helper(&s(&["map-test", "../x", "a"])), 1);                          // hwmon inválido
        assert_eq!(root_helper(&s(&["install-package", "rm"])), 1);                          // pacote fora da lista
        assert_eq!(root_helper(&s(&["persist-module", "../../etc/x"])), 1);                  // módulo fora da lista
        assert!(valid_hw("hwmon12") && !valid_hw("hwmon") && !valid_hw("hwmon1/../x") && !valid_hw("hwmon12345"));
    }
    #[test]
    fn mapping_decision_and_candidates() {
        let m = |v: &[(u32, u32)]| v.iter().copied().collect::<BTreeMap<u32, u32>>();
        assert_eq!(decide_map(&m(&[(1, 0), (2, 1100), (3, 1600)]), &m(&[(1, 0), (2, 600), (3, 1580)])), Some(2)); // a 2 desacelerou
        assert_eq!(decide_map(&m(&[(1, 0), (7, 0)]), &m(&[(1, 0), (7, 0)])), None);                                  // nada girando
        assert_eq!(decide_map(&m(&[(2, 1100)]), &m(&[(2, 1050)])), None);                                            // queda pequena: ruído
        assert_eq!(candidates("ASUSTeK COMPUTER INC.")[0], "nct6775");
        assert_eq!(candidates("Gigabyte Technology Co., Ltd.")[0], "it87");
        assert!(candidates("Desconhecido").iter().all(|c| MODULES.contains(c)));
        assert!(["ASUS", "MSI", "Gigabyte", "ASRock", "Dell Inc.", "X"].iter().all(|v| candidates(v).iter().all(|c| MODULES.contains(c))));
    }
    #[test]
    fn install_command_per_distro() {
        let only = |p: &'static str| move |x: &str| x == p;
        assert_eq!(install_command("smartmontools", only("/usr/bin/apt-get")).unwrap().0, "apt-get");
        assert_eq!(install_command("smartmontools", only("/usr/bin/dnf")).unwrap().0, "dnf");
        assert_eq!(install_command("smartmontools", only("/usr/bin/pacman")).unwrap().1, ["-S", "--noconfirm", "smartmontools"]);
        assert!(install_command("smartmontools", |_| false).is_none());
        assert!(install_command("bash", only("/usr/bin/apt-get")).is_none());
    }
    #[test]
    fn overrides_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("cf_ov_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &tmp) };
        track_override("hwmon", "hwmon9", 2, "chip", true);
        track_override("hwmon", "hwmon9", 2, "chip", true); // sem duplicar
        assert_eq!(load_overrides().len(), 1);
        assert!(active_overrides().is_empty()); // hwmon9 não existe: não vale mais
        track_override("hwmon", "hwmon9", 2, "chip", false);
        assert!(load_overrides().is_empty());
        let _ = fs::remove_dir_all(tmp);
    }
    #[test]
    fn fan_spec_format() {
        let f = crate::profiles::FanSetting { kind: "hwmon".into(), hw: "hwmon3".into(), pwm: 2, chip: "nct6798".into(), label: "x".into(), percent: Some(60) };
        assert_eq!(fan_spec(&f, "hwmon7"), "hwmon:hwmon7:2:nct6798:60");
        assert_eq!(fan_spec(&crate::profiles::FanSetting { percent: None, ..f }, "hwmon3"), "hwmon:hwmon3:2:nct6798:auto");
    }
    #[test]
    fn report_on_this_machine() {
        let r = fans_report();
        for c in &r.channels { println!("{} · {} · rpm {:?} · {:?} % · {} · controlável {}", c.id, c.label, c.rpm, c.percent, c.mode, c.controllable); }
        println!("{:?}", r.hints);
    }
}
