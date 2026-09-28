//! 一问一答：装请求、发请求、取回结果。非流式与流式两条都在这里。
//!
//! 两者共用 `protocol.rs` 的请求体装配与 `sse.rs` 的累加规则，所以「流式与非流式
//! 给出不同结论」这种偏差不可能发生 —— 折叠规则只有一份。

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::AppError;

use super::protocol::{
    ChatMsg, ToolDef, TurnOutcome, Upstream, auth_headers, build_body, endpoint, parse_turn,
};
use super::sse::{SseParser, StreamAccumulator};

/// 发一轮对话并解析结果。
///
/// 工具调用只是「模型要求执行」，本函数不执行也不续轮 —— 那是 `agent` 的事，
/// 这里保持单纯的一问一答，才能被 agent 循环与单次对话共用。
///
/// # Errors
///
/// 协议不支持工具、网络失败、上游返回错误或形状认不出时返回错误。
pub fn one_shot(
    up: Upstream<'_>,
    messages: &[ChatMsg],
    tools: &[ToolDef],
) -> Result<TurnOutcome, AppError> {
    // 先装请求体：TOOLS_UNSUPPORTED 这类错误要在发网络请求之前就暴露。
    let body = build_body(up.protocol, up.model, messages, tools, false)?;

    let owned = auth_headers(up.protocol, up.api_key);
    let mut headers: Vec<(&str, &str)> = owned
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    headers.push(("content-type", "application/json"));

    let response: serde_json::Value =
        route_core::http::post_json(&endpoint(up.base_url, up.protocol), &headers, &body)?;
    parse_turn(up.protocol, &response)
}

/// 发一轮流式对话，边读边把新增正文交给 `on_delta`。
///
/// `cancelled` 每读完一块检查一次。**只在块边界检查**：真正中断连接要靠 drop
/// `Response`，而 drop 只能在不再读它的时候发生，所以块中间无处可查。一块最多
/// 几 KB，用户点「停止」到实际停下的延迟可以忽略。
///
/// # Errors
///
/// 协议不支持工具或流式、网络失败、上游返回错误时返回错误。被取消时返回
/// `CANCELLED`（status 499）—— 用一个与真实失败区分得开的码，界面才能不弹错误提示。
pub fn stream_once(
    up: Upstream<'_>,
    messages: &[ChatMsg],
    tools: &[ToolDef],
    cancelled: &AtomicBool,
    on_delta: &mut dyn FnMut(&str),
) -> Result<TurnOutcome, AppError> {
    let body = build_body(up.protocol, up.model, messages, tools, true)?;

    let owned = auth_headers(up.protocol, up.api_key);
    let mut headers: Vec<(&str, &str)> = owned
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    headers.push(("content-type", "application/json"));
    // 有中转商在缺这个头时静默降级成非流式，回一整个 JSON。那时 SSE 解析拿不到
    // 任何 data 行，表现是「模型什么都没说」而不是报错。
    headers.push(("accept", "text/event-stream"));

    let url = endpoint(up.base_url, up.protocol);
    let mut response = route_core::http::post_stream(&url, &headers, &body)?;

    let mut parser = SseParser::new();
    let mut accumulator = StreamAccumulator::new(up.protocol);
    let mut buf = [0_u8; 8192];

    loop {
        if cancelled.load(Ordering::Relaxed) {
            // 提前 return 就会 drop response，连接随之断开，上游停止计费。
            return Err(cancelled_error());
        }

        let read = response
            .read(&mut buf)
            .map_err(|error| AppError::new("STREAM_READ", format!("读取流失败：{error}"), 502))?;
        if read == 0 {
            break;
        }

        for payload in parser.feed(&buf[..read]) {
            let delta = accumulator.push(&payload)?;
            if !delta.is_empty() {
                on_delta(&delta);
            }
        }
    }

    accumulator.finish()
}

/// 被用户取消。
///
/// 499 是「客户端主动关闭」的惯例码，与 5xx 的真实失败分得开：界面据此判断该不该
/// 弹错误提示。
fn cancelled_error() -> AppError {
    AppError::new("CANCELLED", "已取消本次回答。", 499)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;
    use crate::credentials::LlmProtocol;

    fn anthropic() -> Upstream<'static> {
        Upstream {
            base_url: "https://api.example.com/v1",
            protocol: LlmProtocol::AnthropicMessages,
            api_key: "sk-1",
            model: "claude-4",
        }
    }

    #[test]
    fn 协议不支持工具时不发网络请求() {
        // 断言能在没有网络的测试环境里通过，本身就证明请求体装配先于发送。
        let error = one_shot(
            anthropic(),
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
