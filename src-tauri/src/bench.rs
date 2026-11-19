//! Bench CPU: path tracer no modelo do Cinebench. Cena, sementes por pixel e tone mapping iguais ao app
//! Windows; a imagem é dividida em blocos de 32 px distribuídos entre threads do centro para fora.
//! Nota = amostras/s ÷ 10.000 (numa máquina e versão, comparável entre execuções).
use serde::Serialize;
use std::{
    ops::{Add, Mul, Neg, Sub},
    sync::{atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering::Relaxed}, Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

pub const W: usize = 960;
pub const H: usize = 540;
const BUCKET: usize = 32;
pub const SPP: u32 = 1024;
const MAX_DEPTH: u32 = 6;
const LAMP_POWER: f32 = 9.0;

#[derive(Clone, Copy)]
struct V(f32, f32, f32);
impl Add for V { type Output = V; fn add(self, o: V) -> V { V(self.0 + o.0, self.1 + o.1, self.2 + o.2) } }
impl Sub for V { type Output = V; fn sub(self, o: V) -> V { V(self.0 - o.0, self.1 - o.1, self.2 - o.2) } }
impl Mul<f32> for V { type Output = V; fn mul(self, k: f32) -> V { V(self.0 * k, self.1 * k, self.2 * k) } }
impl Neg for V { type Output = V; fn neg(self) -> V { V(-self.0, -self.1, -self.2) } }
impl V {
    fn dot(self, o: V) -> f32 { self.0 * o.0 + self.1 * o.1 + self.2 * o.2 }
    fn cross(self, o: V) -> V { V(self.1 * o.2 - self.2 * o.1, self.2 * o.0 - self.0 * o.2, self.0 * o.1 - self.1 * o.0) }
    fn len2(self) -> f32 { self.dot(self) }
    fn norm(self) -> V { self * (1.0 / self.len2().sqrt()) }
    fn reflect(self, n: V) -> V { self - n * (2.0 * self.dot(n)) }
}

#[derive(Clone, Copy, PartialEq)]
enum Mat { Diffuse, Metal, Glass }

#[derive(Clone, Copy)]
struct Sphere { c: V, r: f32, m: Mat, albedo: f32, rough: f32 }

const LAMP: Sphere = Sphere { c: V(-4.0, 7.0, -3.0), r: 2.2, m: Mat::Diffuse, albedo: 0.0, rough: 0.0 };

fn scene() -> &'static [Sphere] {
    static S: OnceLock<Vec<Sphere>> = OnceLock::new();
    S.get_or_init(|| {
        let sp = |c, r, m, albedo, rough| Sphere { c, r, m, albedo, rough };
        let mut s = vec![
            sp(V(0.0, 1.1, 0.0), 1.1, Mat::Glass, 0.98, 0.0),
            sp(V(-2.5, 0.9, 0.9), 0.9, Mat::Metal, 0.92, 0.03),
            sp(V(2.4, 0.8, 0.7), 0.8, Mat::Diffuse, 0.85, 0.0),
            sp(V(1.0, 0.45, -1.9), 0.45, Mat::Metal, 0.85, 0.25),
            sp(V(-1.1, 0.35, -2.0), 0.35, Mat::Diffuse, 0.15, 0.0),
        ];
        for i in 0..14 {
            let a = i as f32 / 14.0 * std::f32::consts::TAU;
            let m = match i % 3 { 0 => Mat::Metal, 1 => Mat::Diffuse, _ => Mat::Glass };
            s.push(sp(V(a.cos() * 4.2, 0.28, a.sin() * 4.2 + 1.5), 0.28, m, if i % 2 == 0 { 0.9 } else { 0.3 }, 0.1));
        }
        s
    })
}

struct Cam { o: V, lower: V, h: V, v: V }
fn cam() -> &'static Cam {
    static C: OnceLock<Cam> = OnceLock::new();
    C.get_or_init(|| {
        let o = V(0.0, 2.1, -7.2);
        let target = V(0.0, 0.75, 0.0);
        let vfov = 38f32.to_radians();
        let hh = (vfov / 2.0).tan();
        let hw = (W as f32 / H as f32) * hh;
        let w = (o - target).norm();
        let u = V(0.0, 1.0, 0.0).cross(w).norm();
        let v = w.cross(u);
        Cam { o, lower: o - u * hw + v * hh - w, h: u * (2.0 * hw), v: -(v * (2.0 * hh)) }
    })
}

fn rnd(s: &mut u32) -> f32 {
    *s ^= *s << 13; *s ^= *s >> 17; *s ^= *s << 5;
    (*s >> 8) as f32 * (1.0 / 16777216.0)
}

