//! Disco: benchmark estilo CrystalDiskMark (sequencial 1 MB e aleatório 4 KB Q1/Q8, com O_DIRECT) e saúde (smartctl + sysfs).
use crate::sysinfo::read;
use serde::Serialize;
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{FileExt, OpenOptionsExt},
    path::PathBuf,
    process::Command,
    sync::{atomic::{AtomicBool, Ordering::Relaxed}, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_arch = "aarch64")]
const O_DIRECT: i32 = 0o200000;
#[cfg(not(target_arch = "aarch64"))]
const O_DIRECT: i32 = 0o40000;

const BLOCK: usize = 1 << 20;
const SMALL: usize = 4096;
const RANDOM_SECS: f64 = 4.0;

#[derive(Serialize)]
pub struct Mount { path: String, source: String, size_gb: f64, free_gb: f64 }

/// Volumes de verdade (/dev/...) com pelo menos 1,5 GB livres.
#[tauri::command(async)]
pub fn disk_mounts() -> Vec<Mount> {
    let out = Command::new("df").env("LC_ALL", "C").args(["-B1", "--output=source,target,size,avail"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    out.lines().skip(1).filter_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 4 || !f[0].starts_with("/dev/") || f[1].starts_with("/boot") || f[1].starts_with("/snap") { return None; }
        let (size, free) = (f[2].parse::<f64>().ok()?, f[3].parse::<f64>().ok()?);
        (free >= 1.5e9 * 1.0737 && seen.insert(f[0].to_string())).then(|| Mount { path: f[1].into(), source: f[0].into(), size_gb: size / 1e9, free_gb: free / 1e9 })
    }).collect()
}

struct Aligned(*mut u8, Layout);
unsafe impl Send for Aligned {}
unsafe impl Sync for Aligned {}
impl Aligned {
    fn new(n: usize) -> Self { let l = Layout::from_size_align(n, 4096).unwrap(); Aligned(unsafe { alloc_zeroed(l) }, l) }
    fn slice(&mut self) -> &mut [u8] { unsafe { std::slice::from_raw_parts_mut(self.0, self.1.size()) } }
}
impl Drop for Aligned { fn drop(&mut self) { unsafe { dealloc(self.0, self.1) } } }

fn xorshift(s: &mut u64) -> u64 { *s ^= *s << 13; *s ^= *s >> 7; *s ^= *s << 17; *s }

/// Abre com O_DIRECT; sistemas de arquivos que não aceitam (tmpfs, alguns FUSE) caem para E/S com cache, e o resultado avisa.
fn open(path: &PathBuf, write: bool, direct: &mut bool) -> std::io::Result<File> {
    let mk = |d: bool| {
        let mut o = OpenOptions::new();
        o.read(true).write(write).create(write);
        if d { o.custom_flags(O_DIRECT | if write { 0o4010000 /* O_DSYNC */ } else { 0 }); }
        o.open(path)
    };
    if *direct { match mk(true) { Ok(f) => return Ok(f), Err(e) if e.raw_os_error() == Some(22) => *direct = false, Err(e) => return Err(e) } }
    mk(false)
}

#[derive(Serialize, Clone, Default)]
pub struct DiskResult {
    mount: String, file_mb: u64, direct: bool,
    seq_read_mbs: f64, seq_write_mbs: f64,
    read_iops_q1: f64, write_iops_q1: f64, read_latency_us: f64, write_latency_us: f64, read_iops_q8: f64,
}

#[derive(Default)]
struct Inner { phase: String, progress: f64, result: Option<DiskResult>, error: String }

#[derive(Default)]
pub struct DiskState { inner: Arc<Mutex<Inner>>, running: Arc<AtomicBool>, cancel: Arc<AtomicBool> }

#[derive(Serialize)]
pub struct DiskStatus { running: bool, phase: String, progress: f64, result: Option<DiskResult>, error: String }

