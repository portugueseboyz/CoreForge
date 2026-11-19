//! Ferramentas: programas de inicialização (autostart XDG), portas seriais/USB (CH340, CP210x, FTDI, Arduino...) e instalação de apps (Flatpak).
use crate::sysinfo::read;
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
};

fn home() -> PathBuf { PathBuf::from(std::env::var("HOME").unwrap_or_default()) }
fn run(exe: &str, args: &[&str]) -> (bool, String) {
    match Command::new(exe).args(args).output() {
        Ok(o) => (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (false, e.to_string()),
    }
}

// ------------------------------------------------------------------ inicialização

#[derive(Serialize)]
pub struct StartupItem { id: String, name: String, exec: String, scope: &'static str, enabled: bool }

fn desktop_field(txt: &str, key: &str) -> Option<String> {
    txt.lines().take_while(|l| !l.starts_with("[Desktop Action")).find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')).map(|v| v.trim().to_string()))
}

fn is_enabled(txt: &str) -> bool {
    desktop_field(txt, "Hidden").as_deref() != Some("true") && desktop_field(txt, "X-GNOME-Autostart-enabled").as_deref() != Some("false")
}

fn user_dir() -> PathBuf { home().join(".config/autostart") }

#[tauri::command(async)]
pub fn startup_list() -> Vec<StartupItem> {
    let mut by_id: std::collections::BTreeMap<String, StartupItem> = Default::default();
    for (dir, scope) in [(PathBuf::from("/etc/xdg/autostart"), "sistema"), (user_dir(), "usuário")] { // o do usuário substitui o do sistema
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if !id.ends_with(".desktop") { continue; }
            let Some(txt) = read(&e.path().to_string_lossy()) else { continue };
            if desktop_field(&txt, "NoDisplay").as_deref() == Some("true") && scope == "sistema" && desktop_field(&txt, "Name").is_none() { continue; }
            by_id.insert(id.clone(), StartupItem {
                name: desktop_field(&txt, "Name").unwrap_or_else(|| id.trim_end_matches(".desktop").into()),
                exec: desktop_field(&txt, "Exec").unwrap_or_default(), scope, enabled: is_enabled(&txt), id,
            });
        }
    }
    by_id.into_values().collect()
}

#[tauri::command(async)]
pub fn startup_set(id: String, enabled: bool) -> Result<(), String> {
    if id.contains('/') || !id.ends_with(".desktop") { return Err("Item inválido".into()); }
    let (user, sys) = (user_dir().join(&id), PathBuf::from("/etc/xdg/autostart").join(&id));
    if enabled && sys.exists() { // reativar um item do sistema = remover o override do usuário
        if user.exists() { fs::remove_file(&user).map_err(|e| e.to_string())?; }
        return Ok(());
    }
    let src = if user.exists() { user.clone() } else { sys };
    let txt = read(&src.to_string_lossy()).ok_or("Não consegui ler o item")?;
    let mut lines: Vec<String> = txt.lines().filter(|l| !l.starts_with("Hidden=") && !l.starts_with("X-GNOME-Autostart-enabled=")).map(String::from).collect();
    if !enabled && let Some(i) = lines.iter().position(|l| l.trim() == "[Desktop Entry]") { lines.insert(i + 1, "Hidden=true".into()); }
    fs::create_dir_all(user_dir()).map_err(|e| e.to_string())?;
    fs::write(&user, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ portas seriais e USB

#[derive(Serialize)]
pub struct SerialPort { dev: String, chip: String, vid_pid: String, product: String, driver: String, access: bool }

#[derive(Serialize)]
pub struct UsbDevice { vid_pid: String, name: String, chip: String, serial_ports: Vec<String> }

#[derive(Serialize)]
pub struct SerialReport { ports: Vec<SerialPort>, usb: Vec<UsbDevice>, in_dialout: bool, brltty: bool, hints: Vec<String> }

fn chip_name(vid: &str, pid: &str) -> &'static str {
    match (vid, pid) {
        ("1a86", "7523") => "CH340", ("1a86", "5523") | ("1a86", "55d4") => "CH341 / CH9102",
        ("10c4", "ea60") => "CP2102 (Silicon Labs)", ("10c4", "ea70") => "CP2105", ("0403", "6001") => "FTDI FT232R", ("0403", "6015") => "FTDI FT230X",
        ("067b", "2303") => "Prolific PL2303", ("2341", _) | ("2a03", _) => "Arduino", ("303a", _) => "Espressif (ESP32-Sx)", _ => "",
    }
}

fn usb_ancestor(sys_dev: &Path) -> Option<PathBuf> {
    let mut p = fs::canonicalize(sys_dev).ok()?;
    loop {
        if p.join("idVendor").exists() { return Some(p); }
        if !p.pop() || p.as_os_str().len() < 6 { return None; }
    }
}

fn can_open(dev: &str) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    // O_NONBLOCK | O_NOCTTY: abrir só para testar a permissão, sem mexer em DTR/RTS (que reiniciaria um Arduino)
    fs::OpenOptions::new().read(true).write(true).custom_flags(0o4000 | 0o400).open(dev).is_ok()
}

#[tauri::command(async)]
pub fn serial_report() -> SerialReport {
    let mut ports = vec![];
    let mut names: Vec<String> = fs::read_dir("/sys/class/tty").into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("ttyUSB") || n.starts_with("ttyACM")).collect();
    names.sort();
    for n in &names {
        let base = PathBuf::from(format!("/sys/class/tty/{n}/device"));
        let usb = usb_ancestor(&base);
        let f = |k: &str| usb.as_ref().and_then(|u| read(&u.join(k).to_string_lossy())).unwrap_or_default();
        let (vid, pid) = (f("idVendor"), f("idProduct"));
        let driver = fs::read_link(base.join("driver")).ok().and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned())).unwrap_or_default();
        ports.push(SerialPort { dev: format!("/dev/{n}"), chip: chip_name(&vid, &pid).into(), vid_pid: format!("{vid}:{pid}"), product: f("product"), driver, access: can_open(&format!("/dev/{n}")) });
    }
    let mut usb = vec![];
    for e in fs::read_dir("/sys/bus/usb/devices").into_iter().flatten().flatten() {
        let p = e.path();
        let n = e.file_name().to_string_lossy().into_owned();
        if n.contains(':') || !p.join("idVendor").exists() { continue; }
        let r = |k: &str| read(&p.join(k).to_string_lossy()).unwrap_or_default();
        let (vid, pid) = (r("idVendor"), r("idProduct"));
        if vid == "1d6b" { continue; } // hubs raiz do Linux
        let mine: Vec<String> = ports.iter().filter(|s| s.vid_pid == format!("{vid}:{pid}")).map(|s| s.dev.clone()).collect();
        usb.push(UsbDevice { name: format!("{} {}", r("manufacturer"), r("product")).trim().to_string(), vid_pid: format!("{vid}:{pid}"), chip: chip_name(&vid, &pid).into(), serial_ports: mine });
    }
    let groups = run("id", &["-nG"]).1;
    let in_dialout = groups.split_whitespace().any(|g| g == "dialout" || g == "uucp");
    let brltty = run("dpkg-query", &["-W", "-f=${Status}", "brltty"]).1.contains("install ok installed");
    let mut hints = vec![];
    if !in_dialout { hints.push("Seu usuário não está no grupo dialout: sem isso, IDEs como a do Arduino não conseguem abrir /dev/ttyUSB* e /dev/ttyACM*. Depois de adicionar, saia e entre na sessão de novo.".into()); }
    if brltty { hints.push("O brltty (leitor de braille) está instalado e costuma capturar adaptadores CH340/CH341, fazendo a porta sumir logo após conectar. Se você não usa braille, remova-o.".into()); }
    if usb.iter().any(|u| u.chip.starts_with("CH34")) && ports.is_empty() { hints.push("Há um CH340/CH341 conectado, mas nenhuma porta /dev/ttyUSB apareceu: provável captura pelo brltty, cabo só de carga ou driver ch341 ausente.".into()); }
    SerialReport { ports, usb, in_dialout, brltty, hints }
}

