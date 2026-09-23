//! 档案库的读写。

use std::path::Path;

use rusqlite::Connection;

use super::crypto;
use super::{MapProviderDto, MapProviderInput};
use crate::error::AppError;
use crate::logging::mask_secret;

/// 档案库。
pub struct Store {
    conn: Connection,
}

impl Store {
    /// 打开默认位置的库（`%LOCALAPPDATA%\AutoBee\settings.db`）。
    ///
    /// # Errors
    ///
    /// 目录建不出来、文件打不开、建表失败时返回错误。
    pub fn open() -> Result<Self, AppError> {
        let dir = crate::paths::data_dir();
        std::fs::create_dir_all(&dir).map_err(|error| {
            AppError::new(
                "DB_ERROR",
                format!("建不了数据目录 {}：{error}", dir.display()),
                500,
            )
        })?;
        Self::open_at(&crate::paths::settings_db())
    }

    /// 打开指定路径的库。
    ///
    /// # Errors
    ///
    /// 文件打不开或建表失败时返回错误。
    pub fn open_at(path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(path).map_err(|error| {
            tracing::error!(%error, path = %path.display(), "档案库打不开");
            AppError::new(
                "DB_ERROR",
                format!(
                    "服务商档案库打不开：{error}。可以删掉 {} 后重启（档案要重填）。",
                    path.display()
                ),
                500,
            )
        })?;
        super::schema::prepare(&conn)?;
        Ok(Self { conn })
    }

    /// 一个只在内存里的库。测试用。
    ///
    /// # Errors
    ///
    /// 建表失败时返回错误。
    #[cfg(test)]
    pub(super) fn memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory()
            .map_err(|error| AppError::new("DB_ERROR", format!("内存库开不了：{error}"), 500))?;
        super::schema::prepare(&conn)?;
        Ok(Self { conn })
    }

    /// 列出全部档案，密钥已掩码。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn list(&self) -> Result<Vec<MapProviderDto>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT slug, name, is_preset, enabled, priority, api_key_enc
                 FROM map_providers ORDER BY is_preset DESC, sort_order, name",
            )
            .map_err(|error| sql("准备查询", &error))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Raw {
                    slug: row.get(0)?,
                    name: row.get(1)?,
                    is_preset: row.get::<_, i64>(2)? != 0,
                    enabled: row.get::<_, i64>(3)? != 0,
                    priority: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
                    cipher: row.get(5)?,
                })
            })
            .map_err(|error| sql("读档案", &error))?;

        let mut out = Vec::new();
        for row in rows {
            let raw = row.map_err(|error| sql("读一行档案", &error))?;
            out.push(dto_of(&raw));
        }
        Ok(out)
    }

    /// 取某家的明文密钥。
    ///
    /// 解不开（换了机器或账户）时返回 `Ok(None)` —— 那是「没填密钥」，
    /// 不是错误。报错会让设置页整个打不开，而用户那时最需要的是能重填。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn secret_of(&self, slug: &str) -> Result<Option<String>, AppError> {
        let cipher: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT api_key_enc FROM map_providers WHERE slug = ?1",
                [slug],
                |row| row.get(0),
            )
            .map_err(|error| sql("读密钥", &error))?;

        let Some(cipher) = cipher else {
            return Ok(None);
        };
        Ok(crypto::unprotect(&cipher))
    }

    /// 保存一条档案。
    ///
    /// `input.api_key` 为 `None` 表示「没动过」，保留库里那份密文 ——
    /// 前端拿到的是掩码，不改时原样提交会把掩码串写成新 Key。
    ///
    /// # Errors
    ///
    /// 预置档案不允许改名以外的破坏性操作、加密失败、SQL 失败时返回错误。
    pub fn save(&self, input: &MapProviderInput) -> Result<(), AppError> {
        let trimmed = input.api_key.as_deref().map(str::trim).unwrap_or_default();

        match input.api_key.as_deref() {
            None => {
                // 密钥不动，只改其余字段。
                self.conn
                    .execute(
                        "UPDATE map_providers SET name = ?1, enabled = ?2, priority = ?3
                         WHERE slug = ?4",
                        (
                            &input.name,
                            i64::from(input.enabled),
                            input.priority.map(i64::from),
                            &input.slug,
                        ),
                    )
                    .map_err(|error| sql("保存档案", &error))?;
            }
            Some(plain) => {
                let cipher = crypto::protect(plain.trim()).ok_or_else(|| {
                    AppError::new(
                        "ENCRYPT_FAILED",
                        "密钥加密失败，本次没有保存。请确认系统 DPAPI 可用（换机器或账户后需要重填）。",
                        500,
                    )
                })?;
                self.conn
                    .execute(
                        "UPDATE map_providers SET name = ?1, enabled = ?2, priority = ?3,
                         api_key_enc = ?4 WHERE slug = ?5",
                        (
                            &input.name,
                            i64::from(input.enabled),
                            input.priority.map(i64::from),
                            cipher,
                            &input.slug,
                        ),
                    )
                    .map_err(|error| sql("保存档案与密钥", &error))?;
                tracing::info!(
                    provider = %input.slug,
                    key = %mask_secret(trimmed),
                    "已保存服务商密钥"
                );
            }
        }
        Ok(())
    }

    /// 删掉一条档案。预置的不允许删。
    ///
    /// # Errors
    ///
    /// 试图删预置档案、或 SQL 失败时返回错误。
    pub fn delete(&self, slug: &str) -> Result<(), AppError> {
        let is_preset: Option<i64> = self
            .conn
            .query_row(
                "SELECT is_preset FROM map_providers WHERE slug = ?1",
                [slug],
                |row| row.get(0),
            )
            .ok();

        if is_preset == Some(1) {
            return Err(AppError::new(
                "PRESET_PROVIDER",
                "内置的六家不允许删除，停用即可。",
                400,
            ));
        }

        self.conn
            .execute("DELETE FROM map_providers WHERE slug = ?1", [slug])
            .map_err(|error| sql("删除档案", &error))?;
        Ok(())
    }

    /// 读一个偏好值。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn pref(&self, key: &str) -> Result<Option<String>, AppError> {
        match self
            .conn
            .query_row("SELECT value FROM prefs WHERE key = ?1", [key], |row| {
                row.get(0)
            }) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(sql("读偏好", &error)),
        }
    }

    /// 写一个偏好值。
    ///
    /// # Errors
    ///
    /// SQL 失败时返回错误。
    pub fn set_pref(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.conn
            .execute(
                "INSERT INTO prefs (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (key, value),
            )
            .map_err(|error| sql("写偏好", &error))?;
        Ok(())
    }
}

