//! 业务错误。
//!
//! # 为什么要带 code
//!
//! 前端要按错误类型给不同的提示：「跨境不支持」该提示换一个起终点，
//! 「所有服务都失败」该提示去设置页检查 Key。只靠 message 匹配中文会
//! 在改文案时静默失效。
//!
//! # 为什么实现 Serialize
//!
//! Tauri 的 command 返回 `Result<T, E>` 时 `E` 必须能序列化，否则编译不过。

use serde::Serialize;

/// 一次失败的原因，带一个稳定的 code 与可直接展示的中文说明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct AppError {
    /// 稳定的错误码，前端据此分支。
    pub code: String,
    /// 已经写好的中文提示，前端可以直接展示。
    pub message: String,
    /// 语义化的 HTTP 状态，便于日后排查。
    pub status: u16,
}

impl AppError {
    /// 构造一个错误。
    pub fn new(code: impl Into<String>, message: impl Into<String>, status: u16) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status,
        }
    }

    /// 上游厂商返回了非 0 状态。
    pub fn upstream(provider: &str, detail: &str) -> Self {
        Self::new("UPSTREAM", format!("{provider} 返回错误：{detail}"), 502)
    }

    /// 地点解析不出来。
    pub fn resolve_failed(text: &str) -> Self {
        Self::new(
            "RESOLVE_FAILED",
            format!("无法识别地点「{text}」，建议从下拉候选里点选。"),
            400,
        )
    }

    /// 拿到了坐标但算不出路线。
    pub fn no_route() -> Self {
        Self::new(
            "NO_ROUTE",
            "没有找到符合条件的路线，换个出行方式或起终点试试。",
            404,
        )
    }

    /// 起终点分属不同国家。
    pub fn cross_border(from: &str, to: &str) -> Self {
        Self::new(
            "CROSS_BORDER",
            format!("暂不支持跨境路线（{from} → {to}），请分别规划境内外路段。"),
            400,
        )
    }

    /// 一家可用的服务商都没有（一个 Key 都没配）。
    pub fn no_provider() -> Self {
        Self::new(
            "NO_PROVIDER",
            "没有可用的地图服务：请到设置页至少填一个服务商的 API Key。",
            503,
        )
    }

    /// 候选链全部失败。
    pub fn all_failed(tried: &[String], detail: &str) -> Self {
        Self::new(
            "ALL_PROVIDERS_FAILED",
            format!(
                "所有地图服务都失败了（已尝试：{}）。{detail}",
                tried.join("、")
            ),
            502,
        )
    }

    /// 该厂商的 adapter 还没实现。planner 会跳过而不是报错。
    pub fn not_implemented(provider: &str) -> Self {
        Self::new("NOT_IMPLEMENTED", format!("{provider} 的接入还没实现"), 501)
    }

    /// 网络层失败。
    pub fn network(detail: &str) -> Self {
        Self::new("NETWORK", format!("网络请求失败：{detail}"), 502)
    }
}
