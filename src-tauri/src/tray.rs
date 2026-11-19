//! Ícone na bandeja do sistema. Fechar a janela só a esconde se o usuário ligou "manter na bandeja";
//! por padrão fechar encerra o app, porque em alguns desktops (GNOME puro) a bandeja não aparece.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Window, WindowEvent};

static KEEP: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn tray_close_set(on: bool) { KEEP.store(on, Ordering::Relaxed); }

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") { let _ = w.show(); let _ = w.unminimize(); let _ = w.set_focus(); }
}

pub fn init(app: &App) -> tauri::Result<()> {
    let pt = std::env::var("LANG").is_ok_and(|l| l.starts_with("pt"));
    let open = MenuItem::with_id(app, "show", if pt { "Abrir CoreForge" } else { "Open CoreForge" }, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", if pt { "Sair" } else { "Quit" }, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut b = TrayIconBuilder::new().tooltip("CoreForge").menu(&menu).show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() { "show" => show(app), "quit" => app.exit(0), _ => {} })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e { show(t.app_handle()); }
        });
    if let Some(icon) = app.default_window_icon() { b = b.icon(icon.clone()); }
    b.build(app)?;
    Ok(())
}

pub fn on_window_event(w: &Window, e: &WindowEvent) {
    if w.label() == "main" && KEEP.load(Ordering::Relaxed) && let WindowEvent::CloseRequested { api, .. } = e {
        api.prevent_close();
        let _ = w.hide();
    }
}
