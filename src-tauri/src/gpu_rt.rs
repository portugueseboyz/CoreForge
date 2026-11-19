//! Bench RT (GPU): path tracing com ray queries do Vulkan (núcleos de RT em RTX 20+, Radeon RX 6000+, Intel Arc).
//! 4 mil esferas animadas (TLAS refeita a cada quadro), aquecimento, 3 passadas e nota pela mediana do FPS × 10.
use crate::gpu_bench::{cross, norm, sub};
use serde::Serialize;
use std::{
    sync::{atomic::{AtomicBool, Ordering::Relaxed}, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const OBJECTS: usize = 4000;
const PASS_SECS: f64 = 10.0;

#[derive(Serialize, Clone, Default)]
pub struct RtResult { adapter: String, width: u32, height: u32, preset: String, pass_fps: Vec<f64>, median_fps: f64, score: f64 }

#[derive(Serialize, Clone, Default)]
pub struct RtStatus { running: bool, phase: String, progress: f64, fps_now: f64, adapter: String, result: Option<RtResult>, error: String }

#[derive(Default)]
pub struct RtState { st: Arc<Mutex<RtStatus>>, cancel: Arc<AtomicBool> }

fn icosphere() -> (Vec<[f32; 3]>, Vec<u32>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut v: Vec<[f32; 3]> = [[-1., t, 0.], [1., t, 0.], [-1., -t, 0.], [1., -t, 0.], [0., -1., t], [0., 1., t], [0., -1., -t], [0., 1., -t], [t, 0., -1.], [t, 0., 1.], [-t, 0., -1.], [-t, 0., 1.]]
        .iter().map(|p| norm(*p)).collect();
    let mut f: Vec<[u32; 3]> = vec![[0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11], [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9], [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1]];
    for _ in 0..2 {
        let mut cache = std::collections::HashMap::new();
        let mut mid = |a: u32, b: u32, v: &mut Vec<[f32; 3]>| *cache.entry((a.min(b), a.max(b))).or_insert_with(|| {
            let (p, q) = (v[a as usize], v[b as usize]);
            v.push(norm([(p[0] + q[0]) / 2., (p[1] + q[1]) / 2., (p[2] + q[2]) / 2.])); (v.len() - 1) as u32
        });
        let mut nf = vec![];
        for [a, b, c] in &f { let (ab, bc, ca) = (mid(*a, *b, &mut v), mid(*b, *c, &mut v), mid(*c, *a, &mut v)); nf.extend([[*a, ab, ca], [*b, bc, ab], [*c, ca, bc], [ab, bc, ca]]); }
        f = nf;
    }
    (v, f.into_iter().flatten().collect())
}

fn hash01(i: u32) -> f32 { let mut x = i.wrapping_mul(747796405).wrapping_add(2891336453); x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737); ((x >> 22) ^ x) as f32 / 4294967296.0 }

#[cfg_attr(not(test), allow(dead_code))]
struct Rig {
    device: wgpu::Device, queue: wgpu::Queue, name: String, pipe: wgpu::ComputePipeline, bg: wgpu::BindGroup,
    ubuf: wgpu::Buffer, tlas: wgpu::Tlas, blas_sphere: wgpu::Blas, tex: wgpu::Texture, w: u32, h: u32,
    built: bool, geo: Geo,
}
struct Geo { sph_vb: wgpu::Buffer, sph_ib: wgpu::Buffer, sph_size: wgpu::BlasTriangleGeometrySizeDescriptor, floor_vb: wgpu::Buffer, floor_ib: wgpu::Buffer, floor_size: wgpu::BlasTriangleGeometrySizeDescriptor, blas_floor: wgpu::Blas }

impl Rig {
    fn new(w: u32, h: u32) -> Result<Rig, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN)).into_iter()
            .filter(|a| a.get_info().device_type != wgpu::DeviceType::Cpu && a.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY))
            .max_by_key(|a| matches!(a.get_info().device_type, wgpu::DeviceType::DiscreteGpu))
            .ok_or("Nenhuma GPU com ray tracing por hardware (VK_KHR_ray_query) encontrada")?;
        let name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::EXPERIMENTAL_RAY_QUERY, required_limits: adapter.limits(),
            // Ray query é experimental no wgpu: pedimos explicitamente, como a documentação exige.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() }, ..Default::default()
        })).map_err(|e| e.to_string())?;

        let (sv, si) = icosphere();
        let buf = |data: &[u8], usage| { let b = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: data.len() as u64, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }); queue.write_buffer(&b, 0, data); b };
        let flat: Vec<u8> = sv.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let sph_vb = buf(&flat, wgpu::BufferUsages::BLAS_INPUT);
        let sph_ib = buf(&si.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<u8>>(), wgpu::BufferUsages::BLAS_INPUT);
        let fv: [[f32; 3]; 4] = [[-400., 0., -400.], [400., 0., -400.], [400., 0., 400.], [-400., 0., 400.]];
        let floor_vb = buf(&fv.iter().flatten().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>(), wgpu::BufferUsages::BLAS_INPUT);
        let floor_ib = buf(&[0u32, 2, 1, 0, 3, 2].iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<u8>>(), wgpu::BufferUsages::BLAS_INPUT);
        let size = |vc: usize, ic: usize| wgpu::BlasTriangleGeometrySizeDescriptor { vertex_format: wgpu::VertexFormat::Float32x3, vertex_count: vc as u32,
            index_format: Some(wgpu::IndexFormat::Uint32), index_count: Some(ic as u32), flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE };
        let (sph_size, floor_size) = (size(sv.len(), si.len()), size(4, 6));
        let bdesc = wgpu::CreateBlasDescriptor { label: None, flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE, update_mode: wgpu::AccelerationStructureUpdateMode::Build };
        let blas_sphere = device.create_blas(&bdesc, wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: vec![sph_size.clone()] });
        let blas_floor = device.create_blas(&bdesc, wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: vec![floor_size.clone()] });
        let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor { label: None, max_instances: OBJECTS as u32 + 1,
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE, update_mode: wgpu::AccelerationStructureUpdateMode::Build });
        let ident = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.];
        for i in 0..OBJECTS { *tlas.get_mut_single(i).unwrap() = Some(wgpu::TlasInstance::new(&blas_sphere, ident, i as u32, 0xFF)); }
        *tlas.get_mut_single(OBJECTS).unwrap() = Some(wgpu::TlasInstance::new(&blas_floor, ident, 255, 0xFF));

        let tex = device.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 96, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("gpu_rt.wgsl").into()) });
        let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &pipe.get_bind_group_layout(0), entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: tlas.as_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) },
        ] });
        Ok(Rig { device, queue, name, pipe, bg, ubuf, tlas, blas_sphere, tex, w, h, built: false,
                 geo: Geo { sph_vb, sph_ib, sph_size, floor_vb, floor_ib, floor_size, blas_floor } })
    }

    fn frame(&mut self, t: f32, frame: u32, spp: u32, bounces: u32) {
        // objetos animados: oscilam em y; a TLAS é refeita a cada quadro
        for i in 0..OBJECTS {
            let (gx, gz) = ((i % 80) as f32 - 40.0, (i / 80) as f32 - 10.0);
            let r = 0.5 + 0.5 * hash01(i as u32 * 7 + 3);
            let y = r + 0.35 * (t * (0.6 + hash01(i as u32 + 11)) + hash01(i as u32 + 99) * std::f32::consts::TAU).sin().abs();
            let (x, z) = (gx * 2.4 + (hash01(i as u32 + 5) - 0.5) * 1.2, gz * 2.4 + (hash01(i as u32 + 17) - 0.5) * 1.2);
            let tr = [r, 0., 0., x, 0., r, 0., y, 0., 0., r, z];
            let slot = self.tlas.get_mut_single(i).unwrap();
            *slot = Some(wgpu::TlasInstance::new(&self.blas_sphere, tr, i as u32, 0xFF));
        }
        let a = t * 0.15;
        let eye = [a.sin() * 40.0, 5.0 + 1.5 * (t * 0.4).sin(), a.cos() * 18.0 - 6.0];
        let at = [(a * 0.7).cos() * 20.0, 1.5, 10.0];
        let fwd = norm(sub(at, eye)); let right = norm(cross(fwd, [0., 1., 0.])); let up = cross(right, fwd);
        let f = 0.9f32; // fator de campo de visão
        let mut d: Vec<u8> = vec![];
        for v in [[eye[0], eye[1], eye[2], 0.0], [fwd[0], fwd[1], fwd[2], 0.0], [right[0] * f, right[1] * f, right[2] * f, 0.0], [up[0] * f, up[1] * f, up[2] * f, 0.0]] { d.extend(v.iter().flat_map(|x| x.to_le_bytes())); }
        for v in [spp, bounces, frame, 0] { d.extend(v.to_le_bytes()); }
        for v in [t, 0.0f32, 0.0, 0.0] { d.extend(v.to_le_bytes()); }
        self.queue.write_buffer(&self.ubuf, 0, &d);

        let mut enc = self.device.create_command_encoder(&Default::default());
        if !self.built {
            let entries = [
                wgpu::BlasBuildEntry { blas: &self.blas_sphere, geometry: wgpu::BlasGeometries::TriangleGeometries(vec![wgpu::BlasTriangleGeometry {
                    size: &self.geo.sph_size, vertex_buffer: &self.geo.sph_vb, first_vertex: 0, vertex_stride: 12, index_buffer: Some(&self.geo.sph_ib), first_index: Some(0),
                    transform_buffer: None, transform_buffer_offset: None }]) },
                wgpu::BlasBuildEntry { blas: &self.geo.blas_floor, geometry: wgpu::BlasGeometries::TriangleGeometries(vec![wgpu::BlasTriangleGeometry {
                    size: &self.geo.floor_size, vertex_buffer: &self.geo.floor_vb, first_vertex: 0, vertex_stride: 12, index_buffer: Some(&self.geo.floor_ib), first_index: Some(0),
                    transform_buffer: None, transform_buffer_offset: None }]) },
            ];
            enc.build_acceleration_structures(entries.iter(), [&self.tlas]);
            self.built = true;
        } else {
            enc.build_acceleration_structures(std::iter::empty::<&wgpu::BlasBuildEntry>(), [&self.tlas]);
        }
        {
            let mut p = enc.begin_compute_pass(&Default::default());
            p.set_pipeline(&self.pipe); p.set_bind_group(0, &self.bg, &[]);
            p.dispatch_workgroups(self.w.div_ceil(8), self.h.div_ceil(8), 1);
        }
        let idx = self.queue.submit([enc.finish()]);
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None });
    }

    #[cfg(test)]
    fn read(&self) -> Vec<u8> {
        let row = (self.w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * self.h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(self.tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(self.h) } },
            wgpu::Extent3d { width: self.w, height: self.h, depth_or_array_layers: 1 });
        self.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let data = buf.slice(..).get_mapped_range().unwrap();
        (0..self.h as usize).flat_map(|y| data[y * row as usize..y * row as usize + (self.w * 4) as usize].to_vec()).collect()
    }
}

