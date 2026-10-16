//! 应用外壳：装 command、接 Aether、起窗口。
//!
//! 业务逻辑一行也不在这里 —— 地图在 `route-core`，大模型在 Aether。

pub mod commands;
mod host;

use std::sync::Arc;

use aether_observability::{LogConfig, LogLevel};
use autobee_shell::paths;

/// 启动应用。
pub fn run() {
    if let Err(error) = try_run() {
        // 也留一行 stderr：日志初始化本身失败时那条路径是空的。
        tracing::error!(%error, "启动失败");
        eprintln!("启动失败：{error}");
    }
}

fn try_run() -> Result<(), Box<dyn std::error::Error>> {
    // 数据跟着 exe 走（便携版，见 `paths`）；Aether 的各个库也放在这里。
    let root = paths::data_dir();
    std::fs::create_dir_all(&root)?;

    // 必须在 tauri::Builder 之前：插件 setup 期间的日志才不会丢。
    let logs = aether_observability::init(
        &root.join("logs"),
        LogConfig {
            file_stem: "autobee",
            default_level: LogLevel::INFO,
            env_var: Some("RUST_LOG"),
            ..LogConfig::default()
        },
        None,
    )?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), path = %root.display(), "AutoBee 启动");

    let conversations: Arc<dyn tauri_plugin_aether::ConversationStore> =
        Arc::new(aether_chat::SqliteStore::open(&root)?);
    let aether = tauri_plugin_aether::Builder::new(Arc::new(host::AutoBeeHost), logs)
        .providers(aether_providers::Store::open(&root)?)
        .chat(
            tauri_plugin_aether::ChatConfig::default(),
            Some(conversations),
        )
        .skills(aether_skills::Store::open(&root, &[])?)
        .mcp_client(aether_mcp_client::Store::open(&root)?)
        .build()?;

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(aether)
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
            commands::save_route_snapshot,
            commands::read_route_snapshot,
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}
