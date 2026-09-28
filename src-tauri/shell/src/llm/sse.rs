//! 流式响应的解析：字节 → SSE 负载 → 与非流式一致的 [`TurnOutcome`]。
//!
//! 分成两层是因为两层的失败方式完全不同：[`SseParser`] 只会在「行被 chunk 切断」
//! 上出错，[`StreamAccumulator`] 只会在「tool call 分片配对」上出错。合成一层后
//! 这两类 bug 会互相掩盖，谁都测不准。
//!
//! 结论必须与 [`super::protocol::parse_turn`] 逐条对齐 —— 流式和非流式是同一个
//! agent 循环的两条入口，一旦两边对「截断」「有没有工具调用」的判断不同，
//! 用户换个开关就会得到不同行为。

use serde_json::Value;

use crate::credentials::LlmProtocol;
use crate::error::AppError;

use super::protocol::{ToolCallReq, TurnOutcome};

/// OpenAI 用这一行收尾，它不是 JSON。
const DONE_MARKER: &str = "[DONE]";

/// SSE 字节流的行解析器。
///
/// 不按 `\n\n` 切事件块：OpenAI 的每个事件就是单独一个 `data:` 行，而有的服务端
/// 在事件之间只发一个 `\n`，按空行切会一直攒着不吐。
#[derive(Debug, Default)]
pub struct SseParser {
    /// 还没收到行尾的残行。跨 chunk 的半行全靠它兜住。
    buf: Vec<u8>,
}

impl SseParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂一块字节，返回这一块里解析出的完整 `data:` 负载（可能 0 条或多条）。
    ///
    /// 只有找到行尾才切行，剩下的留在缓冲里等下一块：HTTP chunk 的边界与行边界
    /// 毫无关系，一个 JSON 负载被切成两半是常态。顺带也保住了多字节字符 ——
    /// 只在完整行上做 UTF-8 解码，就不会把一个汉字劈成两个替换符。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);

        let mut payloads = Vec::new();
        while let Some(end) = self.buf.iter().position(|byte| *byte == b'\n') {
            let raw = &self.buf[..end];
            // 认 CRLF：不去掉 \r 的话负载尾部会多一个字符，serde 还能过，
            // 但 `[DONE]` 这类字面量比对会静默失配。
            let line = raw.strip_suffix(b"\r").unwrap_or(raw);
            if let Some(payload) = data_payload(line) {
                payloads.push(payload);
            }
            self.buf.drain(..=end);
        }
        payloads
    }
}

/// 取出一行的 `data:` 负载；非负载行返回 `None`。
///
/// `event:` / `id:` / `retry:` / `:` 注释行都在这里被丢掉 —— 它们进不了下游的
/// JSON 反序列化，`[DONE]` 与空 data 行同理。
fn data_payload(line: &[u8]) -> Option<String> {
    let rest = line.strip_prefix(b"data:")?;
    // SSE 规范：冒号后紧跟一个空格时吃掉这一个，再多的空格属于数据本身。
    let rest = rest.strip_prefix(b" ").unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(rest);
    if text.trim() == DONE_MARKER {
        return None;
    }
    Some(text.into_owned())
}

/// 一次工具调用的累加中间态。
///
/// 和 [`ToolCallReq`] 分开：流里 `id` / `name` 只来一次，中途状态允许它们是空的，
/// 而 `ToolCallReq` 是给 `dispatch` 用的成品。
#[derive(Debug)]
struct PartialCall {
    index: i64,
    id: String,
    name: String,
    arguments: String,
}

/// 把流式分片累加成一轮结论。
#[derive(Debug)]
pub struct StreamAccumulator {
    protocol: LlmProtocol,
    text: String,
    /// 按 `index` 累加的工具调用。用 `Vec` 而非 `HashMap`：一轮最多几个调用，
    /// 线性查找更省，而且天然保住上游给的顺序（HashMap 还要再排一次）。
    calls: Vec<PartialCall>,
    /// 最后一个非 null 的 `finish_reason`。`length` 与 `tool_calls` 的区别是安全边界。
    finish_reason: Option<String>,
}

impl StreamAccumulator {
    #[must_use]
    pub fn new(protocol: LlmProtocol) -> Self {
        Self {
            protocol,
            text: String::new(),
            calls: Vec::new(),
            finish_reason: None,
        }
    }

    /// 吃一条 `data:` 负载，返回它带来的新增正文（供上层发 Delta 事件），没有则空串。
    ///
    /// 认不出的形状一律当「没有新增正文」跳过：流里混着 `role` 分片、空 `delta`、
    /// `usage` 统计块和中转商的非 JSON 心跳行，对这些报错等于正常对话必炸。
    ///
    /// # Errors
    ///
    /// 协议不是 `openai-chat` 时返回 `STREAM_UNSUPPORTED`；上游在流里塞了 `error`
    /// 时返回 `UPSTREAM` —— 这条必须立刻往上抛，静默跳过会让用户只看到空回复，
    /// 看不到限流、欠费这类真实原因。
    pub fn push(&mut self, payload: &str) -> Result<String, AppError> {
        if self.protocol != LlmProtocol::OpenaiChat {
            return Err(stream_unsupported());
        }

        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return Ok(String::new());
        };
        if let Some(error) = upstream_error(&value) {
            return Err(error);
        }

