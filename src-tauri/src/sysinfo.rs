//! Informações do sistema e sensores no Linux: /etc/os-release, /proc, /sys (dmi, hwmon, cpufreq), lspci e lsblk.
use serde::Serialize;
use std::{collections::HashMap, fs, process::Command, sync::Mutex};

#[derive(Serialize, Default)]
pub struct InfoNode {
    pub(crate) header: String,
    value: String,
    pub(crate) children: Vec<InfoNode>,
}

impl InfoNode {
    pub(crate) fn new(h: &str) -> Self { Self { header: h.into(), ..Default::default() } }
    pub(crate) fn add(&mut self, h: &str, v: impl Into<String>) { self.children.push(Self { header: h.into(), value: v.into(), children: vec![] }); }
}

#[derive(Serialize)]
pub struct Sensor {
    id: String,
    hardware: String,
    category: &'static str,
    name: String,
    value: f64,
    unit: &'static str,
}

pub(crate) fn read(p: &str) -> Option<String> { fs::read_to_string(p).ok().map(|s| s.trim().to_string()) }

fn kv(text: &str, sep: char) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once(sep))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect()
}

pub(crate) fn kv_pub(text: &str) -> HashMap<String, String> { kv(text, ':') }

fn run(exe: &str, args: &[&str]) -> String {
    Command::new(exe).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn gb(kb: Option<&String>) -> String {
    kb.and_then(|v| v.trim_end_matches("kB").trim().parse::<f64>().ok()).map_or("—".into(), |k| format!("{:.1} GB", k / 1048576.0))
}

#[tauri::command(async)]
pub fn system_info() -> Vec<InfoNode> {
    let mut os = InfoNode::new("Sistema operacional");
    let rel = kv(&read("/etc/os-release").unwrap_or_default(), '=');
    os.add("Distribuição", rel.get("PRETTY_NAME").cloned().unwrap_or("Linux".into()));
    os.add("Kernel", read("/proc/sys/kernel/osrelease").unwrap_or("—".into()));
    os.add("Arquitetura", std::env::consts::ARCH);
    os.add("Host", read("/proc/sys/kernel/hostname").unwrap_or("—".into()));
    os.add("Sessão", std::env::var("XDG_SESSION_TYPE").unwrap_or("—".into()));
    os.add("Desktop", std::env::var("XDG_CURRENT_DESKTOP").unwrap_or("—".into()));
    if let Some(up) = read("/proc/uptime").and_then(|s| s.split(' ').next().and_then(|x| x.parse::<f64>().ok())) {
        let s = up as u64;
        os.add("Ligado há", format!("{}d {:02}:{:02}:{:02}", s / 86400, s / 3600 % 24, s / 60 % 60, s % 60));
    }

    let mut board = InfoNode::new("Placa-mãe e BIOS");
    for (label, f) in [("Fabricante", "board_vendor"), ("Modelo", "board_name"), ("Versão", "board_version"),
                       ("BIOS", "bios_vendor"), ("Versão da BIOS", "bios_version"), ("Data da BIOS", "bios_date")] {
        if let Some(v) = read(&format!("/sys/class/dmi/id/{f}")).filter(|v| !v.is_empty()) { board.add(label, v); }
    }
    if board.children.is_empty() { board.add("Sem dados DMI", "(máquina virtual ou sem /sys/class/dmi)"); }

    let mut cpu = InfoNode::new("Processador");
    let info = read("/proc/cpuinfo").unwrap_or_default();
    let blocks: Vec<HashMap<String, String>> = info.split("\n\n").filter(|b| !b.trim().is_empty()).map(|b| kv(b, ':')).collect();
    let first = blocks.first().cloned().unwrap_or_default();
    let g = |k: &str| first.get(k).cloned().unwrap_or("—".into());
    cpu.add("Modelo", g("model name"));
    cpu.add("Fabricante", g("vendor_id"));
    cpu.add("Threads", blocks.len().to_string());
    let cores: std::collections::HashSet<_> = blocks.iter()
        .map(|b| (b.get("physical id").cloned().unwrap_or_default(), b.get("core id").cloned().unwrap_or_default())).collect();
    cpu.add("Núcleos físicos", cores.len().to_string());
    cpu.add("Cache", g("cache size"));
    if let Some(m) = read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq").and_then(|s| s.parse::<f64>().ok()) {
        cpu.add("Clock máx.", format!("{:.0} MHz", m / 1000.0));
    }
    cpu.add("Governor", read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").unwrap_or("—".into()));
    let flags = g("flags");
    let have: Vec<&str> = ["sse4_2", "avx", "avx2", "avx512f", "aes", "fma", "vmx", "svm"].into_iter().filter(|f| flags.split(' ').any(|x| x == *f)).collect();
    cpu.add("Instruções", have.join(" "));

    let mut mem = InfoNode::new("Memória");
    let m = kv(&read("/proc/meminfo").unwrap_or_default(), ':');
    mem.add("Total", gb(m.get("MemTotal")));
    mem.add("Disponível", gb(m.get("MemAvailable")));
    mem.add("Swap", gb(m.get("SwapTotal")));
    // módulos (tipo/velocidade/slots) pedem dmidecode (root); fica para quando houver fluxo com pkexec.

    let mut gpu = InfoNode::new("Placa de vídeo");
    for line in run("lspci", &["-mm"]).lines() {
        let f = split_quoted(line);
        if f.len() >= 4 && ["VGA", "3D", "Display"].iter().any(|k| f[1].contains(k)) {
            let mut n = InfoNode::new(&format!("{} {}", f[2], f[3]));
            n.add("Slot PCI", f[0].clone());
            let drv = fs::read_link(format!("/sys/bus/pci/devices/0000:{}/driver", f[0])).ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned())).unwrap_or("—".into());
            n.add("Driver", drv);
            gpu.children.push(n);
        }
    }
    if gpu.children.is_empty() { gpu.add("Nenhuma GPU encontrada", "(lspci indisponível?)"); }

    let mut disks = InfoNode::new("Armazenamento");
    for line in run("lsblk", &["-dn", "-b", "-P", "-o", "NAME,MODEL,SIZE,ROTA,TRAN,TYPE"]).lines() {
        let k = pairs(line);
        if k.get("TYPE").map(String::as_str) != Some("disk") { continue; }
        let name = format!("{} (/dev/{})", k.get("MODEL").map(|s| s.trim()).unwrap_or(""), k.get("NAME").map(String::as_str).unwrap_or(""));
        let mut d = InfoNode::new(name.trim());
        if let Some(b) = k.get("SIZE").and_then(|s| s.parse::<f64>().ok()) { d.add("Tamanho", format!("{:.0} GB", b / 1e9)); }
        let tran = k.get("TRAN").map(String::as_str).unwrap_or("");
        d.add("Tipo", if k.get("ROTA").map(String::as_str) == Some("1") { "HDD" } else if tran == "nvme" { "NVMe" } else { "SSD" });
        d.add("Interface", if tran.is_empty() { "—" } else { tran });
        disks.children.push(d);
    }
    if disks.children.is_empty() { disks.add("Nenhum disco encontrado", "(lsblk indisponível?)"); }

    vec![os, board, cpu, mem, gpu, disks]
}

