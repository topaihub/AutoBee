//! 应用外壳：装 command、起窗口。
//!
//! 业务逻辑一行也不在这里 —— 它在 `route-core`，被 command 调。

pub mod commands;

/// 启动应用。
pub fn run() {
    autobee_shell::logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "AutoBee 启动");

    if let Err(error) = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // 正在跑的流的取消标志表。`llm_chat_start` 登记，`llm_chat_cancel` 置位。
        .manage(commands::llm::StreamRegistry::default())
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
            commands::list_llm_providers,
            commands::save_llm_provider,
            commands::delete_llm_provider,
            commands::fetch_llm_models,
            commands::save_llm_models,
            commands::list_conversations,
            commands::read_conversation,
            commands::read_conversation_route,
            commands::delete_conversation,
            commands::llm_chat_start,
            commands::llm_chat_cancel,
            commands::list_mcp_servers,
            commands::save_mcp_server,
            commands::delete_mcp_server,
            commands::list_mcp_tools,
            commands::list_skills,
            commands::import_skill_package,
            commands::set_skill_enabled,
            commands::set_skill_mcp_servers,
            commands::delete_skill,
        ])
        .run(tauri::generate_context!())
    {
        // 也留一行 stderr：日志初始化本身失败时那条路径是空的。
        tracing::error!(%error, "启动失败");
        eprintln!("启动失败：{error}");
    }
}