        let Some(choice) = value.get("choices").and_then(|choices| choices.get(0)) else {
            return Ok(String::new());
        };
        // as_str 顺手滤掉了中途分片的 `finish_reason: null`，不会把已记下的结论冲掉。
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(reason.to_owned());
        }

        let Some(delta) = choice.get("delta") else {
            return Ok(String::new());
        };
        if let Some(fragments) = delta.get("tool_calls").and_then(Value::as_array) {
            for fragment in fragments {
                self.merge_tool_call(fragment);
            }
        }

        let Some(chunk) = delta.get("content").and_then(Value::as_str) else {
            return Ok(String::new());
        };
        self.text.push_str(chunk);
        Ok(chunk.to_owned())
    }

    /// 按 `index` 配对分片，不按 `id`。
    ///
    /// `id` 与 `function.name` 只在某个 index 的第一个分片里出现，后续分片这两个
    /// 字段是 null，只带 `arguments` 的碎片。按 `id` 配对的话后续碎片全配不上，
    /// 结果是每个碎片各成一次调用、每个的参数都是残缺 JSON。
    fn merge_tool_call(&mut self, fragment: &Value) {
        // 单调用的流有实现不给 index，缺省当 0，否则这一路分片会各自为政。
        let index = fragment.get("index").and_then(Value::as_i64).unwrap_or(0);
        let slot = match self.calls.iter().position(|call| call.index == index) {
            Some(pos) => &mut self.calls[pos],
            None => {
                self.calls.push(PartialCall {
                    index,
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
                let last = self.calls.len() - 1;
                &mut self.calls[last]
            }
        };

        // 空串不覆盖：有的实现每个分片都带 `id: ""`，覆盖会把真 id 抹掉。
        if let Some(id) = fragment.get("id").and_then(Value::as_str)
            && !id.is_empty()
        {
            slot.id = id.to_owned();
        }
        let Some(function) = fragment.get("function") else {
            return;
        };
        if let Some(name) = function.get("name").and_then(Value::as_str)
            && !name.is_empty()
        {
            slot.name = name.to_owned();
        }
        if let Some(part) = function.get("arguments").and_then(Value::as_str) {
            slot.arguments.push_str(part);
        }
    }

    /// 收尾，给出与 [`super::protocol::parse_turn`] 一致的结论。
    ///
    /// # Errors
    ///
    /// 协议不是 `openai-chat` 时返回 `STREAM_UNSUPPORTED`。
    pub fn finish(self) -> Result<TurnOutcome, AppError> {
        if self.protocol != LlmProtocol::OpenaiChat {
            return Err(stream_unsupported());
        }

        let calls: Vec<ToolCallReq> = self
            .calls
            .into_iter()
            .map(|call| ToolCallReq {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
            })
            .collect();

        if !calls.is_empty() {
            // 与非流式同一条折叠规则：被 token 上限截断的参数是残缺 JSON，
            // 交给 dispatch 可能变成「删错目录」，必须由上层拒绝执行。
            if self.finish_reason.as_deref() == Some("length") {
                return Ok(TurnOutcome::Truncated(calls));
            }
            return Ok(TurnOutcome::ToolCalls(calls));
        }
        Ok(TurnOutcome::Text(self.text))
    }
}

/// 只有 `openai-chat` 走流式。
///
/// `build_body` 本来也只允许它带 tools，而流式的价值全在「边出字边跑工具」上；
/// 另两个协议走 `chat::one_shot` 就够，为它们写半个流式实现只是多一处会腐坏的代码。
fn stream_unsupported() -> AppError {
    AppError::new(
        "STREAM_UNSUPPORTED",
        "这个协议不支持流式对话，请改用非流式请求。",
        400,
    )
}

/// 与 `protocol.rs` 里非流式那条判断同形：流里也会出现 HTTP 200 包着的 `error`。
///
/// 那边的同名函数是私有的，这里重写一份而不是把它放开 —— 流式只关心 `error`
/// 这一个字段，共用会把非流式的解析细节也拖进流式路径。
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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    /// 一段真实形状的流：带 event 行、注释行、空行、多字节正文和 `[DONE]`。
    const SAMPLE: &str = concat!(
        "event: message\n",
        r#"data: {"choices":[{"delta":{"role":"assistant","content":"你"},"finish_reason":null}]}"#,
        "\n\n",
        ": keep-alive\n",
        r#"data: {"choices":[{"delta":{"content":"好，世界"},"finish_reason":null}]}"#,
        "\n",
        r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        "\n",
        "data: [DONE]\n",
    );

    fn feed_bytewise(text: &str) -> Vec<String> {
        let mut parser = SseParser::new();
        let mut payloads = Vec::new();
        for byte in text.as_bytes() {
            payloads.extend(parser.feed(&[*byte]));
        }
        payloads
    }

    fn accumulate(payloads: &[&str]) -> StreamAccumulator {
        let mut accumulator = StreamAccumulator::new(LlmProtocol::OpenaiChat);
        for payload in payloads {
            accumulator.push(payload).unwrap();
        }
        accumulator
    }

    #[test]
    fn 逐字节喂与整块喂得到同一串负载() {
        let mut parser = SseParser::new();
        let whole = parser.feed(SAMPLE.as_bytes());

        // 逐字节是最狠的切分：能过它就能过任何 chunk 边界，包括把汉字劈开。
        assert_eq!(feed_bytewise(SAMPLE), whole);
        assert_eq!(whole.len(), 3);
    }

    #[test]
    fn crlf行尾能正确解析() {
        let mut parser = SseParser::new();
        let payloads = parser.feed(
            concat!(
                r#"data: {"choices":[{"delta":{"content":"嗨"}}]}"#,
                "\r\n\r\n",
                "data: [DONE]\r\n",
            )
            .as_bytes(),
        );

        assert_eq!(
            payloads,
            vec![r#"{"choices":[{"delta":{"content":"嗨"}}]}"#.to_owned()]
        );
    }

    #[test]
    fn done标记不作为负载返回() {
        let mut parser = SseParser::new();

        assert_eq!(
            parser.feed(b"data: [DONE]\ndata: {\"tail\":1}\n"),
            vec!["{\"tail\":1}".to_owned()]
        );
    }

    #[test]
    fn 非data行与空data行被丢掉() {
        let mut parser = SseParser::new();
        let payloads =
            parser.feed(b"event: ping\nid: 42\nretry: 3000\n: comment\ndata:\ndata: \ndata: {}\n");

        assert_eq!(payloads, vec!["{}".to_owned()]);
    }

    #[test]
    fn 工具调用分片按index累加成一次调用() {
        let accumulator = accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":null,"function":{"name":null,"arguments":"\"a.txt\""}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":null,"function":{"arguments":"}"}}]},"finish_reason":"tool_calls"}]}"#,
        ]);

        assert_eq!(
            accumulator.finish().unwrap(),
            TurnOutcome::ToolCalls(vec![ToolCallReq {
                id: "call_1".to_owned(),
                name: "read_file".to_owned(),
                arguments: r#"{"path":"a.txt"}"#.to_owned(),
            }])
        );
    }

    #[test]
    fn 两个并发工具调用的参数不串味() {
        let accumulator = accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"list_dir","arguments":"{\"dir\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.txt\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"src\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        ]);

        assert_eq!(
            accumulator.finish().unwrap(),
            TurnOutcome::ToolCalls(vec![
                ToolCallReq {
                    id: "call_a".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: r#"{"path":"a.txt"}"#.to_owned(),
                },
                ToolCallReq {
                    id: "call_b".to_owned(),
                    name: "list_dir".to_owned(),
                    arguments: r#"{"dir":"src"}"#.to_owned(),
                },
            ])
        );
    }

    #[test]
    fn 截断的工具调用折成truncated() {
        let accumulator = accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"write_file","arguments":"{\"path\":\"a"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#,
        ]);

        assert_eq!(
            accumulator.finish().unwrap(),
            TurnOutcome::Truncated(vec![ToolCallReq {
                id: "call_1".to_owned(),
                name: "write_file".to_owned(),
                arguments: r#"{"path":"a"#.to_owned(),
            }])
        );
    }

    #[test]
    fn 流里的error立刻报错() {
        let mut accumulator = StreamAccumulator::new(LlmProtocol::OpenaiChat);
        let error = accumulator
            .push(r#"{"error":{"message":"上游限流"}}"#)
            .unwrap_err();

        assert_eq!(
            (error.code.as_str(), error.status),
            ("UPSTREAM", 502),
            "{}",
            error.message
        );
    }

    #[test]
    fn 纯文本流拼成text且认不出的分片被跳过() {
        let mut accumulator = StreamAccumulator::new(LlmProtocol::OpenaiChat);
        let deltas: Vec<String> = [
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"{"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{}}]}"#,
            "OPENROUTER PROCESSING",
            r#"{"usage":{"total_tokens":12}}"#,
            r#"{"choices":[{"delta":{"content":"，世界"},"finish_reason":"stop"}]}"#,
        ]
        .into_iter()
        .map(|payload| accumulator.push(payload).unwrap())
        .collect();

        assert_eq!(
            deltas,
            vec!["", "你好", "", "", "", "，世界"],
            "只有带 content 的分片才该产出新增正文"
        );
        assert_eq!(
            accumulator.finish().unwrap(),
            TurnOutcome::Text("你好，世界".to_owned())
        );
    }

    #[test]
    fn 非openai协议直接拒绝流式() {
        let mut accumulator = StreamAccumulator::new(LlmProtocol::AnthropicMessages);
        let push_error = accumulator.push(r#"{"choices":[]}"#).unwrap_err();
        let finish_error = accumulator.finish().unwrap_err();

        assert_eq!(
            (push_error.code.as_str(), finish_error.code.as_str()),
            ("STREAM_UNSUPPORTED", "STREAM_UNSUPPORTED")
        );
    }
}
