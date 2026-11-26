//! AgentMeter — Tauri shell around `agentmeter-core`.

mod apply;
mod commands;
mod fx;
mod hotkeys;
mod http;
mod i18n;
mod menu;
mod setup;
mod state;
mod tray_icon;
mod updater;
mod windows;
mod workers;

use state::{AppState, UsageCmd};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_global_shortcut::ShortcutState;

/// Save settings and the usage cache (before quitting or installing an update).
pub fn flush(app: &AppHandle) {
    let st = app.state::<AppState>();
    st.quitting.store(true, Ordering::SeqCst);
    let (tx, rx) = std::sync::mpsc::channel();
    st.send_usage(UsageCmd::Stop(tx));
    let _ = rx.recv_timeout(Duration::from_secs(2));
    st.save_settings();
}

/// Flush everything and exit.
pub fn quit(app: &AppHandle) {
    flush(app);
    app.exit(0);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::open_dashboard(app, None)
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        hotkeys::on_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .invoke_handler(commands::handler())
        .on_menu_event(|app, e| menu::handle_menu(app, e.id().as_ref()))
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            let (state, first_run) = AppState::load(data_dir);
            app.manage(state);

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            if let Err(e) = menu::create_tray(&handle) {
                eprintln!("[tray] {e}");
            }
            if app.state::<AppState>().settings().widget_visible {
                windows::create_widget(&handle)?;
            }
            hotkeys::register(&handle);
            // An update can move the exe (portable copy -> installed copy): point the
            // "start with the system" entry at the one that is running now.
            if updater::RELEASE_BUILD && app.state::<AppState>().settings().launch_at_login {
                use tauri_plugin_autostart::ManagerExt;
                let _ = handle.autolaunch().enable();
            }
            if first_run {
                windows::open_dashboard(&handle, Some("overview".into()));
            }
            workers::spawn_usage(handle.clone());
            workers::spawn_system(handle.clone());
            // setup runs on the main thread, whose message loop the hook needs
            #[cfg(target_os = "windows")]
            windows::zorder::install();
            updater::spawn(handle.clone());
            fx::spawn(handle.clone());
            workers::spawn_keeper(handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building AgentMeter")
        .run(|app, event| {
            // Keep living in the tray when every window is closed.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() && !app.state::<AppState>().quitting.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
