use std::path::Path;

use rusqlite::Connection;

use super::crypto;
use super::{MapProviderDto, MapProviderInput};
use crate::error::AppError;
use crate::logging::mask_secret;

pub struct Store {
    conn: Connection,
}

impl Store {
    /// 打开默认位置的库（`<exe 目录>\data\settings.db`，见 [`crate::paths::data_dir`]）。
    /// 这里**不是** `%LOCALAPPDATA%`：便携版要求复制整个目录就能迁移，数据必须跟着 exe 走。
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

    #[cfg(test)]
    pub(super) fn memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory()
            .map_err(|error| AppError::new("DB_ERROR", format!("内存库开不了：{error}"), 500))?;
        super::schema::prepare(&conn)?;
        Ok(Self { conn })
    }

    /// 大模型配置（[`crate::llm::store`]）与地图档案共用同一个 `settings.db`,必须共用
    /// **同一个连接**:另开一条的现象不是报错，而是并发写时偶发 `database is locked`。
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
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
                "SELECT slug, name, is_preset, enabled, priority, api_key_enc, browser_key_enc
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
                    browser_cipher: row.get(6)?,
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

    /// 解不开（换了机器或账户）时返回 `Ok(None)`:那是「没填密钥」而不是错误,报错会让设置页整个打不开。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn secret_of(&self, slug: &str) -> Result<Option<String>, AppError> {
        self.secret_column(slug, "api_key_enc")
    }

    /// # Errors
    ///
    /// 查询失败时返回错误。
    pub fn browser_secret_of(&self, slug: &str) -> Result<Option<String>, AppError> {
        self.secret_column(slug, "browser_key_enc")
    }

    /// `column` 只来自本模块的字面量,见 [`Self::save_secret`]。
    fn secret_column(&self, slug: &str, column: &'static str) -> Result<Option<String>, AppError> {
        let cipher: Option<Vec<u8>> = self
            .conn
            .query_row(
                &format!("SELECT {column} FROM map_providers WHERE slug = ?1"),
                [slug],
                |row| row.get(0),
            )
            .map_err(|error| sql("读密钥", &error))?;

        let Some(cipher) = cipher else {
            return Ok(None);
        };
        Ok(crypto::unprotect(&cipher))
    }

    /// 两个密钥字段同一条约定:`None` = 「没动过」,保留库里那份密文 —— 前端拿到的是掩码,原样提交会把掩码串写成新 Key。
    /// 两把各有动/不动两种状态,按组合写整条 UPDATE 要四个分支,所以拆成先更新非密字段再各自按需更新密文列。
    ///
    /// # Errors
    ///
    /// 加密失败、SQL 失败时返回错误。
    pub fn save(&self, input: &MapProviderInput) -> Result<(), AppError> {
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

        self.save_secret(&input.slug, "api_key_enc", input.api_key.as_deref())?;
        self.save_secret(&input.slug, "browser_key_enc", input.browser_key.as_deref())?;
        Ok(())
    }

    /// `plain` 为 `None` 时什么都不做,见 [`Self::save`]。
    ///
    /// 列名不能参数绑定只能拼进 SQL —— `column` 必须是本模块的字面量,不能是调用方给的字符串。
    fn save_secret(
        &self,
        slug: &str,
        column: &'static str,
        plain: Option<&str>,
    ) -> Result<(), AppError> {
        let Some(plain) = plain else {
            return Ok(());
        };
        let trimmed = plain.trim();
        let cipher = crypto::protect(trimmed).ok_or_else(|| {
            AppError::new(
                "ENCRYPT_FAILED",
                "密钥加密失败，本次没有保存。请确认系统 DPAPI 可用（换机器或账户后需要重填）。",
                500,
            )
        })?;
        self.conn
            .execute(
                &format!("UPDATE map_providers SET {column} = ?1 WHERE slug = ?2"),
                (cipher, slug),
            )
            .map_err(|error| sql("保存密钥", &error))?;
        tracing::info!(
            provider = %slug,
            column,
            key = %mask_secret(trimmed),
            "已保存服务商密钥"
        );
        Ok(())
    }

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

struct Raw {
    slug: String,
    name: String,
    is_preset: bool,
    enabled: bool,
    priority: Option<u32>,
    cipher: Option<Vec<u8>>,
    browser_cipher: Option<Vec<u8>>,
}

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
    let browser_plain = raw
        .browser_cipher
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
        browser_key_masked: if browser_plain.is_empty() {
            String::new()
        } else {
            mask_secret(&browser_plain)
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
            browser_key: None,
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

    /// 防 DPAPI 退化成空实现(`crypto.rs` 里 `#[cfg(not(windows))]` 的桩):那时 `secret_of` 返回 `None`,而只用 `memory()` 的测试照样全绿。
    #[cfg(windows)]
    #[test]
    fn 真实文件路径上密钥能加密落盘再读回() {
        let path =
            std::env::temp_dir().join(format!("autobee-store-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open_at(&path).unwrap();

        let plain = "FAKEtest1234567890TEST";
        store
            .save(&MapProviderInput {
                slug: "tencent".into(),
                name: "腾讯位置服务".into(),
                enabled: true,
                priority: None,
                api_key: Some(plain.into()),
                browser_key: None,
            })
            .unwrap();

        assert_eq!(
            store.secret_of("tencent").unwrap().as_deref(),
            Some(plain),
            "读回的明文必须与存进去的一致 —— 不一致说明 DPAPI 没在工作"
        );

        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes.windows(plain.len()).any(|w| w == plain.as_bytes()),
            "db 文件里搜到了明文 Key"
        );

        let _ = std::fs::remove_file(&path);
    }
}
