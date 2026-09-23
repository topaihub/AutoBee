//! 应用外壳：装 command、起窗口。
//!
//! 业务逻辑一行也不在这里 —— 它在 `route-core`，被 command 调。

pub mod commands;
pub mod credentials;
pub mod error;
pub mod logging;
pub mod logs;
pub mod paths;

/// 启动应用。
pub fn run() {
    logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "AutoBee 启动");

    if let Err(error) = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_map_providers,
            commands::save_map_provider,
            commands::delete_map_provider,
            commands::test_map_provider,
            commands::get_settings,
            commands::save_settings,
            commands::suggest,
            commands::plan_route,
            commands::get_tile,
            commands::get_map_sdk_key,
            commands::get_life_circle,
            commands::read_logs,
        ])
        .run(tauri::generate_context!())
    {
        // 也留一行 stderr：日志初始化本身失败时那条路径是空的。
        tracing::error!(%error, "启动失败");
        eprintln!("启动失败：{error}");
    }
}
