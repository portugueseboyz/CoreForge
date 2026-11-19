mod bench;
mod report_file;
mod disk;
mod fans;
mod games;
mod gpu;
mod gpu_bench;
mod gpu_sysfs;
mod gpu_rt;
mod gpu_stress;
mod leds;
mod metrics;
mod profiles;
mod net;
mod setup;
mod tray;
mod power;
mod curves;
mod stress;
mod tools;
mod sysinfo;

pub use fans::root_helper;

/// Roda trabalho de fundo convertendo pânico em erro, para que o estado "rodando" nunca fique preso.
pub(crate) fn guard<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        let m = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
        Err(format!("Falha interna inesperada ({m})"))
    })
}
pub use profiles::apply_start_profile_headless;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // No Wayland um aplicativo não escolhe a posição da janela (o compositor ignora), e o overlay precisa ir ao canto escolhido.
    // Usamos o X11 (XWayland) nessa sessão; COREFORGE_WAYLAND=1 mantém o Wayland nativo.
    #[cfg(target_os = "linux")]
    if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") && std::env::var_os("DISPLAY").is_some()
        && std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("COREFORGE_WAYLAND").is_none() {
        // SAFETY: ainda não há outras threads; é a primeira coisa que o programa faz antes de iniciar o GTK.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
    tauri::Builder::default()
        .setup(|app| { if let Err(e) = tray::init(app) { eprintln!("CoreForge: bandeja do sistema indisponível: {e}"); } Ok(()) })
        .on_window_event(tray::on_window_event)
        .manage(sysinfo::CpuPrev::default())
        .manage(stress::StressState::default())
        .manage(bench::BenchState::default())
        .manage(disk::DiskState::default())
        .manage(net::NetState::default())
        .manage(metrics::MetricsState::default())
        .manage(gpu_stress::GpuStressState::default())
        .manage(gpu_bench::GpuBenchState::default())
        .manage(gpu_rt::RtState::default())
        .manage(tools::InstallState::default())
        .manage(curves::CurveState::default())
        .invoke_handler(tauri::generate_handler![report_file::report_save, sysinfo::system_info, sysinfo::sensors, gpu::gpu_info, gpu::gpu_live,
            stress::stress_start, stress::stress_stop, stress::stress_status,
            bench::bench_start, bench::bench_cancel, bench::bench_status, bench::bench_image,
            disk::disk_mounts, disk::disk_bench_start, disk::disk_bench_cancel, disk::disk_bench_status, disk::disk_health,
            net::net_start, net::net_cancel, net::net_status, net::net_interfaces,
            gpu_stress::gpu_stress_start, gpu_stress::gpu_stress_stop, gpu_stress::gpu_stress_status,
            games::games_scan, games::game_launch, games::fps_table, metrics::live_metrics, metrics::overlay_toggle, metrics::mangohud_hint, metrics::notify, tray::tray_close_set, curves::curves_get, curves::curves_save, curves::curves_start, curves::curves_stop, curves::curves_status, power::power_info, power::power_profile_set,
            gpu_bench::gpu_bench_start, gpu_bench::gpu_bench_cancel, gpu_bench::gpu_bench_status,
            tools::startup_list, tools::startup_set, tools::serial_report, tools::serial_fix,
            tools::apps_list, tools::app_install, tools::app_install_status,
            gpu_rt::rt_start, gpu_rt::rt_cancel, gpu_rt::rt_status,
            fans::fans_report, fans::fan_apply, fans::fan_load_module, fans::fan_persist_module, fans::fan_overrides, fans::fan_restore_all, setup::system_check, setup::system_fix, fans::fan_scan_modules, fans::fan_map_test,
            leds::leds_status, leds::leds_set, leds::leds_discover, profiles::profiles_list, profiles::profile_capture, profiles::profile_save, profiles::profile_delete, profiles::profile_set_start, profiles::profile_apply, profiles::profile_autostart_get, profiles::profile_autostart_set, leds::leds_report, leds::leds_setup])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("CoreForge não conseguiu abrir a janela: {e}\nVerifique se há uma sessão gráfica (DISPLAY/WAYLAND_DISPLAY) e se o WebKitGTK está instalado.");
            std::process::exit(1);
        });
}

#[cfg(test)]
mod guard_tests {
    #[test]
    fn guard_turns_panic_into_error() {
        let r: Result<(), String> = super::guard(|| panic!("boom"));
        assert!(r.unwrap_err().contains("boom"));
        assert_eq!(super::guard(|| Ok(1)), Ok(1));
    }
}
