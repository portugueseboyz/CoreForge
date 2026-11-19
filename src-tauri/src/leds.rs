//! LEDs RGB (placa-mãe, ventoinhas, memória, periféricos) pelo OpenRGB, que cobre centenas de dispositivos (inclusive o ASUS Aura).
//! O CoreForge chama a CLI do OpenRGB (nativa ou Flatpak); instalar o OpenRGB e as regras udev é um passo do usuário.
use serde::Serialize;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct LedDevice { pub(crate) index: usize, pub(crate) name: String, kind: String, modes: Vec<String>, active_mode: String, pub(crate) zones: Vec<String>, zone_leds: Vec<usize>, led_names: Vec<Vec<String>> }

#[derive(Serialize)]
pub struct LedStatus { backend: String, installed: bool, pub(crate) devices: Vec<LedDevice>, hints: Vec<String> }

/// Comando do OpenRGB: nativo ou Flatpak.
pub(crate) fn backend() -> Option<(String, Vec<String>)> {
    let on_path = std::env::var("PATH").unwrap_or_default().split(':').any(|d| std::path::Path::new(d).join("openrgb").exists());
    if on_path { return Some(("openrgb".into(), vec![])); }
    let fp = Command::new("flatpak").args(["list", "--columns=application"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("org.openrgb.OpenRGB")).unwrap_or(false);
    fp.then(|| ("flatpak".into(), vec!["run".into(), "org.openrgb.OpenRGB".into()]))
}

fn run_timeout(cmd: &str, args: &[String], secs: u64) -> Result<String, String> {
    let mut child = Command::new(cmd).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if t0.elapsed() > Duration::from_secs(secs) => { let _ = child.kill(); return Err("O OpenRGB demorou demais para responder".into()); }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let (mut o, mut e) = (String::new(), String::new());
    child.stdout.take().map(|mut s| s.read_to_string(&mut o));
    child.stderr.take().map(|mut s| s.read_to_string(&mut e));
    Ok(format!("{o}{e}"))
}

/// Divide "Direct 'Spectrum Cycle' [Static]" em (texto, ativo?): palavras soltas, entre aspas simples, e o modo ativo entre colchetes.
fn tokens(v: &str) -> Vec<(String, bool)> {
    let c: Vec<char> = v.chars().collect();
    let (mut i, mut out) = (0, vec![]);
    while i < c.len() {
        if c[i].is_whitespace() { i += 1; continue; }
        let active = c[i] == '[';
        if active { i += 1; }
        let mut t = String::new();
        if i < c.len() && c[i] == '\'' {
            i += 1;
            while i < c.len() && c[i] != '\'' { t.push(c[i]); i += 1; }
            i += 1;
        } else {
            while i < c.len() && !c[i].is_whitespace() && c[i] != ']' { t.push(c[i]); i += 1; }
        }
        if active && i < c.len() && c[i] == ']' { i += 1; }
        if !t.is_empty() { out.push((t, active)); }
    }
    out
}

/// Lê a saída de `openrgb --list-detailed` (OpenRGB 1.x): "0: Nome" seguido de linhas indentadas "Type:", "Modes:" e "Zones:".
pub fn parse_devices(text: &str) -> Vec<LedDevice> {
    let mut out: Vec<LedDevice> = vec![];
    for line in text.lines() {
        let t = line.trim();
        if !line.starts_with(' ') {
            if let Some((i, name)) = t.split_once(": ")
                && let Ok(index) = i.parse::<usize>() { out.push(LedDevice { index, name: name.trim().into(), ..Default::default() }); }
            continue;
        }
        let Some(d) = out.last_mut() else { continue };
        if let Some(v) = t.strip_prefix("Type:") { d.kind = v.trim().into(); }
        if let Some(v) = t.strip_prefix("Modes:") {
            for (m, active) in tokens(v) { if active { d.active_mode = m.clone(); } d.modes.push(m); }
        }
        if let Some(v) = t.strip_prefix("Zones:") { d.zones = tokens(v).into_iter().map(|(z, _)| z).collect(); }
        if let Some(v) = t.strip_prefix("LEDs:") {
            // Nomes de LED vêm como "Zona, LED 3" ou "Zona, Nome": quantos LEDs cada zona tem (zona endereçável com 0 = tamanho não configurado).
            let names: Vec<String> = tokens(v).into_iter().map(|(n, _)| n).collect();
            d.zone_leds = d.zones.iter().map(|z| names.iter().filter(|n| n.starts_with(&format!("{z}, "))).count()).collect();
            // Nome de cada LED sem o prefixo da zona ("RGB Header 1", "LED 3"), para rotular um por um.
            d.led_names = d.zones.iter().map(|z| names.iter().filter_map(|n| n.strip_prefix(&format!("{z}, ")).map(String::from)).collect()).collect();
        }
    }
    out
}

#[tauri::command(async)]
pub fn leds_status() -> LedStatus {
    let Some((cmd, pre)) = backend() else {
        return LedStatus { backend: String::new(), installed: false, devices: vec![], hints: vec![
            format!("O OpenRGB não está instalado. Instale-o na página Instalar apps (ou com {}) e volte aqui.", crate::fans::install_hint("openrgb"))] };
    };
    let mut args = pre.clone(); args.push("--list-detailed".into());
    match run_timeout(&cmd, &args, 25) {
        Ok(text) => {
            let devices = parse_devices(&text);
            let mut hints = vec![];
            if devices.is_empty() {
                hints.push("O OpenRGB não encontrou dispositivos. Em geral faltam as regras udev (arquivo 60-openrgb.rules do projeto, copiado para /etc/udev/rules.d/, seguido de udevadm control --reload-rules e de reconectar o USB) ou o seu usuário não tem acesso ao /dev/hidraw.".into());
            }
            LedStatus { backend: if cmd == "flatpak" { "OpenRGB (Flatpak)".into() } else { "OpenRGB".into() }, installed: true, devices, hints }
        }
        Err(e) => LedStatus { backend: cmd, installed: true, devices: vec![], hints: vec![e] },
    }
}

fn valid_mode(m: &str) -> bool { !m.is_empty() && !m.starts_with('-') && m.len() <= 32 && m.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_') }

/// Aplica modo, cor (RRGGBB), brilho e velocidade (0 a 100) a um dispositivo. Se a tela informar os nomes, guarda o ajuste para os perfis.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)] // cada argumento é um campo do pedido vindo da interface
pub fn leds_set(device: usize, zone: Option<usize>, size: Option<u32>, mode: Option<String>, color: Option<String>, colors: Option<Vec<String>>, brightness: Option<u32>, speed: Option<u32>,
                device_name: Option<String>, zone_name: Option<String>) -> Result<String, String> {
    let out = leds_run(device, zone, size, mode.clone(), color.clone(), colors.clone(), brightness, speed)?;
    if let Some(device) = device_name {
        crate::profiles::record_led(crate::profiles::LedSetting { device, zone: zone_name, mode, color, colors, brightness, size });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn leds_run(device: usize, zone: Option<usize>, size: Option<u32>, mode: Option<String>, color: Option<String>, colors: Option<Vec<String>>, brightness: Option<u32>, speed: Option<u32>) -> Result<String, String> {
    let (cmd, mut args) = backend().ok_or("OpenRGB não instalado")?;
    args.extend(["--device".into(), device.to_string()]);
    if let Some(z) = zone {
        args.extend(["--zone".into(), z.to_string()]);
        // Conectores ARGB começam com 0 LEDs: sem informar quantos há, nada acende. O OpenRGB guarda o valor.
        if let Some(n) = size { if n > 0 && n <= 1024 { args.extend(["--size".into(), n.to_string()]); } else { return Err("Quantidade de LEDs entre 1 e 1024".into()); } }
    }
    if let Some(m) = mode { if !valid_mode(&m) { return Err("Modo inválido".into()); } args.extend(["--mode".into(), m]); }
    if let Some(list) = colors.filter(|l| !l.is_empty()) {
        // Uma cor por LED, na ordem da zona (o OpenRGB repete a última nos LEDs que sobrarem). Só no modo Direct.
        if list.len() > 1024 { return Err("Cores demais".into()); }
        let mut hex = vec![];
        for c in &list {
            let c = c.trim_start_matches('#');
            if c.len() != 6 || !c.chars().all(|x| x.is_ascii_hexdigit()) { return Err("Cor inválida: use RRGGBB".into()); }
            hex.push(c.to_string());
        }
        args.extend(["--color".into(), hex.join(",")]);
    } else if let Some(c) = color {
        let c = c.trim_start_matches('#').to_string();
        if c.len() != 6 || !c.chars().all(|x| x.is_ascii_hexdigit()) { return Err("Cor inválida: use RRGGBB".into()); }
        args.extend(["--color".into(), c]);
    }
    if let Some(b) = brightness { args.extend(["--brightness".into(), b.min(100).to_string()]); }
    if let Some(s) = speed { args.extend(["--speed".into(), s.min(100).to_string()]); }
    let out = run_timeout(&cmd, &args, 30)?;
    let low = out.to_lowercase();
    if low.contains("error") || low.contains("not found") || low.contains("invalid") { Err(out.trim().to_string()) } else { Ok(out.trim().to_string()) }
}

// ------------------------------------------------------------------ descoberta de hardware RGB

/// Fabricantes de periféricos que costumam ter RGB (ID USB → nome). Serve só para sugerir; quem confirma o suporte é o OpenRGB.
const RGB_VENDORS: &[(&str, &str)] = &[
    ("0b05", "ASUS"), ("1b1c", "Corsair"), ("1532", "Razer"), ("1038", "SteelSeries"), ("046d", "Logitech"), ("0951", "HyperX"), ("03f0", "HP/HyperX"),
    ("1e71", "NZXT"), ("1462", "MSI"), ("1044", "Gigabyte"), ("0414", "Gigabyte"), ("26ce", "ASRock"), ("2516", "Cooler Master"), ("264a", "Thermaltake"),
    ("3842", "EVGA"), ("0cf2", "Lian Li"), ("258a", "Glorious/Redragon"), ("1e7d", "Roccat"), ("0416", "Winbond RGB"), ("1b3f", "Generic RGB"),
];

#[derive(Serialize)]
pub struct HidCandidate { vid_pid: String, vendor: String, name: String, pub(crate) accessible: bool }

#[derive(Serialize)]
pub struct I2cBus { dev: String, adapter: String, kind: &'static str, pub(crate) accessible: bool }

#[derive(Serialize)]
pub struct GpuRgb { pub(crate) name: String, slot: String, pub(crate) ids: String, subsystem: String, board_maker: String, i2c_buses: Vec<String>, i2c_accessible: bool }

#[derive(Serialize)]
pub struct RgbDiscovery { pub(crate) openrgb: String, pub(crate) hid: Vec<HidCandidate>, pub(crate) i2c: Vec<I2cBus>, pub(crate) i2c_dev_loaded: bool, gpus: Vec<GpuRgb>, hints: Vec<String> }

/// Fabricantes de placas de vídeo (ID de subsistema PCI → nome).
const BOARD_MAKERS: &[(&str, &str)] = &[("1043", "ASUS"), ("1462", "MSI"), ("1458", "Gigabyte"), ("3842", "EVGA"), ("19da", "Zotac"), ("1569", "Palit"), ("10b0", "Gainward"),
    ("1682", "XFX"), ("1787", "PowerColor/HIS"), ("148c", "PowerColor"), ("1da2", "Sapphire"), ("1849", "ASRock"), ("196e", "PNY"), ("1048", "Elsa"), ("7377", "Colorful"), ("1b4c", "Galax/KFA2"), ("10de", "NVIDIA (Founders)"), ("1002", "AMD (referência)"), ("172f", "Biostar"), ("1ae0", "Google")];

/// Placas de vídeo: modelo, fabricante da placa (subsistema) e barramentos I²C que pertencem a elas. É o que decide se o OpenRGB pode ter suporte.
pub(crate) fn scan_gpus() -> Vec<GpuRgb> {
    let hex = |p: &str| read_trim(p).trim_start_matches("0x").to_lowercase();
    let buses: Vec<(String, String, String)> = std::fs::read_dir("/sys/class/i2c-dev").into_iter().flatten().flatten().map(|e| {
        let n = e.file_name().to_string_lossy().into_owned();
        let dev = std::fs::canonicalize(format!("/sys/class/i2c-dev/{n}/device")).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        (n.clone(), dev, format!("/dev/{n}"))
    }).collect();
    let mut out = vec![];
    let mut slots: Vec<_> = std::fs::read_dir("/sys/bus/pci/devices").into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    slots.sort();
    for slot in slots {
        let base = format!("/sys/bus/pci/devices/{slot}");
        if !read_trim(&format!("{base}/class")).starts_with("0x03") { continue; } // controladora de vídeo (0300 VGA, 0302 3D)
        let (v, d, sv, sd) = (hex(&format!("{base}/vendor")), hex(&format!("{base}/device")), hex(&format!("{base}/subsystem_vendor")), hex(&format!("{base}/subsystem_device")));
        let short = slot.strip_prefix("0000:").unwrap_or(&slot).to_string();
        let name = std::process::Command::new("lspci").args(["-mm", "-s", &short]).output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|t| t.split('"').nth(5).map(String::from)).unwrap_or_else(|| format!("{v}:{d}"));
        let mine: Vec<&(String, String, String)> = buses.iter().filter(|(_, dev, _)| dev.contains(&format!("/{slot}/"))).collect();
        let mut mine = mine; mine.sort_by_key(|b| b.0.trim_start_matches("i2c-").parse::<u32>().unwrap_or(0));
        out.push(GpuRgb { name, slot: short, ids: format!("{v}:{d}"), subsystem: format!("{sv}:{sd}"),
            board_maker: BOARD_MAKERS.iter().find(|(id, _)| *id == sv).map(|(_, n)| n.to_string()).unwrap_or_else(|| sv.clone()),
            i2c_buses: mine.iter().map(|b| b.2.clone()).collect(), i2c_accessible: !mine.is_empty() && mine.iter().all(|b| can_open(&b.2)) });
    }
    out
}

fn can_open(path: &str) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).custom_flags(0o4000 | 0o400).open(path).is_ok()
}

fn read_trim(p: &str) -> String { std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default() }

/// Barramentos que costumam levar RGB de memória e de placa de vídeo.
fn i2c_kind(adapter: &str) -> Option<(&'static str, &'static str)> {
    let a = adapter.to_lowercase();
    if a.contains("piix4") { Some(("piix4", "SMBus AMD (memória e placa-mãe)")) }
    else if a.contains("i801") { Some(("i801", "SMBus Intel (memória e placa-mãe)")) }
    else if a.contains("nvidia") { Some(("nvidia", "Placa de vídeo NVIDIA")) }
    else if a.contains("amdgpu") || a.contains("radeon") { Some(("amdgpu", "Placa de vídeo AMD")) }
    else { None }
}

/// Procura hardware que pode ter RGB (USB por fabricante, SMBus/I²C) e diz o que falta: OpenRGB, módulo i2c-dev, permissões.
#[tauri::command(async)]
pub fn leds_discover() -> RgbDiscovery {
    let mut hid: Vec<HidCandidate> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut nodes: Vec<_> = std::fs::read_dir("/sys/class/hidraw").into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    nodes.sort();
    for n in &nodes {
        let ue = read_trim(&format!("/sys/class/hidraw/{n}/device/uevent"));
        let field = |k: &str| ue.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).map(String::from)).unwrap_or_default();
        let id: Vec<String> = field("HID_ID").split(':').map(|x| x.trim_start_matches('0').to_lowercase()).collect();
        if id.len() != 3 || field("HID_ID").split(':').next() != Some("0003") { continue; } // só USB
        let (vid, pid) = (format!("{:0>4}", id[1]), format!("{:0>4}", id[2]));
        let Some((_, vendor)) = RGB_VENDORS.iter().find(|(v, _)| *v == vid) else { continue };
        let vp = format!("{vid}:{pid}");
        let ok = can_open(&format!("/dev/{n}"));
        if let Some(c) = hid.iter_mut().find(|c| c.vid_pid == vp) { c.accessible |= ok; continue; }
        if seen.insert(vp.clone()) { hid.push(HidCandidate { vid_pid: vp, vendor: (*vendor).into(), name: field("HID_NAME"), accessible: ok }); }
    }
    let mut i2c = vec![];
    let mut buses: Vec<_> = std::fs::read_dir("/sys/class/i2c-dev").into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    buses.sort_by_key(|b| b.trim_start_matches("i2c-").parse::<u32>().unwrap_or(0));
    for b in buses {
        let adapter = read_trim(&format!("/sys/class/i2c-dev/{b}/name"));
        if let Some((kind, _)) = i2c_kind(&adapter) { let accessible = can_open(&format!("/dev/{b}")); i2c.push(I2cBus { dev: format!("/dev/{b}"), adapter, kind, accessible }); }
    }
    let i2c_dev_loaded = std::path::Path::new("/sys/module/i2c_dev").exists() || std::fs::read_dir("/dev").into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().starts_with("i2c-"));
    let openrgb = match backend() { Some((c, _)) if c == "flatpak" => "Flatpak".into(), Some(_) => "Nativo".into(), None => String::new() };
    let mut hints = vec![];
    if openrgb.is_empty() { hints.push("O OpenRGB não está instalado. Ele é quem fala com os dispositivos RGB; instale-o pela página Instalar apps ou pelo gerenciador de pacotes da sua distribuição.".into()); }
    if hid.iter().any(|c| !c.accessible) { hints.push("Há dispositivos RGB USB sem permissão de acesso para o seu usuário. Libere o acesso abaixo (só para os dispositivos encontrados).".into()); }
    if !i2c_dev_loaded { hints.push("O módulo i2c-dev não está carregado, então memória e placa de vídeo RGB não podem ser vistas. Dá para carregá-lo abaixo.".into()); }
    else if i2c.iter().any(|b| !b.accessible) { hints.push("Os barramentos I²C (memória e placa de vídeo) existem, mas o seu usuário não tem acesso. Liberar isso é opcional e tem um risco, descrito abaixo.".into()); }
    RgbDiscovery { openrgb, hid, i2c, i2c_dev_loaded, gpus: scan_gpus(), hints }
}