fn rand_in_sphere(s: &mut u32) -> V {
    loop {
        let v = V(rnd(s) * 2.0 - 1.0, rnd(s) * 2.0 - 1.0, rnd(s) * 2.0 - 1.0);
        if v.len2() < 1.0 { return v; }
    }
}

fn basis(n: V) -> (V, V) {
    let a = (if n.0.abs() > 0.9 { V(0.0, 1.0, 0.0) } else { V(1.0, 0.0, 0.0) }).cross(n).norm();
    (a, n.cross(a))
}

fn cosine_hemisphere(n: V, s: &mut u32) -> V {
    let (r1, r2) = (rnd(s), rnd(s));
    let (r, phi) = (r1.sqrt(), std::f32::consts::TAU * r2);
    let (a, b) = basis(n);
    (a * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - r1).sqrt()).norm()
}

fn hit_sphere(s: &Sphere, o: V, d: V, t_max: f32) -> Option<f32> {
    let oc = o - s.c;
    let (b, c) = (oc.dot(d), oc.len2() - s.r * s.r);
    let disc = b * b - c;
    if disc < 0.0 { return None; }
    let sq = disc.sqrt();
    let mut t = -b - sq;
    if t < 1e-4 { t = -b + sq; }
    (t > 1e-4 && t < t_max).then_some(t)
}

/// (t, índice: -1 chão / >=0 esfera, bateu na lâmpada?)
fn intersect(o: V, d: V) -> Option<(f32, i32, bool)> {
    let mut t_min = f32::MAX;
    let mut idx = -2;
    let mut lamp = false;
    if d.1 < -1e-6 {
        let t = -o.1 / d.1;
        if t > 1e-4 && t < 60.0 { t_min = t; idx = -1; }
    }
    for (i, s) in scene().iter().enumerate() {
        if let Some(t) = hit_sphere(s, o, d, t_min) { t_min = t; idx = i as i32; }
    }
    if let Some(t) = hit_sphere(&LAMP, o, d, t_min) { t_min = t; lamp = true; }
    (idx != -2 || lamp).then_some((t_min, idx, lamp))
}

fn sky(d: V) -> f32 { 0.06 + 0.22 * d.1.max(0.0) }

fn direct_light(p: V, n: V, s: &mut u32) -> f32 {
    let to_c = LAMP.c - p;
    let dist2 = to_c.len2();
    let cos_max = (1.0 - LAMP.r * LAMP.r / dist2).max(0.0).sqrt();
    let w = to_c * (1.0 / dist2.sqrt());
    let (r1, r2) = (rnd(s), rnd(s));
    let cos_t = 1.0 - r1 * (1.0 - cos_max);
    let sin_t = (1.0 - cos_t * cos_t).sqrt();
    let phi = std::f32::consts::TAU * r2;
    let (a, b) = basis(w);
    let dir = (a * (phi.cos() * sin_t) + b * (phi.sin() * sin_t) + w * cos_t).norm();
    let cos_n = dir.dot(n);
    if cos_n <= 0.0 { return 0.0; }
    match intersect(p + n * 1e-3, dir) { Some((_, _, true)) => LAMP_POWER * cos_n * 2.0 * (1.0 - cos_max), _ => 0.0 }
}

fn trace(mut o: V, mut d: V, s: &mut u32) -> f32 {
    let (mut radiance, mut through) = (0.0f32, 1.0f32);
    let mut specular = true;
    for depth in 0..MAX_DEPTH {
        let Some((t, idx, lamp)) = intersect(o, d) else { radiance += through * sky(d); break };
        if lamp { if specular { radiance += through * LAMP_POWER; } break; }
        let p = o + d * t;
        let (n, mat, albedo);
        if idx < 0 {
            n = V(0.0, 1.0, 0.0); mat = Mat::Diffuse;
            let check = (((p.0 * 0.9).floor() as i32 + (p.2 * 0.9).floor() as i32) & 1) == 0;
            albedo = if check { 0.62 } else { 0.38 };
        } else {
            let sp = scene()[idx as usize];
            n = (p - sp.c) * (1.0 / sp.r); mat = sp.m; albedo = sp.albedo;
        }
        let rough = if idx >= 0 { scene()[idx as usize].rough } else { 0.0 };
        match mat {
            Mat::Diffuse => {
                radiance += through * albedo * direct_light(p, n, s);
                through *= albedo;
                d = cosine_hemisphere(n, s);
                o = p + n * 1e-3;
                specular = false;
            }
            Mat::Metal => {
                let r = d.reflect(n) + rand_in_sphere(s) * rough;
                if r.dot(n) <= 0.0 { return radiance; }
                d = r.norm(); o = p + n * 1e-3; through *= albedo; specular = true;
            }
            Mat::Glass => {
                const IOR: f32 = 1.5;
                let outside = d.dot(n) < 0.0;
                let nn = if outside { n } else { -n };
                let eta = if outside { 1.0 / IOR } else { IOR };
                let cos = (-d.dot(nn)).min(1.0);
                let sin2 = eta * eta * (1.0 - cos * cos);
                let mut r0 = (1.0 - IOR) / (1.0 + IOR); r0 *= r0;
                let fresnel = r0 + (1.0 - r0) * (1.0 - cos).powi(5);
                if sin2 > 1.0 || rnd(s) < fresnel {
                    d = d.reflect(nn); o = p + nn * 1e-3;
                } else {
                    d = (d * eta + nn * (eta * cos - (1.0 - sin2).sqrt())).norm(); o = p - nn * 1e-3;
                }
                through *= albedo; specular = true;
            }
        }
        if depth >= 3 {
            let q = (1.0 - through).max(0.05);
            if rnd(s) < q { break; }
            through /= 1.0 - q;
        }
    }
    radiance
}