fn run(w: u32, h: u32, preset: &str, st: &Mutex<RtStatus>, cancel: &AtomicBool) -> Result<RtResult, String> {
    let (spp, bounces) = match preset { "Normal" => (2, 2), "Extrema" => (8, 4), _ => (4, 3) };
    let mut rig = Rig::new(w, h)?;
    st.lock().unwrap_or_else(|e| e.into_inner()).adapter = rig.name.clone();
    st.lock().unwrap_or_else(|e| e.into_inner()).phase = "Aquecimento".into();
    for i in 0..15 { rig.frame(i as f32 * 0.02, i, spp, bounces); }
    let mut pass_fps = vec![];
    for pass in 0..3 {
        if cancel.load(Relaxed) { return Err("Cancelado".into()); }
        { let mut g = st.lock().unwrap_or_else(|e| e.into_inner()); g.phase = format!("Passada {}/3", pass + 1); g.progress = pass as f64 / 3.0; }
        let (t0, mut n, mut win) = (Instant::now(), 0u32, (Instant::now(), 0u32));
        while t0.elapsed().as_secs_f64() < PASS_SECS && !cancel.load(Relaxed) {
            rig.frame(t0.elapsed().as_secs_f32(), n, spp, bounces);
            n += 1; win.1 += 1;
            if win.0.elapsed() >= Duration::from_millis(500) {
                let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
                g.fps_now = win.1 as f64 / win.0.elapsed().as_secs_f64();
                g.progress = (pass as f64 + t0.elapsed().as_secs_f64() / PASS_SECS) / 3.0;
                win = (Instant::now(), 0);
            }
        }
        pass_fps.push(n as f64 / t0.elapsed().as_secs_f64());
    }
    if cancel.load(Relaxed) { return Err("Cancelado".into()); }
    let mut s = pass_fps.clone(); s.sort_by(|a, b| a.total_cmp(b));
    Ok(RtResult { adapter: rig.name.clone(), width: w, height: h, preset: preset.into(), pass_fps, median_fps: s[1], score: s[1] * 10.0 })
}

