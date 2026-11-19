//! Diagnóstico de rede para lag: ping/jitter/perda até o roteador e a internet, DNS, download/upload e latência sob carga.
//! Os testes de velocidade usam speed.cloudflare.com (nada do usuário é enviado além de bytes aleatórios).
use serde::Serialize;
use std::{
    io::Read,
    net::ToSocketAddrs,
    process::Command,
    sync::{atomic::{AtomicBool, AtomicU64, Ordering::Relaxed}, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Serialize, Clone)]
pub struct Row { label: String, value: String, level: &'static str } // level: ok | warn | bad | info

#[derive(Default)]
struct Inner { phase: String, progress: f64, rows: Vec<Row> }

#[derive(Default)]
pub struct NetState { inner: Arc<Mutex<Inner>>, running: Arc<AtomicBool>, cancel: Arc<AtomicBool> }

#[derive(Serialize)]
pub struct NetStatus { running: bool, phase: String, progress: f64, rows: Vec<Row> }

fn sh(exe: &str, args: &[&str]) -> String {
    Command::new(exe).args(args).env("LC_ALL", "C").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn gateway() -> Option<String> {
    let j: serde_json::Value = serde_json::from_str(&sh("ip", &["-j", "route", "show", "default"])).ok()?;
    j.get(0)?.get("gateway")?.as_str().map(String::from)
}

#[derive(Default, Clone, Copy)]
struct Ping { sent: u32, lost_pct: f64, min: f64, avg: f64, max: f64, jitter: f64 }

fn parse_ping(out: &str) -> Option<Ping> {
    let (mut p, mut ok) = (Ping::default(), false);
    for l in out.lines() {
        if l.contains("packets transmitted") {
            let f: Vec<&str> = l.split(',').collect();
            p.sent = f[0].split_whitespace().next()?.parse().ok()?;
            p.lost_pct = f.iter().find(|x| x.contains("packet loss"))?.trim().split('%').next()?.parse().ok()?;
        }
        if let Some(r) = l.split(" = ").nth(1).filter(|_| l.starts_with("rtt") || l.starts_with("round-trip")) {
            let v: Vec<f64> = r.trim_end_matches(" ms").split('/').filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 4 { (p.min, p.avg, p.max, p.jitter) = (v[0], v[1], v[2], v[3]); ok = true; }
        }
    }
    (p.sent > 0).then_some(p).filter(|_| ok || p.lost_pct >= 100.0)
}

fn ping(host: &str, count: u32, interval: f64) -> Option<Ping> {
    parse_ping(&sh("ping", &["-n", "-q", "-c", &count.to_string(), "-i", &interval.to_string(), "-W", "1", host]))
}

fn ping_row(label: &str, host: &str, count: u32) -> Row {
    match ping(host, count, 0.2) {
        Some(p) if p.lost_pct < 100.0 => Row {
            label: label.into(),
            value: format!("média {:.1} ms · mín {:.1} · máx {:.1} · jitter {:.1} ms · perda {:.0}%", p.avg, p.min, p.max, p.jitter, p.lost_pct),
            level: if p.lost_pct > 2.0 || p.jitter > 30.0 { "bad" } else if p.lost_pct > 0.0 || p.jitter > 10.0 || p.avg > 100.0 { "warn" } else { "ok" },
        },
        _ => Row { label: label.into(), value: "não responde".into(), level: "bad" },
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(30))).build().into()
}

fn mbps(bytes: &AtomicU64, t: Instant) -> Option<f64> {
    let (b, s) = (bytes.load(Relaxed), t.elapsed().as_secs_f64());
    (b > 0 && s > 0.0).then(|| b as f64 * 8.0 / 1e6 / s)
}

/// Roda `work` em `n` threads por `secs`, medindo a latência até 1.1.1.1 ao mesmo tempo.
fn under_load(secs: u64, n: usize, cancel: &AtomicBool, work: impl Fn(&AtomicU64, &AtomicBool) + Sync) -> (Option<f64>, Option<Ping>) {
    let (bytes, stop) = (AtomicU64::new(0), AtomicBool::new(false));
    let t = Instant::now();
    
    thread::scope(|sc| {
        for _ in 0..n { sc.spawn(|| work(&bytes, &stop)); }
        let pinger = sc.spawn(|| ping("1.1.1.1", (secs * 5).saturating_sub(5).max(5) as u32, 0.2));
        while t.elapsed() < Duration::from_secs(secs) && !cancel.load(Relaxed) { thread::sleep(Duration::from_millis(100)); }
        let r = mbps(&bytes, t);
        stop.store(true, Relaxed);
        (r, pinger.join().ok().flatten())
    })
}

fn run(inner: &Mutex<Inner>, cancel: &AtomicBool) {
    let step = |phase: &str, p: f64| { let mut g = inner.lock().unwrap_or_else(|e| e.into_inner()); g.phase = phase.into(); g.progress = p; };
    let push = |r: Row| inner.lock().unwrap_or_else(|e| e.into_inner()).rows.push(r);
    macro_rules! stop { () => { if cancel.load(Relaxed) { return; } } }

    step("Roteador", 0.02);
    match gateway() { Some(g) => push(ping_row(&format!("Roteador ({g})"), &g, 20)), None => push(Row { label: "Roteador".into(), value: "gateway não encontrado".into(), level: "warn" }) }
    stop!();
    step("Internet (1.1.1.1)", 0.15);
    push(ping_row("Cloudflare (1.1.1.1)", "1.1.1.1", 20)); stop!();
    step("Internet (8.8.8.8)", 0.25);
    push(ping_row("Google (8.8.8.8)", "8.8.8.8", 20)); stop!();

    step("DNS", 0.35);
    let times: Vec<f64> = ["www.google.com", "www.cloudflare.com", "www.wikipedia.org"].iter().filter_map(|h| {
        let t = Instant::now();
        (format!("{h}:443").to_socket_addrs().ok()?.next()).map(|_| t.elapsed().as_secs_f64() * 1000.0)
    }).collect();
    if times.is_empty() { push(Row { label: "DNS".into(), value: "falhou".into(), level: "bad" }); }
    else {
        let avg = times.iter().sum::<f64>() / times.len() as f64;
        push(Row { label: "Resolução de DNS".into(), value: format!("{avg:.0} ms em média"), level: if avg > 150.0 { "warn" } else { "ok" } });
    }
    stop!();

    let idle = ping("1.1.1.1", 10, 0.2).map(|p| p.avg);
    let bloat = |label: &str, loaded: Option<Ping>| if let (Some(i), Some(l)) = (idle, loaded) {
        let d = l.avg - i;
        push(Row { label: format!("Latência sob {label}"), value: format!("{:.0} ms (+{:.0} ms)", l.avg, d.max(0.0)), level: if d > 100.0 { "bad" } else if d > 30.0 { "warn" } else { "ok" } });
    };

    step("Download (8 s)", 0.5);
    let ag = agent();
    let (down, lp) = under_load(8, 4, cancel, |bytes, stop| {
        let mut buf = vec![0u8; 81920];
        while !stop.load(Relaxed) {
            let Ok(resp) = ag.get("https://speed.cloudflare.com/__down?bytes=25000000").call() else { return };
            let mut rd = resp.into_body().into_reader();
            while !stop.load(Relaxed) { match rd.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => { bytes.fetch_add(n as u64, Relaxed); } } }
        }
    });
    match down { Some(m) => push(Row { label: "Download".into(), value: format!("{m:.0} Mbps"), level: "info" }), None => push(Row { label: "Download".into(), value: "falhou (sem acesso a speed.cloudflare.com?)".into(), level: "bad" }) }
    bloat("download", lp);
    stop!();

