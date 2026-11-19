//! GPUs AMD (amdgpu) e Intel (i915/xe) lidas pelo sysfs, sem ferramenta externa: uso, temperatura, clocks, consumo, ventoinha e VRAM.
//! Complementa o caminho da NVIDIA (NVML). A raiz do sysfs é um parâmetro para poder testar com uma árvore falsa.
use crate::sysinfo::InfoNode;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

#[derive(Clone, Debug)]
pub struct SysGpu { pub name: String, pub vendor: &'static str, pub slot: String, pub dev: PathBuf, pub card: PathBuf }

#[derive(Default, Debug, PartialEq)]
pub struct Sample {
    pub util: Option<u32>, pub temp: Option<f64>, pub hotspot: Option<f64>, pub mem_temp: Option<f64>,
    pub core_mhz: Option<u32>, pub mem_mhz: Option<u32>, pub power_w: Option<f64>, pub power_cap_w: Option<f64>,
    pub fan_rpm: Option<u32>, pub fan_pct: Option<u32>, pub vram_used_mb: Option<f64>, pub vram_total_mb: Option<f64>,
}

fn rd(p: &Path) -> Option<String> { fs::read_to_string(p).ok().map(|s| s.trim().to_string()) }
fn num(p: &Path) -> Option<f64> { rd(p)?.parse::<f64>().ok() }

fn hwmon_dir(dev: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dev.join("hwmon")).ok()?.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("hwmon"))).collect();
    v.sort();
    v.into_iter().next()
}

/// Nome do modelo pelo lspci ("Navi 23 [Radeon RX 6600]"); cai num nome genérico se faltar.
fn lspci_name(slot: &str) -> Option<String> {
    let o = Command::new("lspci").args(["-mm", "-s", slot]).output().ok()?;
    let t = String::from_utf8(o.stdout).ok()?;
    let d = t.split('"').nth(5)?.to_string();
    Some(d.split('[').nth(1).map(|s| s.trim_end_matches(']').to_string()).unwrap_or(d))
}

/// Acha GPUs AMD (0x1002) e Intel (0x8086) em `<root>/class/drm/card*`. `namer` resolve o nome pelo slot PCI.
pub fn discover(root: &Path, namer: impl Fn(&str) -> Option<String>) -> Vec<SysGpu> {
    let mut out = vec![];
    let mut cards: Vec<PathBuf> = fs::read_dir(root.join("class/drm")).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| { let n = n.to_string_lossy(); n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit()) })).collect();
    cards.sort();
    for card in cards {
        let dev = card.join("device");
        let vendor = match rd(&dev.join("vendor")).as_deref() { Some("0x1002") => "AMD", Some("0x8086") => "Intel", _ => continue };
        let slot = fs::canonicalize(&dev).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        let short = slot.strip_prefix("0000:").unwrap_or(&slot).to_string();
        let name = namer(&short).unwrap_or_else(|| format!("GPU {vendor}"));
        out.push(SysGpu { name, vendor, slot: short, dev, card });
    }
    out
}

/// GPUs encontradas na máquina (lidas uma vez: o nome vem de um processo externo).
pub fn gpus() -> &'static [SysGpu] {
    static G: OnceLock<Vec<SysGpu>> = OnceLock::new();
    G.get_or_init(|| discover(Path::new("/sys"), lspci_name))
}

/// Clock atual em MHz de um arquivo pp_dpm_*: a linha marcada com `*` ("1: 1905Mhz *").
fn dpm_current(p: &Path) -> Option<u32> {
    rd(p)?.lines().find(|l| l.trim_end().ends_with('*'))
        .and_then(|l| l.split(':').nth(1)).and_then(|r| r.trim().trim_end_matches('*').trim().to_lowercase().trim_end_matches("mhz").trim().parse::<u32>().ok())
}

