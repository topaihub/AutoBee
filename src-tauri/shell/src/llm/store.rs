//! 大模型服务入口与模型选择的读写。
//!
//! 密钥走与 `map_providers` 同一条 DPAPI 通道（[`crate::credentials::crypto`]），密文
//! 直接放在 `llm_providers.api_key_enc`，不新建 credentials 表。「解不开一律当没填
//! 密钥」同样照搬：db 从别的机器拷来必然解不开，那时要能打开设置页重填而非整页报错。

use rusqlite::Connection;

use crate::credentials::crypto;
use crate::credentials::{
    LlmModelDto, LlmModelInput, LlmProtocol, LlmProviderDto, LlmProviderInput,
};
use crate::error::AppError;
use crate::logging::mask_secret;

/// Key 下发的是掩码，每个入口带上挑中的模型。
///
/// # Errors
///
/// 查询失败时返回错误。
pub fn list(conn: &Connection) -> Result<Vec<LlmProviderDto>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, base_url, protocol, enabled, api_key_enc
             FROM llm_providers ORDER BY sort_order, name",
        )
        .map_err(|error| sql("准备查询大模型入口", &error))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get::<_, Option<Vec<u8>>>(5)?,
            ))
        })
        .map_err(|error| sql("读大模型入口", &error))?;

    let mut out = Vec::new();
    for row in rows {
        let (id, name, base_url, protocol, enabled, cipher) =
            row.map_err(|error| sql("读一行大模型入口", &error))?;
        let models = models_of(conn, &id)?;
        out.push(LlmProviderDto {
            api_key_masked: cipher
                .as_deref()
                .and_then(crypto::unprotect)
                .map(|plain| mask_secret(&plain))
                .unwrap_or_default(),
            id,
            name,
            base_url,
            protocol: LlmProtocol::from_str_or_default(&protocol),
            enabled,
            models,
        });
    }
    Ok(out)
}

/// 解不开时返回 `Ok(None)` —— 那是「没填密钥」，不是错误（见模块文档）。
///
/// # Errors
///
/// 查询失败时返回错误；入口不存在时同样是 `Ok(None)`。
pub fn secret_of(conn: &Connection, id: &str) -> Result<Option<String>, AppError> {
    let cipher: Option<Vec<u8>> = match conn.query_row(
        "SELECT api_key_enc FROM llm_providers WHERE id = ?1",
        [id],
        |row| row.get(0),
    ) {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(sql("读大模型密钥", &error)),
    };

    Ok(cipher.as_deref().and_then(crypto::unprotect))
}

/// # Errors
///
/// 查询失败时返回错误；入口不存在时返回 `LLM_PROVIDER_NOT_FOUND`。
pub fn endpoint_of(conn: &Connection, id: &str) -> Result<(String, LlmProtocol), AppError> {
    match conn.query_row(
        "SELECT base_url, protocol FROM llm_providers WHERE id = ?1",
        [id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ) {
        Ok((base_url, protocol)) => Ok((base_url, LlmProtocol::from_str_or_default(&protocol))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(AppError::new(
            "LLM_PROVIDER_NOT_FOUND",
            "这个大模型服务入口不存在，可能已经被删掉了。",
            404,
        )),
        Err(error) => Err(sql("读大模型入口", &error)),
    }
}

/// `input.api_key` 为 `None` 或空串表示「没动过」，保留库里那份密文。
///
/// # Errors
///
/// 名称或 `base_url` 为空、加密失败、SQL 失败时返回错误。
pub fn save(conn: &Connection, input: &LlmProviderInput) -> Result<String, AppError> {
    let name = input.name.trim();
    let base_url = input.base_url.trim().trim_end_matches('/');
    if name.is_empty() {
        return Err(AppError::new("BAD_INPUT", "服务入口得有个名字。", 400));
    }
    if base_url.is_empty() {
        return Err(AppError::new(
            "BAD_INPUT",
            "base_url 不能为空，形如 https://example.com/v1。",
            400,
        ));
    }

    let plain = input.api_key.as_deref().map(str::trim).unwrap_or_default();
    let cipher = if plain.is_empty() {
        None
    } else {
        Some(crypto::protect(plain).ok_or_else(|| {
            AppError::new(
                "ENCRYPT_FAILED",
                "密钥加密失败，本次没有保存。请确认系统 DPAPI 可用（换机器或账户后需要重填）。",
                500,
            )
        })?)
    };

    let Some(id) = input.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        let id = new_id();
        let order: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM llm_providers",
                [],
                |row| row.get(0),
            )
            .map_err(|error| sql("读排序位", &error))?;
        conn.execute(
            "INSERT INTO llm_providers (id, name, base_url, protocol, api_key_enc, enabled, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                &id,
                name,
                base_url,
                input.protocol.as_str(),
                cipher.as_deref(),
                i64::from(input.enabled),
                order,
            ),
        )
        .map_err(|error| sql("新建大模型入口", &error))?;
        log_saved(&id, plain);
        return Ok(id);
    };

    // 空 Key 时整列不写：NULL 与「清空」在 SQL 里无法区分，只能分两条语句。
    match cipher {
        None => conn
            .execute(
                "UPDATE llm_providers SET name = ?1, base_url = ?2, protocol = ?3, enabled = ?4
                 WHERE id = ?5",
                (
                    name,
                    base_url,
                    input.protocol.as_str(),
                    i64::from(input.enabled),
                    id,
                ),
            )
            .map_err(|error| sql("保存大模型入口", &error))?,
        Some(cipher) => conn
            .execute(
                "UPDATE llm_providers SET name = ?1, base_url = ?2, protocol = ?3, enabled = ?4,
                 api_key_enc = ?5 WHERE id = ?6",
                (
                    name,
                    base_url,
                    input.protocol.as_str(),
                    i64::from(input.enabled),
                    cipher,
                    id,
                ),
            )
            .map_err(|error| sql("保存大模型入口与密钥", &error))?,
    };
    log_saved(id, plain);
    Ok(id.to_owned())
}