    step("Upload (8 s)", 0.75);
    let payload = vec![0x5Au8; 1_000_000];
    let (up, lp) = under_load(8, 3, cancel, |bytes, stop| {
        while !stop.load(Relaxed) {
            if ag.post("https://speed.cloudflare.com/__up").send(&payload[..]).is_err() { return; }
            bytes.fetch_add(payload.len() as u64, Relaxed);
        }
    });
    match up { Some(m) => push(Row { label: "Upload".into(), value: format!("{m:.0} Mbps"), level: "info" }), None => push(Row { label: "Upload".into(), value: "falhou".into(), level: "bad" }) }
    bloat("upload", lp);
    step("Concluído", 1.0);
}

#[tauri::command]
pub fn net_start(st: tauri::State<NetState>) -> Result<(), String> {
    if st.running.swap(true, Relaxed) { return Err("Já está rodando".into()); }
    st.cancel.store(false, Relaxed);
    *st.inner.lock().unwrap_or_else(|e| e.into_inner()) = Inner::default();
    let (inner, running, cancel) = (st.inner.clone(), st.running.clone(), st.cancel.clone());
    thread::spawn(move || { run(&inner, &cancel); running.store(false, Relaxed); });
    Ok(())
}

#[tauri::command]
pub fn net_cancel(st: tauri::State<NetState>) { st.cancel.store(true, Relaxed); }

