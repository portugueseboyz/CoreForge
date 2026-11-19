English | [Português](README.pt-BR.md)

# CoreForge

> Early release (0.1). Tested on one desktop (NVIDIA GPU, Nuvoton nct6798 fan controller). Fan control, laptops, AMD/Intel GPUs and other game stores are implemented but not yet verified on other hardware. No installer yet: run from source.

Hardware toolkit for Linux: system information, live sensors, CPU/RAM/GPU stress tests, CPU/GPU/ray-tracing benchmarks, disk and network
diagnostics, fan speed control and RGB lighting. Built with [Tauri 2](https://tauri.app) (Rust backend, React + TypeScript UI).

Interface languages: English and Portuguese (switch in Settings). The app is a Linux port inspired by
[GustavoReset/ResetExplode](https://github.com/GustavoReset/ResetExplode) (MIT, Windows/WPF); see `LICENSE`.

## What it finds on its own

- **Fans:** scans every `hwmon` chip and NVIDIA GPU, tries the right motherboard driver for your vendor (`nct6775`, `it87`, …) and runs a test
  that maps each PWM control to the fan it actually moves.
- **RGB:** detects RGB-capable USB and I²C hardware and the graphics card model, reports what OpenRGB can control, and creates udev rules only
  for the devices it found.
- **Setup page:** checks CPU/disk sensors, Vulkan, SMART, the privileged helper and system tools, and offers a fix for each missing item.

## Other features

- **Profiles:** save the fan speeds and LED settings the app applied, reapply them with one click, and optionally reapply the lights at login
  (`coreforge --apply-profile`, no window and no password; fans need the system password, so they are only applied from the app).
- **Fan curves:** per-fan temperature→speed curves (CPU or GPU source, 2–8 points, presets) in Fans & LEDs → Curves. A privileged engine
  (`coreforge-helper curve-run`, one password) applies them while the app is open, ramps up fast and down slowly, goes to 100 % if the
  temperature cannot be read, and returns every fan to automatic when stopped or when the app closes.
- **Temperature alert:** warns on screen and with a desktop notification (`notify-send`) when the CPU or GPU stays above a limit (default 90 °C,
  changeable in Sensors). "Keep in tray on close" (also in Sensors) keeps the alerts running with the window closed; it is off by default because
  some desktops show no tray.
- **Laptops:** battery level, health, cycles and power draw, plus the power profile (`power-profiles-daemon`), in System → Power.
- **GPUs:** NVIDIA through NVML; AMD and Intel through sysfs (Intel shows no usage or VRAM).

## Privileges

Reading is unprivileged. Anything that writes to hardware (fan speed, loading drivers, installing a package, udev rules) goes through
`coreforge-helper`, a separate binary installed in `/usr/libexec/coreforge/` and run with `pkexec` (polkit policy in
`src-tauri/packaging/org.coreforge.helper.policy`). Fan control and curves ask for the password once (remembered for a few minutes); installing packages, udev rules, boot-time drivers and serial-port access ask every time. It accepts a closed set of actions with validated arguments and never runs free text.
In development it uses the helper built next to the app (`cargo build --bin coreforge-helper` runs automatically with `npm run tauri dev`).

## Development

Requirements: Node 20+, Rust stable, and the Tauri Linux dependencies (`libwebkit2gtk-4.1-dev`, `build-essential`, `libssl-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`).

```bash
npm install
npm run tauri dev        # app with hot reload
npm run dev              # UI only; open http://localhost:1420/?mock to use a fake backend
(cd src-tauri && cargo test)
```

GPU tests need Vulkan; ray tracing needs `VK_KHR_ray_query`. Optional tools: `smartmontools` (disk health), OpenRGB (LEDs).

## Safety notes

Fan control, kernel-module loading and I²C access can affect hardware. The app asks for confirmation and your system password for each
change, keeps a 25 % floor on manual fan speed, and can return every fan it changed to automatic. Use at your own risk (MIT, no warranty).