/// 库里的一行。
struct Raw {
    slug: String,
    name: String,
    is_preset: bool,
    enabled: bool,
    priority: Option<u32>,
    cipher: Option<Vec<u8>>,
}

/// 一行档案 + 注册表的静态信息 → 下发前端的 DTO。
fn dto_of(raw: &Raw) -> MapProviderDto {
    let id = route_core::models::ProviderId::parse(&raw.slug);
    let (coverage, coord_sys, capabilities, default_priority, key_apply_url) =
        match id.and_then(|i| route_core::registry::get_meta(i).ok()) {
            Some(meta) => (
                match meta.coverage {
                    route_core::registry::Coverage::Global => "全球".to_string(),
                    route_core::registry::Coverage::Countries(list) => list.join(" / "),
                },
                format!("{:?}", meta.coord_sys),
                caps_text(meta.capabilities),
                meta.priority,
                meta.key_apply_url.to_string(),
            ),
            None => (
                "未知".to_string(),
                "WGS84".to_string(),
                "未知".to_string(),
                99,
                String::new(),
            ),
        };

    let plain = raw
        .cipher
        .as_deref()
        .and_then(crypto::unprotect)
        .unwrap_or_default();

    MapProviderDto {
        slug: raw.slug.clone(),
        name: raw.name.clone(),
        is_preset: raw.is_preset,
        enabled: raw.enabled,
        priority: raw.priority,
        default_priority,
        coverage,
        coord_sys,
        capabilities,
        api_key_masked: if plain.is_empty() {
            String::new()
        } else {
            mask_secret(&plain)
        },
        implemented: id.is_some_and(route_core::factory::is_implemented),
        key_apply_url,
    }
}

fn caps_text(caps: route_core::registry::Capabilities) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if caps.transit {
        parts.push("公交");
        parts.push("地铁");
    }
    if caps.walking {
        parts.push("步行");
    }
    if parts.is_empty() {
        return "不支持".to_string();
    }
    parts.join(" · ")
}

