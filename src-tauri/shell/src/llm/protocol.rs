//! 三种协议的请求体装配与响应解析。
//!
//! `openai-chat` / `openai-responses` / `anthropic-messages` 的差异只有三处:
//! 端点路径、消息体形状、tool 声明字段。差异集中在本模块，上层只见归一化后的
//! [`TurnOutcome`]。首期只有 `openai-chat` 支持工具调用，另两个走纯对话。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::credentials::LlmProtocol;
use crate::error::AppError;

/// Anthropic 缺这个头会直接 400。
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic 的 `max_tokens` 是必填项，OpenAI 两家可以不给。
///
/// 给得太小会让长回答被截断成 [`TurnOutcome::Truncated`]，取一个当前各模型都支持的上限。
const ANTHROPIC_MAX_TOKENS: u32 = 8192;

/// 一个上游入口：打哪儿、按什么协议、用什么密钥、调哪个模型。
///
/// 这四项永远同时出现、永远同时变，拆成四个参数只会让每个调用点都多抄三行，还给
/// 「协议和 base_url 配错对」留了空子。
#[derive(Debug, Clone, Copy)]
pub struct Upstream<'a> {
    pub base_url: &'a str,
    pub protocol: LlmProtocol,
    /// 明文密钥。只在本次调用期间借用，不落库不打日志。
    pub api_key: &'a str,
    pub model: &'a str,
}

/// 一个可供模型调用的工具声明。
///
/// `parameters` 是 JSON Schema 对象，原样透给上游 —— 各家对 schema 的支持子集不同，
/// 在这里裁剪只会把「模型其实能用」的字段丢掉。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 模型要求执行的一次工具调用。
///
/// `arguments` 一律是 JSON 字符串而非对象：OpenAI 给的就是字符串，Anthropic 给对象，
/// 统一成字符串后上层只有一条反序列化路径，也便于原样落库复盘。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallReq {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 一轮非流式对话的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    /// 模型给出了最终回答。
    Text(String),
    /// 模型要求先执行工具。
    ToolCalls(Vec<ToolCallReq>),
    /// 工具调用因为 token 上限被截断，参数很可能是残缺的 JSON。
    ///
    /// 与 [`Self::ToolCalls`] 分开是安全边界：残缺参数落到 `dispatch` 上
    /// 可能变成「删错目录」这类不可逆动作，必须由上层拒绝执行而不是试着解析。
    Truncated(Vec<ToolCallReq>),
}

/// 归一化的一条对话消息。
///
/// 三种协议的消息形状不同，但共有这四个维度，转换在本模块内完成。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMsg {
    /// `system` / `user` / `assistant` / `tool`。
    pub role: String,
    pub content: String,
    /// assistant 消息里模型要求的调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallReq>>,
    /// tool 消息回填的是哪一次调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMsg {
    /// 纯文本消息（`system` / `user` / 无工具调用的 `assistant`）。
    #[must_use]
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_owned(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

/// 拼出对话端点。
///
/// `base_url` 由用户手填，末尾带不带 `/` 都要拼对，否则会出现 `//messages` 这种
/// 被某些中转商判成 404 的路径。
#[must_use]
pub fn endpoint(base_url: &str, protocol: LlmProtocol) -> String {
    let path = match protocol {
        LlmProtocol::OpenaiChat => "chat/completions",
        LlmProtocol::OpenaiResponses => "responses",
        LlmProtocol::AnthropicMessages => "messages",
    };
    format!("{}/{path}", base_url.trim_end_matches('/'))
}

/// 鉴权头。返回 `String` 对而非 `&str`：`Bearer ` 前缀是拼出来的，借用活不过调用。
#[must_use]
pub fn auth_headers(protocol: LlmProtocol, api_key: &str) -> Vec<(String, String)> {
    match protocol {
        LlmProtocol::OpenaiChat | LlmProtocol::OpenaiResponses => {
            vec![("Authorization".to_owned(), format!("Bearer {api_key}"))]
        }
        LlmProtocol::AnthropicMessages => vec![
            ("x-api-key".to_owned(), api_key.to_owned()),
            ("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned()),
        ],
    }
}

/// 装请求体。
///
/// # Errors
///
/// `tools` 非空但协议不是 `openai-chat` 时返回 `TOOLS_UNSUPPORTED`。
pub fn build_body(
    protocol: LlmProtocol,
    model: &str,
    messages: &[ChatMsg],
    tools: &[ToolDef],
    stream: bool,
) -> Result<Value, AppError> {
    if !tools.is_empty() && protocol != LlmProtocol::OpenaiChat {
        return Err(AppError::new(
            "TOOLS_UNSUPPORTED",
            "该协议暂不支持工具调用，请换用 openai-chat 协议的服务入口。",
            400,
        ));
    }

    Ok(match protocol {
        LlmProtocol::OpenaiChat => {
            let mut body = json!({
                "model": model,
                "messages": messages.iter().map(openai_chat_msg).collect::<Vec<_>>(),
                "stream": stream,
            });
            if !tools.is_empty() {
                let declared = tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.parameters,
                            },
                        })
                    })
                    .collect::<Vec<_>>();
                body["tools"] = Value::Array(declared);
            }
            body
        }
        LlmProtocol::OpenaiResponses => json!({
            "model": model,
            "input": messages
                .iter()
                .map(|m| json!({ "role": m.role, "content": m.content }))
                .collect::<Vec<_>>(),
            "stream": stream,
        }),
        LlmProtocol::AnthropicMessages => {
            // Anthropic 不接受 messages 里的 system 角色，必须提到顶层。
            let system = messages
                .iter()
                .filter(|m| m.role == "system")
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let turns = messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| json!({ "role": m.role, "content": m.content }))
                .collect::<Vec<_>>();

            let mut body = json!({
                "model": model,
                "messages": turns,
                "max_tokens": ANTHROPIC_MAX_TOKENS,
                "stream": stream,
            });
            if !system.is_empty() {
                body["system"] = Value::String(system);
            }
            body
        }
    })
}