pub fn sample(g: &SysGpu) -> Sample {
    let mut s = Sample::default();
    let hw = hwmon_dir(&g.dev);
    let h = |f: &str| hw.as_ref().and_then(|d| num(&d.join(f)));
    s.temp = h("temp1_input").map(|m| m / 1000.0);
    s.hotspot = h("temp2_input").map(|m| m / 1000.0);
    s.mem_temp = h("temp3_input").map(|m| m / 1000.0);
    s.power_w = h("power1_average").or_else(|| h("power1_input")).map(|u| u / 1e6);
    s.power_cap_w = h("power1_cap").map(|u| u / 1e6);
    s.fan_rpm = h("fan1_input").map(|r| r as u32);
    s.fan_pct = h("pwm1").map(|p| (100.0 * p / h("pwm1_max").unwrap_or(255.0)).round() as u32);
    match g.vendor {
        "AMD" => {
            s.util = num(&g.dev.join("gpu_busy_percent")).map(|v| v as u32);
            s.core_mhz = dpm_current(&g.dev.join("pp_dpm_sclk")).or_else(|| h("freq1_input").map(|f| (f / 1e6) as u32));
            s.mem_mhz = dpm_current(&g.dev.join("pp_dpm_mclk")).or_else(|| h("freq2_input").map(|f| (f / 1e6) as u32));
            s.vram_used_mb = num(&g.dev.join("mem_info_vram_used")).map(|b| b / 1048576.0);
            s.vram_total_mb = num(&g.dev.join("mem_info_vram_total")).map(|b| b / 1048576.0);
        }
        _ => {
            // i915 expõe gt_cur_freq_mhz no cartão; o xe, em device/tile0/gt0/freq0/cur_freq. Uso e VRAM não existem sem ferramentas de perf.
            s.core_mhz = num(&g.card.join("gt_cur_freq_mhz")).or_else(|| num(&g.dev.join("tile0/gt0/freq0/cur_freq"))).map(|f| f as u32);
        }
    }
    s
}

/// Ficha estática no mesmo formato que a da NVIDIA (grupos Geral, Memória, Clocks e energia, PCIe).
pub fn info_node(g: &SysGpu) -> InfoNode {
    let mut n = InfoNode::new(&g.name);
    let mut geral = InfoNode::new("Geral");
    geral.add("Fabricante", g.vendor);
    if let Some(d) = fs::read_link(g.dev.join("driver")).ok().and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned())) { geral.add("Driver", d); }
    geral.add("Barramento PCI", g.slot.clone());
    if let Some(v) = rd(&g.dev.join("vbios_version")) { geral.add("VBIOS", v); }
    n.children.push(geral);

    let s = sample(g);
    if let Some(t) = s.vram_total_mb {
        let mut mem = InfoNode::new("Memória");
        mem.add("VRAM", format!("{:.1} GB", t / 1024.0));
        n.children.push(mem);
    }
    let mut clk = InfoNode::new("Clocks e energia");
    if let Some(max) = fs::read_to_string(g.dev.join("pp_dpm_sclk")).ok().and_then(|t| t.lines().filter_map(|l| l.split(':').nth(1)).filter_map(|r| r.trim().trim_end_matches('*').trim().to_lowercase().trim_end_matches("mhz").trim().parse::<u32>().ok()).max()) {
        clk.add("Clock máx. do núcleo", format!("{max} MHz"));
    } else if let Some(m) = num(&g.card.join("gt_max_freq_mhz")) { clk.add("Clock máx. do núcleo", format!("{m:.0} MHz")); }
    if let Some(c) = s.power_cap_w { clk.add("Limite de energia", format!("{c:.0} W")); }
    if !clk.children.is_empty() { n.children.push(clk); }
    let (cur, max) = (rd(&g.dev.join("current_link_speed")), rd(&g.dev.join("max_link_speed")));
    if cur.is_some() || max.is_some() {
        let mut pcie = InfoNode::new("PCIe");
        if let Some(m) = max { pcie.add("Máximo", format!("{m} x{}", rd(&g.dev.join("max_link_width")).unwrap_or_default())); }
        if let Some(c) = cur { pcie.add("Em uso", format!("{c} x{}", rd(&g.dev.join("current_link_width")).unwrap_or_default())); }
        n.children.push(pcie);
    }
    n
}

