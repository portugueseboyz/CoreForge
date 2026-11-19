//! Notebooks: bateria (sysfs) e perfil de energia (power-profiles-daemon, via `powerprofilesctl`).
use serde::Serialize;
use std::{fs, path::Path, process::Command};

#[derive(Serialize, Default, PartialEq, Debug)]
pub struct Battery { name: String, status: String, percent: Option<f64>, health_pct: Option<f64>, power_w: Option<f64>, cycles: Option<u64>, limit_pct: Option<u64>, technology: String }

#[derive(Serialize, Default)]
pub struct Power { batteries: Vec<Battery>, ac_online: Option<bool>, profile: String, profiles: Vec<String> }

fn num(dir: &Path, f: &str) -> Option<f64> { fs::read_to_string(dir.join(f)).ok()?.trim().parse().ok() }
fn txt(dir: &Path, f: &str) -> String { fs::read_to_string(dir.join(f)).map(|s| s.trim().to_string()).unwrap_or_default() }

/// Lê baterias e fonte de energia de uma pasta tipo /sys/class/power_supply (aceita uma falsa nos testes).
fn read_supplies(root: &Path) -> (Vec<Battery>, Option<bool>) {
    let (mut bats, mut ac) = (vec![], None);
    let mut dirs: Vec<_> = fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        match txt(&d, "type").as_str() {
            "Mains" => if let Some(on) = num(&d, "online") { ac = Some(ac.unwrap_or(false) || on > 0.0) },
            "Battery" if txt(&d, "scope") != "Device" => {
                // Algumas baterias usam energia (µWh), outras carga (µAh): a razão cheia/projeto vale nos dois casos.
                let (full, design) = (num(&d, "energy_full").or(num(&d, "charge_full")), num(&d, "energy_full_design").or(num(&d, "charge_full_design")));
                let power = num(&d, "power_now").map(|p| p / 1e6).or_else(|| Some(num(&d, "current_now")? * num(&d, "voltage_now")? / 1e12));
                bats.push(Battery {
                    name: d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    status: txt(&d, "status"), percent: num(&d, "capacity"), technology: txt(&d, "technology"),
                    health_pct: full.zip(design).filter(|(_, g)| *g > 0.0).map(|(f, g)| 100.0 * f / g),
                    power_w: power.filter(|p| *p > 0.0), cycles: num(&d, "cycle_count").map(|c| c as u64).filter(|c| *c > 0),
                    limit_pct: num(&d, "charge_control_end_threshold").map(|c| c as u64).filter(|c| *c < 100),
                });
            }
            _ => {}
        }
    }
    (bats, ac)
}

/// Nomes de perfis na saída de `powerprofilesctl list` (linhas "  balanced:" ou "* performance:").
fn parse_profiles(out: &str) -> Vec<String> {
    out.lines().filter(|l| l.starts_with("* ") || (l.starts_with("  ") && !l.starts_with("   ")))
        .filter_map(|l| l.trim().trim_start_matches("* ").strip_suffix(':').map(str::to_string)).collect()
}

fn ctl(args: &[&str]) -> Option<String> {
    let o = Command::new("powerprofilesctl").args(args).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

#[tauri::command(async)]
pub fn power_info() -> Power {
    let (batteries, ac_online) = read_supplies(Path::new("/sys/class/power_supply"));
    Power { batteries, ac_online, profile: ctl(&["get"]).unwrap_or_default().trim().into(), profiles: ctl(&["list"]).map(|o| parse_profiles(&o)).unwrap_or_default() }
}

#[tauri::command(async)]
pub fn power_profile_set(name: String) -> Result<(), String> {
    if !["power-saver", "balanced", "performance"].contains(&name.as_str()) { return Err("perfil inválido".into()); }
    ctl(&["set", &name]).map(|_| ()).ok_or_else(|| "Não foi possível mudar o perfil de energia (o serviço power-profiles-daemon está ativo?).".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put(d: &Path, f: &str, v: &str) { fs::create_dir_all(d).unwrap(); fs::write(d.join(f), v).unwrap(); }

    #[test]
    fn battery_and_profiles() {
        let root = std::env::temp_dir().join(format!("cf-power-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let b = root.join("BAT0");
        for (f, v) in [("type", "Battery"), ("status", "Discharging"), ("capacity", "80"), ("energy_full", "40000000"), ("energy_full_design", "50000000"),
            ("power_now", "12500000"), ("cycle_count", "212"), ("charge_control_end_threshold", "80")] { put(&b, f, v); }
        put(&root.join("AC"), "type", "Mains"); put(&root.join("AC"), "online", "0");
        put(&root.join("hidpp_battery_0"), "type", "Battery"); put(&root.join("hidpp_battery_0"), "scope", "Device"); // mouse: ignorado
        let (bats, ac) = read_supplies(&root);
        assert_eq!(ac, Some(false));
        assert_eq!(bats.len(), 1);
        assert_eq!((bats[0].percent, bats[0].health_pct, bats[0].power_w, bats[0].cycles, bats[0].limit_pct), (Some(80.0), Some(80.0), Some(12.5), Some(212), Some(80)));
        let _ = fs::remove_dir_all(&root);
        let out = "  performance:\n    Driver:     platform_profile\n\n* balanced:\n    Driver:     platform_profile\n\n  power-saver:\n    Driver:     platform_profile\n";
        assert_eq!(parse_profiles(out), ["performance", "balanced", "power-saver"]);
    }
}