/// assistant 的工具调用与 tool 的回填都要带上 id，否则上游无法把结果对回调用。
fn openai_chat_msg(msg: &ChatMsg) -> Value {
    let mut out = json!({ "role": msg.role, "content": msg.content });
    if let Some(calls) = &msg.tool_calls {
        out["tool_calls"] = Value::Array(
            calls
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": { "name": c.name, "arguments": c.arguments },
                    })
                })
                .collect(),
        );
    }
    if let Some(id) = &msg.tool_call_id {
        out["tool_call_id"] = Value::String(id.clone());
    }
    out
}

/// 解析非流式响应。
///
/// # Errors
///
/// 响应带 `error` 字段时返回 `UPSTREAM`；形状认不出时返回 `LLM_BAD_RESPONSE`。
pub fn parse_turn(protocol: LlmProtocol, body: &Value) -> Result<TurnOutcome, AppError> {
    if let Some(error) = upstream_error(body) {
        return Err(error);
    }
    match protocol {
        LlmProtocol::OpenaiChat => parse_openai_chat(body),
        LlmProtocol::OpenaiResponses => parse_openai_responses(body),
        LlmProtocol::AnthropicMessages => parse_anthropic(body),
    }
}

/// 中转商常在 HTTP 200 里塞 `error`，这类响应到不了 `post_json` 的非 2xx 分支。
fn upstream_error(body: &Value) -> Option<AppError> {
    let error = body.get("error")?;
    if error.is_null() {
        return None;
    }
    let detail = error
        .get("message")
        .and_then(Value::as_str)
        .map_or_else(|| error.to_string(), ToOwned::to_owned);
    Some(AppError::new(
        "UPSTREAM",
        format!("上游返回错误：{detail}"),
        502,
    ))
}

fn bad_response(detail: &str) -> AppError {
    AppError::new(
        "LLM_BAD_RESPONSE",
        format!("没看懂上游返回的内容：{detail}"),
        502,
    )
}