/// 模型行由外键级联删掉（见 `schema::prepare` 里的 `PRAGMA`）。
///
/// # Errors
///
/// SQL 失败时返回错误。
pub fn delete(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM llm_providers WHERE id = ?1", [id])
        .map_err(|error| sql("删除大模型入口", &error))?;
    Ok(())
}

/// 覆盖而不是追加：界面上那是一组多选框，用户取消勾选的必须真的消失。
///
/// # Errors
///
/// SQL 失败时返回错误。
pub fn save_models(
    conn: &Connection,
    provider_id: &str,
    models: &[LlmModelInput],
) -> Result<(), AppError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| sql("开事务", &error))?;
    tx.execute(
        "DELETE FROM llm_models WHERE provider_id = ?1",
        [provider_id],
    )
    .map_err(|error| sql("清空旧模型选择", &error))?;

    for (index, model) in models.iter().enumerate() {
        let name = model.model.trim();
        if name.is_empty() {
            continue;
        }
        tx.execute(
            "INSERT OR REPLACE INTO llm_models (provider_id, model, alias, tools_ok, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                provider_id,
                name,
                model
                    .alias
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
                i64::from(model.tools_ok),
                i64::try_from(index).unwrap_or(i64::MAX),
            ),
        )
        .map_err(|error| sql("写入模型选择", &error))?;
    }

    tx.commit().map_err(|error| sql("提交模型选择", &error))?;
    Ok(())
}

fn models_of(conn: &Connection, provider_id: &str) -> Result<Vec<LlmModelDto>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT model, alias, tools_ok FROM llm_models
             WHERE provider_id = ?1 ORDER BY sort_order, model",
        )
        .map_err(|error| sql("准备查询模型", &error))?;

    let rows = stmt
        .query_map([provider_id], |row| {
            Ok(LlmModelDto {
                model: row.get(0)?,
                alias: row.get(1)?,
                tools_ok: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(|error| sql("读模型选择", &error))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|error| sql("读一行模型选择", &error))?);
    }
    Ok(out)
}

fn log_saved(id: &str, plain: &str) {
    if plain.is_empty() {
        return;
    }
    tracing::info!(provider = %id, key = %mask_secret(plain), "已保存大模型入口密钥");
}

/// 不引 `uuid`：这里只要求「同一个库里不重复」，128 位随机 + 时间前缀已经够。
fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&nanos, &mut hasher);
    std::hash::Hash::hash(&std::thread::current().id(), &mut hasher);
    let salt = std::hash::Hasher::finish(&hasher);
    format!("{nanos:x}-{salt:x}")
}

