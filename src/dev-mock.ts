// Backend falso para ver a interface num navegador comum (npm run dev, abrir com ?mock). Não entra no build.
const node = (header: string, value = "", children: unknown[] = []) => ({ header, value, children });
const info = [
  node("Sistema operacional", "", [node("Distribuição", "Ubuntu 26.04.1 LTS"), node("Kernel", "7.0.0-43-generic"), node("Arquitetura", "x86_64"), node("Sessão", "wayland"), node("Ligado há", "0d 02:41:12")]),
  node("Placa-mãe e BIOS", "", [node("Fabricante", "ASUSTeK COMPUTER INC."), node("Modelo", "PRIME A520M-A II"), node("Versão da BIOS", "2803"), node("Data da BIOS", "04/27/2022")]),
  node("Processador", "", [node("Modelo", "AMD Ryzen 5 5500"), node("Threads", "12"), node("Núcleos físicos", "6"), node("Clock máx.", "4269 MHz"), node("Instruções", "sse4_2 avx avx2 aes fma svm")]),
  node("Memória", "", [node("Total", "14,9 GB"), node("Disponível", "8,7 GB"), node("Swap", "4,0 GB")]),
  node("Placa de vídeo", "", [node("NVIDIA Corporation GA106 [Geforce RTX 3050]", "", [node("Slot PCI", "01:00.0"), node("Driver", "nvidia")])]),
  node("Armazenamento", "", [node("KIOXIA-EXCERIA SSD (/dev/nvme0n1)", "", [node("Tamanho", "500 GB"), node("Tipo", "NVMe"), node("Interface", "nvme")])]),
];
const metrics = { gpu_name: "RTX 3050", gpu_util: 71, gpu_temp: 63, gpu_clock: 1905, gpu_mem_clock: 7001, gpu_power_w: 94.2, gpu_fan: 48, fan_rpm: 1180, vram_used_mb: 3100, vram_total_mb: 8192,
  gpu_throttle: null, cpu_load: 38, cpu_busiest: 71, cpu_temp: 52, cpu_clock_mhz: 3600, ram_used_gb: 6.2, ram_total_gb: 14.9 };
const sensors = [["k10temp", "Temperatura", "Tctl", 52.1, "°C"], ["nvme", "Temperatura", "Composite", 39, "°C"], ["CPU", "Carga", "CPU Total", 38.2, "%"], ["CPU", "Clock", "CPU Core #1", 3612, "MHz"], ["amdgpu", "Ventoinhas", "fan1", 1180, "RPM"]]
  .map(([hardware, category, name, value, unit], i) => ({ id: String(i), hardware, category, name, value, unit }));
const gpuInfo = [node("NVIDIA GeForce RTX 3050", "", [
  node("Geral", "", [node("Driver", "595.99.02"), node("CUDA (driver)", "13.2"), node("Núcleos CUDA", "2560")]),
  node("Memória", "", [node("VRAM", "8.0 GB"), node("Barramento", "128 bits")]),
  node("PCIe", "", [node("Máximo", "PCIe 4.0 x8"), node("Em uso", "PCIe 4.0 x8")])])];
const live = [{ index: 0, items: [["Uso da GPU", "71 %"], ["Temperatura", "63 °C"], ["Clock do núcleo", "1905 MHz"], ["Consumo", "94.2 W (72 % do limite)"], ["Ventoinha", "48 %"]] }];
const gpuBench = { running: false, progress: 1, fps_now: 0, frames: 7385, adapter: "NVIDIA GeForce RTX 3050", error: "",
  result: { adapter: "NVIDIA GeForce RTX 3050", width: 1920, height: 1080, seconds: 30, frames: 7385, avg_fps: 246.2, low1_fps: 210.7, min_fps: 161.9, score: 2462 } };
const stress = { running: true, elapsed_s: 74, temp: 71, max_temp: 73, mem_errors: 0, mem_passes: 12, mem_gb: 6.1, reason: "" };
const startup = [["at-spi-dbus-bus.desktop", "AT-SPI D-Bus Bus", "/usr/libexec/at-spi-bus-launcher", "sistema", true], ["snap-userd-autostart.desktop", "Snap user daemon", "snap userd --autostart", "sistema", false]]
  .map(([id, name, exec, scope, enabled]) => ({ id, name, exec, scope, enabled }));
