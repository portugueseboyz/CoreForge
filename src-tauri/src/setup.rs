//! Preparação do sistema: verifica sozinho, ao abrir, o que o CoreForge precisa (sensores, ventoinhas, GPU, Vulkan, SMART, ajudante,
//! OpenRGB, ferramentas) e oferece a correção de cada item. As verificações só leem; qualquer correção pede confirmação e senha.
use crate::{fans, leds, sysinfo};
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Serialize, Clone)]
pub struct Fix { kind: &'static str, target: String, label: String } // kind: helper | page

#[derive(Serialize, Clone)]
pub struct Check { id: &'static str, title: String, status: &'static str, detail: String, fix: Option<Fix> } // status: ok | warn | missing | info

fn check(id: &'static str, title: &str, status: &'static str, detail: impl Into<String>, fix: Option<Fix>) -> Check {
    Check { id, title: title.into(), status, detail: detail.into(), fix }
}
fn page(target: &str, label: &str) -> Option<Fix> { Some(Fix { kind: "page", target: target.into(), label: label.into() }) }
fn helper(target: &str, label: &str) -> Option<Fix> { Some(Fix { kind: "helper", target: target.into(), label: label.into() }) }

fn on_path(bin: &str) -> bool {
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':').chain(["/usr/sbin", "/sbin", "/usr/local/sbin"]).any(|d| Path::new(d).join(bin).exists())
}
fn module_loaded(m: &str) -> bool { Path::new(&format!("/sys/module/{}", m.replace('-', "_"))).exists() }
fn module_exists(m: &str) -> bool { std::process::Command::new("modinfo").args(["-n", m]).output().is_ok_and(|o| o.status.success()) }
fn vendor_of_cpu() -> String { sysinfo::read("/proc/cpuinfo").and_then(|t| t.lines().find(|l| l.starts_with("vendor_id")).and_then(|l| l.split(':').nth(1)).map(|v| v.trim().to_string())).unwrap_or_default() }