#[tauri::command]
pub fn rt_start(state: tauri::State<RtState>, res: String, preset: String) -> Result<(), String> {
    let (w, h) = if res == "1920x1080" { (1920, 1080) } else { (1280, 720) };
    if state.st.lock().unwrap_or_else(|e| e.into_inner()).running { return Err("Já está rodando".into()); }
    state.cancel.store(false, Relaxed);
    *state.st.lock().unwrap_or_else(|e| e.into_inner()) = RtStatus { running: true, ..Default::default() };
    let (st, cancel) = (state.st.clone(), state.cancel.clone());
    thread::spawn(move || {
        let r = run(w, h, &preset, &st, &cancel);
        let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
        g.running = false;
        match r { Ok(v) => { g.result = Some(v); g.progress = 1.0; g.phase = "Concluído".into(); } Err(e) => g.error = e }
    });
    Ok(())
}

#[tauri::command]
pub fn rt_cancel(state: tauri::State<RtState>) { state.cancel.store(true, Relaxed); }

#[tauri::command]
pub fn rt_status(state: tauri::State<RtState>) -> RtStatus { state.st.lock().unwrap_or_else(|e| e.into_inner()).clone() }

#[cfg(test)]
mod tests {
    use super::*;
    // Renderiza alguns quadros 960x540 e salva RGBA cru em $CF_RT_OUT (se definido) para conferência visual.
    #[test]
    fn renders_scene() {
        let mut rig = match Rig::new(960, 540) { Ok(r) => r, Err(e) => { println!("sem RT: {e}"); return; } };
        for i in 0..3 { rig.frame(2.0 + i as f32 * 0.1, i, 4, 3); }
        let px = rig.read();
        let mean = px.chunks(4).map(|p| p[0] as f64 + p[1] as f64 + p[2] as f64).sum::<f64>() / (px.len() as f64 / 4.0) / 3.0;
        let distinct = px.chunks(4).step_by(499).map(|p| (p[0], p[1], p[2])).collect::<std::collections::HashSet<_>>().len();
        println!("{} média {mean:.1} cores {distinct}", rig.name);
        assert!(mean > 20.0 && mean < 235.0 && distinct > 50);
        if let Ok(p) = std::env::var("CF_RT_OUT") { std::fs::write(p, &px).unwrap(); }
    }
}

#[cfg(test)]
mod full {
    use super::*;
    // Manual: cargo test full_rt -- --ignored --nocapture
    #[test]
    #[ignore]
    fn full_rt() {
        let st = Mutex::new(RtStatus::default());
        let r = run(1280, 720, "Alta", &st, &AtomicBool::new(false)).unwrap();
        println!("{} {}x{} {}: passadas {:?} · mediana {:.1} FPS · nota {:.0}", r.adapter, r.width, r.height, r.preset, r.pass_fps, r.median_fps, r.score);
    }
}
