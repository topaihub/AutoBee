//! 大模型服务入口的 command：入口 CRUD、拉模型列表、保存模型选择。
//!
//! 通用约定（`spawn_blocking`、每次重新读配置）见父模块 [`crate::commands`]。

use autobee_shell::credentials::store::Store;
use autobee_shell::credentials::{LlmModelInput, LlmProviderDto, LlmProviderInput};
use autobee_shell::error::AppError;
use autobee_shell::llm::{probe, store as llm_store};

/// 后台任务本身崩了（panic）时的兜底。
fn join_failed(detail: &str) -> AppError {
    AppError::new("INTERNAL", format!("后台任务失败：{detail}"), 500)
}

/// 列出全部大模型服务入口（Key 已掩码，各自带上挑中的模型）。
///
/// # Errors
///
/// 配置库读不出来时返回错误。
#[tauri::command]
pub async fn list_llm_providers() -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(|| llm_store::list(Store::open()?.conn()))
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 保存一个入口。空 Key 表示不改动原 Key。
///
/// # Errors
///
/// 名称或 `base_url` 为空、密钥加密失败、SQL 失败时返回错误。
#[tauri::command]
pub async fn save_llm_provider(input: LlmProviderInput) -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        llm_store::save(store.conn(), &input)?;
        llm_store::list(store.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 删掉一个入口。它挑中的模型行由外键级联删掉。
///
/// # Errors
///
/// SQL 失败时返回错误。
#[tauri::command]
pub async fn delete_llm_provider(id: String) -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        llm_store::delete(store.conn(), &id)?;
        tracing::info!(provider = %id, "已删除大模型服务入口");
        llm_store::list(store.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 拉某个入口可用的模型列表。**不落库** —— 结果只供当次挑选。
///
/// # Errors
///
/// 入口不存在、没填 Key、网络或鉴权失败、返回空列表时返回错误。
#[tauri::command]
pub async fn fetch_llm_models(id: String) -> Result<Vec<String>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        let (base_url, protocol) = llm_store::endpoint_of(store.conn(), &id)?;
        let key = llm_store::secret_of(store.conn(), &id)?;
        probe::fetch_models(&base_url, protocol, key.as_deref())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 覆盖某个入口挑中的模型列表。
///
/// 是覆盖而不是追加：界面上那是一组多选框，取消勾选的必须真的消失。
///
/// # Errors
///
/// SQL 失败时返回错误。
#[tauri::command]
pub async fn save_llm_models(
    provider_id: String,
    models: Vec<LlmModelInput>,
) -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        llm_store::save_models(store.conn(), &provider_id, &models)?;
        llm_store::list(store.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}