fn bench(mount: &str, mb: u64, inner: &Mutex<Inner>, cancel: &AtomicBool) -> Result<DiskResult, String> {
    let dir = PathBuf::from(mount).join("CoreForgeBench");
    fs::create_dir_all(&dir).map_err(|e| format!("Sem permissão para escrever em {mount}: {e}"))?;
    let path = dir.join("bench.tmp");
    let set = |p: &str, f: f64| { let mut g = inner.lock().unwrap_or_else(|e| e.into_inner()); g.phase = p.into(); g.progress = f; };
    let res = (|| -> Result<DiskResult, String> {
        let mut direct = true;
        let size = mb << 20;
        let blocks = (size as usize) / BLOCK;
        let mut buf = Aligned::new(BLOCK);
        let mut s = 42u64;
        for c in buf.slice().chunks_mut(8) { c.copy_from_slice(&xorshift(&mut s).to_le_bytes()[..c.len()]); } // incompressível

        set("Escrita sequencial", 0.0);
        let mut f = open(&path, true, &mut direct).map_err(|e| e.to_string())?;
        let t = Instant::now();
        for i in 0..blocks {
            if cancel.load(Relaxed) { return Err("Cancelado".into()); }
            f.write_all(buf.slice()).map_err(|e| e.to_string())?;
            if i % 8 == 0 { set("Escrita sequencial", 0.05 + 0.25 * i as f64 / blocks as f64); }
        }
        f.sync_all().map_err(|e| e.to_string())?;
        let seq_write = size as f64 / 1e6 / t.elapsed().as_secs_f64();
        drop(f);

        set("Leitura sequencial", 0.3);
        let mut f = open(&path, false, &mut direct).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(0)).ok();
        let t = Instant::now();
        for i in 0..blocks {
            if cancel.load(Relaxed) { return Err("Cancelado".into()); }
            f.read_exact(buf.slice()).map_err(|e| e.to_string())?;
            if i % 8 == 0 { set("Leitura sequencial", 0.3 + 0.25 * i as f64 / blocks as f64); }
        }
        let seq_read = size as f64 / 1e6 / t.elapsed().as_secs_f64();
        drop(f);

        // Aleatório 4 KB: leitura Q1, escrita Q1 (O_DSYNC) e leitura com 8 threads.
        let slots = size / SMALL as u64;
        let rand_run = |write: bool, threads: usize, direct: bool, label: &str, base: f64| -> Result<(f64, f64), String> {
            set(label, base);
            let mut d = direct;
            let end = Instant::now() + Duration::from_secs_f64(RANDOM_SECS);
            let ops: u64 = thread::scope(|sc| {
                let hs: Vec<_> = (0..threads).map(|k| {
                    let (path, mut d) = (path.clone(), d);
                    sc.spawn(move || {
                        let Ok(f) = open(&path, write, &mut d) else { return (0u64, 0.0f64) };
                        let mut b = Aligned::new(SMALL);
                        b.slice().fill(0xA5);
                        let (mut r, mut n, mut lat) = (0x9E3779B97F4A7C15u64 ^ (k as u64 + 1), 0u64, 0.0f64);
                        while Instant::now() < end && !cancel.load(Relaxed) {
                            let off = (xorshift(&mut r) % slots) * SMALL as u64;
                            let t = Instant::now();
                            let ok = if write { f.write_at(b.slice(), off).is_ok() } else { f.read_at(b.slice(), off).is_ok() };
                            if ok { n += 1; lat += t.elapsed().as_secs_f64(); }
                        }
                        (n, lat)
                    })
                }).collect();
                hs.into_iter().map(|h| h.join().unwrap_or((0, 0.0))).fold(0u64, |a, (n, _)| a + n)
            });
            let _ = &mut d;
            let iops = ops as f64 / RANDOM_SECS;
            Ok((iops, if iops > 0.0 { 1e6 * threads as f64 / iops } else { 0.0 }))
        };
        let (r1, rl) = rand_run(false, 1, direct, "Leitura 4K (Q1)", 0.55)?;
        let (w1, wl) = rand_run(true, 1, direct, "Escrita 4K (Q1)", 0.7)?;
        let (r8, _) = rand_run(false, 8, direct, "Leitura 4K (Q8)", 0.85)?;
        if cancel.load(Relaxed) { return Err("Cancelado".into()); }
        Ok(DiskResult { mount: mount.into(), file_mb: mb, direct, seq_read_mbs: seq_read, seq_write_mbs: seq_write,
            read_iops_q1: r1, write_iops_q1: w1, read_latency_us: rl, write_latency_us: wl, read_iops_q8: r8 })
    })();
    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
    res
}

