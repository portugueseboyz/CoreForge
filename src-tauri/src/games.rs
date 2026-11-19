//! Jogos: biblioteca da Steam (libraryfolders.vdf + appmanifest_*.acf), lançamento via steam:// e estimativa local de FPS.
//! Valores do catálogo são APROXIMADOS (compilados de benchmarks públicos), os mesmos do app Windows.
use crate::sysinfo::read;
use serde::Serialize;
use std::{collections::HashMap, fs, path::PathBuf, process::Command};

// ------------------------------------------------------------------ VDF (formato de texto da Valve)

#[derive(Debug, Default, Clone)]
pub struct Vdf { val: Option<String>, kids: HashMap<String, Vdf> }
impl Vdf {
    fn get(&self, k: &str) -> Option<&Vdf> { self.kids.get(k) }
    fn str(&self, k: &str) -> Option<&str> { self.kids.get(k)?.val.as_deref() }
}

pub fn parse_vdf(text: &str) -> Vdf {
    let mut toks: Vec<String> = vec![];
    let (mut it, mut buf, mut quoted) = (text.chars().peekable(), String::new(), false);
    while let Some(c) = it.next() {
        match c {
            '"' if quoted => { toks.push(std::mem::take(&mut buf)); quoted = false; }
            '"' => quoted = true,
            '\\' if quoted => { if let Some(n) = it.next() { buf.push(match n { 'n' => '\n', 't' => '\t', o => o }); } }
            '{' | '}' if !quoted => toks.push(c.to_string().replace('{', "\u{1}").replace('}', "\u{2}")),
            _ if quoted => buf.push(c),
            _ => {}
        }
    }
    fn block(t: &[String], i: &mut usize) -> Vdf {
        let mut v = Vdf::default();
        while *i < t.len() {
            if t[*i] == "\u{2}" { *i += 1; break; }
            let key = t[*i].clone(); *i += 1;
            if *i < t.len() && t[*i] == "\u{1}" { *i += 1; v.kids.insert(key, block(t, i)); }
            else if *i < t.len() { v.kids.insert(key, Vdf { val: Some(t[*i].clone()), ..Default::default() }); *i += 1; }
        }
        v
    }
    let mut i = 0;
    block(&toks, &mut i)
}

// ------------------------------------------------------------------ biblioteca

#[derive(Serialize, Clone)]
pub struct Game { store: &'static str, id: String, name: String, install_dir: String, size_gb: f64 }

fn steam_root() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    [".local/share/Steam", ".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"].iter()
        .map(|p| home.join(p)).find(|p| p.join("steamapps").exists())
}

pub fn scan_steam() -> Vec<Game> {
    let Some(root) = steam_root() else { return vec![] };
    let mut libs = vec![root.clone()];
    if let Some(txt) = read(&format!("{}/steamapps/libraryfolders.vdf", root.display())) {
        let v = parse_vdf(&txt);
        if let Some(f) = v.get("libraryfolders") {
            for e in f.kids.values() { if let Some(p) = e.str("path") { libs.push(PathBuf::from(p)); } }
        }
    }
    libs.sort(); libs.dedup();
    let mut out = vec![];
    for lib in libs {
        let apps = lib.join("steamapps");
        let Ok(rd) = fs::read_dir(&apps) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) { continue; }
            let Some(txt) = read(&e.path().to_string_lossy()) else { continue };
            let v = parse_vdf(&txt);
            let Some(st) = v.get("AppState") else { continue };
            let (Some(id), Some(name)) = (st.str("appid"), st.str("name")) else { continue };
            // Ferramentas da Steam não são jogos; StateFlags & 4 = totalmente instalado.
            if ["Proton", "Steam Linux Runtime", "Steamworks Common", "Steam Play"].iter().any(|k| name.contains(k)) { continue; }
            if st.str("StateFlags").and_then(|s| s.parse::<u32>().ok()).is_some_and(|f| f & 4 == 0) { continue; }
            out.push(Game {
                store: "Steam", id: id.into(), name: name.into(),
                install_dir: apps.join("common").join(st.str("installdir").unwrap_or("")).to_string_lossy().into_owned(),
                size_gb: st.str("SizeOnDisk").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) / 1e9,
            });
        }
    }
    out.sort_by_key(|g| g.name.to_lowercase());
    out
}

