//! CPU: detalhes (lscpu, cpufreq, flags) e limites/estrangulamento (throttle, RAPL, boost). Tudo somente leitura.
use serde::Serialize;
use std::{fs, path::Path, process::Command};

fn rd(p: impl AsRef<Path>) -> Option<String> { fs::read_to_string(p).ok().map(|s| s.trim().to_string()) }
fn num(p: impl AsRef<Path>) -> Option<u64> { rd(p)?.parse().ok() }

#[derive(Serialize, Debug, Default, PartialEq)]
pub struct Core { id: u32, cur_mhz: Option<u64>, max_mhz: Option<u64>, governor: String }
#[derive(Serialize, Debug, Default)]
pub struct CpuDetails { lscpu: Vec<(String, String)>, cores: Vec<Core>, driver: String, boost: Option<bool>, flags: Vec<String> }
#[derive(Serialize, Debug, PartialEq)]
pub struct Finding { label: String, value: String, level: &'static str } // level: ok | warn | info

/// Campos de interesse do `lscpu` ("Chave: valor"), na ordem de saída.
pub fn parse_lscpu(txt: &str) -> Vec<(String, String)> {
    const KEYS: [&str; 14] = ["Model name", "Architecture", "Socket(s)", "Core(s) per socket", "Thread(s) per core", "CPU max MHz", "CPU min MHz",
        "L1d cache", "L1i cache", "L2 cache", "L3 cache", "Virtualization", "Vulnerability Spectre v2", "Vulnerability Meltdown"];
    txt.lines().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim(), v.trim()))
        .filter(|(k, v)| KEYS.contains(k) && !v.is_empty()).map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

pub fn parse_flags(cpuinfo: &str) -> Vec<String> {
    cpuinfo.lines().find(|l| l.starts_with("flags") || l.starts_with("Features")).and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.split_whitespace().map(String::from).collect()).unwrap_or_default()
}

/// Boost ligado? (`cpufreq/boost` = 1 ligado; intel_pstate/no_turbo = 1 desligado.)
fn boost(sys: &Path) -> Option<bool> {
    num(sys.join("cpu/cpufreq/boost")).map(|v| v == 1).or_else(|| num(sys.join("cpu/intel_pstate/no_turbo")).map(|v| v == 0))
}

fn cores(sys: &Path) -> Vec<Core> {
    let mut v: Vec<Core> = fs::read_dir(sys.join("cpu")).into_iter().flatten().flatten().filter_map(|e| {
        let id: u32 = e.file_name().to_str()?.strip_prefix("cpu")?.parse().ok()?;
        let f = e.path().join("cpufreq");
        Some(Core { id, cur_mhz: num(f.join("scaling_cur_freq")).map(|k| k / 1000), max_mhz: num(f.join("cpuinfo_max_freq")).map(|k| k / 1000), governor: rd(f.join("scaling_governor")).unwrap_or_default() })
    }).collect();
    v.sort_by_key(|c| c.id);
    v
}

pub fn details_at(sys: &Path, lscpu: &str, cpuinfo: &str) -> CpuDetails {
    CpuDetails { lscpu: parse_lscpu(lscpu), cores: cores(sys), driver: rd(sys.join("cpu/cpu0/cpufreq/scaling_driver")).unwrap_or_default(), boost: boost(sys), flags: parse_flags(cpuinfo) }
}

/// Estrangulamento e limites de potência vistos pelo kernel.
pub fn findings_at(sys: &Path) -> Vec<Finding> {
    let mut out = vec![];
    let mut f = |label: &str, value: String, level: &'static str| out.push(Finding { label: label.into(), value, level });
    match boost(sys) { Some(b) => f("Boost / turbo", if b { "Ligado" } else { "Desligado" }.into(), if b { "ok" } else { "warn" }), None => f("Boost / turbo", "Não informado pelo kernel".into(), "info") }
    let cs = cores(sys);
    let capped = cs.iter().filter(|c| c.max_mhz.is_some_and(|m| num(sys.join(format!("cpu/cpu{}/cpufreq/scaling_max_freq", c.id))).is_some_and(|s| s / 1000 + 50 < m))).count();
    if !cs.is_empty() { f("Núcleos com clock máximo limitado", capped.to_string(), if capped > 0 { "warn" } else { "ok" }); }
    let (mut core_t, mut pkg_t, mut seen) = (0, 0, false);
    for c in &cs {
        let d = sys.join(format!("cpu/cpu{}/thermal_throttle", c.id));
        if let Some(n) = num(d.join("core_throttle_count")) { core_t += n; seen = true; }
        if let Some(n) = num(d.join("package_throttle_count")) { pkg_t = pkg_t.max(n); }
    }
    if seen { f("Estrangulamentos térmicos desde o boot", format!("{core_t} (núcleo) · {pkg_t} (pacote)"), if core_t + pkg_t > 0 { "warn" } else { "ok" }); }
    for e in fs::read_dir(sys.join("../class/powercap")).into_iter().flatten().flatten() {
        let p = e.path(); let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("intel-rapl:") || name.matches(':').count() != 1 { continue; } // só o pacote, não subdomínios
        let pkg = rd(p.join("name")).unwrap_or(name);
        for i in 0..2 {
            if let (Some(n), Some(uw)) = (rd(p.join(format!("constraint_{i}_name"))), num(p.join(format!("constraint_{i}_power_limit_uw")))) {
                f(&format!("Limite de potência {pkg} ({n})"), format!("{} W", uw / 1_000_000), "info");
            }
        }
    }
    out
}

