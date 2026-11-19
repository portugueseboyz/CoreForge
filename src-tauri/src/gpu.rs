//! GPU NVIDIA via NVML (libnvidia-ml.so, carregada em tempo de execução) e GPUs AMD/Intel via sysfs (gpu_sysfs). Sem driver NVIDIA, só as outras aparecem.
use crate::sysinfo::InfoNode;
use nvml_wrapper::{enum_wrappers::device::{Clock, TemperatureSensor}, Nvml};
use serde::Serialize;

#[derive(Serialize)]
pub struct GpuLive {
    index: u32,
    items: Vec<(String, String)>,
}

fn add<T: std::fmt::Display, E>(n: &mut InfoNode, label: &str, v: Result<T, E>, unit: &str) {
    if let Ok(v) = v { n.add(label, format!("{v}{unit}")); }
}

fn pcie(ver: u32) -> String { format!("PCIe {ver}.0") }

#[tauri::command(async)]
pub fn gpu_info() -> Vec<InfoNode> {
    let mut out = nvidia_info();
    out.extend(crate::gpu_sysfs::gpus().iter().map(crate::gpu_sysfs::info_node));
    out
}

fn nvidia_info() -> Vec<InfoNode> {
    let Ok(nvml) = Nvml::init() else { return vec![] };
    let mut out = vec![];
    for i in 0..nvml.device_count().unwrap_or(0) {
        let Ok(d) = nvml.device_by_index(i) else { continue };
        let mut g = InfoNode::new(&d.name().unwrap_or("GPU NVIDIA".into()));
        let mut geral = InfoNode::new("Geral");
        add(&mut geral, "Driver", nvml.sys_driver_version(), "");
        if let Ok(v) = nvml.sys_cuda_driver_version() { geral.add("CUDA (driver)", format!("{}.{}", v / 1000, v % 1000 / 10)); }
        if let Ok(c) = d.cuda_compute_capability() { geral.add("Compute capability", format!("{}.{}", c.major, c.minor)); }
        add(&mut geral, "VBIOS", d.vbios_version(), "");
        if let Ok(p) = d.pci_info() { geral.add("Barramento PCI", p.bus_id); }
        add(&mut geral, "Núcleos CUDA", d.num_cores(), "");
        g.children.push(geral);

        let mut mem = InfoNode::new("Memória");
        if let Ok(m) = d.memory_info() { mem.add("VRAM", format!("{:.1} GB", m.total as f64 / 1073741824.0)); }
        add(&mut mem, "Barramento", d.memory_bus_width(), " bits");
        if let Ok(m) = d.max_clock_info(Clock::Memory) { mem.add("Clock máx. da memória", format!("{m} MHz")); }
        if let Ok(b) = d.bar1_memory_info() { mem.add("BAR1", format!("{:.0} MB", b.total as f64 / 1048576.0)); }
        g.children.push(mem);

        let mut clk = InfoNode::new("Clocks e energia");
        if let Ok(m) = d.max_clock_info(Clock::Graphics) { clk.add("Clock máx. do núcleo", format!("{m} MHz")); }
        if let Ok(m) = d.max_clock_info(Clock::SM) { clk.add("Clock máx. SM", format!("{m} MHz")); }
        if let Ok(p) = d.power_management_limit() { clk.add("Limite de energia", format!("{:.0} W", p as f64 / 1000.0)); }
        if let Ok((lo, hi)) = d.power_management_limit_constraints().map(|c| (c.min_limit, c.max_limit)) {
            clk.add("Faixa do limite", format!("{:.0}–{:.0} W", lo as f64 / 1000.0, hi as f64 / 1000.0));
        }
        g.children.push(clk);

        let mut bus = InfoNode::new("PCIe");
        if let (Ok(ge), Ok(w)) = (d.max_pcie_link_gen(), d.max_pcie_link_width()) { bus.add("Máximo", format!("{} x{w}", pcie(ge))); }
        if let (Ok(ge), Ok(w)) = (d.current_pcie_link_gen(), d.current_pcie_link_width()) { bus.add("Em uso", format!("{} x{w}", pcie(ge))); }
        g.children.push(bus);
        out.push(g);
    }
    out
}

