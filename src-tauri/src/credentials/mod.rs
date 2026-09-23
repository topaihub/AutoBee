//! 地图服务商档案：SQLite 存储 + 密钥加密。
//!
//! 对标 ai-installer 的 `settings/` 模块：一个服务商一条档案，密钥落盘加密，
//! 下发给前端的只有掩码，算路时由后端取明文填进请求。
//!
//! # 用户能改什么 / 不能改什么
//!
//! | 用户能改（存库）            | 不能改（写代码里）                  |
//! | ---                         | ---                                 |
//! | 启用/停用、优先级、Key、名称 | 覆盖国家、支持哪些方式、坐标系      |
//!
//! 后者是**实测出来的事实**（天地图的坐标系是 CGCS2000、HERE 只支持海外），
//! 放在库里让人改，改错的现象是「选了它却算不出地铁」而界面上毫无解释。
//! 这与 ai-installer 把协议留在工具侧是同一个判断。

pub mod crypto;
pub mod resolve;
mod schema;
pub mod store;

use serde::{Deserialize, Serialize};

/// 下发给前端的一条服务商档案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapProviderDto {
    /// 标识，也是 settings 里 default_providers 用的值。
    pub slug: String,
    /// 显示名。
    pub name: String,
    /// 内置的六家。前端据此决定是否显示删除按钮。
    pub is_preset: bool,
    /// 是否参与算路。
    pub enabled: bool,
    /// 用户改过的优先级。`None` 表示用注册表默认值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
    /// 注册表的默认优先级，前端显示「已调整」时用得着。
    pub default_priority: u32,
    /// 覆盖范围，如「CN」或「全球」。
    pub coverage: String,
    /// 原始坐标系，如「GCJ02」。
    pub coord_sys: String,
    /// 支持的能力，中文，如「公交 · 地铁 · 步行」。
    pub capabilities: String,
    /// 掩码后的 Key。空串表示还没填。
    pub api_key_masked: String,
    /// adapter 是否已实现。未实现的在界面上标「待接入」——
    /// 不标的话用户填了 Key 却永远选不中它。
    pub implemented: bool,
    /// 官方申请 API Key 的控制台链接。
    pub key_apply_url: String,
}

/// 前端提交的一条档案。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapProviderInput {
    /// 标识。
    pub slug: String,
    /// 显示名。
    pub name: String,
    /// 是否启用。
    pub enabled: bool,
    /// 用户改过的优先级。`None` 表示恢复默认。
    #[serde(default)]
    pub priority: Option<u32>,
    /// 新密钥的**明文**。`None` 表示没动过，保留库里那份。
    ///
    /// 这个区分不是洁癖：前端拿到的是掩码，用户不改时框里就是掩码串，
    /// 原样提交会把 `FAKE********TEST` 当成真 Key 存进去。
    #[serde(default)]
    pub api_key: Option<String>,
}

/// 界面偏好。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// 按地点自动选服务商。
    pub auto_provider: bool,
    /// 输出坐标系。
    pub out_crs: String,
    /// 允许跨境路线。
    pub allow_cross_border: bool,
    /// 底图瓦片源。
    pub tile_source: String,
    /// 固定链路（关掉自动选择时生效）。
    pub default_providers: Vec<String>,
    /// 是否在主界面显示日志面板。
    ///
    /// 只影响界面，不进 [`route_core::config::Settings`] —— 那个结构体是喂给
    /// 算路的，掺进一个界面开关会让「改了显示偏好」看起来像「改了算路配置」。
    pub show_logs: bool,
}

/// 一次 Key 可用性测试的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    /// 是否可用。
    pub ok: bool,
    /// 结论文字，可直接展示。
    pub message: String,
}
