//! 业务错误。
//!
//! 带稳定 code 是因为前端要按错误类型分支给提示，只匹配中文 message 会在改文案时
//! 静默失效。`Serialize` 是 Tauri command 返回 `Result<T, E>` 的硬要求。

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub status: u16,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, status: u16) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status,
        }
    }

    pub fn upstream(provider: &str, detail: &str) -> Self {
        Self::new("UPSTREAM", format!("{provider} 返回错误：{detail}"), 502)
    }

    pub fn resolve_failed(text: &str) -> Self {
        Self::new(
            "RESOLVE_FAILED",
            format!("无法识别地点「{text}」，建议从下拉候选里点选。"),
            400,
        )
    }

    /// 坐标已拿到，但算不出路线（区别于 `resolve_failed`：那是地点没解析出来）。
    pub fn no_route() -> Self {
        Self::new(
            "NO_ROUTE",
            "没有找到符合条件的路线，换个出行方式或起终点试试。",
            404,
        )
    }

    pub fn cross_border(from: &str, to: &str) -> Self {
        Self::new(
            "CROSS_BORDER",
            format!("暂不支持跨境路线（{from} → {to}），请分别规划境内外路段。"),
            400,
        )
    }

    /// 一个 Key 都没配。
    pub fn no_provider() -> Self {
        Self::new(
            "NO_PROVIDER",
            "没有可用的地图服务：请到设置页至少填一个服务商的 API Key。",
            503,
        )
    }

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

    /// 该厂商 adapter 未实现；planner 会跳过而不是报错。
    pub fn not_implemented(provider: &str) -> Self {
        Self::new("NOT_IMPLEMENTED", format!("{provider} 的接入还没实现"), 501)
    }

    pub fn network(detail: &str) -> Self {
        Self::new("NETWORK", format!("网络请求失败：{detail}"), 502)
    }
}
