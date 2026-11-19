// Bench RT: path tracing por ray queries (VK_KHR_ray_query) sobre ~4 mil esferas (fosco, metal, espelho, luz) e um chão.
// Sombra suave do sol, reflexos e iluminação indireta pelos rebatimentos. A TLAS é refeita a cada quadro (objetos animados).
enable wgpu_ray_query;

struct U { pos: vec4<f32>, fwd: vec4<f32>, right: vec4<f32>, up: vec4<f32>, p: vec4<u32> /* spp, bounces, frame, 0 */, t: vec4<f32> }
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var acc: acceleration_structure;
@group(0) @binding(2) var out_img: texture_storage_2d<rgba8unorm, write>;

fn pcg(v: u32) -> u32 { let s = v * 747796405u + 2891336453u; let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u; return (w >> 22u) ^ w; }
var<private> rng: u32;
fn rnd() -> f32 { rng = pcg(rng); return f32(rng) * (1.0 / 4294967296.0); }

fn hash3(i: u32) -> vec3<f32> { return vec3<f32>(f32(pcg(i * 3u + 1u)), f32(pcg(i * 3u + 2u)), f32(pcg(i * 3u + 3u))) * (1.0 / 4294967296.0); }

fn basis_cos(n: vec3<f32>) -> vec3<f32> {
    let r1 = rnd(); let r2 = rnd();
    let r = sqrt(r1); let phi = 6.2831853 * r2;
    let a = normalize(cross(select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9), n));
    let b = cross(n, a);
    return normalize(a * (r * cos(phi)) + b * (r * sin(phi)) + n * sqrt(1.0 - r1));
}
fn sky(d: vec3<f32>) -> vec3<f32> { return mix(vec3<f32>(0.75, 0.82, 0.92), vec3<f32>(0.22, 0.42, 0.85), clamp(d.y * 1.4, 0.0, 1.0)) * 0.9; }

const SUN = vec3<f32>(0.45, 0.75, 0.30);

fn occluded(o: vec3<f32>, d: vec3<f32>) -> bool {
    var rq: ray_query;
    rayQueryInitialize(&rq, acc, RayDesc(RAY_FLAG_TERMINATE_ON_FIRST_HIT, 0xFFu, 0.002, 500.0, o, d));
    while (rayQueryProceed(&rq)) {}
    return rayQueryGetCommittedIntersection(&rq).kind != RAY_QUERY_INTERSECTION_NONE;
}

fn trace(o0: vec3<f32>, d0: vec3<f32>) -> vec3<f32> {
    var o = o0; var d = d0;
    var col = vec3<f32>(0.0); var thr = vec3<f32>(1.0);
    for (var b = 0u; b < u.p.y; b = b + 1u) {
        var rq: ray_query;
        rayQueryInitialize(&rq, acc, RayDesc(RAY_FLAG_NONE, 0xFFu, 0.002, 500.0, o, d));
        while (rayQueryProceed(&rq)) {}
        let h = rayQueryGetCommittedIntersection(&rq);
        if (h.kind == RAY_QUERY_INTERSECTION_NONE) { col += thr * sky(d); break; }
        let p = o + d * h.t;
        var n = vec3<f32>(0.0, 1.0, 0.0);
        var mat = 0u; var albedo = vec3<f32>(0.55);
        if (h.instance_custom_data != 255u) {
            let m = h.object_to_world;
            n = normalize(p - vec3<f32>(m[3][0], m[3][1], m[3][2]));
            let id = h.instance_custom_data;
            let r = hash3(id);
            mat = select(select(select(0u, 1u, r.x > 0.55), 2u, r.x > 0.8), 3u, r.x > 0.95); // 55% fosco, 25% metal, 15% espelho, 5% luz
            albedo = 0.25 + 0.7 * hash3(id + 7919u);
            if (mat == 3u) { col += thr * (albedo * 0.5 + 0.5) * 7.0; break; }
        } else {
            let chk = (i32(floor(p.x * 0.5)) + i32(floor(p.z * 0.5))) & 1;
            albedo = select(vec3<f32>(0.62), vec3<f32>(0.3), chk == 0);
        }
        if (dot(n, d) > 0.0) { n = -n; }
        let q = p + n * 0.003;
        if (mat == 0u) {
            // luz direta do sol com sombra suave
            let l = normalize(SUN + (vec3<f32>(rnd(), rnd(), rnd()) - 0.5) * 0.12);
            let ndl = dot(n, l);
            if (ndl > 0.0 && !occluded(q, l)) { col += thr * albedo * ndl * vec3<f32>(1.3, 1.15, 0.95); }
            thr *= albedo;
            d = basis_cos(n);
        } else if (mat == 1u) {
            d = normalize(reflect(d, n) + 0.15 * (vec3<f32>(rnd(), rnd(), rnd()) - 0.5));
            thr *= albedo;
        } else {
            d = reflect(d, n);
            thr *= 0.92;
        }
        o = q;
    }
    return col;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dim = textureDimensions(out_img);
    if (gid.x >= dim.x || gid.y >= dim.y) { return; }
    rng = pcg(gid.x + dim.x * gid.y + 977u * u.p.z);
    var acc_c = vec3<f32>(0.0);
    for (var s = 0u; s < u.p.x; s = s + 1u) {
        let uv = (vec2<f32>(gid.xy) + vec2<f32>(rnd(), rnd())) / vec2<f32>(dim) * 2.0 - 1.0;
        let aspect = f32(dim.x) / f32(dim.y);
        let d = normalize(u.fwd.xyz + u.right.xyz * (uv.x * aspect) + u.up.xyz * (-uv.y));
        acc_c += min(trace(u.pos.xyz, d), vec3<f32>(8.0));
    }
    var c = acc_c / f32(u.p.x);
    c = c * (2.51 * c + 0.03) / (c * (2.43 * c + 0.59) + 0.14);
    textureStore(out_img, vec2<i32>(gid.xy), vec4<f32>(pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2)), 1.0));
}
