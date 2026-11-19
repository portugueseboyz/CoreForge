//! Leituras ao vivo para o overlay e o detector de gargalo (GPU via NVML, CPU/RAM via /proc e /sys).
use crate::sysinfo::{cpu_temp, read};
use nvml_wrapper::{enum_wrappers::device::{Clock, TemperatureSensor}, Nvml};
use serde::Serialize;
use std::sync::{Mutex, OnceLock};

static NV: OnceLock<Option<Nvml>> = OnceLock::new();

#[derive(Default)]
pub struct MetricsState(Mutex<Vec<(u64, u64)>>);

#[derive(Serialize, Default)]
pub struct Metrics {
    gpu_name: Option<String>, gpu_util: Option<u32>, gpu_temp: Option<u32>, gpu_clock: Option<u32>, gpu_mem_clock: Option<u32>,
    gpu_power_w: Option<f64>, gpu_fan: Option<u32>, fan_rpm: Option<f64>, vram_used_mb: Option<f64>, vram_total_mb: Option<f64>, gpu_throttle: Option<String>,
    cpu_load: Option<f64>, cpu_busiest: Option<f64>, cpu_temp: Option<f64>, cpu_clock_mhz: Option<f64>,
    ram_used_gb: Option<f64>, ram_total_gb: Option<f64>,
    fps: Option<f64>, fps_stale: bool,
}

#[tauri::command]
#[allow(clippy::redundant_closure_call)] // o closure `load` é usado duas vezes (total e núcleo mais ocupado)
pub fn live_metrics(st: tauri::State<MetricsState>) -> Metrics {
    let mut m = Metrics::default();
    if let Some(Some(nv)) = Some(NV.get_or_init(|| Nvml::init().ok()))
        && let Ok(d) = nv.device_by_index(0) {
            m.gpu_name = d.name().ok();
            m.gpu_util = d.utilization_rates().ok().map(|u| u.gpu);
            m.gpu_temp = d.temperature(TemperatureSensor::Gpu).ok();
            m.gpu_clock = d.clock_info(Clock::Graphics).ok();
            m.gpu_mem_clock = d.clock_info(Clock::Memory).ok();
            m.gpu_power_w = d.power_usage().ok().map(|p| p as f64 / 1000.0);
            m.gpu_fan = d.fan_speed(0).ok();
            if let Ok(mem) = d.memory_info() { m.vram_used_mb = Some(mem.used as f64 / 1048576.0); m.vram_total_mb = Some(mem.total as f64 / 1048576.0); }
            m.gpu_throttle = d.current_throttle_reasons().ok().filter(|r| !r.is_empty()).map(|r| format!("{r:?}"));
        }
    // Sem NVIDIA: AMD/Intel pelo sysfs (a discreta, com mais VRAM, primeiro)
    if m.gpu_name.is_none() {
        let mut list: Vec<_> = crate::gpu_sysfs::gpus().iter().map(|g| (g, crate::gpu_sysfs::sample(g))).collect();
        list.sort_by(|a, b| b.1.vram_total_mb.unwrap_or(0.0).total_cmp(&a.1.vram_total_mb.unwrap_or(0.0)));
        if let Some((g, s)) = list.into_iter().next() {
            m.gpu_name = Some(g.name.clone()); m.gpu_util = s.util; m.gpu_temp = s.temp.map(|t| t.round() as u32);
            m.gpu_clock = s.core_mhz; m.gpu_mem_clock = s.mem_mhz; m.gpu_power_w = s.power_w; m.gpu_fan = s.fan_pct;
            m.vram_used_mb = s.vram_used_mb; m.vram_total_mb = s.vram_total_mb;
        }
    }
    // Carga da CPU (total e do núcleo mais ocupado) entre duas leituras de /proc/stat.
    let stat = read("/proc/stat").unwrap_or_default();
    let cur: Vec<(u64, u64)> = stat.lines().filter(|l| l.starts_with("cpu")).map(|l| {
        let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
        (v.get(3).unwrap_or(&0) + v.get(4).unwrap_or(&0), v.iter().sum())
    }).collect();
    let mut prev = st.0.lock().unwrap_or_else(|e| e.into_inner());
    if prev.len() == cur.len() && !cur.is_empty() {
        let load = |i: usize| { let (di, dt) = (cur[i].0.saturating_sub(prev[i].0), cur[i].1.saturating_sub(prev[i].1)); (dt > 0).then(|| 100.0 * (dt - di) as f64 / dt as f64) };
        m.cpu_load = load(0);
        m.cpu_busiest = (1..cur.len()).filter_map(load).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))));
    }
    *prev = cur;
    m.cpu_temp = cpu_temp();
    m.fan_rpm = crate::sysinfo::fan_rpm();
    let clocks: Vec<f64> = (0..256).map_while(|n| {
        let p = format!("/sys/devices/system/cpu/cpu{n}");
        std::path::Path::new(&p).exists().then(|| read(&format!("{p}/cpufreq/scaling_cur_freq")).and_then(|s| s.parse::<f64>().ok()))
    }).flatten().collect();
    if !clocks.is_empty() { m.cpu_clock_mhz = Some(clocks.iter().sum::<f64>() / clocks.len() as f64 / 1000.0); }
    let mi = crate::sysinfo::kv_pub(&read("/proc/meminfo").unwrap_or_default());
    let kb = |k: &str| mi.get(k).and_then(|v| v.trim_end_matches("kB").trim().parse::<f64>().ok());
    if let (Some(t), Some(a)) = (kb("MemTotal"), kb("MemAvailable")) { m.ram_total_gb = Some(t / 1048576.0); m.ram_used_gb = Some((t - a) / 1048576.0); }
    if let Some((f, stale)) = mangohud_fps() { m.fps = Some(f); m.fps_stale = stale; }
    m
}