fn parse_openai_chat(body: &Value) -> Result<TurnOutcome, AppError> {
    let choice = body
        .get("choices")
        .and_then(|c| c.get(0))
        .ok_or_else(|| bad_response("响应里没有 choices。"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| bad_response("choices[0] 里没有 message。"))?;

    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|raw| {
            raw.iter()
                .map(|call| {
                    let function = call.get("function");
                    ToolCallReq {
                        id: str_at(call, "id").to_owned(),
                        name: function
                            .map(|f| str_at(f, "name"))
                            .unwrap_or_default()
                            .to_owned(),
                        // OpenAI 这里给的本来就是字符串，原样存不做解析。
                        arguments: function
                            .map(|f| str_at(f, "arguments"))
                            .unwrap_or_default()
                            .to_owned(),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !calls.is_empty() {
        if str_at(choice, "finish_reason") == "length" {
            return Ok(TurnOutcome::Truncated(calls));
        }
        return Ok(TurnOutcome::ToolCalls(calls));
    }

    match message.get("content").and_then(Value::as_str) {
        Some(text) => Ok(TurnOutcome::Text(text.to_owned())),
        None => Err(bad_response("message 里既没有 content 也没有 tool_calls。")),
    }
}

fn parse_openai_responses(body: &Value) -> Result<TurnOutcome, AppError> {
    let output = body
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| bad_response("响应里没有 output。"))?;

    let mut text = String::new();
    let mut calls = Vec::new();
    for item in output {
        match str_at(item, "type") {
            "function_call" => calls.push(ToolCallReq {
                // Responses 用 call_id 对回结果，id 是 output 项自己的编号。
                id: match item.get("call_id").and_then(Value::as_str) {
                    Some(id) => id.to_owned(),
                    None => str_at(item, "id").to_owned(),
                },
                name: str_at(item, "name").to_owned(),
                arguments: str_at(item, "arguments").to_owned(),
            }),
            "message" => {
                let parts = item.get("content").and_then(Value::as_array);
                for part in parts.into_iter().flatten() {
                    if let Some(chunk) = part.get("text").and_then(Value::as_str) {
                        text.push_str(chunk);
                    }
                }
            }
            _ => {}
        }
    }

    if !calls.is_empty() {
        // 截断时 status 是 incomplete，参数同样可能残缺。
        if str_at(body, "status") == "incomplete" {
            return Ok(TurnOutcome::Truncated(calls));
        }
        return Ok(TurnOutcome::ToolCalls(calls));
    }
    if text.is_empty() {
        return Err(bad_response(
            "output 里没有可用的 message 或 function_call。",
        ));
    }
    Ok(TurnOutcome::Text(text))
}

fn parse_anthropic(body: &Value) -> Result<TurnOutcome, AppError> {
    let blocks = body
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| bad_response("响应里没有 content。"))?;

    let mut text = String::new();
    let mut calls = Vec::new();
    for block in blocks {
        match str_at(block, "type") {
            "text" => text.push_str(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            "tool_use" => calls.push(ToolCallReq {
                id: str_at(block, "id").to_owned(),
                name: str_at(block, "name").to_owned(),
                // Anthropic 给的是对象，序列化成字符串与 OpenAI 对齐（见 ToolCallReq）。
                arguments: block
                    .get("input")
                    .map_or_else(|| "{}".to_owned(), ToString::to_string),
            }),
            _ => {}
        }
    }

    if !calls.is_empty() {
        if str_at(body, "stop_reason") == "max_tokens" {
            return Ok(TurnOutcome::Truncated(calls));
        }
        return Ok(TurnOutcome::ToolCalls(calls));
    }
    if text.is_empty() {
        return Err(bad_response("content 里没有 text 也没有 tool_use。"));
    }
    Ok(TurnOutcome::Text(text))
}

/// 缺字段回落成空串：上游少给一个字段不该让整轮对话报错，空值在各调用处都有兜底。
fn str_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    fn user(text: &str) -> ChatMsg {
        ChatMsg::text("user", text)
    }

    fn a_tool() -> ToolDef {
        ToolDef {
            name: "read_file".to_owned(),
            description: "读一个文件".to_owned(),
            parameters: json!({ "type": "object", "properties": { "path": { "type": "string" } } }),
        }
    }

    #[test]
    fn 端点对带斜杠和不带斜杠的_base_url_一致() {
        for protocol in [
            LlmProtocol::OpenaiChat,
            LlmProtocol::OpenaiResponses,
            LlmProtocol::AnthropicMessages,
        ] {
            assert_eq!(
                endpoint("https://x/v1", protocol),
                endpoint("https://x/v1/", protocol),
            );
        }
        assert_eq!(
            endpoint("https://x/v1/", LlmProtocol::OpenaiChat),
            "https://x/v1/chat/completions",
        );
        assert_eq!(
            endpoint("https://x/v1", LlmProtocol::OpenaiResponses),
            "https://x/v1/responses",
        );
        assert_eq!(
            endpoint("https://x/v1", LlmProtocol::AnthropicMessages),
            "https://x/v1/messages",
        );
    }

    #[test]
    fn 鉴权头按协议分家() {
        assert_eq!(
            auth_headers(LlmProtocol::OpenaiChat, "sk-1"),
            [("Authorization".to_owned(), "Bearer sk-1".to_owned())],
        );
        let anthropic = auth_headers(LlmProtocol::AnthropicMessages, "sk-2");
        assert_eq!(anthropic[0], ("x-api-key".to_owned(), "sk-2".to_owned()));
        assert_eq!(anthropic[1].1, "2023-06-01");
    }

    #[test]
    fn openai_chat_请求体带_tools_的函数包装() {
        let body = build_body(
            LlmProtocol::OpenaiChat,
            "gpt-4o",
            &[user("你好")],
            &[a_tool()],
            false,
        )
        .unwrap();

        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "你好");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(body["tools"][0]["function"]["description"], "读一个文件");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["path"]["type"],
            "string",
        );
    }

    #[test]
    fn openai_responses_请求体用_input() {
        let body =
            build_body(LlmProtocol::OpenaiResponses, "o4", &[user("嗨")], &[], true).unwrap();
        assert_eq!(body["model"], "o4");
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], "嗨");
        assert!(body.get("messages").is_none());
    }

    #[test]
    fn anthropic_请求体把_system_提到顶层() {
        let body = build_body(
            LlmProtocol::AnthropicMessages,
            "claude-4",
            &[
                ChatMsg::text("system", "你是助手"),
                user("你好"),
                ChatMsg::text("system", "再补一条"),
            ],
            &[],
            false,
        )
        .unwrap();

        assert_eq!(body["system"], "你是助手\n\n再补一条");
        assert_eq!(body["max_tokens"], 8192);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages.iter().all(|m| m["role"] != "system"));
        assert_eq!(messages[0]["content"], "你好");
    }

    #[test]
    fn 非_openai_chat_协议带工具时明确报错() {
        for protocol in [LlmProtocol::OpenaiResponses, LlmProtocol::AnthropicMessages] {
            let error = build_body(protocol, "m", &[user("x")], &[a_tool()], false).unwrap_err();
            assert_eq!(error.code, "TOOLS_UNSUPPORTED");
            assert!(error.message.contains("openai-chat"));
        }
        // 工具为空时这两个协议仍然可用。
        assert!(build_body(LlmProtocol::OpenaiResponses, "m", &[user("x")], &[], false).is_ok());
    }

    #[test]
    fn openai_chat_取出原始_arguments_字符串() {
        let body = json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "read_file", "arguments": "{\"path\":\"a.txt\"}" },
                    }],
                },
            }],
        });

        assert_eq!(
            parse_turn(LlmProtocol::OpenaiChat, &body).unwrap(),
            TurnOutcome::ToolCalls(vec![ToolCallReq {
                id: "call_1".to_owned(),
                name: "read_file".to_owned(),
                // OpenAI 给的是字符串，原样存不做解析。
                arguments: r#"{"path":"a.txt"}"#.to_owned(),
            }]),
        );
    }

    #[test]
    fn 截断的工具调用不能当成可执行调用() {
        // 安全边界：finish_reason=length 时参数是残缺 JSON，执行等于拿错参数动文件。
        let body = json!({
            "choices": [{
                "finish_reason": "length",
                "message": {
                    "tool_calls": [{
                        "id": "call_9",
                        "function": { "name": "delete_file", "arguments": "{\"path\":\"a" },
                    }],
                },
            }],
        });

        assert_eq!(
            parse_turn(LlmProtocol::OpenaiChat, &body).unwrap(),
            TurnOutcome::Truncated(vec![ToolCallReq {
                id: "call_9".to_owned(),
                name: "delete_file".to_owned(),
                arguments: r#"{"path":"a"#.to_owned(),
            }]),
        );
    }

    #[test]
    fn openai_chat_纯文本回答() {
        let body = json!({
            "choices": [{ "finish_reason": "stop", "message": { "content": "答案是 42" } }],
        });
        assert_eq!(
            parse_turn(LlmProtocol::OpenaiChat, &body).unwrap(),
            TurnOutcome::Text("答案是 42".to_owned()),
        );
    }

    #[test]
    fn openai_responses_读_output_里的文本与调用() {
        let text = json!({
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": "前半" }, { "text": "后半" }],
            }],
        });
        assert_eq!(
            parse_turn(LlmProtocol::OpenaiResponses, &text).unwrap(),
            TurnOutcome::Text("前半后半".to_owned()),
        );

        let call = json!({
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "fc_1",
                "name": "read_file",
                "arguments": "{\"path\":\"b\"}",
            }],
        });
        assert_eq!(
            parse_turn(LlmProtocol::OpenaiResponses, &call).unwrap(),
            TurnOutcome::ToolCalls(vec![ToolCallReq {
                id: "fc_1".to_owned(),
                name: "read_file".to_owned(),
                arguments: r#"{"path":"b"}"#.to_owned(),
            }]),
        );
    }

    #[test]
    fn anthropic_的_tool_use_输入被序列化成字符串() {
        let body = json!({
            "stop_reason": "tool_use",
            "content": [
                { "type": "text", "text": "我来读一下" },
                { "type": "tool_use", "id": "toolu_1", "name": "read_file",
                  "input": { "path": "c.txt" } },
            ],
        });

        assert_eq!(
            parse_turn(LlmProtocol::AnthropicMessages, &body).unwrap(),
            TurnOutcome::ToolCalls(vec![ToolCallReq {
                id: "toolu_1".to_owned(),
                name: "read_file".to_owned(),
                // 与 OpenAI 对齐成字符串，上层只有一条反序列化路径。
                arguments: r#"{"path":"c.txt"}"#.to_owned(),
            }]),
        );
    }

    #[test]
    fn anthropic_的_max_tokens_算截断() {
        let body = json!({
            "stop_reason": "max_tokens",
            "content": [{ "type": "tool_use", "id": "t1", "name": "f", "input": {} }],
        });
        assert!(matches!(
            parse_turn(LlmProtocol::AnthropicMessages, &body).unwrap(),
            TurnOutcome::Truncated(_),
        ));
    }

    #[test]
    fn 响应里的_error_字段折成_upstream_并带原文() {
        let body = json!({ "error": { "message": "insufficient balance", "type": "billing" } });
        for protocol in [
            LlmProtocol::OpenaiChat,
            LlmProtocol::OpenaiResponses,
            LlmProtocol::AnthropicMessages,
        ] {
            let error = parse_turn(protocol, &body).unwrap_err();
            assert_eq!(error.code, "UPSTREAM");
            assert!(error.message.contains("insufficient balance"));
        }

        // 有中转商把 error 塞成裸字符串。
        let raw = json!({ "error": "rate limited" });
        let error = parse_turn(LlmProtocol::OpenaiChat, &raw).unwrap_err();
        assert_eq!(error.code, "UPSTREAM");
        assert!(error.message.contains("rate limited"));
    }

    #[test]
    fn 认不出的形状报_llm_bad_response() {
        let error = parse_turn(LlmProtocol::OpenaiChat, &json!({ "choices": [] })).unwrap_err();
        assert_eq!(error.code, "LLM_BAD_RESPONSE");
    }

    #[test]
    fn openai_chat_带工具调用的历史消息能回放() {
        // assistant 的 tool_calls 与 tool 的 tool_call_id 必须都在，否则上游对不回结果。
        let history = [
            ChatMsg {
                role: "assistant".to_owned(),
                content: String::new(),
                tool_calls: Some(vec![ToolCallReq {
                    id: "call_1".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: r#"{"path":"a"}"#.to_owned(),
                }]),
                tool_call_id: None,
            },
            ChatMsg {
                role: "tool".to_owned(),
                content: "文件内容".to_owned(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_owned()),
            },
        ];
        let body = build_body(LlmProtocol::OpenaiChat, "gpt-4o", &history, &[], false).unwrap();

        assert_eq!(body["messages"][0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            body["messages"][0]["tool_calls"][0]["function"]["arguments"],
            r#"{"path":"a"}"#,
        );
        assert_eq!(body["messages"][1]["tool_call_id"], "call_1");
    }
}
