pub mod bios;
pub mod catalog;
pub mod commands;
pub mod covers;
pub mod dat;
pub mod identify;
pub mod library;
pub mod runtime;
pub mod saves;
pub mod settings;
pub mod systems;

use std::sync::Arc;

use tauri::Manager;

/// Inicia a aplicação Tauri e bloqueia até a janela principal ser fechada.
///
/// # Panics
///
/// Entra em pânico se o Tauri não conseguir inicializar o runtime ou a
/// janela (por exemplo, sem WebView disponível no sistema). Sem isso não
/// há interface para reportar o erro, então encerrar é o único caminho.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = app.path();
            let state = commands::AppState::new(
                paths.app_config_dir()?,
                paths.app_data_dir()?,
                paths.app_cache_dir()?,
            );
            app.manage(Arc::new(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_consoles,
            commands::get_library_dir,
            commands::set_library_dir,
            commands::scan_library,
            commands::game_cover,
            commands::launch_game,
            commands::get_controls,
            commands::set_controls,
            commands::delete_save,
            commands::export_game_save,
            commands::import_game_save,
            commands::get_console_settings,
            commands::set_console_settings,
            commands::get_bios,
            commands::set_bios,
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar a aplicação Tauri");
}
