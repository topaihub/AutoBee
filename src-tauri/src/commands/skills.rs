//! 用户侧 Skill 的 command：导入离线包、启停、绑定 MCP server、删除。
//!
//! ZIP 解析与 SQLite 事务在 `autobee_shell::llm::skills`；本层只处理 Tauri 的
//! 参数边界与阻塞线程，避免 WebView 线程读压缩包或数据库时卡住。

use autobee_shell::credentials::store::Store;
use autobee_shell::error::AppError;
use autobee_shell::llm::skills::{self, SkillDto};

/// 后台任务本身崩了（panic）时的兜底。
fn join_failed(detail: &str) -> AppError {
    AppError::new("INTERNAL", format!("后台任务失败：{detail}"), 500)
}

/// 列出已安装 Skill。`instructions` 是已校验的纯文本，供用户在设置页查看。
///
/// # Errors
///
/// 配置库读不出来时返回错误。
#[tauri::command]
pub async fn list_skills() -> Result<Vec<SkillDto>, AppError> {
    tauri::async_runtime::spawn_blocking(|| skills::list(Store::open()?.conn()))
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 导入一个本地 `.zip` Skill 包。
///
/// 包只允许根目录 `skill.json` 与 `SKILL.md`，不会解压或执行包内文件；新声明的 MCP
/// server 默认关闭，用户必须在设置页明确开启。
///
/// # Errors
///
/// ZIP、manifest、大小、路径、疑似凭据、MCP 名称冲突或 SQL 失败时返回错误且不留半条数据。
#[tauri::command]
pub async fn import_skill_package(bytes: Vec<u8>) -> Result<Vec<SkillDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let package = skills::parse_package(&bytes)?;
        let store = Store::open()?;
        skills::install(store.conn(), &package)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 启用或停用一个 Skill。最多同时启用三个，避免挤占模型上下文。
///
/// # Errors
///
/// Skill 不存在、超过启用上限或 SQL 失败时返回错误。
#[tauri::command]
pub async fn set_skill_enabled(id: String, enabled: bool) -> Result<Vec<SkillDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        skills::set_enabled(Store::open()?.conn(), &id, enabled)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 覆盖一个 Skill 的 MCP server 绑定。server 仍由 MCP 设置区管理，删除 Skill 不会删 server。
///
/// # Errors
///
/// Skill 或 server 不存在、SQL 失败时返回错误。
#[tauri::command]
pub async fn set_skill_mcp_servers(
    id: String,
    server_ids: Vec<String>,
) -> Result<Vec<SkillDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        skills::set_bindings(Store::open()?.conn(), &id, &server_ids)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 删除一个 Skill 与其绑定关系，不删除共享 MCP server。
///
/// # Errors
///
/// Skill 不存在或 SQL 失败时返回错误。
#[tauri::command]
pub async fn delete_skill(id: String) -> Result<Vec<SkillDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || skills::delete(Store::open()?.conn(), &id))
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}