pub fn run_checks() -> Vec<Check> {
    let mut out = vec![];

    // 1. Temperatura da CPU: driver de sensores do fabricante.
    if sysinfo::cpu_temp().is_some() {
        out.push(check("cpu-temp", "Temperatura da CPU", "ok", "Sensor encontrado.", None));
    } else {
        let m = if vendor_of_cpu().contains("Intel") { "coretemp" } else { "k10temp" };
        if module_exists(m) && !module_loaded(m) { out.push(check("cpu-temp", "Temperatura da CPU", "warn", format!("Nenhum sensor de CPU, mas o driver {m} existe e não está carregado."), helper(&format!("load:{m}"), &format!("Carregar {m}")))); }
        else { out.push(check("cpu-temp", "Temperatura da CPU", "warn", "Nenhum sensor de temperatura da CPU encontrado em /sys/class/hwmon. Em máquinas virtuais isso é normal.", None)); }
    }

    // 2. Temperatura de discos SATA (drivetemp).
    let sata = fs::read_dir("/sys/block").into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().starts_with("sd"));
    if sata {
        if module_loaded("drivetemp") { out.push(check("disk-temp", "Temperatura dos discos SATA", "ok", "Driver drivetemp carregado.", None)); }
        else if module_exists("drivetemp") { out.push(check("disk-temp", "Temperatura dos discos SATA", "warn", "Há discos SATA, mas o driver drivetemp não está carregado, então a temperatura deles não aparece.", helper("load:drivetemp", "Carregar drivetemp"))); }
    }

    // 3. Ventoinhas.
    let chans = fans::scan_hwmon();
    let hw_fans = chans.iter().filter(|c| c.controllable_pub() || c.has_rpm()).count();
    if hw_fans > 0 { out.push(check("fans", "Ventoinhas", "ok", format!("{hw_fans} canal(is) de ventoinha encontrado(s) pelo kernel."), None)); }
    else { out.push(check("fans", "Ventoinhas", "warn", "O kernel não expõe ventoinhas da placa-mãe. O app pode tentar carregar o driver certo para o seu fabricante.", helper("scan-fans", "Procurar controladores"))); }

    // 3b. Velocidades manuais deixadas pelo próprio app.
    let ov = fans::active_overrides();
    if !ov.is_empty() { out.push(check("fan-overrides", "Ventoinhas em velocidade manual", "warn", format!("O CoreForge fixou a velocidade de {} canal(is). Elas ficam assim até reiniciar ou voltarem ao automático.", ov.len()), helper("restore-fans", "Devolver ao automático"))); }

    // 4. Ajudante com privilégios.
    match (fans::helper_path(), on_path("pkexec")) {
        (Ok(_), true) => out.push(check("helper", "Ajudante com privilégios", "ok", "Instalado; mudanças de hardware pedem a senha do sistema.", None)),
        (Err(e), _) => out.push(check("helper", "Ajudante com privilégios", "missing", e, None)),
        (_, false) => out.push(check("helper", "Ajudante com privilégios", "missing", "O pkexec (polkit) não foi encontrado: instale o polkit para controlar ventoinhas e carregar drivers.", None)),
    }

    // 5. GPU: driver e sensores.
    for g in leds::scan_gpus() {
        let vendor = g.ids.split(':').next().unwrap_or("");
        let title = format!("Placa de vídeo: {}", g.name);
        match vendor {
            "10de" => if nvml_ok() { out.push(check("gpu-nvidia", &title, "ok", "Driver NVIDIA (NVML) funcionando.", None)) }
                      else { out.push(check("gpu-nvidia", &title, "warn", "Placa NVIDIA sem o driver proprietário (NVML indisponível): sem temperatura, consumo e ventoinha da GPU. Se está usando o nouveau, instale o driver NVIDIA.", None)) },
            "1002" => if hwmon_has("amdgpu") { out.push(check("gpu-amd", &title, "ok", "Driver amdgpu com sensores.", None)) }
                      else { out.push(check("gpu-amd", &title, "warn", "Placa AMD sem sensores amdgpu visíveis.", None)) },
            "8086" => if crate::gpu_sysfs::gpus().iter().any(|x| x.vendor == "Intel" && crate::gpu_sysfs::sample(x).core_mhz.is_some()) { out.push(check("gpu-intel", &title, "ok", "Clock da GPU Intel lido pelo sysfs (sem uso nem VRAM: o kernel não os expõe).", None)) }
                      else { out.push(check("gpu-intel", &title, "info", "GPU Intel sem leituras no sysfs; os testes de GPU continuam funcionando se houver Vulkan.", None)) },
            _ => out.push(check("gpu-other", &title, "info", "Sem leitura dedicada para esta placa; os testes de GPU continuam funcionando se houver Vulkan.", None)),
        }
    }

    // 6. Vulkan (testes de GPU).
    let vulkan_lib = ["/usr/lib/x86_64-linux-gnu", "/usr/lib64", "/usr/lib", "/usr/lib/aarch64-linux-gnu"].iter().any(|d| Path::new(d).join("libvulkan.so.1").exists());
    let icd = ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"].iter().any(|d| fs::read_dir(d).map(|r| r.count() > 0).unwrap_or(false));
    if vulkan_lib && icd { out.push(check("vulkan", "Vulkan (testes de GPU)", "ok", "Biblioteca e driver Vulkan encontrados.", None)); }
    else { out.push(check("vulkan", "Vulkan (testes de GPU)", "missing", "Faltam a biblioteca ou o driver Vulkan (libvulkan e o ICD da sua placa). Sem eles, Stress GPU, Bench GPU e Ray tracing não rodam.", None)); }

    // 7. SMART dos discos.
    let disks = fs::read_dir("/sys/block").into_iter().flatten().flatten().any(|e| { let n = e.file_name().to_string_lossy().into_owned(); n.starts_with("nvme") || n.starts_with("sd") });
    if disks {
        if on_path("smartctl") { out.push(check("smart", "Saúde dos discos (SMART)", "ok", "smartctl instalado.", None)); }
        else if fans::install_command("smartmontools", |p| Path::new(p).exists()).is_some() { out.push(check("smart", "Saúde dos discos (SMART)", "warn", "O smartmontools não está instalado, então a página Disco mostra só a temperatura.", helper("install:smartmontools", "Instalar smartmontools"))); }
        else { out.push(check("smart", "Saúde dos discos (SMART)", "warn", "Instale o pacote smartmontools da sua distribuição para ver a saúde dos discos.", None)); }
    }

    // 8. RGB: só vale falar disso se há hardware RGB.
    let d = leds::leds_discover();
    if !d.hid.is_empty() || !d.i2c.is_empty() {
        if d.openrgb.is_empty() { out.push(check("rgb", "LEDs RGB", "missing", format!("Há hardware RGB ({} USB, {} barramento(s) I²C), mas o OpenRGB não está instalado.", d.hid.len(), d.i2c.len()), page("LEDs", "Abrir a página LEDs"))); }
        else if d.hid.iter().any(|h| !h.accessible) || d.i2c.iter().any(|b| !b.accessible) || !d.i2c_dev_loaded { out.push(check("rgb", "LEDs RGB", "warn", "OpenRGB instalado, mas faltam permissões ou o módulo i2c-dev para alguns dispositivos.", page("LEDs", "Abrir a página LEDs"))); }
        else { out.push(check("rgb", "LEDs RGB", "ok", "OpenRGB instalado e com acesso aos dispositivos encontrados.", None)); }
    }

    // 9. Ferramentas que o app chama.
    let missing: Vec<&str> = [("lspci", "pciutils"), ("lsblk", "util-linux"), ("ip", "iproute2"), ("ping", "iputils"), ("df", "coreutils")].iter().filter(|(b, _)| !on_path(b)).map(|(_, p)| *p).collect();
    if missing.is_empty() { out.push(check("tools", "Ferramentas do sistema", "ok", "lspci, lsblk, ip, ping e df encontrados.", None)); }
    else { out.push(check("tools", "Ferramentas do sistema", "warn", format!("Faltam ferramentas usadas em Informações, Disco e Rede. Pacotes: {}.", missing.join(", ")), None)); }

    // 10. Sessão gráfica (overlay).
    if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") {
        out.push(check("wayland", "Sessão Wayland", "info", "No Wayland o overlay pode não ficar acima de jogos em tela cheia; em janela sem borda ou no X11 costuma funcionar.", None));
    }
    out
}

