//! 运行期配置。
//!
//! # 这一层只有内存结构
//!
//! 配置**从哪来**（SQLite 档案 + DPAPI、环境变量）是应用外壳的事，见应用侧的
//! `credentials` 模块。核心层只收一份填好的 [`Config`]，好处是单测可以随手
//! 造一份配置，不必真的落盘、也不必依赖某个平台。
//!
//! # 密钥不进核心层的持久化
//!
//! [`Config::keys`] 里是明文（算路要用），但它从不上日志、不下发前端。

use std::collections::{BTreeMap, BTreeSet};

use crate::models::{Crs, ProviderId};

/// 界面偏好。对应档案库的 `prefs` 表。
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// 是否按地点自动选服务商。关掉就只用 [`Settings::default_providers`]。
    pub auto_provider: bool,
    /// 输出坐标系。
    ///
    /// 默认是 **GCJ-02**，因为腾讯矢量底图与腾讯路线服务都使用 GCJ-02。
    /// 切换到 WGS84 底图时必须同步改这里，否则路线会整体偏移几百米。
    pub out_crs: Crs,
    /// 是否允许跨境路线。一期不支持：没有单一厂商能算跨境，硬算要给假结果。
    pub allow_cross_border: bool,
    /// 底图瓦片源，见 [`crate::tiles::TILE_SOURCES`]。
    pub tile_source: String,
    /// 关掉自动选择时用的固定链路，按顺序。空表示「所有已配置的都算」。
    pub default_providers: Vec<ProviderId>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_provider: true,
            out_crs: Crs::Gcj02,
            allow_cross_border: false,
            tile_source: "arcgis-street".to_string(),
            default_providers: Vec::new(),
        }
    }
}

/// 一次算路用到的全部配置。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    /// 各家的 API Key。**只填已配置的**，没填的厂商自动不参与候选。
    pub keys: BTreeMap<ProviderId, String>,
    /// 用户在设置页关掉的厂商。
    pub disabled: BTreeSet<ProviderId>,
    /// 用户改过的优先级，覆盖 [`crate::registry::ProviderMeta::priority`]。
    pub priority: BTreeMap<ProviderId, u32>,
    /// 界面偏好。
    pub settings: Settings,
}

impl Config {
    /// 取某家的 Key。没配返回 `None`。
    #[must_use]
    pub fn key(&self, id: ProviderId) -> Option<&str> {
        self.keys
            .get(&id)
            .map(String::as_str)
            .filter(|k| !k.is_empty())
    }

    /// 这家能不能用：配了 Key 且没被用户关掉。
    #[must_use]
    pub fn is_configured(&self, id: ProviderId) -> bool {
        self.key(id).is_some() && !self.disabled.contains(&id)
    }

    /// 优先级：用户改过的优先，否则用注册表里的默认值。
    #[must_use]
    pub fn priority_of(&self, id: ProviderId, fallback: u32) -> u32 {
        self.priority.get(&id).copied().unwrap_or(fallback)
    }

    /// 一份只有腾讯、用于测试的最小配置。
    #[must_use]
    pub fn with_key(id: ProviderId, key: impl Into<String>) -> Self {
        let mut keys = BTreeMap::new();
        keys.insert(id, key.into());
        Self {
            keys,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空字符串不算配了_key() {
        // 用户在设置页填了又清空，界面上看着「有」，实际不能发请求。
        let mut config = Config::default();
        config.keys.insert(ProviderId::Tencent, String::new());
        assert!(!config.is_configured(ProviderId::Tencent));
    }

    #[test]
    fn 关掉的厂商即使有_key_也不用() {
        let mut config = Config::with_key(ProviderId::Tencent, "k");
        assert!(config.is_configured(ProviderId::Tencent));
        config.disabled.insert(ProviderId::Tencent);
        assert!(!config.is_configured(ProviderId::Tencent));
    }

    #[test]
    fn 用户改过的优先级覆盖默认值() {
        let mut config = Config::default();
        assert_eq!(config.priority_of(ProviderId::Amap, 2), 2);
        config.priority.insert(ProviderId::Amap, 9);
        assert_eq!(config.priority_of(ProviderId::Amap, 2), 9);
    }

    #[test]
    fn 默认输出_gcj02_因为底图与路线服务都是_gcj02() {
        // 改这里之前先确认底图换了 —— 两者不一致时路线会整体偏移几百米。
        assert_eq!(Settings::default().out_crs, Crs::Gcj02);
    }
}
