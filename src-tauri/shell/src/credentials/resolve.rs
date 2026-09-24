//! 把档案库里的内容拼成核心层要的 [`Config`]。每次请求都重新读一遍库：改了 Key
//! 之后下一次算路必须立即生效，而做进程内缓存就要维护「什么时候失效」。

use route_core::config::{Config, Settings};
use route_core::models::{Crs, ProviderId};
use route_core::tiles::DEFAULT_TILE_SOURCE;

use super::store::Store;
use super::{DEFAULT_LONG_WALK_METERS, DEFAULT_MANY_TRANSFERS, SettingsDto};
use crate::error::AppError;

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

/// 取百度**浏览器端** AK 的明文,供 JS API GL 用。
///
/// 取的是 `browser_key_enc` 列,不是服务端那把([`load_config`] 用的
/// `api_key_enc`)。两把分开是百度控制台的应用类型约束:浏览器端 AK 调不了
/// Web 服务,服务端 AK 加载不了 JS API。
///
/// 明文必须离开进程 —— SDK 脚本 URL 里带 `ak=明文`,由浏览器直接发给百度的域,
/// 没有代理掉的办法。泄露是已知代价,缓解手段是给这个 AK 配 Referer 白名单
/// (服务端那把不带 Referer,所以两把分开之后才能各配各的限制)。
///
/// # Errors
///
/// 档案库打不开或读不出来时返回错误。
///
/// 百度没配 Key、被用户停用、或 Key 是空串时返回 `Ok(None)` —— 那不是故障,
/// 调用方据此显示「请先在设置里填百度 AK」而不是报错。
pub fn baidu_sdk_key() -> Result<Option<String>, AppError> {
    let store = Store::open()?;
    baidu_sdk_key_from(&store)
}

fn baidu_sdk_key_from(store: &Store) -> Result<Option<String>, AppError> {
    let slug = ProviderId::Baidu.as_str();
    // 停用的厂商不给 Key。`list()` 已经带了 enabled,不必另查一次。
    let enabled = store
        .list()?
        .iter()
        .any(|dto| dto.slug == slug && dto.enabled);
    if !enabled {
        return Ok(None);
    }
    Ok(store
        .browser_secret_of(slug)?
        .filter(|plain| !plain.trim().is_empty()))
}

/// 缺的键用默认值 —— 用户从旧版本升上来时那些键还不存在。
fn load_settings_from(store: &Store) -> Result<Settings, AppError> {
    let auto = store
        .pref("auto_provider")?
        .map(|v| v != "0")
        .unwrap_or(true);
    // 缺键时必须与 `Settings::default()` 一致（GCJ02）：若退回 WGS84，底图（腾讯 GCJ-02）与路线就差一个坐标系，偏移几百米，不报错也不崩溃。
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
        show_logs: store.pref("show_logs")?.map(|v| v == "1").unwrap_or(false),
        // 缺键与非法值都退回 §4.2 的保守默认（500 米 / 2 次）。前端 `DEFAULT_LABEL_THRESHOLDS` 是同一组数字,改这里要一起改。
        long_walk_meters: store
            .pref("long_walk_meters")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_LONG_WALK_METERS),
        many_transfers: store
            .pref("many_transfers")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MANY_TRANSFERS),
    })
}