/// Relatório de texto para pedir suporte ao OpenRGB (ou conferir o que foi detectado). Roda o OpenRGB com log detalhado (~10 s).
#[tauri::command]
pub async fn leds_report() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let d = leds_discover();
        let mut t = String::from("CoreForge · relatório de hardware RGB\n");
        t += &format!("Kernel: {}\n", read_trim("/proc/sys/kernel/osrelease"));
        t += &format!("OpenRGB: {}\n", if d.openrgb.is_empty() { "não instalado".into() } else { d.openrgb.clone() });
        for g in &d.gpus { t += &format!("GPU: {} · PCI {} · IDs {} · subsistema {} ({}) · I2C {:?} · acesso {}\n", g.name, g.slot, g.ids, g.subsystem, g.board_maker, g.i2c_buses, g.i2c_accessible); }
        for h in &d.hid { t += &format!("USB: {} {} {} · acesso {}\n", h.vendor, h.name, h.vid_pid, h.accessible); }
        if let Some((cmd, pre)) = backend() {
            let mut args = pre; args.extend(["--list-devices".into(), "-vv".into()]);
            if let Ok(out) = run_timeout(&cmd, &args, 40) {
                t += "--- OpenRGB (linhas relevantes) ---\n";
                for l in out.lines().filter(|l| l.contains("Registering") || l.contains("Detected") || l.contains("Error") || l.contains("Warning") || (l.starts_with(|c: char| c.is_ascii_digit()) && l.contains(": "))).take(80) { t += l; t.push('\n'); }
            }
        }
        t
    }).await.map_err(|e| e.to_string())
}

