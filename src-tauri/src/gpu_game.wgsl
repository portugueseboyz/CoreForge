// Bench GPU · cena de jogo: terreno fractal, 40 mil árvores por instancing, sombras 4096² com PCF, água, neblina e pós HDR.
struct U { vp: mat4x4<f32>, lvp: mat4x4<f32>, cam: vec4<f32>, sun: vec4<f32>, t: vec4<f32> }
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var shadow_tex: texture_depth_2d;
@group(0) @binding(2) var shadow_smp: sampler_comparison;

const N: u32 = 384u;        // quadrados por lado do terreno
const S: f32 = 900.0;       // tamanho do mundo
const SKY_H = vec3<f32>(0.66, 0.77, 0.92);

fn h2(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(0.1031, 0.1030));
    q += dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p); let f = fract(p); let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(h2(i), h2(i + vec2<f32>(1.0, 0.0)), w.x), mix(h2(i + vec2<f32>(0.0, 1.0)), h2(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}
fn fbm(p: vec2<f32>) -> f32 {
    var a = 0.5; var s = 0.0; var q = p;
    for (var i = 0; i < 6; i = i + 1) { s += a * vnoise(q); q = q * 2.03 + vec2<f32>(17.1, 9.2); a *= 0.5; }
    return s;
}
fn height(x: f32, z: f32) -> f32 { return (fbm(vec2<f32>(x, z) * 0.006) - 0.35) * 90.0; }
fn sky_color(d: vec3<f32>) -> vec3<f32> { return mix(SKY_H, vec3<f32>(0.25, 0.45, 0.85), clamp(d.y * 1.5, 0.0, 1.0)); }

struct VO { @builtin(position) pos: vec4<f32>, @location(0) wpos: vec3<f32>, @location(1) nrm: vec3<f32>, @location(2) col: vec3<f32> }

struct TW { p: vec3<f32>, n: vec3<f32>, ok: bool }
fn terrain_world(vi: u32) -> TW {
    let q = vi / 6u; let k = vi % 6u;
    var cx = array<f32, 6>(0.0, 1.0, 0.0, 0.0, 1.0, 1.0);
    var cz = array<f32, 6>(0.0, 0.0, 1.0, 1.0, 0.0, 1.0);
    let x = (f32(q % N) + cx[k]) / f32(N) * S - S * 0.5;
    let z = (f32(q / N) + cz[k]) / f32(N) * S - S * 0.5;
    let e = 2.0;
    let n = normalize(vec3<f32>(height(x - e, z) - height(x + e, z), 2.0 * e, height(x, z - e) - height(x, z + e)));
    return TW(vec3<f32>(x, height(x, z), z), n, true);
}

struct TreeV { p: vec3<f32>, n: vec3<f32>, c: vec3<f32> }
fn tree_vertex(vi: u32) -> TreeV {
    var corner = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    if (vi < 48u) { // tronco: prisma de 8 lados
        let seg = vi / 6u; let c = corner[vi % 6u];
        let a = f32(seg + (c & 1u)) / 8.0 * 6.2831853;
        return TreeV(vec3<f32>(cos(a) * 0.25, f32(c >> 1u) * 2.2, sin(a) * 0.25), vec3<f32>(cos(a), 0.0, sin(a)), vec3<f32>(0.32, 0.2, 0.1));
    }
    let j = vi - 48u; let cone = j / 36u; let seg = (j % 36u) / 3u; let k = j % 3u;
    let r = 2.3 - f32(cone) * 0.9; let by = 2.0 + f32(cone) * 3.0; let ay = 7.0 + f32(cone) * 2.5;
    let a0 = f32(seg) / 12.0 * 6.2831853; let a1 = f32(seg + 1u) / 12.0 * 6.2831853; let am = (a0 + a1) * 0.5;
    var p = vec3<f32>(0.0, ay, 0.0);
    if (k == 0u) { p = vec3<f32>(cos(a0) * r, by, sin(a0) * r); }
    if (k == 1u) { p = vec3<f32>(cos(a1) * r, by, sin(a1) * r); }
    return TreeV(p, normalize(vec3<f32>(cos(am), 0.55, sin(am))), vec3<f32>(0.1, 0.32 + 0.1 * f32(cone), 0.12));
}
struct TW2 { p: vec3<f32>, n: vec3<f32>, c: vec3<f32>, ok: bool }
fn tree_world(vi: u32, ii: u32) -> TW2 {
    let r1 = h2(vec2<f32>(f32(ii), 1.7)); let r2 = h2(vec2<f32>(f32(ii), 9.1)); let r3 = h2(vec2<f32>(f32(ii), 4.3));
    let x = (r1 - 0.5) * S * 0.95; let z = (r2 - 0.5) * S * 0.95;
    let y = height(x, z);
    let tv = tree_vertex(vi);
    let sc = 0.8 + r3 * 0.9;
    return TW2(vec3<f32>(x, y, z) + tv.p * sc, tv.n, tv.c, y > 2.5 && y < 36.0);
}

@vertex fn vs_terrain(@builtin(vertex_index) vi: u32) -> VO {
    let w = terrain_world(vi);
    var o: VO;
    o.pos = u.vp * vec4<f32>(w.p, 1.0); o.wpos = w.p; o.nrm = w.n; o.col = vec3<f32>(w.p.y, 0.0, 0.0);
    return o;
}
@vertex fn vs_tree(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
    let w = tree_world(vi, ii);
    var o: VO;
    o.pos = select(vec4<f32>(0.0, 0.0, 2.0, 1.0), u.vp * vec4<f32>(w.p, 1.0), w.ok);
    o.wpos = w.p; o.nrm = w.n; o.col = w.c;
    return o;
}
@vertex fn vs_water(@builtin(vertex_index) vi: u32) -> VO {
    var cx = array<f32, 6>(-1.0, 1.0, -1.0, -1.0, 1.0, 1.0);
    var cz = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let p = vec3<f32>(cx[vi] * S * 0.7, 0.0, cz[vi] * S * 0.7);
    var o: VO;
    o.pos = u.vp * vec4<f32>(p, 1.0); o.wpos = p; o.nrm = vec3<f32>(0.0, 1.0, 0.0); o.col = vec3<f32>(0.0);
    return o;
}
@vertex fn vs_shadow_terrain(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return u.lvp * vec4<f32>(terrain_world(vi).p, 1.0);
}
@vertex fn vs_shadow_tree(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> @builtin(position) vec4<f32> {
    let w = tree_world(vi, ii);
    return select(vec4<f32>(0.0, 0.0, 2.0, 1.0), u.lvp * vec4<f32>(w.p, 1.0), w.ok);
}

fn shadow_pcf(wp: vec3<f32>) -> f32 {
    let ls = u.lvp * vec4<f32>(wp, 1.0);
    let uv = vec2<f32>(ls.x * 0.5 + 0.5, 0.5 - ls.y * 0.5);
    let texel = 1.0 / 4096.0;
    var s = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            s += textureSampleCompareLevel(shadow_tex, shadow_smp, uv + vec2<f32>(f32(x), f32(y)) * texel, ls.z - 0.0008);
        }
    }
    return s / 9.0;
}
fn fog(c: vec3<f32>, wp: vec3<f32>) -> vec3<f32> {
    let d = distance(u.cam.xyz, wp);
    return mix(c, SKY_H, 1.0 - exp(-d * 0.0013));
}