/// ACES aproximado + gama 2.2 → cinza 0..255.
fn to_gray(mut v: f32) -> u8 {
    v *= 1.1;
    v = v * (2.51 * v + 0.03) / (v * (2.43 * v + 0.59) + 0.14);
    (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
}

fn buckets() -> Vec<(usize, usize, usize, usize)> {
    let mut l = vec![];
    for y in (0..H).step_by(BUCKET) { for x in (0..W).step_by(BUCKET) { l.push((x, y, BUCKET.min(W - x), BUCKET.min(H - y))); } }
    let key = |b: &(usize, usize, usize, usize)| {
        let dx = b.0 as f64 + b.2 as f64 / 2.0 - W as f64 / 2.0;
        let dy = (b.1 as f64 + b.3 as f64 / 2.0 - H as f64 / 2.0) * 1.6;
        dx * dx + dy * dy
    };
    l.sort_by(|a, b| key(a).total_cmp(&key(b)));
    l
}

pub struct Shared {
    pixels: Vec<AtomicU8>,
    samples: AtomicU64,
    buckets_done: AtomicU32,
    buckets_total: AtomicU32,
    passes: AtomicU32,
    running: AtomicBool,
    cancel: AtomicBool,
    result: Mutex<Option<BenchResult>>,
    started: Mutex<Option<Instant>>,
}

#[derive(Serialize, Clone)]
pub struct BenchResult { threads: usize, seconds: f64, samples: u64, passes: u32, score: f64 }

#[derive(Serialize)]
pub struct BenchStatus {
    running: bool,
    progress: f64,
    passes: u32,
    elapsed_s: f64,
    result: Option<BenchResult>,
}

pub struct BenchState(pub Arc<Shared>);
impl Default for BenchState {
    fn default() -> Self {
        Self(Arc::new(Shared {
            pixels: (0..W * H).map(|_| AtomicU8::new(0)).collect(),
            samples: AtomicU64::new(0), buckets_done: AtomicU32::new(0), buckets_total: AtomicU32::new(1), passes: AtomicU32::new(0),
            running: AtomicBool::new(false), cancel: AtomicBool::new(false), result: Mutex::new(None), started: Mutex::new(None),
        }))
    }
}

fn render_bucket(sh: &Shared, b: (usize, usize, usize, usize), spp: u32) {
    for y in b.1..b.1 + b.3 {
        if sh.cancel.load(Relaxed) { return; }
        for x in b.0..b.0 + b.2 {
            let mut seed = ((y * W + x) as u32).wrapping_mul(9781).wrapping_add(0x9E3779B9);
            let mut sum = 0.0f32;
            let c = cam();
            for _ in 0..spp {
                let u = (x as f32 + rnd(&mut seed)) / W as f32;
                let v = (y as f32 + rnd(&mut seed)) / H as f32;
                let dir = (c.lower + c.h * u + c.v * v - c.o).norm();
                sum += trace(c.o, dir, &mut seed).min(4.0); // limita "fireflies"
            }
            sh.pixels[y * W + x].store(to_gray(sum / spp as f32), Relaxed);
        }
        sh.samples.fetch_add((b.2 as u64) * spp as u64, Relaxed);
    }
}

pub fn run(sh: Arc<Shared>, threads: usize, min: Duration, spp: u32) {
    let list = buckets();
    sh.buckets_total.store(list.len() as u32, Relaxed);
    sh.samples.store(0, Relaxed);
    sh.passes.store(0, Relaxed);
    let t0 = Instant::now();
    *sh.started.lock().unwrap_or_else(|e| e.into_inner()) = Some(t0);
    loop {
        sh.buckets_done.store(0, Relaxed);
        for p in &sh.pixels { p.store(0, Relaxed); }
        let next = AtomicUsize::new(0);
        thread::scope(|sc| {
            for _ in 0..threads {
                sc.spawn(|| loop {
                    let i = next.fetch_add(1, Relaxed);
                    if i >= list.len() || sh.cancel.load(Relaxed) { break; }
                    render_bucket(&sh, list[i], spp);
                    sh.buckets_done.fetch_add(1, Relaxed);
                });
            }
        });
        if sh.cancel.load(Relaxed) { break; }
        sh.passes.fetch_add(1, Relaxed);
        if t0.elapsed() >= min { break; }
    }
    if !sh.cancel.load(Relaxed) {
        let secs = t0.elapsed().as_secs_f64();
        let samples = sh.samples.load(Relaxed);
        *sh.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(BenchResult { threads, seconds: secs, samples, passes: sh.passes.load(Relaxed), score: samples as f64 / secs / 10_000.0 });
    }
    sh.running.store(false, Relaxed);
}

#[tauri::command]
pub fn bench_start(st: tauri::State<BenchState>, multi: bool, min_minutes: f64, spp: Option<u32>) -> Result<(), String> {
    let sh = st.0.clone();
    if sh.running.swap(true, Relaxed) { return Err("Já está rodando".into()); }
    sh.cancel.store(false, Relaxed);
    *sh.result.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let threads = if multi { thread::available_parallelism().map_or(1, |n| n.get()) } else { 1 };
    let min = Duration::from_secs_f64(min_minutes.max(0.0) * 60.0);
    thread::spawn(move || {
        let s2 = sh.clone();
        let _ = crate::guard(|| { run(sh, threads, min, spp.unwrap_or(SPP).max(1)); Ok(()) });
        s2.running.store(false, Relaxed);
    });
    Ok(())
}

#[tauri::command]
pub fn bench_cancel(st: tauri::State<BenchState>) { st.0.cancel.store(true, Relaxed); }

#[tauri::command]
pub fn bench_status(st: tauri::State<BenchState>) -> BenchStatus {
    let s = &st.0;
    let total = s.buckets_total.load(Relaxed).max(1) as f64;
    BenchStatus {
        running: s.running.load(Relaxed),
        progress: s.buckets_done.load(Relaxed) as f64 / total,
        passes: s.passes.load(Relaxed),
        elapsed_s: s.started.lock().unwrap_or_else(|e| e.into_inner()).map_or(0.0, |t| t.elapsed().as_secs_f64()),
        result: s.result.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}

/// Imagem atual em tons de cinza, W×H bytes (o front desenha num canvas).
#[tauri::command]
pub fn bench_image(st: tauri::State<BenchState>) -> tauri::ipc::Response {
    tauri::ipc::Response::new(st.0.pixels.iter().map(|p| p.load(Relaxed)).collect::<Vec<u8>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Cena pequena (4 amostras/pixel, 1 thread): termina em segundos e a imagem não pode sair preta nem igual em todo pixel.
    #[test]
    fn renders_image() {
        let st = BenchState::default();
        st.0.running.store(true, Relaxed);
        run(st.0.clone(), 4, Duration::ZERO, 4);
        let r = st.0.result.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap();
        assert_eq!(r.samples, (W * H) as u64 * 4);
        let px: Vec<u8> = st.0.pixels.iter().map(|p| p.load(Relaxed)).collect();
        let mean = px.iter().map(|&v| v as f64).sum::<f64>() / px.len() as f64;
        assert!(mean > 20.0 && mean < 200.0, "média {mean}");
        assert!(px.iter().any(|&v| v > 200) && px.iter().any(|&v| v < 60));
        println!("score(spp=4)={:.1} em {:.2}s", r.score, r.seconds);
    }
}

#[cfg(test)]
mod full {
    use super::*;
    // Manual: cargo test full_multi -- --ignored --nocapture
    #[test]
    #[ignore]
    fn full_multi() {
        let st = BenchState::default();
        st.0.running.store(true, Relaxed);
        let n = thread::available_parallelism().unwrap().get();
        run(st.0.clone(), n, Duration::ZERO, SPP);
        let r = st.0.result.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap();
        println!("multi {} threads: score {:.0} em {:.1}s", r.threads, r.score, r.seconds);
    }
}
