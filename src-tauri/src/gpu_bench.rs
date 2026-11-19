//! Bench GPU · cena de jogo (GPU) em wgpu/Vulkan, fora da tela: terreno fractal, 40 mil árvores por instancing,
//! sombras 4096² com PCF, água, neblina e pós HDR. Câmera em trajeto fixo por tempo. Nota = FPS médio × 10.
use serde::Serialize;
use std::{
    sync::{atomic::{AtomicBool, Ordering::Relaxed}, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const TREES: u32 = 40_000;
const TREE_VERTS: u32 = 120;
const TERRAIN_VERTS: u32 = 384 * 384 * 6;
const SHADOW: u32 = 4096;

type M = [f32; 16]; // coluna por coluna, como o WGSL
fn mul(a: &M, b: &M) -> M {
    let mut r = [0f32; 16];
    for c in 0..4 { for rr in 0..4 { r[c * 4 + rr] = (0..4).map(|k| a[k * 4 + rr] * b[c * 4 + k]).sum(); } }
    r
}
pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
pub(crate) fn norm(a: [f32; 3]) -> [f32; 3] { let l = dot(a, a).sqrt(); [a[0] / l, a[1] / l, a[2] / l] }
fn look_at(eye: [f32; 3], at: [f32; 3]) -> M {
    let z = norm(sub(eye, at)); let x = norm(cross([0.0, 1.0, 0.0], z)); let y = cross(z, x);
    [x[0], y[0], z[0], 0.0, x[1], y[1], z[1], 0.0, x[2], y[2], z[2], 0.0, -dot(x, eye), -dot(y, eye), -dot(z, eye), 1.0]
}
fn perspective(fovy: f32, aspect: f32, n: f32, f: f32) -> M {
    let t = 1.0 / (fovy / 2.0).tan();
    [t / aspect, 0.0, 0.0, 0.0, 0.0, t, 0.0, 0.0, 0.0, 0.0, f / (n - f), -1.0, 0.0, 0.0, n * f / (n - f), 0.0]
}
fn ortho(l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) -> M {
    [2.0 / (r - l), 0.0, 0.0, 0.0, 0.0, 2.0 / (t - b), 0.0, 0.0, 0.0, 0.0, 1.0 / (n - f), 0.0, -(r + l) / (r - l), -(t + b) / (t - b), n / (n - f), 1.0]
}

#[derive(Serialize, Clone, Default)]
pub struct GpuBenchResult { adapter: String, width: u32, height: u32, seconds: f64, frames: u64, avg_fps: f64, low1_fps: f64, min_fps: f64, score: f64 }

#[derive(Serialize, Clone, Default)]
pub struct GpuBenchStatus { running: bool, progress: f64, fps_now: f64, frames: u64, adapter: String, result: Option<GpuBenchResult>, error: String }

#[derive(Default)]
pub struct GpuBenchState { st: Arc<Mutex<GpuBenchStatus>>, cancel: Arc<AtomicBool> }

#[cfg_attr(not(test), allow(dead_code))]
pub struct Rig {
    device: wgpu::Device, queue: wgpu::Queue, adapter_name: String,
    ubuf: wgpu::Buffer, shadow_view: wgpu::TextureView,
    bg_shadow: wgpu::BindGroup, bg_main: wgpu::BindGroup, bg_post: wgpu::BindGroup,
    p_sh_terrain: wgpu::RenderPipeline, p_sh_tree: wgpu::RenderPipeline,
    p_terrain: wgpu::RenderPipeline, p_tree: wgpu::RenderPipeline, p_water: wgpu::RenderPipeline, p_post: wgpu::RenderPipeline,
    hdr_view: wgpu::TextureView, depth_view: wgpu::TextureView, ldr: wgpu::Texture, ldr_view: wgpu::TextureView,
    w: u32, h: u32,
}

impl Rig {
    pub fn new(w: u32, h: u32) -> Result<Rig, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN)).into_iter()
            .filter(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
            .max_by_key(|a| matches!(a.get_info().device_type, wgpu::DeviceType::DiscreteGpu)).ok_or("Nenhuma GPU Vulkan encontrada")?;
        let adapter_name = adapter.get_info().name;
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: limits, ..Default::default() })).map_err(|e| e.to_string())?;

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("gpu_game.wgsl").into()) });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 176, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let tex = |label: &str, w, h, fmt, usage| device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: fmt, usage, view_formats: &[],
        });
        let rt_tex = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let shadow = tex("shadow", SHADOW, SHADOW, wgpu::TextureFormat::Depth32Float, rt_tex);
        let hdr = tex("hdr", w, h, wgpu::TextureFormat::Rgba16Float, rt_tex);
        let depth = tex("depth", w, h, wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let ldr = tex("ldr", w, h, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let (shadow_view, hdr_view, depth_view, ldr_view) = (shadow.create_view(&Default::default()), hdr.create_view(&Default::default()), depth.create_view(&Default::default()), ldr.create_view(&Default::default()));
        let smp = device.create_sampler(&wgpu::SamplerDescriptor { compare: Some(wgpu::CompareFunction::LessEqual), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });

        let ub = |binding, vis| wgpu::BindGroupLayoutEntry { binding, visibility: vis, count: None,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None } };
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let bgl_shadow = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[ub(0, wgpu::ShaderStages::VERTEX)] });
        let bgl_main = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[
            ub(0, vf),
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, count: None,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false } },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, count: None, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison) },
        ] });
        let lay = |bgl: &wgpu::BindGroupLayout| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(bgl)], immediate_size: 0 });
        let (lay_shadow, lay_main) = (lay(&bgl_shadow), lay(&bgl_main));

        let pipe = |layout: Option<&wgpu::PipelineLayout>, vs: &str, fs: Option<(&str, wgpu::TextureFormat, Option<wgpu::BlendState>)>, depth: Option<wgpu::DepthStencilState>| {
            let tgt = fs.as_ref().map(|(_, f, b)| [Some(wgpu::ColorTargetState { format: *f, blend: *b, write_mask: wgpu::ColorWrites::ALL })]);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None, layout,
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(), // sem culling: não depende do sentido dos triângulos
                depth_stencil: depth, multisample: Default::default(),
                fragment: fs.as_ref().zip(tgt.as_ref()).map(|((e, _, _), t)| wgpu::FragmentState { module: &module, entry_point: Some(e),
                    compilation_options: Default::default(), targets: t }),
                multiview_mask: None, cache: None,
            })
        };
        let ds = |write: bool, bias: i32| Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(write), depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(), bias: wgpu::DepthBiasState { constant: bias, slope_scale: if bias != 0 { 2.0 } else { 0.0 }, clamp: 0.0 },
        });
        let hdrf = wgpu::TextureFormat::Rgba16Float;
        let p_sh_terrain = pipe(Some(&lay_shadow), "vs_shadow_terrain", None, ds(true, 2));
        let p_sh_tree = pipe(Some(&lay_shadow), "vs_shadow_tree", None, ds(true, 2));
        let p_terrain = pipe(Some(&lay_main), "vs_terrain", Some(("fs_scene", hdrf, None)), ds(true, 0));
        let p_tree = pipe(Some(&lay_main), "vs_tree", Some(("fs_scene", hdrf, None)), ds(true, 0));
        let p_water = pipe(Some(&lay_main), "vs_water", Some(("fs_water", hdrf, Some(wgpu::BlendState::ALPHA_BLENDING))), ds(false, 0));
        let p_post = pipe(None, "vs_full", Some(("fs_post", wgpu::TextureFormat::Rgba8Unorm, None)), None);

        let bg = |layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry]| device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries });
        let bg_shadow = bg(&bgl_shadow, &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }]);
        let bg_main = bg(&bgl_main, &[
            wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&smp) },
        ]);
        let bg_post = bg(&p_post.get_bind_group_layout(0), &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hdr_view) }]);
        Ok(Rig { device, queue, adapter_name, ubuf, shadow_view, bg_shadow, bg_main, bg_post, p_sh_terrain, p_sh_tree, p_terrain, p_tree, p_water, p_post,
                 hdr_view, depth_view, ldr, ldr_view, w, h })
    }

    /// Renderiza um quadro no instante `t` (segundos) e espera a GPU terminar.
    pub fn frame(&self, t: f32) {
        let a = t * 0.12;
        let eye = [a.sin() * 260.0, 70.0 + 15.0 * (t * 0.3).sin(), a.cos() * 260.0];
        let at = [(a * 0.7).cos() * 80.0, 5.0, (a * 0.7).sin() * 80.0];
        let vp = mul(&perspective(60f32.to_radians(), self.w as f32 / self.h as f32, 1.0, 2500.0), &look_at(eye, at));
        let sun = norm([0.55, 0.75, 0.35]);
        let lvp = mul(&ortho(-560.0, 560.0, -560.0, 560.0, 1.0, 2400.0), &look_at([sun[0] * 1200.0, sun[1] * 1200.0, sun[2] * 1200.0], [0.0, 0.0, 0.0]));
        let mut data: Vec<f32> = vec![];
        data.extend(vp); data.extend(lvp); data.extend([eye[0], eye[1], eye[2], 0.0]); data.extend([sun[0], sun[1], sun[2], 0.0]); data.extend([t, 0.0, 0.0, 0.0]);
        self.queue.write_buffer(&self.ubuf, 0, &data.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>());

        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None, color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            p.set_bind_group(0, &self.bg_shadow, &[]);
            p.set_pipeline(&self.p_sh_terrain); p.draw(0..TERRAIN_VERTS, 0..1);
            p.set_pipeline(&self.p_sh_tree); p.draw(0..TREE_VERTS, 0..TREES);
        }
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.hdr_view, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.66, g: 0.77, b: 0.92, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            p.set_bind_group(0, &self.bg_main, &[]);
            p.set_pipeline(&self.p_terrain); p.draw(0..TERRAIN_VERTS, 0..1);
            p.set_pipeline(&self.p_tree); p.draw(0..TREE_VERTS, 0..TREES);
            p.set_pipeline(&self.p_water); p.draw(0..6, 0..1);
        }
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.ldr_view, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            p.set_pipeline(&self.p_post); p.set_bind_group(0, &self.bg_post, &[]); p.draw(0..3, 0..1);
        }
        let idx = self.queue.submit([enc.finish()]);
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None });
    }

    /// RGBA8 do último quadro (só para o teste visual).
    #[cfg(test)]
    pub fn read_ldr(&self) -> Vec<u8> {
        let row = (self.w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * self.h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(self.ldr.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(self.h) } },
            wgpu::Extent3d { width: self.w, height: self.h, depth_or_array_layers: 1 });
        self.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let data = buf.slice(..).get_mapped_range().unwrap();
        let mut out = Vec::with_capacity((self.w * self.h * 4) as usize);
        for y in 0..self.h as usize { out.extend_from_slice(&data[y * row as usize..y * row as usize + (self.w * 4) as usize]); }
        out
    }
}

