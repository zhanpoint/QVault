#[cfg(desktop)]
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(desktop)]
            let app = _app;
            #[cfg(desktop)]
            {
                let show_item = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
                let quit_item = MenuItem::with_id(app, "quit", "退出应用", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.ico"))?;

                TrayIconBuilder::new()
                    .tooltip("QVault")
                    .icon(icon)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(window) = tray.app_handle().get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    })
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .build(app)?;
            }

            Ok(())
        })
        .manage(commands::archive::ArchiveState::new())
        .manage(commands::archive::contacts::ContactsState::default())
        .manage(commands::qlogin::QLoginState::new())
        .manage(commands::qzone::RecycleAuthState::default())
        .manage(commands::qzone::QzonePageTokenState::default())
        .on_window_event(|_window, _event| {
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = _event {
                if _window.label() == "main" {
                    api.prevent_close();
                    let _ = _window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::qlogin::start_qr_login,
            commands::qlogin::poll_qr_login,
            commands::qlogin::get_login_status,
            commands::qlogin::logout_qzone,
            commands::qzone::open_recycle_password_window,
            commands::qzone::check_recycle_password,
            commands::qzone::close_recycle_password_window,
            commands::archive::gallery::list_recycle_resources,
            commands::archive::gallery::load_library_resource,
            commands::archive::start_feed_archive,
            commands::archive::get_archive_progress,
            commands::archive::cancel_feed_archive,
            commands::archive::abandon_feed_archive,
            commands::archive::list_archive_skips,
            commands::archive::retry_archive_skip,
            commands::archive::list_archived_feeds,
            commands::archive::load_archived_image,
            commands::archive::load_qzone_emoticon,
            commands::archive::load_archived_video,
            commands::archive::contacts::list_archive_contacts,
            commands::archive::contacts::list_contact_feeds,
            commands::archive::library::sync_qzone_library,
            commands::archive::library::list_qzone_library,
            commands::archive::library::list_qzone_library_years,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
