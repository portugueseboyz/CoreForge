//! Stress de GPU por compute shader (wgpu/Vulkan): cadeia longa de FMA em todos os núcleos, sem janela e sem limite de FPS.
//! Para sozinho no limite de temperatura, no tempo escolhido ou ao ser parado. Consumo e temperatura vêm da NVML.
use serde::Serialize;
use std::{
    sync::{atomic::{AtomicBool, Ordering::Relaxed}, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> out: array<vec4<f32>>;
@group(0) @binding(1) var<uniform> cfg: vec4<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    var a = vec4<f32>(f32(id.x) * 0.001 + 1.0, 1.1, 0.9, 1.3);
    var b = vec4<f32>(0.5, 0.7, 0.3, 0.9);
    let c = vec4<f32>(1.0000001, 0.9999999, 1.0000002, 0.9999998);
    for (var i = 0u; i < cfg.x; i = i + 1u) {
        a = fma(a, c, b);
        b = fma(b, c, a * 0.0000001);
    }
    out[id.x & 65535u] = a + b;
}
"#;
const GROUPS: u32 = 16384;                       // x 256 = 4,2 milhões de invocações por dispatch
const FLOPS_PER_ITER: f64 = 20.0;                // 2 FMA vec4 (16) + 1 multiplicação vec4 (4)
const TARGET_MS: f64 = 60.0;                     // cada dispatch ~60 ms: bem abaixo do watchdog do driver

#[derive(Serialize, Clone, Default)]
pub struct GpuStressStatus {
    running: bool, adapter: String, elapsed_s: f64, dispatches: u64, gflops: f64,
    temp: Option<f64>, max_temp: Option<f64>, power_w: Option<f64>, max_power_w: Option<f64>, util: Option<u32>,
    reason: String, error: String,
}

#[derive(Default)]
pub struct GpuStressState { st: Arc<Mutex<GpuStressStatus>>, stop: Arc<AtomicBool>, handle: Mutex<Option<JoinHandle<()>>> }

fn work(intensity: u32, temp_limit: f64, minutes: f64, stop: &AtomicBool, st: &Mutex<GpuStressStatus>) -> Result<String, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));
    let adapter = adapters.into_iter().filter(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
        .max_by_key(|a| matches!(a.get_info().device_type, wgpu::DeviceType::DiscreteGpu))
        .ok_or("Nenhuma GPU Vulkan encontrada")?;
    st.lock().unwrap_or_else(|e| e.into_inner()).adapter = adapter.get_info().name;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).map_err(|e| e.to_string())?;

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None, layout: None, module: &module, entry_point: Some("main"),
        compilation_options: Default::default(), cache: None,
    });
    let out = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 65536 * 16, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
    let cfg = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &pipe.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: out.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: cfg.as_entire_binding() },
        ],
    });

    let dispatch = |iters: u32, n: usize| {
        queue.write_buffer(&cfg, 0, bytemuck_u32([iters, 0, 0, 0]).as_slice());
        let mut cmds = vec![];
        for _ in 0..n {
            let mut enc = device.create_command_encoder(&Default::default());
            { let mut p = enc.begin_compute_pass(&Default::default()); p.set_pipeline(&pipe); p.set_bind_group(0, &bg, &[]); p.dispatch_workgroups(GROUPS, 1, 1); }
            cmds.push(enc.finish());
        }
        let idx = queue.submit(cmds);
        let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None });
    };

    // Calibra as iterações para ~60 ms por dispatch nesta GPU.
    let mut iters = 256u32;
    for _ in 0..8 {
        let t = Instant::now();
        dispatch(iters, 1);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        if ms >= TARGET_MS * 0.7 { break; }
        iters = ((iters as f64 * TARGET_MS / ms.max(0.1)).min(iters as f64 * 8.0)) as u32;
    }

    let (t0, mut last_sample, mut win_start, mut win_flops) = (Instant::now(), Instant::now() - Duration::from_secs(1), Instant::now(), 0.0f64);
    let per_dispatch_flops = GROUPS as f64 * 256.0 * iters as f64 * FLOPS_PER_ITER;
    let mut n = 0u64;
    while !stop.load(Relaxed) {
        let copies = if intensity >= 3 { 2 } else { 1 }; // extrema: fila de 2 dispatches, a GPU nunca fica ociosa
        let d0 = Instant::now();
        dispatch(iters, copies);
        n += copies as u64;
        win_flops += per_dispatch_flops * copies as f64;
        if intensity == 1 { thread::sleep(d0.elapsed().mul_f64(0.25)); } // normal: ~80% de ocupação

        if win_start.elapsed() >= Duration::from_secs(1) {
            let g = win_flops / win_start.elapsed().as_secs_f64() / 1e9;
            st.lock().unwrap_or_else(|e| e.into_inner()).gflops = g; win_start = Instant::now(); win_flops = 0.0;
        }
        let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
        g.dispatches = n; g.elapsed_s = t0.elapsed().as_secs_f64();
        if last_sample.elapsed() >= Duration::from_millis(500) {
            last_sample = Instant::now();
            if let Some((t, p, u)) = crate::gpu::first_sample() {
                g.temp = Some(t); g.power_w = p; g.util = u;
                g.max_temp = Some(g.max_temp.map_or(t, |m| m.max(t)));
                if let Some(p) = p { g.max_power_w = Some(g.max_power_w.map_or(p, |m| m.max(p))); }
                if temp_limit > 0.0 && t >= temp_limit { return Ok(format!("Temperatura limite atingida ({t:.0} °C)")); }
            }
        }
        if minutes > 0.0 && t0.elapsed() >= Duration::from_secs_f64(minutes * 60.0) { return Ok("Tempo concluído".into()); }
    }
    Ok("Parado pelo usuário".into())
}