fn run(w: u32, h: u32, secs: f64, st: &Mutex<GpuBenchStatus>, cancel: &AtomicBool) -> Result<GpuBenchResult, String> {
    let rig = Rig::new(w, h)?;
    st.lock().unwrap_or_else(|e| e.into_inner()).adapter = rig.adapter_name.clone();
    for i in 0..5 { rig.frame(i as f32 * 0.01); } // aquecimento (compila pipelines, enche caches)
    let mut times: Vec<f64> = Vec::new();
    let t0 = Instant::now();
    let mut win = (Instant::now(), 0u32);
    while t0.elapsed().as_secs_f64() < secs && !cancel.load(Relaxed) {
        let f = Instant::now();
        rig.frame(t0.elapsed().as_secs_f32());
        times.push(f.elapsed().as_secs_f64());
        win.1 += 1;
        if win.0.elapsed() >= Duration::from_millis(500) {
            let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
            g.fps_now = win.1 as f64 / win.0.elapsed().as_secs_f64(); g.frames = times.len() as u64; g.progress = t0.elapsed().as_secs_f64() / secs;
            win = (Instant::now(), 0);
        }
    }
    if cancel.load(Relaxed) { return Err("Cancelado".into()); }
    let total = t0.elapsed().as_secs_f64();
    let mut sorted = times.clone(); sorted.sort_by(|a, b| a.total_cmp(b));
    let p99 = sorted[((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1)];
    let avg = times.len() as f64 / total;
    Ok(GpuBenchResult { adapter: rig.adapter_name.clone(), width: w, height: h, seconds: total, frames: times.len() as u64, avg_fps: avg,
        low1_fps: 1.0 / p99, min_fps: 1.0 / sorted[sorted.len() - 1], score: avg * 10.0 })
}

#[tauri::command]
pub fn gpu_bench_start(state: tauri::State<GpuBenchState>, res: String, seconds: f64) -> Result<(), String> {
    let (w, h) = match res.as_str() { "2560x1440" => (2560, 1440), "3840x2160" => (3840, 2160), _ => (1920, 1080) };
    if state.st.lock().unwrap_or_else(|e| e.into_inner()).running { return Err("Já está rodando".into()); }
    state.cancel.store(false, Relaxed);
    *state.st.lock().unwrap_or_else(|e| e.into_inner()) = GpuBenchStatus { running: true, ..Default::default() };
    let (st, cancel) = (state.st.clone(), state.cancel.clone());
    thread::spawn(move || {
        let r = run(w, h, seconds.clamp(10.0, 120.0), &st, &cancel);
        let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
        g.running = false;
        match r { Ok(v) => { g.result = Some(v); g.progress = 1.0 } Err(e) => g.error = e }
    });
    Ok(())
}

#[tauri::command]
pub fn gpu_bench_cancel(state: tauri::State<GpuBenchState>) { state.cancel.store(true, Relaxed); }

#[tauri::command]
pub fn gpu_bench_status(state: tauri::State<GpuBenchState>) -> GpuBenchStatus { state.st.lock().unwrap_or_else(|e| e.into_inner()).clone() }

#[cfg(test)]
mod tests {
    use super::*;
    // Renderiza 1 quadro 1280x720 e salva RGBA cru em $CF_FRAME_OUT (se definido) para conferência visual.
    #[test]
    fn renders_scene() {
        let rig = match Rig::new(1280, 720) { Ok(r) => r, Err(e) => { println!("sem GPU: {e}"); return; } };
        for i in 0..3 { rig.frame(1.0 + i as f32); }
        let px = rig.read_ldr();
        let mean = px.chunks(4).map(|p| p[0] as f64 + p[1] as f64 + p[2] as f64).sum::<f64>() / (px.len() as f64 / 4.0) / 3.0;
        let distinct = px.chunks(4).step_by(997).map(|p| (p[0], p[1], p[2])).collect::<std::collections::HashSet<_>>().len();
        println!("{} média {mean:.1} cores distintas {distinct}", rig.adapter_name);
        assert!(mean > 20.0 && mean < 235.0 && distinct > 50);
        if let Ok(p) = std::env::var("CF_FRAME_OUT") { std::fs::write(p, &px).unwrap(); }
    }
}

#[cfg(test)]
mod full {
    use super::*;
    // Manual: cargo test full_1080p -- --ignored --nocapture
    #[test]
    #[ignore]
    fn full_1080p() {
        let st = Mutex::new(GpuBenchStatus::default());
        let r = run(1920, 1080, 30.0, &st, &AtomicBool::new(false)).unwrap();
        println!("{} {}x{}: média {:.1} FPS · 1% low {:.1} · mín {:.1} · nota {:.0}", r.adapter, r.width, r.height, r.avg_fps, r.low1_fps, r.min_fps, r.score);
    }
}
