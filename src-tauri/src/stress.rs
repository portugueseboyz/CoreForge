//! Stress de CPU (todos os threads, FPU + inteiros) e RAM (grava e verifica padrões), com parada automática.
use crate::sysinfo;
use serde::Serialize;
use std::{
    hint::black_box,
    sync::{atomic::{AtomicBool, AtomicU64, Ordering::Relaxed}, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Stats {
    running: AtomicBool,
    mem_errors: AtomicU64,
    mem_passes: AtomicU64,
    mem_bytes: AtomicU64,
    elapsed_ms: AtomicU64,
    temp_bits: AtomicU64,     // f64::to_bits; NaN = sem leitura
    max_temp_bits: AtomicU64,
    reason: Mutex<String>,
}

struct Run { stop: Arc<AtomicBool>, handles: Vec<JoinHandle<()>>, stats: Arc<Stats> }

#[derive(Default)]
pub struct StressState(Mutex<Option<Run>>);

#[derive(Serialize)]
pub struct StressStatus {
    running: bool,
    elapsed_s: f64,
    temp: Option<f64>,
    max_temp: Option<f64>,
    mem_errors: u64,
    mem_passes: u64,
    mem_gb: f64,
    reason: String,
}

fn opt(bits: u64) -> Option<f64> { let v = f64::from_bits(bits); (!v.is_nan()).then_some(v) }

fn cpu_worker(stop: Arc<AtomicBool>, seed: u64) {
    // Mistura FMA/sqrt/sin com xorshift sobre um buffer que cabe no cache L1/L2.
    let mut buf = vec![0f64; 4096];
    for (i, b) in buf.iter_mut().enumerate() { *b = (i as u64 + seed + 1) as f64; }
    let mut x = (seed + 1).wrapping_mul(0x9E3779B97F4A7C15);
    while !stop.load(Relaxed) {
        for _ in 0..64 {
            for (i, b) in buf.iter_mut().enumerate() {
                let mut v = b.mul_add(1.0000001, 0.5);
                v = (v * v + 1.0).sqrt() + v.sin() * 1e-3;
                *b = if v > 1e12 { (i + 1) as f64 } else { v };
                x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            }
        }
        black_box((&buf, x));
    }
}

fn ram_worker(stop: Arc<AtomicBool>, total: u64, stats: Arc<Stats>) {
    const CHUNK: usize = 16 * 1024 * 1024; // 128 MB por bloco
    let mut chunks: Vec<Vec<u64>> = vec![];
    let mut allocated = 0u64;
    while allocated + (CHUNK as u64) * 8 <= total && !stop.load(Relaxed) {
        let mut v: Vec<u64> = Vec::new();
        if v.try_reserve_exact(CHUNK).is_err() { break; }
        v.resize(CHUNK, 0);
        chunks.push(v);
        allocated += CHUNK as u64 * 8;
    }
    stats.mem_bytes.store(allocated, Relaxed);
    if chunks.is_empty() { return; }
    let mut pass = 0u64;
    while !stop.load(Relaxed) {
        pass += 1;
        let pattern = pass.wrapping_mul(0x9E3779B97F4A7C15);
        for c in chunks.iter_mut() {
            for (i, w) in c.iter_mut().enumerate() { *w = pattern ^ i as u64; }
            if stop.load(Relaxed) { return; }
        }
        for c in &chunks {
            let mut errs = 0u64;
            for (i, w) in c.iter().enumerate() {
                // leitura volátil: impede o compilador de reaproveitar o valor recém-gravado
                if unsafe { std::ptr::read_volatile(w) } != pattern ^ i as u64 { errs += 1; }
            }
            if errs > 0 { stats.mem_errors.fetch_add(errs, Relaxed); }
            if stop.load(Relaxed) { return; }
        }
        stats.mem_passes.fetch_add(1, Relaxed);
    }
}

#[tauri::command]
pub fn stress_start(st: tauri::State<StressState>, cpu: bool, ram: bool, ram_fraction: f64, temp_limit: f64, minutes: f64) -> Result<(), String> {
    let mut g = st.0.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|r| r.stats.running.load(Relaxed)) { return Err("Já está rodando".into()); }
    if !cpu && !ram { return Err("Escolha CPU e/ou RAM".into()); }
    let stats = Arc::new(Stats::default());
    stats.temp_bits.store(f64::NAN.to_bits(), Relaxed);
    stats.max_temp_bits.store(f64::NAN.to_bits(), Relaxed);
    stats.running.store(true, Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = vec![];

    let mut threads = thread::available_parallelism().map_or(1, |n| n.get());
    if cpu && ram && threads > 1 { threads -= 1; } // um thread fica para a RAM
    if cpu {
        for i in 0..threads {
            let s = stop.clone();
            handles.push(thread::spawn(move || cpu_worker(s, i as u64)));
        }
    }
    if ram {
        let avail_kb = sysinfo::read("/proc/meminfo").and_then(|t| t.lines().find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1).and_then(|n| n.parse::<u64>().ok()))).unwrap_or(1 << 20);
        let bytes = (avail_kb as f64 * 1024.0 * ram_fraction.clamp(0.1, 0.9)) as u64;
        let (s, st2) = (stop.clone(), stats.clone());
        handles.push(thread::spawn(move || ram_worker(s, bytes, st2)));
    }

    // Vigia: temperatura limite, tempo máximo e erros de memória param o teste sozinhos.
    let (s, st2) = (stop.clone(), stats.clone());
    let minutes = minutes.min(1440.0); // Duration::from_secs_f64 entra em pânico com valores enormes
    handles.push(thread::spawn(move || {
      let _ = crate::guard(|| { let t0 = Instant::now();
        while !s.load(Relaxed) {
            st2.elapsed_ms.store(t0.elapsed().as_millis() as u64, Relaxed);
            if let Some(t) = sysinfo::cpu_temp() {
                st2.temp_bits.store(t.to_bits(), Relaxed);
                if opt(st2.max_temp_bits.load(Relaxed)).is_none_or(|m| t > m) { st2.max_temp_bits.store(t.to_bits(), Relaxed); }
                if temp_limit > 0.0 && t >= temp_limit {
                    *st2.reason.lock().unwrap_or_else(|e| e.into_inner()) = format!("Temperatura limite atingida ({t:.0} °C)");
                    break;
                }
            }
            if minutes > 0.0 && t0.elapsed() >= Duration::from_secs_f64(minutes * 60.0) {
                *st2.reason.lock().unwrap_or_else(|e| e.into_inner()) = "Tempo concluído".into();
                break;
            }
            if st2.mem_errors.load(Relaxed) > 0 {
                *st2.reason.lock().unwrap_or_else(|e| e.into_inner()) = "Erro de memória detectado".into();
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
        Ok(()) });
        s.store(true, Relaxed);
        st2.running.store(false, Relaxed);
    }));
    *g = Some(Run { stop, handles, stats });
    Ok(())
}

#[tauri::command]
pub fn stress_stop(st: tauri::State<StressState>) {
    let run = st.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(mut r) = run {
        r.stop.store(true, Relaxed);
        for h in r.handles.drain(..) { let _ = h.join(); }
        let mut why = r.stats.reason.lock().unwrap_or_else(|e| e.into_inner());
        if why.is_empty() { *why = "Parado pelo usuário".into(); }
        drop(why);
        // mantém as estatísticas para o status final
        *st.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Run { stop: r.stop, handles: vec![], stats: r.stats });
    }
}

#[tauri::command]
pub fn stress_status(st: tauri::State<StressState>) -> Option<StressStatus> {
    let g = st.0.lock().unwrap_or_else(|e| e.into_inner());
    let s = &g.as_ref()?.stats;
    Some(StressStatus {
        running: s.running.load(Relaxed),
        elapsed_s: s.elapsed_ms.load(Relaxed) as f64 / 1000.0,
        temp: opt(s.temp_bits.load(Relaxed)),
        max_temp: opt(s.max_temp_bits.load(Relaxed)),
        mem_errors: s.mem_errors.load(Relaxed),
        mem_passes: s.mem_passes.load(Relaxed),
        mem_gb: s.mem_bytes.load(Relaxed) as f64 / 1073741824.0,
        reason: s.reason.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ram_passes_without_errors_and_cpu_stops() {
        let (stop, stats) = (Arc::new(AtomicBool::new(false)), Arc::new(Stats::default()));
        let (s, st) = (stop.clone(), stats.clone());
        let ram = thread::spawn(move || ram_worker(s, 256 * 1024 * 1024, st));
        let s = stop.clone();
        let cpu = thread::spawn(move || cpu_worker(s, 0));
        thread::sleep(Duration::from_secs(3));
        stop.store(true, Relaxed);
        ram.join().unwrap(); cpu.join().unwrap();
        assert!(stats.mem_bytes.load(Relaxed) >= 256 * 1024 * 1024);
        assert!(stats.mem_passes.load(Relaxed) >= 1);
        assert_eq!(stats.mem_errors.load(Relaxed), 0);
    }
}