fn nvml_ok() -> bool { nvml_wrapper::Nvml::init().is_ok() }
fn hwmon_has(chip: &str) -> bool { fs::read_dir("/sys/class/hwmon").into_iter().flatten().flatten().any(|e| sysinfo::read(&format!("{}/name", e.path().display())).as_deref() == Some(chip)) }

#[tauri::command]
pub async fn system_check() -> Result<Vec<Check>, String> { tauri::async_runtime::spawn_blocking(run_checks).await.map_err(|e| e.to_string()) }

/// Aplica uma correção de verificação. Só aceita os alvos que as próprias verificações oferecem (lista fechada).
#[tauri::command]
pub async fn system_fix(target: String) -> Result<String, String> {
    if target == "restore-fans" { return fans::fan_restore_all().await; }
    let args: Vec<String> = if target == "scan-fans" { vec!["scan-modules".into()] }
        else if let Some(m) = target.strip_prefix("load:") { vec!["modprobe".into(), m.into()] }
        else if let Some(p) = target.strip_prefix("install:") { vec!["install-package".into(), p.into()] }
        else { return Err("Correção desconhecida".into()) };
    tauri::async_runtime::spawn_blocking(move || fans::run_helper(&args)).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_run_on_this_machine() {
        let c = run_checks();
        for x in &c { println!("[{}] {} · {} {}", x.status, x.title, x.detail.chars().take(70).collect::<String>(), x.fix.as_ref().map(|f| format!("-> {}:{}", f.kind, f.target)).unwrap_or_default()); }
        assert!(c.iter().any(|x| x.id == "helper") && c.iter().any(|x| x.id == "vulkan"));
        let ids: std::collections::HashSet<_> = c.iter().map(|x| x.id).collect();
        assert_eq!(ids.len(), c.len()); // ids únicos
    }
    #[test]
    fn fix_targets_are_whitelisted_in_helper() {
        // os alvos que as verificações oferecem precisam ser aceitos pelo ajudante
        assert!(fans::install_command("smartmontools", |p| p == "/usr/bin/apt-get").is_some());
        for m in ["k10temp", "coretemp", "drivetemp"] { assert!(fans::SENSOR_MODULES.contains(&m)); }
    }
}