#[tauri::command]
pub fn net_status(st: tauri::State<NetState>) -> NetStatus {
    let g = st.inner.lock().unwrap_or_else(|e| e.into_inner());
    NetStatus { running: st.running.load(Relaxed), phase: g.phase.clone(), progress: g.progress, rows: g.rows.clone() }
}

#[derive(Serialize)]
pub struct Iface { name: String, kind: &'static str, up: bool, speed: String, mac: String, addrs: Vec<String> }

#[tauri::command(async)]
pub fn net_interfaces() -> Vec<Iface> {
    let addrs: serde_json::Value = serde_json::from_str(&sh("ip", &["-j", "addr"])).unwrap_or_default();
    let mut v: Vec<Iface> = std::fs::read_dir("/sys/class/net").into_iter().flatten().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "lo" || name.starts_with("veth") || name.starts_with("docker") || name.starts_with("br-") { return None; }
        let r = |f: &str| crate::sysinfo::read(&format!("/sys/class/net/{name}/{f}"));
        let kind = if std::path::Path::new(&format!("/sys/class/net/{name}/wireless")).exists() { "Wi-Fi" } else { "Ethernet" };
        let speed = r("speed").and_then(|s| s.parse::<i64>().ok()).filter(|s| *s > 0).map_or("—".into(), |s| format!("{s} Mbps"));
        let ips = addrs.as_array().into_iter().flatten().filter(|a| a["ifname"] == name.as_str())
            .flat_map(|a| a["addr_info"].as_array().cloned().unwrap_or_default())
            .filter(|i| i["family"] == "inet").filter_map(|i| i["local"].as_str().map(String::from)).collect();
        Some(Iface { up: r("operstate").as_deref() == Some("up"), kind, speed, mac: r("address").unwrap_or_default(), addrs: ips, name })
    }).collect();
    v.sort_by(|a, b| b.up.cmp(&a.up).then(a.name.cmp(&b.name)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_ping_summary() {
        let o = "--- 1.1.1.1 ping statistics ---\n5 packets transmitted, 5 received, 0% packet loss, time 800ms\nrtt min/avg/max/mdev = 9.1/10.2/12.0/1.1 ms\n";
        let p = parse_ping(o).unwrap();
        assert_eq!((p.sent, p.avg, p.jitter), (5, 10.2, 1.1));
        let lost = parse_ping("3 packets transmitted, 0 received, 100% packet loss, time 2000ms\n").unwrap();
        assert_eq!(lost.lost_pct, 100.0);
    }
    #[test]
    fn lists_interfaces() { println!("{}", serde_json::to_string(&net_interfaces()).unwrap()); }
}

#[cfg(test)]
mod live {
    use super::*;
    // Manual (usa a internet): cargo test full_run -- --ignored --nocapture
    #[test]
    #[ignore]
    fn full_run() {
        let inner = Mutex::new(Inner::default());
        run(&inner, &AtomicBool::new(false));
        for r in &inner.lock().unwrap_or_else(|e| e.into_inner()).rows { println!("{} | {} | {}", r.label, r.value, r.level); }
    }
}