fn sql(doing: &str, error: &rusqlite::Error) -> AppError {
    tracing::error!(%error, doing, "档案库操作失败");
    AppError::new("DB_ERROR", format!("服务商档案库{doing}失败：{error}"), 500)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    fn input(slug: &str, key: Option<&str>) -> MapProviderInput {
        MapProviderInput {
            slug: slug.to_string(),
            name: "测试".to_string(),
            enabled: true,
            priority: None,
            api_key: key.map(str::to_string),
        }
    }

    #[test]
    fn 列表里的密钥是掩码不是明文() {
        let store = Store::memory().unwrap();
        store
            .save(&input("tencent", Some("FAKE-KEY-FOR-TEST")))
            .unwrap();
        let list = store.list().unwrap();
        let tencent = list.iter().find(|p| p.slug == "tencent").unwrap();
        assert!(tencent.api_key_masked.contains('*'));
        assert!(!tencent.api_key_masked.contains("KEY-FOR"), "明文不能下发");
    }

    #[test]
    fn 存进去的密钥能读回明文() {
        let store = Store::memory().unwrap();
        store.save(&input("tencent", Some("secret-plain"))).unwrap();
        assert_eq!(
            store.secret_of("tencent").unwrap().as_deref(),
            Some("secret-plain")
        );
    }

    #[test]
    fn 没动过的密钥不会被掩码串覆盖() {
        // 这是 SecretInput 那个坑的后端防线。
        let store = Store::memory().unwrap();
        store
            .save(&input("tencent", Some("real-key-1234567890")))
            .unwrap();
        store.save(&input("tencent", None)).unwrap();
        assert_eq!(
            store.secret_of("tencent").unwrap().as_deref(),
            Some("real-key-1234567890")
        );
    }

    #[test]
    fn 清空密钥等于取消配置() {
        let store = Store::memory().unwrap();
        store
            .save(&input("tencent", Some("real-key-1234567890")))
            .unwrap();
        store.save(&input("tencent", Some(""))).unwrap();
        let plain = store.secret_of("tencent").unwrap().unwrap_or_default();
        assert!(plain.is_empty());
        let list = store.list().unwrap();
        assert!(
            list.iter()
                .find(|p| p.slug == "tencent")
                .unwrap()
                .api_key_masked
                .is_empty()
        );
    }

    #[test]
    fn 没填密钥时不报错() {
        let store = Store::memory().unwrap();
        assert_eq!(store.secret_of("amap").unwrap(), None);
    }

    #[test]
    fn 预置的不允许删() {
        let store = Store::memory().unwrap();
        let error = store.delete("tencent").unwrap_err();
        assert_eq!(error.code, "PRESET_PROVIDER");
    }

    #[test]
    fn 用户自建的可以删() {
        let store = Store::memory().unwrap();
        store
            .conn
            .execute(
                "INSERT INTO map_providers (slug, name, is_preset) VALUES ('mine', '自建', 0)",
                [],
            )
            .unwrap();
        assert!(store.delete("mine").is_ok());
        assert!(store.list().unwrap().iter().all(|p| p.slug != "mine"));
    }

    #[test]
    fn 停用的会带出来() {
        let store = Store::memory().unwrap();
        let mut disabled = input("tencent", Some("k-1234567890123"));
        disabled.enabled = false;
        store.save(&disabled).unwrap();
        assert!(
            !store
                .list()
                .unwrap()
                .iter()
                .find(|p| p.slug == "tencent")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn 偏好读写往返() {
        let store = Store::memory().unwrap();
        assert_eq!(store.pref("tile_source").unwrap(), None);
        store.set_pref("tile_source", "osm").unwrap();
        assert_eq!(store.pref("tile_source").unwrap().as_deref(), Some("osm"));
        store.set_pref("tile_source", "carto-dark").unwrap();
        assert_eq!(
            store.pref("tile_source").unwrap().as_deref(),
            Some("carto-dark")
        );
    }

    #[test]
    fn dto_带上注册表的静态信息() {
        let store = Store::memory().unwrap();
        let list = store.list().unwrap();
        let tencent = list.iter().find(|p| p.slug == "tencent").unwrap();
        assert_eq!(tencent.coverage, "CN");
        assert_eq!(tencent.coord_sys, "Gcj02");
        assert!(tencent.capabilities.contains("地铁"));
        assert!(tencent.implemented, "腾讯已实现");

        let amap = list.iter().find(|p| p.slug == "amap").unwrap();
        assert!(amap.implemented, "高德已实现");
    }
}