const fans = { hints: ["A ventoinha da GPU está em 0 %: é o modo silencioso (zero RPM), que a placa usa até esquentar. É normal."], modules: ["nct6775", "nct6683", "it87"],
  channels: [{ id: "gpu/0/0", kind: "gpu", label: "NVIDIA GeForce RTX 3050 · ventoinha 1", chip: "nvidia", hw: "0", pwm: 0, rpm: 1180, percent: 42, mode: "Automático", controllable: true, min: 30, max: 100, fan: null, tested: false, responsive: true },
             { id: "hwmon/hwmon3/1", kind: "hwmon", label: "CPU Fan", chip: "nct6798", hw: "hwmon3", pwm: 1, rpm: 1320, percent: 55, mode: "Manual", controllable: true, min: 25, max: 100, fan: 1, tested: true, responsive: true },
             { id: "hwmon/hwmon3/fan4", kind: "hwmon", label: "Rear Fan (tach only)", chip: "nct6798", hw: "hwmon3", pwm: 0, rpm: 940, percent: null, mode: "Só leitura", controllable: false, min: 25, max: 100, fan: 4, tested: false, responsive: true },
             { id: "hwmon/hwmon3/2", kind: "hwmon", label: "Chassis Fan 1", chip: "nct6798", hw: "hwmon3", pwm: 2, rpm: 0, percent: 100, mode: "Sem controle (100 %)", controllable: true, min: 25, max: 100, fan: null, tested: true, responsive: false }] };
const leds = { backend: "OpenRGB", installed: true, hints: [], devices: [{ index: 0, name: "ASUS PRIME A520M-A II", kind: "Motherboard", modes: ["Direct", "Off", "Static", "Breathing", "Spectrum Cycle"], active_mode: "Direct", zones: ["Aura Mainboard", "Addressable RGB Header 1", "Addressable RGB Header 2"], zone_leds: [2, 120, 0], led_names: [["RGB Header 1", "RGB Header 2"], Array.from({ length: 120 }, (_, i) => `LED ${i + 1}`), []] }] };
const discover = { openrgb: "Flatpak", i2c_dev_loaded: true, hints: ["Há dispositivos RGB USB sem permissão de acesso para o seu usuário. Libere o acesso abaixo (só para os dispositivos encontrados)."],
  hid: [{ vid_pid: "0b05:19af", vendor: "ASUS", name: "AURA LED Controller", accessible: true }, { vid_pid: "1038:1836", vendor: "SteelSeries", name: "Aerox 3", accessible: false }],
  gpus: [{ name: "GA106 [Geforce RTX 3050]", slot: "01:00.0", ids: "10de:2507", subsystem: "1043:887c", board_maker: "ASUS", i2c_buses: ["/dev/i2c-3"], i2c_accessible: true }],
  i2c: [{ dev: "/dev/i2c-0", adapter: "SMBus PIIX4", kind: "piix4", accessible: true }, { dev: "/dev/i2c-3", adapter: "NVIDIA i2c adapter 1", kind: "nvidia", accessible: false }] };
const checks = [
  { id: "cpu-temp", title: "Temperatura da CPU", status: "ok", detail: "Sensor encontrado.", fix: null },
  { id: "fans", title: "Ventoinhas", status: "warn", detail: "O kernel não expõe ventoinhas da placa-mãe. O app pode tentar carregar o driver certo para o seu fabricante.", fix: { kind: "helper", target: "scan-fans", label: "Procurar controladores" } },
  { id: "smart", title: "Saúde dos discos (SMART)", status: "warn", detail: "O smartmontools não está instalado, então a página Disco mostra só a temperatura.", fix: { kind: "helper", target: "install:smartmontools", label: "Instalar smartmontools" } },
  { id: "vulkan", title: "Vulkan (testes de GPU)", status: "missing", detail: "Faltam a biblioteca ou o driver Vulkan.", fix: null },
  { id: "rgb", title: "LEDs RGB", status: "ok", detail: "OpenRGB instalado e com acesso aos dispositivos encontrados.", fix: null }];