/// Campos de `lspci -mm`: valores entre aspas ou palavras soltas.
fn split_quoted(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut q) = (vec![], String::new(), false);
    for c in line.chars() {
        match c {
            '"' => q = !q,
            ' ' if !q => { if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

/// Pares KEY="value" de `lsblk -P`.
pub(crate) fn pairs(line: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut rest = line;
    while let Some((k, r)) = rest.split_once("=\"") {
        let Some((v, r2)) = r.split_once('"') else { break };
        m.insert(k.trim().to_string(), v.to_string());
        rest = r2;
    }
    m
}

/// Último (idle, total) lido de /proc/stat por CPU, para calcular a carga entre duas leituras.
#[derive(Default)]
pub struct CpuPrev(Mutex<Vec<(u64, u64)>>);

#[tauri::command]
pub fn sensors(prev: tauri::State<CpuPrev>) -> Vec<Sensor> {
    let mut out = vec![];
    let mut hw: Vec<_> = fs::read_dir("/sys/class/hwmon").into_iter().flatten().flatten().map(|e| e.path()).collect();
    hw.sort();
    for dir in hw {
        let base = dir.file_name().unwrap().to_string_lossy().into_owned();
        let chip = read(&format!("{}/name", dir.display())).unwrap_or(base.clone());
        let Ok(files) = fs::read_dir(&dir) else { continue };
        let mut names: Vec<String> = files.flatten().map(|f| f.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with("_input")).collect();
        names.sort();
        for f in names {
            let p = &f[..f.len() - 6];
            let (category, unit, div) = match p {
                x if x.starts_with("temp") => ("Temperatura", "°C", 1000.0),
                x if x.starts_with("fan") => ("Ventoinhas", "RPM", 1.0),
                x if x.starts_with("in") => ("Tensão", "V", 1000.0),
                x if x.starts_with("power") => ("Consumo", "W", 1e6),
                x if x.starts_with("curr") => ("Corrente", "A", 1000.0),
                _ => continue,
            };
            let Some(raw) = read(&format!("{}/{f}", dir.display())).and_then(|s| s.parse::<f64>().ok()) else { continue };
            let label = read(&format!("{}/{p}_label", dir.display())).unwrap_or(p.to_string());
            out.push(Sensor { id: format!("{chip}/{p}/{base}"), hardware: chip.clone(), category, name: label, value: raw / div, unit });
        }
    }

    let stat = read("/proc/stat").unwrap_or_default();
    let mut prev = prev.0.lock().unwrap_or_else(|e| e.into_inner());
    let cur: Vec<(u64, u64)> = stat.lines().filter(|l| l.starts_with("cpu")).map(|l| {
        let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
        (v.get(3).unwrap_or(&0) + v.get(4).unwrap_or(&0), v.iter().sum())
    }).collect();
    if prev.len() == cur.len() {
        for (i, (c, p)) in cur.iter().zip(prev.iter()).enumerate() {
            let (di, dt) = (c.0.saturating_sub(p.0), c.1.saturating_sub(p.1));
            if dt > 0 {
                let name = if i == 0 { "CPU Total".to_string() } else { format!("CPU Core #{i}") };
                out.push(Sensor { id: format!("cpu/load/{i}"), hardware: "CPU".into(), category: "Carga", name, value: 100.0 * (dt - di) as f64 / dt as f64, unit: "%" });
            }
        }
    }
    *prev = cur;

    let mut cpus: Vec<_> = fs::read_dir("/sys/devices/system/cpu").into_iter().flatten().flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_prefix("cpu").and_then(|n| n.parse::<u32>().ok())).collect();
    cpus.sort();
    for n in cpus {
        if let Some(khz) = read(&format!("/sys/devices/system/cpu/cpu{n}/cpufreq/scaling_cur_freq")).and_then(|s| s.parse::<f64>().ok()) {
            out.push(Sensor { id: format!("cpu/clk/{n}"), hardware: "CPU".into(), category: "Clock", name: format!("CPU Core #{}", n + 1), value: khz / 1000.0, unit: "MHz" });
        }
    }
    out
}

/// Temperatura principal da CPU em °C: Package/Tctl/Tdie, senão a maior entre os sensores do chip da CPU.
pub fn cpu_temp() -> Option<f64> {
    let mut best: Option<f64> = None;
    for e in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let d = e.path();
        let chip = read(&format!("{}/name", d.display())).unwrap_or_default();
        if !["k10temp", "coretemp", "zenpower", "cpu_thermal"].contains(&chip.as_str()) { continue; }
        for n in 1..=32 {
            let Some(v) = read(&format!("{}/temp{n}_input", d.display())).and_then(|s| s.parse::<f64>().ok()) else { continue };
            let label = read(&format!("{}/temp{n}_label", d.display())).unwrap_or_default();
            let main = ["Package", "Tctl", "Tdie"].iter().any(|k| label.contains(k));
            if main { return Some(v / 1000.0); }
            best = Some(best.map_or(v / 1000.0, |b| b.max(v / 1000.0)));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    #[test]
    fn collects_info() {
        let n = super::system_info();
        assert_eq!(n.len(), 6);
        assert!(n.iter().all(|x| !x.children.is_empty()));
        assert_eq!(super::pairs(r#"NAME="sda" MODEL="A B" SIZE="5""#)["MODEL"], "A B");
        assert_eq!(super::split_quoted(r#"01:00.0 "VGA" "NVIDIA Corp""#), vec!["01:00.0", "VGA", "NVIDIA Corp"]);
    }
}