/// Notificação do desktop (libnotify). Sem `notify-send` instalado, falha em silêncio: o aviso dentro do app continua.
#[tauri::command(async)]
pub fn notify(title: String, body: String) {
    let _ = std::process::Command::new("notify-send").args(["-u", "critical", "-a", "CoreForge", "--", &title, &body]).status();
}

/// Abre ou fecha o overlay: janela transparente, sem borda, sempre no topo, que ignora cliques.
/// `corner`: TL, TR, BL ou BR (no monitor principal). Em Wayland o compositor pode ignorar "sempre no topo".
#[tauri::command]
pub fn overlay_toggle(app: tauri::AppHandle, corner: String, width: f64, height: f64) -> Result<bool, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window("overlay") { w.close().map_err(|e| e.to_string())?; return Ok(false); }
    let (mut x, mut y) = (20.0, 20.0);
    if let Ok(Some(mon)) = app.primary_monitor() {
        let (sw, sh) = (mon.size().width as f64 / mon.scale_factor(), mon.size().height as f64 / mon.scale_factor());
        if corner.contains('R') { x = sw - width - 20.0; }
        if corner.starts_with('B') { y = sh - height - 20.0; }
    }
    let w = WebviewWindowBuilder::new(&app, "overlay", WebviewUrl::App("index.html?overlay=1".into()))
        .title("CoreForge overlay").inner_size(width, height).position(x, y)
        .decorations(false).transparent(true).always_on_top(true).skip_taskbar(true).resizable(false).focused(false)
        .build().map_err(|e| e.to_string())?;
    // Alguns gerenciadores de janela ignoram a posição pedida na criação: reaplica depois que a janela existe.
    let _ = w.set_position(tauri::LogicalPosition::new(x, y));
    let _ = w.set_always_on_top(true);
    w.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Pasta onde o MangoHud grava o CSV de log (opcional: sem MangoHud ou sem log ativo, o FPS fica ausente).
fn mangohud_dir() -> std::path::PathBuf {
    // sem HOME o caminho seria relativo ao diretório atual: usa a pasta temporária
    std::env::var_os("HOME").map_or_else(std::env::temp_dir, std::path::PathBuf::from).join(".cache/coreforge/mangohud")
}

/// Opção de inicialização do Steam/terminal que faz o MangoHud gravar o log lido pelo overlay.
/// `log_interval` precisa ser > 0 (com 0 o MangoHud não grava linhas); flush a cada amostra.
#[tauri::command]
pub fn mangohud_hint() -> String {
    let d = mangohud_dir();
    format!("MANGOHUD_CONFIG=output_folder={},log_interval=250,autostart_log=1,log_duration=86400 mangohud %command%", d.display())
}

/// Log mais antigo que isto: o jogo parou de gravar, FPS some (—). Entre `FPS_FRESH` e este limite o último valor segue, marcado como defasado.
const FPS_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(10);
const FPS_FRESH: std::time::Duration = std::time::Duration::from_secs(3);