fn bytemuck_u32(v: [u32; 4]) -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() }

#[tauri::command]
pub fn gpu_stress_start(state: tauri::State<GpuStressState>, intensity: u32, temp_limit: f64, minutes: f64) -> Result<(), String> {
    let mut h = state.handle.lock().unwrap_or_else(|e| e.into_inner());
    if h.as_ref().is_some_and(|j| !j.is_finished()) { return Err("Já está rodando".into()); }
    state.stop.store(false, Relaxed);
    *state.st.lock().unwrap_or_else(|e| e.into_inner()) = GpuStressStatus { running: true, ..Default::default() };
    let (st, stop) = (state.st.clone(), state.stop.clone());
    *h = Some(thread::spawn(move || {
        let r = crate::guard(|| work(intensity.clamp(1, 3), temp_limit, minutes, &stop, &st));
        let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
        g.running = false;
        match r { Ok(why) => g.reason = why, Err(e) => g.error = e }
    }));
    Ok(())
}

#[tauri::command]
pub fn gpu_stress_stop(state: tauri::State<GpuStressState>) {
    state.stop.store(true, Relaxed);
    if let Some(j) = state.handle.lock().unwrap_or_else(|e| e.into_inner()).take() { let _ = j.join(); }
}

#[tauri::command]
pub fn gpu_stress_status(state: tauri::State<GpuStressState>) -> GpuStressStatus { state.st.lock().unwrap_or_else(|e| e.into_inner()).clone() }

#[cfg(test)]
mod tests {
    use super::*;
    // Roda 4 s na GPU real (se houver Vulkan) e confere que mediu trabalho.
    #[test]
    #[ignore = "precisa de Vulkan e GPU reais; rode com --ignored"]
    fn runs_briefly() {
        let st = Mutex::new(GpuStressStatus::default());
        let stop = AtomicBool::new(false);
        thread::scope(|s| {
            s.spawn(|| { thread::sleep(Duration::from_secs(4)); stop.store(true, Relaxed); });
            match work(2, 0.0, 0.0, &stop, &st) {
                Ok(why) => println!("{why}"),
                Err(e) => { println!("sem GPU Vulkan: {e}");}
            }
        });
        let g = st.lock().unwrap_or_else(|e| e.into_inner());
        println!("{} · {} dispatches · {:.0} GFLOPS · {:?} °C · {:?} W", g.adapter, g.dispatches, g.gflops, g.max_temp, g.max_power_w);
        assert!(g.dispatches > 0);
    }
}
