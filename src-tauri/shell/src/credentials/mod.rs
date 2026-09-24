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
pub(crate) mod schema;
pub mod store;

use serde::{Deserialize, Serialize};

/// 下发给前端的一条服务商档案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapProviderDto {
    /// 标识,也是 settings 里 default_providers 用的值。
    pub slug: String,
    pub name: String,
    /// 内置的六家。前端据此决定是否显示删除按钮。
    pub is_preset: bool,
    /// 是否参与算路。
    pub enabled: bool,
    /// 用户改过的优先级。`None` 表示用注册表默认值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
    /// 注册表的默认值,前端显示「已调整」时用得着。
    pub default_priority: u32,
    /// 如「CN」或「全球」。
    pub coverage: String,
    /// 如「GCJ02」。
    pub coord_sys: String,
    /// 中文,如「公交 · 地铁 · 步行」。
    pub capabilities: String,
    /// 掩码后的服务端 Key。空串表示还没填。
    pub api_key_masked: String,
    /// 掩码后的**浏览器端** Key。空串表示还没填,其余厂商恒为空串。
    ///
    /// 只有百度需要第二把,原因见 [`resolve::baidu_sdk_key`](super::resolve)。
    pub browser_key_masked: String,
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
    pub slug: String,
    pub name: String,
    pub enabled: bool,
    /// `None` 表示恢复默认。
    #[serde(default)]
    pub priority: Option<u32>,
    /// 明文。`None` 表示没动过,保留库里那份 —— 前端框里是掩码串,
    /// 原样提交会把 `FAKE********TEST` 当成真 Key 存进去。
    #[serde(default)]
    pub api_key: Option<String>,
    /// 新的**浏览器端**密钥明文。`None` 同上,表示没动过。
    #[serde(default)]
    pub browser_key: Option<String>,
}

/// 「含较长步行」的默认阈值，米。§4.2 取的保守值。
pub const DEFAULT_LONG_WALK_METERS: u32 = 500;

/// 「换乘较多」的默认阈值，次。
pub const DEFAULT_MANY_TRANSFERS: u32 = 2;

/// 界面偏好。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// 按地点自动选服务商。
    pub auto_provider: bool,
    pub out_crs: String,
    pub allow_cross_border: bool,
    pub tile_source: String,
    /// 固定链路,关掉自动选择时生效。
    pub default_providers: Vec<String>,
    /// 只影响界面,不进 [`route_core::config::Settings`] —— 那个结构体是喂给
    /// 算路的,掺进界面开关会让「改了显示偏好」看起来像「改了算路配置」。
    pub show_logs: bool,
    /// 「含较长步行」提醒阈值,米。同 `show_logs` 只影响界面。
    /// §4.2 末尾要求它可配置、不许写死在 UI 里。
    pub long_walk_meters: u32,
    /// 「换乘较多」提醒阈值,次。超过这个数才提醒。
    pub many_transfers: u32,
}

/// 一次 Key 可用性测试的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    /// 结论文字,可直接展示。
    pub message: String,
}

/// 一个大模型服务入口的协议。
///
/// 「中转商」不是单独的抽象:它就是 `base_url` 指向代理、协议选
/// [`Self::OpenaiChat`] 的普通 provider。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LlmProtocol {
    /// Anthropic `/v1/messages`。
    AnthropicMessages,
    /// OpenAI `/v1/responses`。
    OpenaiResponses,
    /// OpenAI `/v1/chat/completions`。绝大多数中转商是这个。
    OpenaiChat,
}

impl LlmProtocol {
    /// 存进库里的字符串。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenaiResponses => "openai-responses",
            Self::OpenaiChat => "openai-chat",
        }
    }

    /// 从库里的字符串读回。
    ///
    /// 认不出的值回落到 [`Self::OpenaiChat`]：库里的协议名可能来自更新的版本，
    /// 那时整个设置页应当仍然能打开让用户改，而不是一行坏数据全页报错。
    #[must_use]
    pub fn from_str_or_default(raw: &str) -> Self {
        match raw {
            "anthropic-messages" => Self::AnthropicMessages,
            "openai-responses" => Self::OpenaiResponses,
            _ => Self::OpenaiChat,
        }
    }
}

/// 下发给前端的一个大模型服务入口。Key 已掩码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderDto {
    pub id: String,
    pub name: String,
    /// 形如 `https://xxx/v1`。
    pub base_url: String,
    pub protocol: LlmProtocol,
    pub enabled: bool,
    /// 空串表示还没填。
    pub api_key_masked: String,
    /// 用户挑中的模型。
    pub models: Vec<LlmModelDto>,
}

/// 用户从某个入口挑中的一个模型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelDto {
    /// 供应商侧的模型标识。
    pub model: String,
    /// `None` 则显示 `model`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub tools_ok: bool,
}

/// 前端提交的一个服务入口。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderInput {
    /// `None` 表示新建。
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub protocol: LlmProtocol,
    pub enabled: bool,
    /// 明文。`None` 或空串表示没动过,保留库里那份 ——
    /// 前端框里是掩码串,原样提交不能把掩码写成真 Key。
    #[serde(default)]
    pub api_key: Option<String>,
}

/// 前端提交的一条模型选择。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelInput {
    pub model: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default = "默认支持工具")]
    pub tools_ok: bool,
}

/// `tools_ok` 缺省为 true：绝大多数在用的模型都支持，
/// 缺省成 false 会让工具静默不可用而界面上看不出原因。
const fn 默认支持工具() -> bool {
    true
}