/// Correções que pedem root, via ajudante instalado + pkexec. Só duas ações fixas, sem texto livre.
#[tauri::command]
pub async fn serial_fix(action: String) -> Result<String, String> {
    let args = match action.as_str() { "dialout" => "dialout", "brltty" => "remove-brltty", _ => return Err("Ação inválida".into()) };
    tauri::async_runtime::spawn_blocking(move || crate::fans::run_helper(&[args.into()])).await.map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ instalar apps (Flatpak, sem root)

/// Nome, id do Flatpak (o que o botão instala), descrição, pacote Snap, pacote .deb e executáveis (para achar instalações de qualquer origem).
struct Entry { name: &'static str, id: &'static str, desc: &'static str, snap: &'static str, deb: &'static str, bins: &'static [&'static str] }
const APPS: &[Entry] = &[
    Entry { name: "Steam", id: "com.valvesoftware.Steam", desc: "Loja de jogos", snap: "steam", deb: "steam-launcher", bins: &["steam"] },
    Entry { name: "Discord", id: "com.discordapp.Discord", desc: "Chat de voz e texto", snap: "discord", deb: "discord", bins: &["discord"] },
    Entry { name: "VLC", id: "org.videolan.VLC", desc: "Player de vídeo", snap: "vlc", deb: "vlc", bins: &["vlc"] },
    Entry { name: "Google Chrome", id: "com.google.Chrome", desc: "Navegador", snap: "", deb: "google-chrome-stable", bins: &["google-chrome", "google-chrome-stable"] },
    Entry { name: "Firefox", id: "org.mozilla.firefox", desc: "Navegador", snap: "firefox", deb: "firefox", bins: &["firefox"] },
    Entry { name: "OBS Studio", id: "com.obsproject.Studio", desc: "Gravação e streaming", snap: "obs-studio", deb: "obs-studio", bins: &["obs"] },
    Entry { name: "Heroic Games Launcher", id: "com.heroicgameslauncher.hgl", desc: "Epic, GOG e Amazon Games", snap: "", deb: "heroic", bins: &["heroic"] },
    Entry { name: "Arduino IDE 2", id: "cc.arduino.IDE2", desc: "Desenvolvimento Arduino", snap: "", deb: "arduino-ide", bins: &["arduino-ide"] },
    Entry { name: "Flameshot", id: "org.flameshot.Flameshot", desc: "Capturas de tela (como o Lightshot)", snap: "flameshot", deb: "flameshot", bins: &["flameshot"] },
    Entry { name: "OpenRGB", id: "org.openrgb.OpenRGB", desc: "Controle de LEDs RGB (usado pela página LEDs)", snap: "", deb: "openrgb", bins: &["openrgb"] },
    Entry { name: "MangoHud", id: "org.freedesktop.Platform.VulkanLayer.MangoHud//24.08", desc: "Overlay de FPS para jogos Vulkan", snap: "", deb: "mangohud", bins: &["mangohud"] },
];

#[derive(Serialize)]
pub struct App { name: &'static str, id: &'static str, desc: &'static str, installed: bool, source: &'static str }

#[derive(Serialize)]
pub struct AppsReport { flatpak: bool, apps: Vec<App> }

fn on_path(bin: &str) -> bool {
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':').chain(["/snap/bin", "/usr/local/bin", "/opt"]).any(|d| Path::new(d).join(bin).exists())
}

/// Onde o app está instalado: Flatpak, Snap, pacote do sistema (.deb) ou um executável no PATH. Vazio = não instalado.
fn detect(a: &Entry, flat: &HashSet<String>, snaps: &HashSet<String>, debs: &HashSet<String>) -> &'static str {
    if flat.contains(a.id.split("//").next().unwrap()) { "Flatpak" }
    else if !a.snap.is_empty() && snaps.contains(a.snap) { "Snap" }
    else if debs.contains(a.deb) { "Pacote do sistema" }
    else if a.bins.iter().any(|b| on_path(b)) { "Instalado no sistema" }
    else { "" }
}

fn installed_sets() -> (bool, HashSet<String>, HashSet<String>, HashSet<String>) {
    let (fp_ok, fp) = run("flatpak", &["list", "--columns=application"]);
    let flat = if fp_ok { fp.lines().map(|l| l.trim().to_string()).collect() } else { HashSet::new() };
    let snaps = run("snap", &["list"]).1.lines().skip(1).filter_map(|l| l.split_whitespace().next().map(String::from)).collect();
    let pkgs: Vec<&str> = APPS.iter().map(|a| a.deb).collect();
    let mut args = vec!["-W", "-f=${Package} ${Status}\n"];
    args.extend(pkgs);
    let debs = run("dpkg-query", &args).1.lines().filter(|l| l.ends_with("install ok installed")).filter_map(|l| l.split(' ').next().map(String::from)).collect();
    (fp_ok, flat, snaps, debs)
}

#[derive(Default)]
pub struct InstallState { log: Arc<Mutex<(bool, String, String)>> } // (rodando, app, saída)

#[tauri::command(async)]
pub fn apps_list() -> AppsReport {
    let (flatpak, flat, snaps, debs) = installed_sets();
    let apps = APPS.iter().map(|a| { let source = detect(a, &flat, &snaps, &debs); App { name: a.name, id: a.id, desc: a.desc, installed: !source.is_empty(), source } }).collect();
    AppsReport { flatpak, apps }
}

#[tauri::command]
pub fn app_install(st: tauri::State<InstallState>, id: String) -> Result<(), String> {
    let app = APPS.iter().find(|a| a.id == id).ok_or("App fora da lista")?; // só instala o que está na lista
    if st.log.lock().unwrap_or_else(|e| e.into_inner()).0 { return Err("Já há uma instalação em andamento".into()); }
    *st.log.lock().unwrap_or_else(|e| e.into_inner()) = (true, app.name.into(), String::new());
    let (log, id) = (st.log.clone(), id.clone());
    thread::spawn(move || {
        let add = run("flatpak", &["remote-add", "--user", "--if-not-exists", "flathub", "https://flathub.org/repo/flathub.flatpakrepo"]);
        let (ok, out) = if add.0 { run("flatpak", &["install", "--user", "-y", "flathub", &id]) } else { add };
        let mut g = log.lock().unwrap_or_else(|e| e.into_inner());
        g.0 = false;
        g.2 = format!("{}{}", if ok { "Instalado.\n" } else { "Falhou.\n" }, out.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
    });
    Ok(())
}

#[derive(Serialize)]
pub struct InstallStatus { running: bool, app: String, output: String }

#[tauri::command]
pub fn app_install_status(st: tauri::State<InstallState>) -> InstallStatus {
    let g = st.log.lock().unwrap_or_else(|e| e.into_inner());
    InstallStatus { running: g.0, app: g.1.clone(), output: g.2.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_parsing() {
        let t = "[Desktop Entry]\nName=Foo\nExec=foo --bar\nX-GNOME-Autostart-enabled=false\n[Desktop Action x]\nHidden=true\n";
        assert_eq!(desktop_field(t, "Name").as_deref(), Some("Foo"));
        assert!(!is_enabled(t));
        assert!(is_enabled("[Desktop Entry]\nName=Foo\n"));
        assert!(!is_enabled("[Desktop Entry]\nHidden=true\n"));
        assert_eq!(chip_name("1a86", "7523"), "CH340");
    }
    #[test]
    fn detects_installed_apps() {
        let r = apps_list();
        for a in &r.apps { println!("{:<22} {}", a.name, if a.installed { a.source } else { "-" }); }
        assert_eq!(r.apps.len(), APPS.len());
    }
    #[test]
    fn real_machine() {
        let s = startup_list();
        let r = serial_report();
        println!("{} autostart · {} portas · {} USB · dialout={} brltty={} · {:?}", s.len(), r.ports.len(), r.usb.len(), r.in_dialout, r.brltty, r.hints);
        assert!(!s.is_empty());
    }
    // Liga e desliga um item de verdade, num HOME temporário, e confere que o original do sistema não é alterado.
    #[test]
    fn toggle_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("cf_home_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("HOME", &tmp) }; // teste isolado (--test-threads=1)
        let id = "at-spi-dbus-bus.desktop".to_string();
        if !Path::new("/etc/xdg/autostart").join(&id).exists() { return; }
        startup_set(id.clone(), false).unwrap();
        assert!(!startup_list().iter().find(|i| i.id == id).unwrap().enabled);
        startup_set(id.clone(), true).unwrap();
        assert!(startup_list().iter().find(|i| i.id == id).unwrap().enabled);
        assert!(!tmp.join(".config/autostart").join(&id).exists()); // override removido
        let _ = fs::remove_dir_all(tmp);
    }
}