const idle = { running: false, phase: "", progress: 0, fps_now: 0, frames: 0, adapter: "", error: "", result: null, rows: [], passes: 0, elapsed_s: 0 };
const answers: Record<string, unknown> = {
  update_check: { current: "0.1.0", latest: "0.2.0", url: "", notes_excerpt: "", update_available: true, known: true }, update_open: null,
  cleanup_scan: [{ id: "thumbs", label: "Miniaturas de imagens", bytes: 48e6, cleanable: true, hint: "" }, { id: "trash", label: "Lixeira", bytes: 0, cleanable: true, hint: "" }, { id: "cache", label: "Cache dos programas (~/.cache)", bytes: 2.4e9, cleanable: true, hint: "Os programas recriam o que precisarem; o primeiro uso depois pode ser mais lento." },
    { id: "pkg", label: "Cache de pacotes do sistema", bytes: 1.1e9, cleanable: false, hint: "Exige root; rode no terminal: sudo apt clean (ou pacman -Sc / dnf clean all)" }, { id: "journal", label: "Logs do sistema (journal)", bytes: 0, cleanable: false, hint: "Exige root; rode no terminal: sudo journalctl --vacuum-size=200M" },
    { id: "kernels", label: "Kernels antigos", bytes: 0, cleanable: false, hint: "Nenhum kernel antigo encontrado (ou o sistema não usa dpkg)." }], cleanup_run: 48e6,
  cpu_details: { lscpu: [["Architecture", "x86_64"], ["Model name", "AMD Ryzen 5 5500"], ["Core(s) per socket", "6"], ["Thread(s) per core", "2"], ["CPU max MHz", "4269"], ["L1d cache", "192 KiB (6 instances)"], ["L2 cache", "3 MiB (6 instances)"], ["L3 cache", "16 MiB (1 instance)"]],
    cores: Array.from({ length: 12 }, (_, id) => ({ id, cur_mhz: 3600 - id * 90, max_mhz: 4269, governor: "powersave" })), driver: "amd-pstate-epp", boost: true, flags: "fpu vme sse4_2 avx avx2 aes fma svm".split(" ") },
  throttle_report: [{ label: "Boost / turbo", value: "Ligado", level: "ok" }, { label: "Núcleos com clock máximo limitado", value: "2", level: "warn" }, { label: "Estrangulamentos térmicos desde o boot", value: "0 (núcleo) · 0 (pacote)", level: "ok" }, { label: "Limite de potência package-0 (long_term)", value: "65 W", level: "info" }],
 report_save: "/home/user/Documentos/coreforge-relatorio-0.md", profiles_list: [{ name: "Gamer", apply_on_start: true, fans: [{ kind: "hwmon", label: "Placa-mãe (nct6798) · CPU Fan", percent: 70 }], leds: [{ device: "ASUS PRIME A520M-A II", zone: "Addressable RGB Header 1", mode: "Direct", color: "b4ff1f", colors: null }, { device: "HyperX Fury RGB", zone: "HyperX Slot 1", mode: "Static", color: "ff3b30", colors: null }] }],
  power_info: { batteries: [{ name: "BAT0", status: "Discharging", percent: 72, health_pct: 88, power_w: 11.4, cycles: 212, limit_pct: 80, technology: "Li-ion" }], ac_online: false, profile: "balanced", profiles: ["performance", "balanced", "power-saver"] }, power_profile_set: null,
  curves_get: [], curves_save: null, curves_start: null, curves_stop: null, curves_status: { running: false, error: "" },
  profile_capture: { fans: [{ kind: "hwmon", label: "CPU Fan", percent: 55 }], leds: [{ device: "x", zone: null, mode: "Direct", color: "ff0000", colors: null }] }, profile_autostart_get: false, games_scan: [], fps_table: { gpu: "RTX 3050", cpu: "Ryzen 5 5500", gpu_known: true, cpu_known: false, rows: [{ name: "Counter-Strike 2", genre: "FPS competitivo", avg: 190, low1: 142, limiter: "GPU" }, { name: "Elden Ring", genre: "RPG de ação", avg: 60, low1: 45, limiter: "Trava do jogo" }] },
  net_status: idle, disk_bench_status: idle, bench_status: idle, rt_status: idle, apps_list: { flatpak: true, apps: [{ name: "Steam", id: "x", desc: "Loja de jogos", installed: true, source: "Pacote do sistema" }, { name: "VLC", id: "y", desc: "Player de vídeo", installed: false, source: "" }] },
  app_install_status: { running: false, app: "", output: "" }, serial_report: { ports: [], usb: [], in_dialout: false, brltty: true, hints: ["Seu usuário não está no grupo dialout: sem isso, IDEs como a do Arduino não conseguem abrir /dev/ttyUSB* e /dev/ttyACM*. Depois de adicionar, saia e entre na sessão de novo."] },
  gpu_stress_status: { ...idle, dispatches: 0, gflops: 0, temp: null, max_temp: null, power_w: null, max_power_w: null, util: null, reason: "" }, fan_overrides: [{ kind: "hwmon", hw: "hwmon3", pwm: 2, chip: "nct6798" }], system_check: checks, leds_report: "CoreForge · relatório de hardware RGB\nGPU: GA106 · subsistema 1043:887c (ASUS)", leds_discover: discover, fans_report: fans, leds_status: leds, system_info: info, live_metrics: metrics, sensors, gpu_info: gpuInfo, gpu_live: live, gpu_bench_status: gpuBench, stress_status: stress, startup_list: startup,
  disk_mounts: [{ path: "/", source: "/dev/mapper/ubuntu--vg-ubuntu--lv", size_gb: 487, free_gb: 395 }], disk_health: [], net_interfaces: [{ name: "enp7s0", kind: "Ethernet", up: true, speed: "1000 Mbps", mac: "", addrs: ["192.168.1.226"] }] };
let phase = 0;
// Valores que oscilam, para ver os gráficos se movendo.
const jitter = () => { phase += 0.35; const w = (base: number, amp: number) => Math.max(0, base + amp * Math.sin(phase) + (Math.random() - 0.5) * amp * 0.6);
  return { ...metrics, cpu_load: w(38, 22), cpu_busiest: w(60, 25), cpu_temp: w(52, 9), gpu_util: w(65, 30), gpu_temp: w(63, 8), gpu_power_w: w(95, 35), fan_rpm: w(1200, 300), ram_used_gb: w(6.2, 0.5), vram_used_mb: w(3100, 400) }; };
(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
  invoke: async (cmd: string) => (cmd === "live_metrics" ? jitter() : cmd === "bench_image" ? new ArrayBuffer(0) : cmd in answers ? answers[cmd] : null),
  transformCallback: () => 0,
};
export {};