#[tauri::command]
pub fn disk_bench_start(st: tauri::State<DiskState>, mount: String, file_mb: u64) -> Result<(), String> {
    if st.running.swap(true, Relaxed) { return Err("Já está rodando".into()); }
    st.cancel.store(false, Relaxed);
    *st.inner.lock().unwrap_or_else(|e| e.into_inner()) = Inner::default();
    let (inner, running, cancel) = (st.inner.clone(), st.running.clone(), st.cancel.clone());
    thread::spawn(move || {
        let r = crate::guard(|| bench(&mount, file_mb.clamp(64, 16384), &inner, &cancel));
        let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
        match r { Ok(v) => { g.result = Some(v); g.progress = 1.0; g.phase = "Concluído".into(); } Err(e) => g.error = e }
        running.store(false, Relaxed);
    });
    Ok(())
}

#[tauri::command]
pub fn disk_bench_cancel(st: tauri::State<DiskState>) { st.cancel.store(true, Relaxed); }

#[tauri::command]
pub fn disk_bench_status(st: tauri::State<DiskState>) -> DiskStatus {
    let g = st.inner.lock().unwrap_or_else(|e| e.into_inner());
    DiskStatus { running: st.running.load(Relaxed), phase: g.phase.clone(), progress: g.progress, result: g.result.clone(), error: g.error.clone() }
}

// ------------------------------------------------------------------ saúde

#[derive(Serialize, Default)]
pub struct DiskHealth {
    name: String, meta: String, serial: String,
    verdict: &'static str, // good | warning | critical | unknown
    reasons: Vec<String>,
    metrics: Vec<(String, String)>,
}

fn smart(dev: &str) -> Option<serde_json::Value> {
    let o = Command::new("smartctl").args(["-j", "-H", "-A", "-i", &format!("/dev/{dev}")]).output().ok()?;
    serde_json::from_slice(&o.stdout).ok().filter(|v: &serde_json::Value| v.get("smart_status").is_some() || v.get("nvme_smart_health_information_log").is_some())
}

fn temp_sysfs(dev: &str) -> Option<f64> {
    let base = format!("/sys/block/{dev}/device");
    for sub in fs::read_dir(format!("{base}/hwmon")).ok()?.flatten() {
        if let Some(t) = read(&format!("{}/temp1_input", sub.path().display())).and_then(|s| s.parse::<f64>().ok()) { return Some(t / 1000.0); }
    }
    None
}