/// Lutris: `lutris -l -j` lista os jogos instalados em JSON (id, name, runner, directory). Sem Lutris, lista vazia.
fn parse_lutris(json: &str) -> Vec<Game> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    list.iter().filter_map(|g| {
        let (id, name) = (g.get("id")?.as_u64()?, g.get("name")?.as_str()?);
        let dir = g.get("directory").and_then(|d| d.as_str()).unwrap_or("");
        Some(Game { store: "Lutris", id: id.to_string(), name: name.into(), install_dir: dir.into(), size_gb: 0.0 })
    }).collect()
}

fn scan_lutris() -> Vec<Game> {
    Command::new("lutris").args(["-l", "-j"]).output().ok().filter(|o| o.status.success())
        .map(|o| parse_lutris(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// Heroic (Epic via legendary, GOG). Formatos lidos de forma defensiva; entradas inválidas são ignoradas.
fn json_size_gb(v: Option<&serde_json::Value>) -> f64 {
    v.and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0.0) / 1e9
}

fn parse_heroic_epic(json: &str) -> Vec<Game> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    map.iter().filter(|(_, g)| g.get("is_dlc").and_then(|d| d.as_bool()) != Some(true)).map(|(id, g)| Game {
        store: "Epic", id: id.clone(),
        name: g.get("title").and_then(|t| t.as_str()).unwrap_or(id).into(),
        install_dir: g.get("install_path").and_then(|t| t.as_str()).unwrap_or("").into(),
        size_gb: json_size_gb(g.get("install_size")),
    }).collect()
}

/// GOG: installed.json = {"installed":[{appName,install_path,install_size}]}; títulos vêm de library.json = {"games":[{app_name,title}]}.
fn parse_heroic_gog(installed: &str, library: &str) -> Vec<Game> {
    let Ok(inst) = serde_json::from_str::<serde_json::Value>(installed) else { return vec![] };
    let lib: serde_json::Value = serde_json::from_str(library).unwrap_or_default();
    let title = |id: &str| lib.get("games").and_then(|g| g.as_array()).and_then(|a| a.iter()
        .find(|g| g.get("app_name").and_then(|n| n.as_str()) == Some(id))).and_then(|g| g.get("title")?.as_str().map(String::from));
    inst.get("installed").and_then(|i| i.as_array()).map(|a| a.iter().filter_map(|g| {
        let id = g.get("appName")?.as_str()?;
        Some(Game { store: "GOG", id: id.into(), name: title(id).unwrap_or_else(|| id.into()),
            install_dir: g.get("install_path").and_then(|t| t.as_str()).unwrap_or("").into(),
            size_gb: json_size_gb(g.get("install_size")) })
    }).collect()).unwrap_or_default()
}

fn scan_heroic() -> Vec<Game> {
    let Ok(home) = std::env::var("HOME") else { return vec![] };
    let mut out = vec![];
    for rel in [".config/heroic", ".var/app/com.heroicgameslauncher.hgl/config/heroic"] {
        let base = PathBuf::from(&home).join(rel);
        let f = |p: &str| read(&base.join(p).to_string_lossy()).unwrap_or_default();
        out.extend(parse_heroic_epic(&f("legendaryConfig/legendary/installed.json")));
        out.extend(parse_heroic_gog(&f("gog_store/installed.json"), &f("gog_store/library.json")));
    }
    out.sort_by(|a, b| (a.store, &a.id).cmp(&(b.store, &b.id)));
    out.dedup_by(|a, b| a.store == b.store && a.id == b.id);
    out
}

#[tauri::command(async)]
pub fn games_scan() -> Vec<Game> {
    let mut v = scan_steam();
    v.extend(scan_lutris());
    v.extend(scan_heroic());
    v.sort_by_key(|g| g.name.to_lowercase());
    v
}

/// Conjunto fechado de lojas; ids validados por loja. Heroic: heroic://launch/<runner>/<appName> (formato de path indicado pelos mantenedores; não testado com Heroic real).
fn launch_uri(store: &str, id: &str) -> Result<String, String> {
    let num = !id.is_empty() && id.chars().all(|c| c.is_ascii_digit());
    let slug = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    match store {
        "Steam" if num => Ok(format!("steam://rungameid/{id}")),
        "Lutris" if num => Ok(format!("lutris:rungameid/{id}")),
        "Epic" if slug => Ok(format!("heroic://launch/legendary/{id}")),
        "GOG" if slug => Ok(format!("heroic://launch/gog/{id}")),
        _ => Err("ID inválido".into()),
    }
}

#[tauri::command(async)]
pub fn game_launch(store: String, id: String) -> Result<(), String> {
    let uri = launch_uri(&store, &id)?;
    Command::new("xdg-open").arg(uri).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ estimativa de FPS

struct Info(&'static str, &'static str, f64, f64, f64); // nome, gênero, GpuRef, CpuCap, EngineCap
const GAMES: &[Info] = &[
    Info("Counter-Strike 2", "FPS competitivo", 340.0, 420.0, 0.0), Info("Valorant", "FPS competitivo", 600.0, 650.0, 0.0),
    Info("Fortnite", "Battle royale", 160.0, 250.0, 0.0), Info("League of Legends", "MOBA", 420.0, 480.0, 0.0),
    Info("Dota 2", "MOBA", 260.0, 280.0, 0.0), Info("Apex Legends", "Battle royale", 220.0, 300.0, 0.0),
    Info("PUBG: Battlegrounds", "Battle royale", 180.0, 220.0, 0.0), Info("Call of Duty: Black Ops 6", "FPS", 140.0, 220.0, 0.0),
    Info("Call of Duty: Warzone", "Battle royale", 130.0, 200.0, 0.0), Info("Battlefield 6", "FPS", 120.0, 170.0, 0.0),
    Info("Rainbow Six Siege", "FPS tático", 350.0, 430.0, 0.0), Info("Overwatch 2", "FPS heroico", 280.0, 380.0, 0.0),
    Info("Marvel Rivals", "FPS heroico", 110.0, 160.0, 0.0), Info("Rocket League", "Esporte", 400.0, 450.0, 0.0),
    Info("EA Sports FC 25", "Esporte", 220.0, 260.0, 0.0), Info("Minecraft", "Sandbox", 300.0, 340.0, 0.0),
    Info("GTA V", "Mundo aberto", 160.0, 190.0, 0.0), Info("Red Dead Redemption 2", "Mundo aberto", 95.0, 160.0, 0.0),
    Info("Cyberpunk 2077", "RPG mundo aberto", 95.0, 150.0, 0.0), Info("The Witcher 3: Wild Hunt", "RPG", 120.0, 160.0, 0.0),
    Info("Elden Ring", "RPG de ação", 110.0, 140.0, 60.0), Info("Baldur's Gate 3", "RPG", 110.0, 120.0, 0.0),
    Info("Hogwarts Legacy", "RPG de ação", 90.0, 120.0, 0.0), Info("Starfield", "RPG espacial", 65.0, 90.0, 0.0),
    Info("Black Myth: Wukong", "Ação", 70.0, 140.0, 0.0), Info("Alan Wake 2", "Terror / ação", 60.0, 140.0, 0.0),
    Info("God of War Ragnarök", "Ação", 110.0, 170.0, 0.0), Info("Forza Horizon 5", "Corrida", 140.0, 200.0, 0.0),
    Info("Helldivers 2", "Cooperativo", 90.0, 110.0, 0.0), Info("Monster Hunter Wilds", "Ação", 60.0, 85.0, 0.0),
    Info("Assassin's Creed Shadows", "Ação mundo aberto", 65.0, 110.0, 0.0), Info("Borderlands 4", "Looter shooter", 55.0, 110.0, 0.0),
];

// Desempenho relativo (raster), RTX 4070 = 100. Aproximado.
const GPU_INDEX: &[(&str, f64)] = &[
    ("RTX 5090", 265.0), ("RTX 5080", 175.0), ("RTX 5070 Ti", 150.0), ("RTX 5070", 118.0), ("RTX 5060 Ti", 95.0), ("RTX 5060", 85.0),
    ("RTX 4090", 210.0), ("RTX 4080 SUPER", 160.0), ("RTX 4080", 157.0), ("RTX 4070 Ti SUPER", 136.0), ("RTX 4070 Ti", 124.0),
    ("RTX 4070 SUPER", 116.0), ("RTX 4070", 100.0), ("RTX 4060 Ti", 82.0), ("RTX 4060", 70.0),
    ("RTX 3090 Ti", 135.0), ("RTX 3090", 125.0), ("RTX 3080 Ti", 122.0), ("RTX 3080", 112.0), ("RTX 3070 Ti", 92.0), ("RTX 3070", 86.0),
    ("RTX 3060 Ti", 78.0), ("RTX 3060", 60.0), ("RTX 3050", 40.0), ("GTX 1660", 38.0), ("GTX 1650", 28.0),
    ("RX 9070 XT", 160.0), ("RX 9070", 140.0), ("RX 7900 XTX", 170.0), ("RX 7900 XT", 150.0), ("RX 7900 GRE", 128.0),
    ("RX 7800 XT", 115.0), ("RX 7700 XT", 98.0), ("RX 7600 XT", 72.0), ("RX 7600", 70.0),
    ("RX 6950 XT", 125.0), ("RX 6800 XT", 112.0), ("RX 6700 XT", 82.0), ("RX 6600", 58.0),
    ("Arc B580", 80.0), ("Arc A770", 68.0), ("Radeon Graphics", 6.0),
];
// Desempenho relativo (limitado pela CPU), Ryzen 7 7700X = 100. Aproximado.
const CPU_INDEX: &[(&str, f64)] = &[
    ("9950X3D", 138.0), ("9800X3D", 140.0), ("7950X3D", 126.0), ("7800X3D", 128.0), ("5800X3D", 95.0), ("5700X3D", 90.0),
    ("9950X", 112.0), ("9900X", 110.0), ("9700X", 108.0), ("9600X", 104.0), ("7950X", 104.0), ("7900X", 102.0), ("7700X", 100.0),
    ("7700", 97.0), ("7600X", 96.0), ("7600", 93.0), ("5950X", 82.0), ("5900X", 81.0), ("5800X", 80.0), ("5700X", 77.0), ("5600X", 76.0), ("5600", 73.0),
    ("5500", 68.0), // não consta no app Windows; valor aproximado (Zen 3, 6 núcleos, sem 3D V-Cache)
    ("285K", 112.0), ("265K", 108.0), ("245K", 100.0), ("14900K", 116.0), ("14700K", 112.0), ("14600K", 104.0), ("13900K", 113.0),
    ("13700K", 108.0), ("13600K", 101.0), ("13400", 84.0), ("12900K", 98.0), ("12700K", 95.0), ("12600K", 90.0), ("12400", 78.0),
];

/// A chave mais longa contida no nome vence ("RTX 5070 Ti" antes de "RTX 5070").
fn lookup(table: &[(&str, f64)], name: &str) -> Option<f64> {
    let n = name.to_lowercase();
    table.iter().filter(|(k, _)| n.contains(&k.to_lowercase())).max_by_key(|(k, _)| k.len()).map(|(_, v)| *v)
}

fn res_factor(r: &str) -> f64 { match r { "2560x1440" => 0.72, "3840x2160" => 0.42, _ => 1.0 } }
fn preset_factor(p: &str) -> f64 { match p { "Baixo" => 1.6, "Médio" => 1.3, "Ultra" => 0.82, _ => 1.0 } }
fn upscale_factor(u: &str, r: &str) -> f64 {
    match (u, r == "1920x1080") { ("Quality", true) => 1.25, ("Quality", false) => 1.4, ("Performance", true) => 1.45, ("Performance", false) => 1.75, _ => 1.0 }
}

#[derive(Serialize)]
pub struct FpsRow { name: &'static str, genre: &'static str, avg: f64, low1: f64, limiter: &'static str }

#[derive(Serialize)]
pub struct FpsTable { gpu: String, cpu: String, gpu_known: bool, cpu_known: bool, rows: Vec<FpsRow> }

fn estimate(g: &Info, gpu: f64, cpu: f64, res: &str, preset: &str, up: &str) -> (f64, &'static str) {
    let gpu_fps = g.2 * gpu / 100.0 * res_factor(res) * preset_factor(preset) * upscale_factor(up, res);
    let cpu_fps = g.3 * cpu / 100.0 * match preset { "Baixo" => 1.1, "Ultra" => 0.95, _ => 1.0 };
    let (mut fps, mut lim) = (gpu_fps.min(cpu_fps), if gpu_fps <= cpu_fps { "GPU" } else { "CPU" });
    if g.4 > 0.0 && g.4 < fps { fps = g.4; lim = "Trava do jogo"; }
    (fps.round(), lim)
}

pub fn gpu_name() -> String {
    let out = Command::new("lspci").arg("-mm").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    out.lines().filter(|l| l.contains("VGA") || l.contains("3D")).filter_map(|l| {
        let q: Vec<&str> = l.split('"').collect(); // [slot, cls, vendor, dev, ...]
        q.get(5).map(|d| d.to_string())
    }).next().map(|d| d.split('[').nth(1).map(|s| s.trim_end_matches(']').to_string()).unwrap_or(d)).unwrap_or_default()
}

pub fn cpu_name() -> String {
    read("/proc/cpuinfo").and_then(|t| t.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|s| s.trim().to_string())).unwrap_or_default()
}

#[tauri::command(async)]
pub fn fps_table(res: String, preset: String, upscaling: String) -> FpsTable {
    let (gpu, cpu) = (gpu_name(), cpu_name());
    let (gi, ci) = (lookup(GPU_INDEX, &gpu), lookup(CPU_INDEX, &cpu));
    let rows = GAMES.iter().map(|g| {
        let (avg, limiter) = estimate(g, gi.unwrap_or(50.0), ci.unwrap_or(80.0), &res, &preset, &upscaling);
        FpsRow { name: g.0, genre: g.1, avg, low1: (avg * 0.75).round(), limiter }
    }).collect();
    FpsTable { gpu, cpu, gpu_known: gi.is_some(), cpu_known: ci.is_some(), rows }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lutris_json() {
        let g = parse_lutris(r#"[{"id":7,"name":"Celeste","runner":"linux","directory":"/g/celeste"},{"name":"sem id"}]"#);
        assert_eq!(g.len(), 1);
        assert_eq!((g[0].id.as_str(), g[0].store), ("7", "Lutris"));
        assert!(parse_lutris("lixo").is_empty());
    }

    #[test]
    fn heroic_json() {
        let e = parse_heroic_epic(r#"{"Sugar":{"title":"Celeste","install_path":"/e/c","install_size":2000000000},"Dlc1":{"is_dlc":true},"Bare":{}}"#);
        assert_eq!(e.len(), 2);
        let c = e.iter().find(|g| g.id == "Sugar").unwrap();
        assert_eq!((c.name.as_str(), c.store, c.size_gb), ("Celeste", "Epic", 2.0));
        let g = parse_heroic_gog(r#"{"installed":[{"appName":"123","install_path":"/g/x","install_size":"1000000000"},{"nope":1}]}"#,
            r#"{"games":[{"app_name":"123","title":"Hades"}]}"#);
        assert_eq!((g.len(), g[0].name.as_str(), g[0].size_gb), (1, "Hades", 1.0));
        assert!(parse_heroic_epic("lixo").is_empty() && parse_heroic_gog("[]", "x").is_empty());
    }

    #[test]
    fn launch_validation() {
        assert_eq!(launch_uri("Steam", "730").unwrap(), "steam://rungameid/730");
        assert_eq!(launch_uri("Epic", "Sugar_1-a").unwrap(), "heroic://launch/legendary/Sugar_1-a");
        assert_eq!(launch_uri("GOG", "123").unwrap(), "heroic://launch/gog/123");
        for (s, i) in [("Epic", "a/../b"), ("Epic", "a?b=1"), ("Steam", "12a"), ("Outra", "1"), ("GOG", "")] { assert!(launch_uri(s, i).is_err()); }
    }

    #[test]
    fn vdf_and_estimate() {
        let v = parse_vdf("\"AppState\"\n{\n\t\"appid\"\t\"730\"\n\t\"name\"\t\"Counter-Strike 2\"\n\t\"StateFlags\"\t\"4\"\n\t\"UserConfig\" { \"language\" \"english\" }\n}\n");
        let a = v.get("AppState").unwrap();
        assert_eq!((a.str("appid"), a.str("name")), (Some("730"), Some("Counter-Strike 2")));
        assert_eq!(a.get("UserConfig").unwrap().str("language"), Some("english"));
        assert_eq!(lookup(GPU_INDEX, "NVIDIA GeForce RTX 5070 Ti"), Some(150.0)); // chave mais longa vence
        let (fps, lim) = estimate(&GAMES[0], 100.0, 100.0, "1920x1080", "Alto", "Desligado");
        assert_eq!((fps, lim), (340.0, "GPU"));
        let (fps, lim) = estimate(&GAMES[20], 100.0, 100.0, "1920x1080", "Alto", "Desligado");
        assert_eq!((fps, lim), (60.0, "Trava do jogo"));
    }
    #[test]
    fn real_machine() {
        println!("{} | {} | {} jogos Steam", gpu_name(), cpu_name(), scan_steam().len());
        assert!(!cpu_name().is_empty());
    }
}
