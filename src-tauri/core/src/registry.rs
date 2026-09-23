//! Provider 注册表。
//!
//! # 新增厂商要动的地方（只有三处）
//!
//! 1. 这里加一条 [`ProviderMeta`]
//! 2. `factory.rs` 里加一个 case
//! 3. 实现 `providers::<name>.rs`
//!
//! 编排层与前端都不感知具体厂商 —— 它们只看 [`ProviderId`]。
//!
//! # 能力写在代码里而不是档案库
//!
//! 覆盖国家、支持哪些出行方式、用什么坐标系，这些是**实测出来的事实**，
//! 不是用户偏好。放到库里让用户改，改错的现象是「选了高德但算不出地铁」
//! 而界面上毫无解释。用户在设置页能改的只有：启用/停用、优先级、Key。
//! 这一点与 ai-installer 的「协议留在工具侧」是同一个判断。

use crate::config::Config;
use crate::models::{Crs, ProviderId, TravelMode};

/// 覆盖范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// 只覆盖列出的国家（ISO 3166-1 alpha-2，大写）。
    Countries(&'static [&'static str]),
    /// 全球。
    Global,
}

impl Coverage {
    /// 是否覆盖某个国家。国家码未知时返回 `false` —— 宁可让上层退化成
    /// 顺序探测，也不要拿一个猜出来的地区去选厂商。
    #[must_use]
    pub fn covers(self, country: &str) -> bool {
        match self {
            Self::Global => true,
            Self::Countries(list) => list.contains(&country.to_uppercase().as_str()),
        }
    }
}

/// 支持哪些能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// 公交/地铁路线规划。
    pub transit: bool,
    /// 原生的「只坐地铁 / 不坐地铁」过滤（天地图的 lineType 位标志）。
    pub subway_filter: bool,
    /// 步行。
    pub walking: bool,
}

/// 一家厂商的静态描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMeta {
    /// 标识。
    pub id: ProviderId,
    /// 覆盖范围。
    pub coverage: Coverage,
    /// 能力。
    pub capabilities: Capabilities,
    /// 原始坐标系。
    pub coord_sys: Crs,
    /// 同一地区内数字越小越优先。
    pub priority: u32,
    /// 官方申请 API Key 的控制台地址。
    pub key_apply_url: &'static str,
}

/// 注册表。**顺序不重要**，优先级看 `priority`。
pub const PROVIDERS: &[ProviderMeta] = &[
    ProviderMeta {
        id: ProviderId::Tencent,
        coverage: Coverage::Countries(&["CN"]),
        capabilities: Capabilities { transit: true, subway_filter: false, walking: true },
        coord_sys: Crs::Gcj02,
        priority: 1,
        key_apply_url: "https://lbs.qq.com/dev/console/application/mine",
    },
    ProviderMeta {
        id: ProviderId::Amap,
        coverage: Coverage::Countries(&["CN"]),
        capabilities: Capabilities { transit: true, subway_filter: false, walking: true },
        coord_sys: Crs::Gcj02,
        priority: 2,
        key_apply_url: "https://console.amap.com/dev/key/app",
    },
    // 天地图的公交规划 lineType 是位标志，第 3 位就是「不坐地铁」，
    // 比在腾讯方案里靠 vehicle 字段自己过滤更省事。
    ProviderMeta {
        id: ProviderId::Tianditu,
        coverage: Coverage::Countries(&["CN"]),
        capabilities: Capabilities { transit: true, subway_filter: true, walking: true },
        coord_sys: Crs::Cgcs2000,
        priority: 3,
        key_apply_url: "https://console.tianditu.gov.cn/api/key",
    },
    ProviderMeta {
        id: ProviderId::Baidu,
        coverage: Coverage::Countries(&["CN"]),
        capabilities: Capabilities { transit: true, subway_filter: false, walking: true },
        coord_sys: Crs::Bd09,
        priority: 4,
        key_apply_url: "https://lbsyun.baidu.com/apiconsole/key",
    },
    ProviderMeta {
        id: ProviderId::Here,
        coverage: Coverage::Global,
        capabilities: Capabilities { transit: true, subway_filter: true, walking: true },
        coord_sys: Crs::Wgs84,
        priority: 1,
        key_apply_url: "https://platform.here.com/admin/apps",
    },
    ProviderMeta {
        id: ProviderId::Google,
        coverage: Coverage::Global,
        capabilities: Capabilities { transit: true, subway_filter: false, walking: true },
        coord_sys: Crs::Wgs84,
        priority: 2,
        key_apply_url: "https://console.cloud.google.com/google/maps-apis/credentials",
    }
];

/// 查一家的元数据。
///
/// # Errors
///
/// 传入未注册的 id 时返回错误 —— 那是代码写错了，不是用户输入问题。
pub fn get_meta(id: ProviderId) -> Result<ProviderMeta, crate::error::AppError> {
    PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .copied()
        .ok_or_else(|| crate::error::AppError::new("UNKNOWN_PROVIDER", "未注册的服务商", 500))
}

/// 该出行方式需要哪一项能力。
fn needs(mode: TravelMode) -> fn(&Capabilities) -> bool {
    match mode {
        TravelMode::Walking => |c| c.walking,
        TravelMode::Subway | TravelMode::Bus => |c| c.transit,
    }
}