/// Libera o acesso só aos dispositivos escolhidos e/ou carrega o i2c-dev. Pede a senha do sistema.
#[tauri::command]
pub async fn leds_setup(hid: Vec<String>, i2c: Vec<String>, load_i2c: bool) -> Result<String, String> {
    let mut args: Vec<String> = vec!["install-rgb-rules".into()];
    args.extend(hid.iter().map(|h| format!("hid:{h}")));
    args.extend(i2c.iter().map(|k| format!("i2c:{k}")));
    if load_i2c { args.push("load-i2c".into()); }
    rgb_rules_text(&args[1..])?; // valida antes de pedir a senha
    tauri::async_runtime::spawn_blocking(move || crate::fans::run_helper(&args)).await.map_err(|e| e.to_string())?
}

/// Texto do arquivo de regras udev para os itens pedidos. Só aceita IDs USB em hexadecimal e tipos de barramento conhecidos.
pub fn rgb_rules_text(tokens: &[String]) -> Result<String, String> {
    let mut out = String::from("# Gerado pelo CoreForge: acesso do usuário logado (uaccess) só aos dispositivos RGB escolhidos.\n");
    for t in tokens {
        if let Some(v) = t.strip_prefix("hid:") {
            let (vid, pid) = v.split_once(':').ok_or("ID inválido")?;
            let ok = |x: &str| x.len() == 4 && x.chars().all(|c| c.is_ascii_hexdigit());
            if !ok(vid) || !ok(pid) { return Err("ID USB inválido".into()); }
            out += &format!("SUBSYSTEM==\"hidraw\", ATTRS{{idVendor}}==\"{}\", ATTRS{{idProduct}}==\"{}\", TAG+=\"uaccess\"\n", vid.to_lowercase(), pid.to_lowercase());
        } else if let Some(k) = t.strip_prefix("i2c:") {
            let name = match k { "piix4" => "SMBus PIIX4*", "i801" => "SMBus I801*", "nvidia" => "NVIDIA i2c adapter*", "amdgpu" => "AMDGPU*", _ => return Err("barramento desconhecido".into()) };
            out += &format!("SUBSYSTEM==\"i2c-dev\", ATTR{{name}}==\"{name}\", TAG+=\"uaccess\"\n");
        } else if t != "load-i2c" { return Err("item desconhecido".into()); }
    }
    Ok(out)
}

