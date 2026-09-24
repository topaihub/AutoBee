//! 非流式对话:装请求、发请求、取回一整段回答。
//!
//! 走 `route_core::http::post_json`(长超时客户端)。流式在 [`super::sse`]。

use crate::credentials::LlmProtocol;
use crate::error::AppError;

use super::protocol::{
    ChatMsg, ToolDef, TurnOutcome, auth_headers, build_body, endpoint, parse_turn,
};

/// 发一轮对话并解析结果。
///
/// 工具调用只是「模型要求执行」，本函数不执行也不续轮 —— 那是 `agent` 的事，
/// 这里保持单纯的一问一答，才能被 agent 循环与单次对话共用。
///
/// # Errors
///
/// 协议不支持工具、网络失败、上游返回错误或形状认不出时返回错误。
pub fn one_shot(
    base_url: &str,
    protocol: LlmProtocol,
    api_key: &str,
    model: &str,
    messages: &[ChatMsg],
    tools: &[ToolDef],
) -> Result<TurnOutcome, AppError> {
    // 先装请求体：TOOLS_UNSUPPORTED 这类错误要在发网络请求之前就暴露。
    let body = build_body(protocol, model, messages, tools, false)?;

    let owned = auth_headers(protocol, api_key);
    let mut headers: Vec<(&str, &str)> = owned
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    headers.push(("content-type", "application/json"));

    let response: serde_json::Value =
        route_core::http::post_json(&endpoint(base_url, protocol), &headers, &body)?;
    parse_turn(protocol, &response)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 协议不支持工具时不发网络请求() {
        // 断言能在没有网络的测试环境里通过，本身就证明请求体装配先于发送。
        let error = one_shot(
            "https://api.example.com/v1",
            LlmProtocol::AnthropicMessages,
            "sk-1",
            "claude-4",
            &[ChatMsg::text("user", "你好")],
            &[ToolDef {
                name: "read_file".to_owned(),
                description: "读文件".to_owned(),
                parameters: serde_json::json!({ "type": "object" }),
            }],
        )
        .unwrap_err();

        assert_eq!(error.code, "TOOLS_UNSUPPORTED");
    }
}