#[tauri::command(async)]
pub fn gpu_live() -> Vec<GpuLive> {
    let mut out = nvidia_live();
    let base = out.len() as u32;
    out.extend(crate::gpu_sysfs::gpus().iter().enumerate().map(|(i, g)| GpuLive { index: base + i as u32, items: crate::gpu_sysfs::live_items(g) }));
    out
}

fn nvidia_live() -> Vec<GpuLive> {
    let Ok(nvml) = Nvml::init() else { return vec![] };
    let mut out = vec![];
    for i in 0..nvml.device_count().unwrap_or(0) {
        let Ok(d) = nvml.device_by_index(i) else { continue };
        let mut it: Vec<(String, String)> = vec![];
        let mut p = |k: &str, v: String| it.push((k.into(), v));
        if let Ok(u) = d.utilization_rates() { p("Uso da GPU", format!("{} %", u.gpu)); p("Controlador de memória", format!("{} %", u.memory)); }
        if let Ok(t) = d.temperature(TemperatureSensor::Gpu) { p("Temperatura", format!("{t} °C")); }
        if let Ok(c) = d.clock_info(Clock::Graphics) { p("Clock do núcleo", format!("{c} MHz")); }
        if let Ok(c) = d.clock_info(Clock::Memory) { p("Clock da memória", format!("{c} MHz")); }
        if let Ok(m) = d.memory_info() { p("VRAM usada", format!("{:.0} / {:.0} MB", m.used as f64 / 1048576.0, m.total as f64 / 1048576.0)); }
        if let (Ok(w), Ok(l)) = (d.power_usage(), d.power_management_limit()) {
            p("Consumo", format!("{:.1} W ({:.0} % do limite)", w as f64 / 1000.0, 100.0 * w as f64 / l.max(1) as f64));
        }
        if let Ok(f) = d.fan_speed(0) { p("Ventoinha", format!("{f} %")); }
        if let Ok(s) = d.performance_state() { p("Estado de desempenho", format!("{s:?}")); }
        if let Ok(r) = d.current_throttle_reasons() { p("PerfCap (limitador)", if r.is_empty() { "Nenhum".into() } else { format!("{r:?}") }); }
        if let Ok(e) = d.encoder_utilization() { p("Encoder (NVENC)", format!("{} %", e.utilization)); }
        if let Ok(e) = d.decoder_utilization() { p("Decoder (NVDEC)", format!("{} %", e.utilization)); }
        out.push(GpuLive { index: i, items: it });
    }
    out
}

/// Temperatura (°C), consumo (W) e uso (%) da primeira GPU NVIDIA, para o stress de GPU vigiar o limite.
pub fn first_sample() -> Option<(f64, Option<f64>, Option<u32>)> {
    if let Some(nvml) = Nvml::init().ok() && let Ok(d) = nvml.device_by_index(0) && let Ok(t) = d.temperature(TemperatureSensor::Gpu) {
        return Some((t as f64, d.power_usage().ok().map(|m| m as f64 / 1000.0), d.utilization_rates().ok().map(|u| u.gpu)));
    }
    // Sem NVIDIA: AMD/Intel pelo sysfs
    let s = crate::gpu_sysfs::sample(crate::gpu_sysfs::gpus().first()?);
    Some((s.temp?, s.power_w, s.util))
}

#[cfg(test)]
mod tests {
    // Sem driver NVIDIA as duas listas vêm vazias; com driver, uma GPU precisa ter dados.
    #[test]
    fn info_and_live_agree() {
        let (i, l) = (super::gpu_info(), super::gpu_live());
        assert_eq!(i.len(), l.len());
        for g in &l { assert!(!g.items.is_empty()); }
        for g in &i { assert!(!g.children.is_empty()); }
        println!("{}", serde_json::to_string_pretty(&(i, l)).unwrap());
    }
}
