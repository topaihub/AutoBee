//! 把档案库里的内容拼成核心层要的 [`Config`]。
//!
//! # 为什么每次请求都重新读一遍库
//!
//! 用户在设置页改了 Key 之后，下一次算路必须立即用新 Key。做进程内缓存就要
//! 维护「什么时候失效」，而 SQLite 开一次库是毫秒级的事。
//! 省下这点时间的代价是「改了不生效」这种最难查的问题。

use route_core::config::{Config, Settings};
use route_core::models::{Crs, ProviderId};
use route_core::tiles::DEFAULT_TILE_SOURCE;

use super::SettingsDto;
use super::store::Store;
use crate::error::AppError;

/// 组装一份完整的 [`Config`]。
///
/// # Errors
///
/// 档案库打不开或读不出来时返回错误。
pub fn load_config() -> Result<Config, AppError> {
    let store = Store::open()?;
    let list = store.list()?;

    let mut config = Config::default();
    for dto in &list {
        let Some(id) = ProviderId::parse(&dto.slug) else {
            continue;
        };
        if !dto.enabled {
            config.disabled.insert(id);
            continue;
        }
        if let Some(plain) = store.secret_of(&dto.slug)? {
            // 空串不算配置：那是用户填了又清空。
            if !plain.trim().is_empty() {
                config.keys.insert(id, plain);
            }
        }
        if let Some(priority) = dto.priority {
            config.priority.insert(id, priority);
        }
    }

    config.settings = load_settings_from(&store)?;
    Ok(config)
}

/// 读界面偏好。缺的键用默认值 —— 用户从旧版本升上来时那些键还不存在。
fn load_settings_from(store: &Store) -> Result<Settings, AppError> {
    let auto = store
        .pref("auto_provider")?
        .map(|v| v != "0")
        .unwrap_or(true);
    // 缺键时必须与 `Settings::default()` 一致（GCJ02）。全新安装时 prefs 没有
    // 这一行（schema.rs 的 SEED 只种服务商），而 schema.rs 那条迁移只改写
    // 已存在的 WGS84 行、键不存在时不做事。这里若退回 WGS84，底图（腾讯
    // GCJ-02）与路线就差一个坐标系：偏移几百米，不报错也不崩溃。
    let out_crs = store
        .pref("out_crs")?
        .and_then(|v| Crs::parse(&v))
        .unwrap_or(Settings::default().out_crs);
    let allow = store
        .pref("allow_cross_border")?
        .map(|v| v == "1")
        .unwrap_or(false);
    let tile_source = store
        .pref("tile_source")?
        .filter(|v| route_core::tiles::tile_url(v, 0, 0, 0).is_ok())
        .unwrap_or_else(|| DEFAULT_TILE_SOURCE.to_string());
    let default_providers = store
        .pref("default_providers")?
        .unwrap_or_default()
        .split(',')
        .filter_map(ProviderId::parse)
        .collect();

    Ok(Settings {
        auto_provider: auto,
        out_crs,
        allow_cross_border: allow,
        tile_source,
        default_providers,
    })
}

/// 读界面偏好（给设置页用）。
///
/// # Errors
///
/// 档案库读不出来时返回错误。
pub fn load_settings() -> Result<SettingsDto, AppError> {
    let store = Store::open()?;
    let settings = load_settings_from(&store)?;
    Ok(SettingsDto {
        auto_provider: settings.auto_provider,
        out_crs: format!("{:?}", settings.out_crs),
        allow_cross_border: settings.allow_cross_border,
        tile_source: settings.tile_source,
        default_providers: settings
            .default_providers
            .iter()
            .map(|p| p.as_str().to_string())
            .collect(),
        // 默认关：日志面板是排查用的，平时占掉一块界面。
        show_logs: store.pref("show_logs")?.map(|v| v == "1").unwrap_or(false),
    })
}

/// 写界面偏好。
///
/// # Errors
///
/// 档案库写不进去时返回错误。
pub fn save_settings(dto: &SettingsDto) -> Result<(), AppError> {
    let store = Store::open()?;
    store.set_pref("auto_provider", if dto.auto_provider { "1" } else { "0" })?;
    // 坐标系认不出时回落到默认坐标系并记一条警告：写进库一个非法值的话
    // 每次算路都会走读取侧的回落，而界面上显示的是他选的那个。
    // 回落值与 `Settings::default()` 同源，三处（默认、读、写）不许分叉。
    let crs = Crs::parse(&dto.out_crs).unwrap_or_else(|| {
        let fallback = Settings::default().out_crs;
        tracing::warn!(value = %dto.out_crs, ?fallback, "不认识的坐标系,回落到默认");
        fallback
    });
    store.set_pref("out_crs", &format!("{crs:?}"))?;
    store.set_pref(
        "allow_cross_border",
        if dto.allow_cross_border { "1" } else { "0" },
    )?;
    store.set_pref("tile_source", &dto.tile_source)?;
    store.set_pref("default_providers", &dto.default_providers.join(","))?;
    store.set_pref("show_logs", if dto.show_logs { "1" } else { "0" })?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::super::store::Store;
    use super::*;

    /// 这些用例只测纯函数部分（偏好 → Settings），不走真实磁盘。
    #[test]
    fn 缺键时退回默认值() {
        let store = Store::memory().unwrap();
        let settings = load_settings_from(&store).unwrap();
        assert!(settings.auto_provider);
        // 不写字面量：这一条的意义是「两处默认值不许分叉」，
        // 写死 GCJ02 的话改 `Settings::default()` 时这里不会响。
        assert_eq!(settings.out_crs, Settings::default().out_crs);
        assert!(!settings.allow_cross_border);
        assert_eq!(settings.tile_source, DEFAULT_TILE_SOURCE);
        assert!(settings.default_providers.is_empty());
    }

    #[test]
    fn 偏好写进去能读回来() {
        let store = Store::memory().unwrap();
        store.set_pref("auto_provider", "0").unwrap();
        store.set_pref("tile_source", "arcgis-street").unwrap();
        store.set_pref("default_providers", "amap,tencent").unwrap();

        let settings = load_settings_from(&store).unwrap();
        assert!(!settings.auto_provider);
        assert_eq!(settings.tile_source, "arcgis-street");
        assert_eq!(
            settings.default_providers,
            vec![ProviderId::Amap, ProviderId::Tencent]
        );
    }

    #[test]
    fn 不认识的瓦片源退回默认() {
        // 库里存了个拼不出 URL 的源名（换版本、手改 db）：
        // 用它会让整张地图空白。
        let store = Store::memory().unwrap();
        store.set_pref("tile_source", "nope").unwrap();
        assert_eq!(
            load_settings_from(&store).unwrap().tile_source,
            DEFAULT_TILE_SOURCE
        );
    }

    #[test]
    fn 非法坐标系退回默认坐标系() {
        // 库里存了个认不出的坐标系（换版本、手改 db）。退回值必须与
        // `Settings::default()` 同一个，否则底图与路线差一个坐标系:
        // 路线整体偏移几百米，而且不报错。
        let store = Store::memory().unwrap();
        store.set_pref("out_crs", "火星坐标").unwrap();
        assert_eq!(
            load_settings_from(&store).unwrap().out_crs,
            Settings::default().out_crs
        );
    }

    #[test]
    fn 固定链路里认不出的_slug_被跳过() {
        let store = Store::memory().unwrap();
        store
            .set_pref("default_providers", "amap,nope,tencent")
            .unwrap();
        assert_eq!(
            load_settings_from(&store).unwrap().default_providers,
            vec![ProviderId::Amap, ProviderId::Tencent]
        );
    }
}