#[tauri::command(async)]
pub fn disk_health() -> Vec<DiskHealth> {
    let out = Command::new("lsblk").args(["-dn", "-b", "-P", "-o", "NAME,MODEL,SERIAL,SIZE,ROTA,TRAN,TYPE"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut cards = vec![];
    for line in out.lines() {
        let k = crate::sysinfo::pairs(line);
        if k.get("TYPE").map(String::as_str) != Some("disk") { continue; }
        let name = k["NAME"].clone();
        let tran = k.get("TRAN").cloned().unwrap_or_default();
        let kind = if k.get("ROTA").map(String::as_str) == Some("1") { "HDD" } else if tran == "nvme" { "NVMe" } else { "SSD" };
        let size = k.get("SIZE").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let mut c = DiskHealth {
            name: format!("{} (/dev/{name})", k.get("MODEL").map(|s| s.trim()).unwrap_or("Disco")),
            meta: format!("{kind} · {:.0} GB · {}", size / 1e9, if tran.is_empty() { "—" } else { &tran }),
            serial: k.get("SERIAL").cloned().unwrap_or_default(), verdict: "unknown", ..Default::default()
        };
        let mut bad: Vec<String> = vec![];
        let mut warn: Vec<String> = vec![];
        let mut have_smart = false;
        if let Some(j) = smart(&name) {
            have_smart = true;
            match j["smart_status"]["passed"].as_bool() {
                Some(true) => c.metrics.push(("Autoavaliação SMART".into(), "Aprovado".into())),
                Some(false) => { c.metrics.push(("Autoavaliação SMART".into(), "REPROVADO".into())); bad.push("O disco reprovou na autoavaliação SMART".into()); }
                None => {}
            }
            if let Some(t) = j["temperature"]["current"].as_f64() { c.metrics.push(("Temperatura".into(), format!("{t:.0} °C"))); if t >= 70.0 { warn.push(format!("Temperatura alta ({t:.0} °C)")); } }
            if let Some(h) = j["power_on_time"]["hours"].as_u64() { c.metrics.push(("Horas ligado".into(), format!("{h} h"))); }
            let n = &j["nvme_smart_health_information_log"];
            if n.is_object() {
                if let Some(u) = n["percentage_used"].as_u64() { c.metrics.push(("Vida consumida".into(), format!("{u} %"))); if u >= 90 { bad.push(format!("Vida útil quase no fim ({u} % usada)")); } else if u >= 70 { warn.push(format!("{u} % da vida útil consumida")); } }
                if let Some(s) = n["available_spare"].as_u64() { c.metrics.push(("Reserva disponível".into(), format!("{s} %"))); if s < 10 { bad.push("Reserva de blocos quase esgotada".into()); } }
                if let Some(e) = n["media_errors"].as_u64() { c.metrics.push(("Erros de mídia".into(), e.to_string())); if e > 0 { bad.push(format!("{e} erro(s) de mídia")); } }
                if let Some(w) = n["data_units_written"].as_u64() { c.metrics.push(("Total gravado".into(), format!("{:.1} TB", w as f64 * 512_000.0 / 1e12))); }
                if let Some(p) = n["power_cycles"].as_u64() { c.metrics.push(("Ciclos de energia".into(), p.to_string())); }
                if let Some(p) = n["unsafe_shutdowns"].as_u64() { c.metrics.push(("Desligamentos inseguros".into(), p.to_string())); }
            }
            for a in j["ata_smart_attributes"]["table"].as_array().into_iter().flatten() {
                let (id, raw) = (a["id"].as_u64().unwrap_or(0), a["raw"]["value"].as_u64().unwrap_or(0));
                let label = match id { 5 => "Setores realocados", 197 => "Setores pendentes", 198 => "Setores incorrigíveis", 199 => "Erros de CRC", _ => continue };
                c.metrics.push((label.into(), raw.to_string()));
                if raw > 0 && matches!(id, 5 | 197 | 198) { bad.push(format!("{label}: {raw}")); }
            }
        } else {
            if let Some(t) = temp_sysfs(&name) { c.metrics.push(("Temperatura".into(), format!("{t:.0} °C"))); if t >= 70.0 { warn.push(format!("Temperatura alta ({t:.0} °C)")); } }
            c.reasons.push(format!("SMART indisponível: instale o smartmontools ({}) e abra o app com permissão de leitura do disco.", crate::fans::install_hint("smartmontools")));
        }
        c.verdict = if !bad.is_empty() { "critical" } else if !warn.is_empty() { "warning" } else if have_smart { "good" } else { "unknown" };
        c.reasons.extend(bad); c.reasons.extend(warn);
        cards.push(c);
    }
    cards
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bench_small_file_in_tmp_dir() {
        let dir = PathBuf::from(std::env::var("CF_TEST_DIR").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into())).join("coreforge_disk_test");
        fs::create_dir_all(&dir).unwrap();
        let inner = Mutex::new(Inner::default());
        let r = bench(dir.to_str().unwrap(), 64, &inner, &AtomicBool::new(false)).unwrap();
        assert!(r.seq_read_mbs > 0.0 && r.seq_write_mbs > 0.0 && r.read_iops_q1 > 0.0 && r.write_iops_q1 > 0.0 && r.read_iops_q8 > 0.0);
        assert!(!dir.join("CoreForgeBench").exists()); // limpou o arquivo
        println!("{}", serde_json::to_string(&r).unwrap());
    }
    #[test]
    fn health_lists_disks() { for c in disk_health() { println!("{} {} {:?}", c.name, c.verdict, c.metrics); } }
}