/// Leituras ao vivo no mesmo formato de pares (rótulo, valor) da NVIDIA.
pub fn live_items(g: &SysGpu) -> Vec<(String, String)> {
    let s = sample(g);
    let mut it = vec![];
    if let Some(u) = s.util { it.push(("Uso da GPU".into(), format!("{u} %"))); }
    if let Some(t) = s.temp { it.push(("Temperatura".into(), format!("{t:.0} °C"))); }
    if let Some(t) = s.hotspot { it.push(("Hotspot".into(), format!("{t:.0} °C"))); }
    if let Some(c) = s.core_mhz { it.push(("Clock do núcleo".into(), format!("{c} MHz"))); }
    if let Some(c) = s.mem_mhz { it.push(("Clock da memória".into(), format!("{c} MHz"))); }
    if let (Some(u), Some(t)) = (s.vram_used_mb, s.vram_total_mb) { it.push(("VRAM usada".into(), format!("{u:.0} / {t:.0} MB"))); }
    if let Some(w) = s.power_w { it.push(("Consumo".into(), match s.power_cap_w { Some(c) if c > 0.0 => format!("{w:.1} W ({:.0} % do limite)", 100.0 * w / c), _ => format!("{w:.1} W") })); }
    if let Some(f) = s.fan_pct { it.push(("Ventoinha".into(), format!("{f} %"))); }
    it
}

#[cfg(test)]
mod tests {
    use super::*;
    fn w(p: &Path, c: &str) { fs::create_dir_all(p.parent().unwrap()).unwrap(); fs::write(p, c).unwrap(); }

    /// Árvore falsa imitando um amdgpu (RX 6600) e um Intel (i915).
    fn fake_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("cf_gpu_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let amd = root.join("class/drm/card0/device");
        w(&amd.join("vendor"), "0x1002\n"); w(&amd.join("gpu_busy_percent"), "73\n");
        w(&amd.join("mem_info_vram_total"), &(8u64 << 30).to_string()); w(&amd.join("mem_info_vram_used"), &(3u64 << 30).to_string());
        w(&amd.join("pp_dpm_sclk"), "0: 500Mhz\n1: 1500Mhz\n2: 2491Mhz *\n"); w(&amd.join("pp_dpm_mclk"), "0: 96Mhz\n1: 1750Mhz *\n");
        let hw = amd.join("hwmon/hwmon5");
        w(&hw.join("temp1_input"), "61000\n"); w(&hw.join("temp2_input"), "75000\n"); w(&hw.join("power1_average"), "98000000\n"); w(&hw.join("power1_cap"), "130000000\n");
        w(&hw.join("fan1_input"), "1450\n"); w(&hw.join("pwm1"), "102\n"); w(&hw.join("pwm1_max"), "255\n");
        let intel = root.join("class/drm/card1/device");
        w(&intel.join("vendor"), "0x8086\n"); w(&root.join("class/drm/card1/gt_cur_freq_mhz"), "650\n"); w(&root.join("class/drm/card1/gt_max_freq_mhz"), "1300\n");
        w(&root.join("class/drm/card1-HDMI-A-1/status"), "connected\n"); // saída de vídeo: não é uma placa
        w(&root.join("class/drm/card2/device/vendor"), "0x10de\n");     // NVIDIA: tratada pelo NVML, ignorada aqui
        root
    }

    #[test]
    fn finds_amd_and_intel_only_and_reads_values() {
        let root = fake_root();
        let g = discover(&root, |slot| Some(format!("modelo-{}", slot.len().min(1))));
        assert_eq!(g.iter().map(|x| x.vendor).collect::<Vec<_>>(), ["AMD", "Intel"]);
        let a = sample(&g[0]);
        assert_eq!((a.util, a.core_mhz, a.mem_mhz), (Some(73), Some(2491), Some(1750)));
        assert_eq!((a.temp, a.hotspot, a.power_w, a.power_cap_w), (Some(61.0), Some(75.0), Some(98.0), Some(130.0)));
        assert_eq!((a.fan_rpm, a.fan_pct, a.vram_used_mb, a.vram_total_mb), (Some(1450), Some(40), Some(3072.0), Some(8192.0)));
        let i = sample(&g[1]);
        assert_eq!((i.core_mhz, i.util, i.temp), (Some(650), None, None));
        let items = live_items(&g[0]);
        assert!(items.iter().any(|(k, v)| k == "Consumo" && v.contains("75 %")), "{items:?}");
        let info = info_node(&g[0]);
        assert!(info.children.iter().any(|c| c.header == "Memória") && info.children.iter().any(|c| c.header == "Clocks e energia"));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn this_machine() { println!("{:?}", gpus().iter().map(|g| (&g.name, g.vendor)).collect::<Vec<_>>()); }
}