/// 选出候选链：配了 Key + 支持该方式 + 覆盖该地区，按优先级升序。
///
/// `country` 为 `None`（阶段一没拿到国家码）时不按地区过滤，退化成
/// 「所有配了 Key 且支持该方式的」，由调用方顺序探测。
#[must_use]
pub fn resolve_providers(country: Option<&str>, mode: TravelMode, config: &Config) -> Vec<ProviderId> {
    let want = needs(mode);
    let mut pool: Vec<ProviderMeta> = PROVIDERS
        .iter()
        .copied()
        .filter(|p| config.is_configured(p.id) && want(&p.capabilities))
        .collect();

    if let Some(cc) = country {
        let hit: Vec<ProviderMeta> = pool.iter().copied().filter(|p| p.coverage.covers(cc)).collect();
        // 一个都没命中（例如只配了海外 Key 却要算国内）时放宽到全部已配置的 ——
        // 让请求真的发出去拿到厂商的错误，好过在本地质疑「没有可用服务」。
        if !hit.is_empty() {
            pool = hit;
        }
    }

    pool.sort_by_key(|p| config.priority_of(p.id, p.priority));
    pool.iter().map(|p| p.id).collect()
}

/// 未开启自动选择时的固定链路。
#[must_use]
pub fn default_chain(config: &Config) -> Vec<ProviderId> {
    let wanted = &config.settings.default_providers;
    if wanted.is_empty() {
        return resolve_providers(None, TravelMode::Subway, config);
    }
    let chain: Vec<ProviderId> = wanted
        .iter()
        .copied()
        .filter(|id| config.is_configured(*id))
        .collect();
    // 用户写的链路全部不可用时回落到「全部已配置」：
    // 否则界面上会显示「没有可用服务」，而他明明在设置页填过 Key。
    if chain.is_empty() {
        resolve_providers(None, TravelMode::Subway, config)
    } else {
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(ids: &[ProviderId]) -> Config {
        let mut config = Config::default();
        for id in ids {
            config.keys.insert(*id, format!("key-{id:?}"));
        }
        config
    }

    #[test]
    fn 只配了腾讯时链里只有腾讯() {
        let config = config_with(&[ProviderId::Tencent]);
        assert_eq!(
            resolve_providers(Some("CN"), TravelMode::Subway, &config),
            vec![ProviderId::Tencent]
        );
    }

    #[test]
    fn 国内按优先级排序() {
        // 故意乱序插入，验证排序是按 priority 而不是插入顺序。
        let config = config_with(&[ProviderId::Baidu, ProviderId::Tencent, ProviderId::Amap]);
        assert_eq!(
            resolve_providers(Some("CN"), TravelMode::Subway, &config),
            vec![ProviderId::Tencent, ProviderId::Amap, ProviderId::Baidu]
        );
    }

    #[test]
    fn 海外地点只留全球厂商() {
        let config = config_with(&[ProviderId::Tencent, ProviderId::Here]);
        assert_eq!(
            resolve_providers(Some("JP"), TravelMode::Subway, &config),
            vec![ProviderId::Here]
        );
    }

    #[test]
    fn 地区没命中时放宽到全部已配置() {
        // 只配了国内 Key 却要算日本：让它真发出去拿到厂商错误。
        let config = config_with(&[ProviderId::Tencent]);
        assert_eq!(
            resolve_providers(Some("JP"), TravelMode::Subway, &config),
            vec![ProviderId::Tencent]
        );
    }

    #[test]
    fn 国家码未知时不过滤地区() {
        let config = config_with(&[ProviderId::Tencent, ProviderId::Here]);
        assert_eq!(resolve_providers(None, TravelMode::Subway, &config).len(), 2);
    }

    #[test]
    fn 用户改了优先级就按用户改的来() {
        let mut config = config_with(&[ProviderId::Tencent, ProviderId::Amap]);
        config.priority.insert(ProviderId::Amap, 0);
        assert_eq!(
            resolve_providers(Some("CN"), TravelMode::Subway, &config),
            vec![ProviderId::Amap, ProviderId::Tencent]
        );
    }

    #[test]
    fn 关掉的厂商不进链() {
        let mut config = config_with(&[ProviderId::Tencent, ProviderId::Amap]);
        config.disabled.insert(ProviderId::Tencent);
        assert_eq!(
            resolve_providers(Some("CN"), TravelMode::Subway, &config),
            vec![ProviderId::Amap]
        );
    }

    #[test]
    fn 固定链路按用户写的顺序() {
        let mut config = config_with(&[ProviderId::Tencent, ProviderId::Amap]);
        config.settings.default_providers = vec![ProviderId::Amap, ProviderId::Tencent];
        assert_eq!(
            default_chain(&config),
            vec![ProviderId::Amap, ProviderId::Tencent],
            "固定链路不排序，用户写的就是他要的"
        );
    }

    #[test]
    fn 固定链路全不可用时不返回空() {
        let mut config = config_with(&[ProviderId::Tencent]);
        config.settings.default_providers = vec![ProviderId::Here];
        assert_eq!(default_chain(&config), vec![ProviderId::Tencent]);
    }

    #[test]
    fn 六家都注册了() {
        assert_eq!(PROVIDERS.len(), 6);
        assert!(get_meta(ProviderId::Tianditu).is_ok());
    }
}