fn sql(doing: &str, error: &rusqlite::Error) -> AppError {
    tracing::error!(%error, doing, "大模型配置库操作失败");
    AppError::new("DB_ERROR", format!("大模型配置{doing}失败：{error}"), 500)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::credentials::schema::prepare(&conn).unwrap();
        conn
    }

    fn input(id: Option<&str>, key: Option<&str>) -> LlmProviderInput {
        LlmProviderInput {
            id: id.map(str::to_owned),
            name: "某中转".into(),
            base_url: "https://api.example.com/v1".into(),
            protocol: LlmProtocol::OpenaiChat,
            enabled: true,
            api_key: key.map(str::to_owned),
        }
    }

    #[test]
    fn 下发的是掩码而不是明文() {
        let conn = memory();
        let id = save(&conn, &input(None, Some("sk-0123456789abcdef"))).unwrap();

        let dto = list(&conn).unwrap();
        let one = dto.iter().find(|p| p.id == id).unwrap();
        assert!(!one.api_key_masked.contains("0123456789"), "明文不许下发");
        if cfg!(windows) {
            // 非 Windows 下 protect 返回 None（不存明文），掩码为空是对的。
            assert_eq!(one.api_key_masked, "sk-0********cdef");
        }
    }

    #[test]
    fn 空_key_的保存不覆盖原_key() {
        let conn = memory();
        let id = save(&conn, &input(None, Some("sk-0123456789abcdef"))).unwrap();
        let before = secret_of(&conn, &id).unwrap();

        // 用户只改名字，Key 框里是掩码串，提交时前端传空。
        let mut again = input(Some(&id), Some("   "));
        again.name = "改了名字".into();
        save(&conn, &again).unwrap();

        assert_eq!(secret_of(&conn, &id).unwrap(), before, "原 Key 必须留着");
        let dto = list(&conn).unwrap();
        assert_eq!(dto.iter().find(|p| p.id == id).unwrap().name, "改了名字");
    }

    #[test]
    fn 解不开的密文当没填密钥() {
        let conn = memory();
        let id = save(&conn, &input(None, None)).unwrap();
        // 模拟从别的机器/账户拷过来的库：密文在，但 DPAPI 解不开。
        conn.execute(
            "UPDATE llm_providers SET api_key_enc = ?1 WHERE id = ?2",
            (b"\x01\x02\x03not-a-valid-blob".as_slice(), &id),
        )
        .unwrap();

        assert_eq!(secret_of(&conn, &id).unwrap(), None);
        let dto = list(&conn).unwrap();
        assert_eq!(
            dto.iter().find(|p| p.id == id).unwrap().api_key_masked,
            "",
            "解不开要当没填，而不是让整个设置页报错"
        );
    }

    #[test]
    fn 取消勾选的模型真的消失() {
        let conn = memory();
        let id = save(&conn, &input(None, None)).unwrap();
        let three = |names: &[&str]| -> Vec<LlmModelInput> {
            names
                .iter()
                .map(|m| LlmModelInput {
                    model: (*m).to_owned(),
                    alias: None,
                    tools_ok: true,
                })
                .collect()
        };

        save_models(&conn, &id, &three(&["a", "b", "c"])).unwrap();
        save_models(&conn, &id, &three(&["b"])).unwrap();

        let dto = list(&conn).unwrap();
        let models = &dto.iter().find(|p| p.id == id).unwrap().models;
        assert_eq!(
            models.iter().map(|m| m.model.as_str()).collect::<Vec<_>>(),
            ["b"],
            "保存是覆盖，不是追加"
        );
    }

    #[test]
    fn base_url_末尾斜杠被去掉() {
        // 留着会拼出 https://x/v1//models，有的中转商直接 404。
        let conn = memory();
        let mut with_slash = input(None, None);
        with_slash.base_url = "https://api.example.com/v1/".into();
        let id = save(&conn, &with_slash).unwrap();

        assert_eq!(
            endpoint_of(&conn, &id).unwrap().0,
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn 名字或_base_url_为空时拒绝保存() {
        let conn = memory();
        let mut empty_name = input(None, None);
        empty_name.name = "  ".into();
        assert_eq!(save(&conn, &empty_name).unwrap_err().code, "BAD_INPUT");

        let mut empty_url = input(None, None);
        empty_url.base_url = String::new();
        assert_eq!(save(&conn, &empty_url).unwrap_err().code, "BAD_INPUT");
    }

    #[test]
    fn 不存在的入口报_not_found_而不是默认值() {
        let conn = memory();
        assert_eq!(
            endpoint_of(&conn, "没有这个").unwrap_err().code,
            "LLM_PROVIDER_NOT_FOUND"
        );
    }

    #[test]
    fn 认不出的协议回落到_openai_chat() {
        // 库里可能留着更新版本写的协议名，那时设置页仍要能打开。
        let conn = memory();
        let id = save(&conn, &input(None, None)).unwrap();
        conn.execute(
            "UPDATE llm_providers SET protocol = 'future-proto' WHERE id = ?1",
            [&id],
        )
        .unwrap();

        let dto = list(&conn).unwrap();
        assert_eq!(
            dto.iter().find(|p| p.id == id).unwrap().protocol,
            LlmProtocol::OpenaiChat
        );
    }
}