/// Último FPS do CSV mais recente em `mangohud_dir` (pasta fixa, a mesma do `mangohud_hint`) e se está defasado.
fn mangohud_fps() -> Option<(f64, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let (path, age) = std::fs::read_dir(mangohud_dir()).ok()?.flatten()
        .filter(|e| { let n = e.file_name(); let n = n.to_string_lossy(); n.ends_with(".csv") && !n.ends_with("_summary.csv") })
        .filter_map(|e| { let age = e.metadata().ok()?.modified().ok()?.elapsed().ok()?; (age < FPS_MAX_AGE).then(|| (e.path(), age)) })
        .min_by_key(|(_, a)| *a)?;
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = vec![0; 4096.min(len as usize)];
    f.read_exact(&mut head).ok()?;
    let col = fps_column(&String::from_utf8_lossy(&head));
    f.seek(SeekFrom::Start(len.saturating_sub(4096))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some((parse_last_fps(&String::from_utf8_lossy(&buf), col)?, age >= FPS_FRESH))
}

/// Índice da coluna `fps` pelo cabeçalho (primeira linha com um campo "fps", ignorando o prefixo `v1`/separadores do
/// `log_versioning`); sem cabeçalho, coluna 0.
fn fps_column(head: &str) -> usize {
    head.lines().find_map(|l| l.split(',').position(|c| c.trim().eq_ignore_ascii_case("fps"))).unwrap_or(0)
}

/// FPS da coluna `col` na última linha de dados completa. Ignora cabeçalhos e a linha final sem `\n` (pode estar cortada).
fn parse_last_fps(text: &str, col: usize) -> Option<f64> {
    let complete = &text[..text.rfind('\n')?];
    let cols: Vec<&str> = complete.lines().next_back()?.split(',').collect();
    let fps: f64 = cols.get(col)?.trim().parse().ok()?;
    (cols.len() >= 5 && fps.is_finite() && fps >= 0.0).then_some(fps)
}

#[cfg(test)]
mod tests {
    use super::{fps_column, parse_last_fps};
    const HEAD: &str = "os,cpu,gpu,ram,kernel,driver,cpuscheduler\nLinux,X,Y,16000,7.0,Mesa,none\nfps,frametime,cpu_load,cpu_power,gpu_load,cpu_temp,gpu_temp,gpu_core_clock,gpu_mem_clock,gpu_vram_used,gpu_power,ram_used,swap_used,process_rss,cpu_mhz,elapsed\n";
    // log_versioning=1: linha de versão e separadores antes do cabeçalho; fps deslocado de coluna.
    const VER: &str = "v1\n--------------------MangoHud Info--------------------\nos,cpu,gpu\nLinux,X,Y\n--------------------FPS Data-------------------------\nframetime,fps,cpu_load,a,b,c\n";
    #[test]
    fn fps_empty_or_truncated() {
        assert_eq!(parse_last_fps("", 0), None);
        assert_eq!(parse_last_fps("no newline", 0), None);
        assert_eq!(parse_last_fps("\n", 5), None);
    }
    #[test]
    fn last_complete_row() {
        let t = format!("{HEAD}60.1,16.6,10,0,50,40,50,1500,800,2,100,8,0,1,3000,1\n143.9,6.9,10,0,50,40,50,1500,800,2,100,8,0,1,3000,2\n");
        assert_eq!(parse_last_fps(&t, fps_column(&t)), Some(143.9));
    }
    #[test]
    fn ignores_cut_last_line_and_headers() {
        assert_eq!(parse_last_fps(&format!("{HEAD}60.0,16.6,1,2,3,4,5\n99."), 0), Some(60.0));
        assert_eq!(parse_last_fps(HEAD, 0), None);
        assert_eq!(parse_last_fps("", 0), None);
        assert_eq!(parse_last_fps("garbage\n", 0), None);
    }
    #[test]
    fn column_by_header_with_versioning() {
        let t = format!("{VER}16.6,60.0,1,2,3,4\n6.9,144.0,1,2,3,4\n");
        assert_eq!(fps_column(&t), 1);
        assert_eq!(parse_last_fps(&t, 1), Some(144.0));
        assert_eq!(fps_column(HEAD), 0);
    }
    #[test]
    fn column_fallback_without_header() {
        assert_eq!(fps_column("60.0,16.6,1,2,3,4\n"), 0);
        assert_eq!(parse_last_fps("60.0,16.6,1,2,3,4\n", 0), Some(60.0));
        assert_eq!(parse_last_fps("60.0,16.6\n", 5), None); // coluna fora do alcance
    }
}