/// Ação do ajudante com root: grava o arquivo de regras, recarrega o udev e (opcional) carrega o i2c-dev agora e em todo boot.
pub fn helper_install_rules(tokens: &[String]) -> Result<String, String> {
    let rules = rgb_rules_text(tokens)?;
    let run = |cmd: &str, args: &[&str]| std::process::Command::new(cmd).args(args).output().map(|o| o.status.success()).unwrap_or(false);
    if rules.lines().count() > 1 {
        std::fs::write("/etc/udev/rules.d/60-coreforge-rgb.rules", rules).map_err(|e| e.to_string())?;
        if !run("udevadm", &["control", "--reload-rules"]) { return Err("udevadm não recarregou as regras".into()); }
        run("udevadm", &["trigger", "--subsystem-match=hidraw", "--subsystem-match=i2c-dev", "--action=change"]);
    }
    if tokens.iter().any(|t| t == "load-i2c") {
        std::fs::write("/etc/modules-load.d/coreforge-i2c-dev.conf", "i2c-dev\n").map_err(|e| e.to_string())?;
        run("modprobe", &["i2c-dev"]);
    }
    Ok("acesso liberado".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_openrgb_listing() {
        let t = "[204       ][Dialog ]<h2>Warning:</h2><p>The OpenRGB udev rules are not installed.</p>\n0: ASUS PRIME A520M-A II\n  Type:           Motherboard\n  Description:    ASUS Aura USB Mainboard Device\n  Modes: [Direct] Off Static Breathing 'Spectrum Cycle' 'Chase Fade'\n  Zones: 'Aura Mainboard' 'Addressable RGB Header 1' 'Addressable RGB Header 2'\n  LEDs: 'Aura Mainboard, RGB Header 1'\n1: Corsair Vengeance\n  Type:           DRAM\n  Modes: Direct ['Spectrum Cycle']\n";
        let d = parse_devices(t);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].index, d[0].name.as_str(), d[0].kind.as_str(), d[0].active_mode.as_str()), (0, "ASUS PRIME A520M-A II", "Motherboard", "Direct"));
        assert_eq!(d[0].modes, ["Direct", "Off", "Static", "Breathing", "Spectrum Cycle", "Chase Fade"]);
        assert_eq!(d[0].zones, ["Aura Mainboard", "Addressable RGB Header 1", "Addressable RGB Header 2"]);
        assert_eq!((d[1].active_mode.as_str(), d[1].modes.len()), ("Spectrum Cycle", 2));
        assert_eq!(d[0].zone_leds, [1, 0, 0]); // zonas ARGB sem LEDs configurados
        assert_eq!(d[0].led_names[0], ["RGB Header 1"]);
    }
    #[test]
    fn rgb_rules_are_generated_only_from_valid_tokens() {
        let t = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let r = rgb_rules_text(&t(&["hid:0B05:19AF", "i2c:piix4", "load-i2c"])).unwrap();
        assert!(r.contains(r#"ATTRS{idVendor}=="0b05", ATTRS{idProduct}=="19af", TAG+="uaccess""#) && r.contains(r#"ATTR{name}=="SMBus PIIX4*""#));
        assert!(rgb_rules_text(&t(&["hid:0b05:19a\"\nRUN+=\"x"])).is_err());   // injeção de regra
        assert!(rgb_rules_text(&t(&["hid:zzzz:0000"])).is_err());
        assert!(rgb_rules_text(&t(&["i2c:../../x"])).is_err());
        assert_eq!(i2c_kind("SMBus PIIX4 adapter port 0 at 0b00").map(|k| k.0), Some("piix4"));
        assert_eq!(i2c_kind("NVIDIA i2c adapter 1 at 1:00.0").map(|k| k.0), Some("nvidia"));
        assert!(i2c_kind("i915 gmbus dpb").is_none());
    }
    #[test]
    #[ignore = "precisa de uma GPU real; rode com --ignored"]
    fn gpu_identified_on_this_machine() {
        let g = scan_gpus();
        for x in &g { println!("{} · {} · {} · sub {} ({}) · {:?} · acesso {}", x.name, x.slot, x.ids, x.subsystem, x.board_maker, x.i2c_buses, x.i2c_accessible); }
        assert!(!g.is_empty());
        assert_eq!(BOARD_MAKERS.iter().find(|(id, _)| *id == "1043").map(|b| b.1), Some("ASUS"));
    }
    #[test]
    fn discovery_on_this_machine() {
        let d = leds_discover();
        println!("openrgb={:?} hid={:?} i2c={:?} i2c_dev={} hints={}", d.openrgb, d.hid.iter().map(|h| (&h.vid_pid, &h.vendor, h.accessible)).collect::<Vec<_>>(), d.i2c.iter().map(|b| (&b.dev, b.kind, b.accessible)).collect::<Vec<_>>(), d.i2c_dev_loaded, d.hints.len());
    }
    #[test]
    fn validates_inputs() {
        assert!(!valid_mode("a;rm -rf") && valid_mode("Color Cycle") && !valid_mode("--profile") && !valid_mode(""));
        // cores por LED: valida cada uma antes de chamar o OpenRGB
        assert!(leds_set(0, Some(0), None, None, None, Some(vec!["FF0000".into(), "zz0000".into()]), None, None, None, None).is_err() || backend().is_none());
        assert!(leds_set(0, None, None, None, Some("12345".into()), None, None, None, None, None).is_err() || backend().is_none());
    }
    #[test]
    fn status_without_openrgb_is_a_hint() { let s = leds_status(); println!("{} {} {:?} {:?}", s.backend, s.installed, s.devices, s.hints); }
}