/// # Errors
///
/// 档案库写不进去时返回错误。
pub fn save_settings(dto: &SettingsDto) -> Result<(), AppError> {
    let store = Store::open()?;
    store.set_pref("auto_provider", if dto.auto_provider { "1" } else { "0" })?;
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
    store.set_pref("long_walk_meters", &dto.long_walk_meters.to_string())?;
    store.set_pref("many_transfers", &dto.many_transfers.to_string())?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::super::MapProviderInput;
    use super::super::store::Store;
    use super::*;

    #[test]
    fn 缺键时退回默认值() {
        let store = Store::memory().unwrap();
        let settings = load_settings_from(&store).unwrap();
        assert!(settings.auto_provider);
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
        // 拼不出 URL 的源名（换版本、手改 db）会让整张地图空白。
        let store = Store::memory().unwrap();
        store.set_pref("tile_source", "nope").unwrap();
        assert_eq!(
            load_settings_from(&store).unwrap().tile_source,
            DEFAULT_TILE_SOURCE
        );
    }

    #[test]
    fn 非法坐标系退回默认坐标系() {
        // 退回值与 `Settings::default()` 分叉的后果见 `load_settings_from`:偏移几百米且不报错。
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

    #[test]
    fn 没填百度_ak_时给_none_而不是报错() {
        // 报错会让地图直接进错误态,而正确的表现是提示用户去设置里填。
        let store = Store::memory().unwrap();
        assert_eq!(baidu_sdk_key_from(&store).unwrap(), None);
    }

    #[test]
    fn 填了百度_ak_时拿到明文而不是掩码() {
        // 拿到掩码串的话地图会加载一个 ak=FAKE****TEST 的脚本,百度判 AK 有误。
        let store = Store::memory().unwrap();
        store
            .save(&MapProviderInput {
                slug: "baidu".to_string(),
                name: "百度地图".to_string(),
                enabled: true,
                priority: None,
                api_key: None,
                browser_key: Some("Cz2b5EGUiLoBgSXaYcsdEo9nG3V2z26a".to_string()),
            })
            .unwrap();
        assert_eq!(
            baidu_sdk_key_from(&store).unwrap().as_deref(),
            Some("Cz2b5EGUiLoBgSXaYcsdEo9nG3V2z26a")
        );
    }

    #[test]
    fn 用户停用百度后不再下发_ak() {
        // 还拿着 AK 渲染等于继续消耗他的配额，而界面上显示的是「已停用」。
        let store = Store::memory().unwrap();
        store
            .save(&MapProviderInput {
                slug: "baidu".to_string(),
                name: "百度地图".to_string(),
                enabled: true,
                priority: None,
                api_key: None,
                browser_key: Some("ak-明文".to_string()),
            })
            .unwrap();
        assert!(baidu_sdk_key_from(&store).unwrap().is_some());

        store
            .save(&MapProviderInput {
                slug: "baidu".to_string(),
                name: "百度地图".to_string(),
                enabled: false,
                priority: None,
                api_key: None,
                browser_key: None,
            })
            .unwrap();
        assert_eq!(baidu_sdk_key_from(&store).unwrap(), None);
    }

    #[test]
    fn 填了又清空等于没填() {
        // 不过滤空串的话脚本 URL 变成 `&ak=`,百度返回的是权限错误而不是「没填」。
        let store = Store::memory().unwrap();
        store
            .save(&MapProviderInput {
                slug: "baidu".to_string(),
                name: "百度地图".to_string(),
                enabled: true,
                priority: None,
                api_key: None,
                browser_key: Some("   ".to_string()),
            })
            .unwrap();
        assert_eq!(baidu_sdk_key_from(&store).unwrap(), None);
    }

    #[test]
    fn 服务端_ak_不会被当成浏览器_ak_下发() {
        // 两把 AK 在百度控制台是两种应用类型,互相不能用。把服务端那把下发给浏览器的表现是地图空白且无提示。
        let store = Store::memory().unwrap();
        store
            .save(&MapProviderInput {
                slug: "baidu".to_string(),
                name: "百度地图".to_string(),
                enabled: true,
                priority: None,
                api_key: Some("服务端那把".to_string()),
                browser_key: None,
            })
            .unwrap();
        assert_eq!(baidu_sdk_key_from(&store).unwrap(), None);

        // 反向：填了浏览器那把之后，服务端那把仍然是原来的值。
        store
            .save(&MapProviderInput {
                slug: "baidu".to_string(),
                name: "百度地图".to_string(),
                enabled: true,
                priority: None,
                api_key: None,
                browser_key: Some("浏览器那把".to_string()),
            })
            .unwrap();
        assert_eq!(
            baidu_sdk_key_from(&store).unwrap().as_deref(),
            Some("浏览器那把")
        );
        assert_eq!(
            store.secret_of("baidu").unwrap().as_deref(),
            Some("服务端那把"),
            "存浏览器 AK 不该动服务端那把"
        );
    }
}