#[tauri::command(async)]
pub fn cpu_details() -> CpuDetails {
    let ls = Command::new("lscpu").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    details_at(Path::new("/sys/devices/system"), &ls, &rd("/proc/cpuinfo").unwrap_or_default())
}

#[tauri::command(async)]
pub fn throttle_report() -> Vec<Finding> { findings_at(Path::new("/sys/devices/system")) }

#[cfg(test)]
mod tests {
    use super::*;
    fn w(p: &Path, s: &str) { fs::create_dir_all(p.parent().unwrap()).unwrap(); fs::write(p, s).unwrap(); }
    fn fake() -> std::path::PathBuf {
        let r = std::env::temp_dir().join(format!("cf-cpu-{}", std::process::id())); let _ = fs::remove_dir_all(&r);
        let s = r.join("system");
        for (i, cur) in [(0, 3600000), (1, 800000)] {
            let c = s.join(format!("cpu/cpu{i}"));
            w(&c.join("cpufreq/scaling_cur_freq"), &cur.to_string()); w(&c.join("cpufreq/cpuinfo_max_freq"), "4200000");
            w(&c.join("cpufreq/scaling_max_freq"), if i == 1 { "2000000" } else { "4200000" }); w(&c.join("cpufreq/scaling_governor"), "powersave\n");
            w(&c.join("thermal_throttle/core_throttle_count"), "3");
        }
        w(&s.join("cpu/cpufreq/boost"), "0");
        w(&r.join("class/powercap/intel-rapl:0/name"), "package-0"); w(&r.join("class/powercap/intel-rapl:0/constraint_0_name"), "long_term");
        w(&r.join("class/powercap/intel-rapl:0/constraint_0_power_limit_uw"), "65000000"); w(&r.join("class/powercap/intel-rapl:0:0/name"), "core");
        s
    }
    #[test]
    fn lscpu_and_flags() {
        let l = parse_lscpu("Architecture:  x86_64\nModel name:   AMD Ryzen 5 5500\nL3 cache:   16 MiB (1 instance)\nFoo: bar\nVirtualization:\n");
        assert_eq!(l, vec![("Architecture".into(), "x86_64".into()), ("Model name".into(), "AMD Ryzen 5 5500".into()), ("L3 cache".into(), "16 MiB (1 instance)".into())]);
        assert_eq!(parse_flags("model name : x\nflags\t\t: fpu avx2 aes\n"), vec!["fpu", "avx2", "aes"]);
        assert!(parse_flags("").is_empty() && parse_lscpu("").is_empty());
    }
    #[test]
    fn details_and_findings_from_fake_sysfs() {
        let s = fake();
        let d = details_at(&s, "", "");
        assert_eq!(d.cores.len(), 2); assert_eq!(d.cores[0].cur_mhz, Some(3600)); assert_eq!(d.cores[1].governor, "powersave"); assert_eq!(d.boost, Some(false));
        let f = findings_at(&s);
        let get = |l: &str| f.iter().find(|x| x.label.starts_with(l)).unwrap_or_else(|| panic!("{l}"));
        assert_eq!(get("Boost").level, "warn");
        assert_eq!(get("Núcleos com clock").value, "1");
        assert_eq!(get("Estrangulamentos").value, "6 (núcleo) · 0 (pacote)");
        assert_eq!(get("Limite de potência package-0 (long_term)").value, "65 W");
        assert_eq!(f.iter().filter(|x| x.label.starts_with("Limite")).count(), 1);
        let _ = fs::remove_dir_all(s.parent().unwrap());
    }
    #[test]
    fn empty_system_is_graceful() {
        let f = findings_at(Path::new("/nonexistent/x"));
        assert_eq!(f.len(), 1); assert_eq!(f[0].level, "info");
    }
}