@fragment fn fs_scene(i: VO) -> @location(0) vec4<f32> {
    let n = normalize(i.nrm);
    var albedo = i.col;
    if (i.col.y == 0.0 && i.col.z == 0.0) { // terreno: cor por altura e inclinação (guardada em col.x)
        let h = i.col.x;
        var c = vec3<f32>(0.30, 0.45, 0.18);
        if (h < 2.0) { c = vec3<f32>(0.76, 0.70, 0.50); }
        c = mix(c, vec3<f32>(0.5, 0.5, 0.52), smoothstep(30.0, 38.0, h));
        if (h > 46.0) { c = vec3<f32>(0.92, 0.93, 0.96); }
        albedo = mix(c, vec3<f32>(0.42, 0.40, 0.38), smoothstep(0.25, 0.5, 1.0 - n.y));
    }
    let L = normalize(u.sun.xyz);
    let amb = mix(vec3<f32>(0.14, 0.16, 0.21), vec3<f32>(0.34, 0.40, 0.52), n.y * 0.5 + 0.5);
    let lit = albedo * (amb + vec3<f32>(1.25, 1.1, 0.9) * max(dot(n, L), 0.0) * shadow_pcf(i.wpos));
    return vec4<f32>(fog(lit, i.wpos), 1.0);
}
@fragment fn fs_water(i: VO) -> @location(0) vec4<f32> {
    let t = u.t.x; let w = i.wpos;
    let n = normalize(vec3<f32>(vnoise(w.xz * 0.35 + t * 0.4) - 0.5, 1.6, vnoise(w.xz * 0.35 - t * 0.3 + 7.0) - 0.5));
    let v = normalize(u.cam.xyz - w);
    let fres = pow(1.0 - max(dot(n, v), 0.0), 4.0) * 0.8 + 0.1;
    var c = mix(vec3<f32>(0.02, 0.12, 0.2), sky_color(reflect(-v, n)), fres);
    let sp = pow(max(dot(reflect(-normalize(u.sun.xyz), n), v), 0.0), 90.0);
    c += vec3<f32>(1.0, 0.9, 0.7) * sp;
    return vec4<f32>(fog(c, w), 0.78);
}

@group(0) @binding(0) var hdr: texture_2d<f32>;
@vertex fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(p[vi], 0.0, 1.0);
}
@fragment fn fs_post(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
    let dim = vec2<f32>(textureDimensions(hdr));
    var c = textureLoad(hdr, vec2<i32>(fp.xy), 0).rgb;
    let q = fp.xy / dim - 0.5;
    c *= 1.0 - dot(q, q) * 0.9; // vinheta
    let a = c * (2.51 * c + 0.03) / (c * (2.43 * c + 0.59) + 0.14); // ACES
    return vec4<f32>(pow(clamp(a, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2)), 1.0);
}
