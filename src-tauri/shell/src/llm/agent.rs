//! 工具调用的回合循环。
//!
//! 一轮用户输入可能要跑多次「问模型 → 执行工具 → 把结果喂回去」，直到模型给出
//! 最终回答。本模块只管这个循环，不碰流式、不碰落库、不碰 Tauri —— 那三件事
//! 各有各的失败模式，混进来会让「循环逻辑对不对」这个问题不可单测。
//!
//! 与模型交互一律经 [`crate::llm::chat::one_shot`]，工具执行一律经
//! [`crate::llm::tools::dispatch`]。本模块自己不发网络请求。

use std::sync::atomic::AtomicBool;

use route_core::config::Config;
use route_core::error::AppError;

use crate::llm::chat::{one_shot, stream_once};
use crate::llm::protocol::{ChatMsg, ToolCallReq, ToolDef, TurnOutcome, Upstream};
use crate::llm::tools::{ToolOutcome, ToolPayload, dispatch, tool_schemas};

/// 一轮用户输入最多问模型几次。
///
/// 超过就停下并告知用户。没有上限的话，一个「反复要求调同一个工具」的模型会把
/// 用户的额度烧光，而界面上只看到一直转圈。5 次足够覆盖「查地点 → 规划 → 补一次
/// 生活圈 → 总结」这类真实链路。
const MAX_ROUNDS: usize = 5;

/// 达到轮次上限时回给用户的话。
///
/// 明说「没能完成」而不是编一个答案 —— 用户需要知道这次结果不可信。
const ROUNDS_EXHAUSTED: &str = "这个问题我连续调用了多次工具仍没能得出结论，先停下来避免无意义的重试。\
请把问题说得更具体一些（比如补上起点或终点的完整名称），或者直接用规划页手动查一次。";

/// 截断的工具调用回灌给模型的说明。
///
/// 关键是告诉它「参数不完整」而不是「参数错误」—— 后者会让它换个值重试，前者
/// 才会让它重新完整地发一次。
const TRUNCATED_NOTE: &str = "上一条回复因长度上限被截断，工具调用的参数不完整，\
已全部作废、一个都没有执行。请重新发起调用，参数写简短一些。";

/// system prompt。
///
/// 三条硬约束，来源与理由：
///
/// 1. **路线与地点必须来自工具**：模型对本地公交线路的记忆既过时又自信，编出来的
///    时长与票价看不出真假（设计方案 §11.2 的「模型编造路线」）。
/// 2. **不念裸经纬度**：各家坐标系不同（腾讯/高德 GCJ-02、百度 BD-09、天地图
///    CGCS2000），模型把 BD-09 当 GPS 讲给用户会偏百米级，而且**不报错、不崩溃、
///    日志里也看不出来**（§5.3 第 3 条）。坐标只是工具之间的中间值。
/// 3. **工具报错要看 code**：错误里带了中文 message 与机器码，照着改参数重试比
///    换个说法重试有效。
const SYSTEM_PROMPT: &str = "你是 AutoBee 的出行助手，帮用户查地点、规划公共交通与步行路线、\
了解一个位置周边的生活设施。

硬性要求：

1. 路线、时长、票价、距离、地点信息**一律来自工具返回值**，绝不凭记忆作答。没调工具就别给具体数字。
2. **回答里不要出现经纬度数字。** 工具返回的坐标只用于你再次调用工具，地点一律用名称指代。\
不同地图服务商的坐标系不同，把坐标念给用户会造成百米级的误导且无法被发现。
3. 工具报错时错误信息里有机器码与中文说明，照它改参数重试；同一个参数不要重复试第二次。
4. 用中文回答，简洁。把工具给的分段信息讲成人能照着走的步骤，不要罗列原始字段。";

/// 跑一轮对话所需的外部依赖。
///
/// 借用而不持有：调用方（Tauri command）本来就持着 `Config` 与密钥，复制一遍
pub struct AgentDeps<'a> {
    /// 地图侧配置，传给内置工具 [`dispatch`]。
    pub config: &'a Config,
    pub up: Upstream<'a>,
    /// 当前对话发现到的 MCP 工具与连接。
    pub mcp: &'a mut crate::llm::mcp::McpRegistry,
}

/// 循环过程中发出的事件，供上层做流式推送或打日志。
///
/// 非流式调用方可以把回调写成空实现，循环本身不依赖有人在听。
///
/// 没有 `Eq`:[`ToolPayload`] 里的路线含 `f64` 距离与坐标。浮点没有自反等价,
/// 硬加 `Eq` 是在断言一件不成立的事。
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// 新增的一段正文。非流式下整段一次给出。
    Delta(String),
    /// 要开始执行某个工具了。`args` 是原始 JSON 串，便于排查参数问题。
    ToolStart { tool: String, args: String },
    /// 某个工具执行完了。`ok` 为假时 `summary` 是错误说明。
    ToolEnd {
        tool: String,
        ok: bool,
        summary: String,
        /// 给地图回灌用的原始结构,**未经裁剪**(6.4.2)。
        ///
        /// `summary` 是给人和模型看的,这个是给地图看的。两者分开,免得前端
        /// 去解析那句中文。失败、截断、以及 `search_place` 都是 `None`。
        payload: Option<ToolPayload>,
    },
    /// 本轮结束，`text` 是最终回答。
    Done { text: String },
    /// 本轮失败。
    Failed { code: String, message: String },
}

/// 一轮对话的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnResult {
    /// 最终回答，直接显示给用户。
    pub reply: String,
    /// 本轮新增的消息，**不含** system prompt，含用户那条。
    ///
    /// 调用方按这个列表落库即可，不需要自己推断中间产生了几条 tool 消息。
    pub appended: Vec<ChatMsg>,
}

/// 跑一轮对话：问模型，按需执行工具，直到拿到最终回答或触到轮次上限。
///
/// `history` 是本会话之前的消息（不含 system prompt，本函数自己加）。
///
/// 本函数只负责把真实的网络调用与工具执行接到 [`run_loop`] 上。循环逻辑在
/// `run_loop` 里，那里不碰 IO，所以「截断的调用是否真的没执行」这类判据能被单测
/// 直接断言 —— 放在这里就只能靠真实 key 跑一次去看，那不是判据。
///
/// # Errors
///
/// 网络失败、上游返回错误、协议不支持工具时返回错误。**工具执行失败不算错误** ——
/// 那是要回灌给模型让它重试的正常流程。
pub fn run_turn(
    deps: &mut AgentDeps,
    history: &[ChatMsg],
    user_text: &str,
    on_event: &mut dyn FnMut(AgentEvent),
) -> Result<TurnResult, AppError> {
    let mut tools = tool_defs();
    tools.extend(deps.mcp.definitions());
    let mut exec = |name: &str, args: &str| match dispatch(deps.config, name, args) {
        Ok(outcome) => Ok(outcome),
        Err(error) if name.starts_with("mcp_") => {
            let arguments: serde_json::Value = serde_json::from_str(args).map_err(|parse| {
                AppError::new(
                    "BAD_TOOL_ARGS",
                    format!("工具参数不是合法 JSON：{parse}"),
                    400,
                )
            })?;
            deps.mcp.call(name, &arguments).map(|summary| ToolOutcome {
                summary,
                payload: None,
            })
        }
        Err(error) => Err(error),
    };
    let mut ask = |messages: &[ChatMsg], on_event: &mut dyn FnMut(AgentEvent)| {
        let outcome = one_shot(deps.up, messages, &tools)?;
        // 非流式也发 Delta：界面只订阅事件，不必为两条路写两套渲染。
        if let TurnOutcome::Text(text) = &outcome {
            on_event(AgentEvent::Delta(text.clone()));
        }
        Ok(outcome)
    };
    run_loop(&mut ask, &mut exec, history, user_text, on_event)
}

/// 跑一轮对话，正文边生成边通过 [`AgentEvent::Delta`] 推出去。
///
/// 与 [`run_turn`] 的唯一差别是「问模型」那一步走流式。回合逻辑、工具执行、
/// 落库列表的构造完全共用 [`run_loop`] —— 流式与非流式给出不同结论是不可能的。
///
/// `cancelled` 由调用方持有并在用户点「停止」时置真。工具执行期间不检查：
/// 一次工具调用是几百毫秒的 HTTP，中途放弃只会留下半个结果。
///
/// # Errors
///
/// 同 [`run_turn`]。另外被取消时返回 `CANCELLED`（status 499）。
pub fn run_turn_streaming(
    deps: &mut AgentDeps,
    history: &[ChatMsg],
    user_text: &str,
    cancelled: &AtomicBool,
    on_event: &mut dyn FnMut(AgentEvent),
) -> Result<TurnResult, AppError> {
    let mut tools = tool_defs();
    tools.extend(deps.mcp.definitions());
    let mut ask = |messages: &[ChatMsg], sink: &mut dyn FnMut(AgentEvent)| {
        let mut on_delta = |chunk: &str| sink(AgentEvent::Delta(chunk.to_owned()));
        stream_once(deps.up, messages, &tools, cancelled, &mut on_delta)
    };
    let mut exec = |name: &str, args: &str| match dispatch(deps.config, name, args) {
        Ok(outcome) => Ok(outcome),
        Err(error) if name.starts_with("mcp_") => {
            let arguments: serde_json::Value = serde_json::from_str(args).map_err(|parse| {
                AppError::new(
                    "BAD_TOOL_ARGS",
                    format!("工具参数不是合法 JSON：{parse}"),
                    400,
                )
            })?;
            deps.mcp.call(name, &arguments).map(|summary| ToolOutcome {
                summary,
                payload: None,
            })
        }
        Err(error) => Err(error),
    };
    run_loop(&mut ask, &mut exec, history, user_text, on_event)
}

/// 「问一次模型」的注入点。
///
/// 第二个参数是事件槽：非流式的实现拿到完整正文后发一次 [`AgentEvent::Delta`]，
/// 流式的实现边读边发。循环本身不区分两者。
type Ask<'a> =
    dyn FnMut(&[ChatMsg], &mut dyn FnMut(AgentEvent)) -> Result<TurnOutcome, AppError> + 'a;

/// 「执行一次工具」的注入点。参数是工具名与 JSON 参数串。
type Exec<'a> = dyn FnMut(&str, &str) -> Result<ToolOutcome, AppError> + 'a;

/// 回合循环本体，不碰网络也不碰地图服务。
///
/// `ask` 问一次模型，`exec` 执行一次工具。两者都注入进来是为了让循环可测：
/// 三条判据（轮次上限、工具报错回灌、截断整批作废）全都是循环的行为，
/// 真实 IO 在场时无法确定性地触发。
///
/// `ask` 也拿到事件槽，因为 [`AgentEvent::Delta`] 是「文本怎么到的」——
/// 非流式一次一整段、流式一块一块，只有 `ask` 知道。循环只管发
/// [`AgentEvent::Done`]（本轮结束了），这样两条路都不会重复推同一段正文。
fn run_loop(
    ask: &mut Ask<'_>,
    exec: &mut Exec<'_>,
    history: &[ChatMsg],
    user_text: &str,
    on_event: &mut dyn FnMut(AgentEvent),
) -> Result<TurnResult, AppError> {
    // 发给模型的完整上下文：system 每次重新加，不进 appended、不落库 ——
    // 改了 prompt 之后老会话也该用新的。
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(ChatMsg::text("system", SYSTEM_PROMPT));
    messages.extend_from_slice(history);

    let user = ChatMsg::text("user", user_text);
    messages.push(user.clone());

    // 只收本轮新增的，用户那条算本轮的第一条。
    let mut appended = vec![user];

    for _ in 0..MAX_ROUNDS {
        let outcome = match ask(&messages, on_event) {
            Ok(outcome) => outcome,
            Err(error) => {
                on_event(AgentEvent::Failed {
                    code: error.code.clone(),
                    message: error.message.clone(),
                });
                return Err(error);
            }
        };

        match outcome {
            TurnOutcome::Text(text) => {
                // Delta 已由 ask 发过，这里只宣告本轮结束。
                on_event(AgentEvent::Done { text: text.clone() });
                appended.push(ChatMsg::text("assistant", text.clone()));
                return Ok(TurnResult {
                    reply: text,
                    appended,
                });
            }
            TurnOutcome::Truncated(calls) => {
                // 参数被截断的 tool call 仍然能解析成合法 JSON（流式累加器会把残缺
                // 内容补成语法正确的串），所以「能解析」证明不了「完整」。整批作废是
                // 唯一挡得住的做法：执行一个参数被截在 `{"from":"孝陵卫","to":"南京南`
                // 的调用，补全后是个真实地名，会算出一条去错地方的路线并被自信地报给
                // 用户 —— 这是无声错误，比报错糟得多。
                record_call_round(&mut messages, &mut appended, &calls);
                for call in &calls {
                    on_event(AgentEvent::ToolEnd {
                        tool: call.name.clone(),
                        ok: false,
                        summary: TRUNCATED_NOTE.to_owned(),
                        // 没执行,没有结构可回灌。
                        payload: None,
                    });
                    push_tool_result(&mut messages, &mut appended, call, TRUNCATED_NOTE);
                }
            }
            TurnOutcome::ToolCalls(calls) => {
                record_call_round(&mut messages, &mut appended, &calls);
                for call in &calls {
                    on_event(AgentEvent::ToolStart {
                        tool: call.name.clone(),
                        args: call.arguments.clone(),
                    });

                    // 工具报错也回灌，不中断本轮：错误里带机器码与中文说明，模型能
                    // 照着改参数重试。直接把错误抛给用户等于让人去猜该怎么改。
                    //
                    // 成功时 summary 与 payload 分两路走:前者进 messages(裁剪过,
                    // 省上下文),后者随事件回前端给地图(未裁剪,折线点是全的)。
                    let (ok, summary, payload) = match exec(&call.name, &call.arguments) {
                        Ok(outcome) => (true, outcome.summary, outcome.payload),
                        Err(error) => (false, error_payload(&error), None),
                    };

                    on_event(AgentEvent::ToolEnd {
                        tool: call.name.clone(),
                        ok,
                        summary: summary.clone(),
                        payload,
                    });
                    push_tool_result(&mut messages, &mut appended, call, &summary);
                }
            }
        }
    }

    // 到这里说明每一轮模型都还在要求调工具。停下并说清楚，不要编一个答案。
    on_event(AgentEvent::Done {
        text: ROUNDS_EXHAUSTED.to_owned(),
    });
    appended.push(ChatMsg::text("assistant", ROUNDS_EXHAUSTED));
    Ok(TurnResult {
        reply: ROUNDS_EXHAUSTED.to_owned(),
        appended,
    })
}

/// `tool_schemas` 的 JSON 转成 [`ToolDef`]。
///
/// schema 由本仓库自己生成，字段一定在；真缺了就说明 `tools.rs` 被改坏了，
/// 这里静默跳过比报错好 —— 剩下的工具还能用。
fn tool_defs() -> Vec<ToolDef> {
    tool_schemas()
        .into_iter()
        .filter_map(|schema| {
            Some(ToolDef {
                name: schema.get("name")?.as_str()?.to_owned(),
                description: schema.get("description")?.as_str()?.to_owned(),
                parameters: schema.get("parameters")?.clone(),
            })
        })
        .collect()
}

/// 记一轮「模型要求调工具」的 assistant 消息。
///
/// 必须同时进 `messages`（下一轮上下文）与 `appended`（落库）：少了前者模型看不到
/// 自己刚发的调用，会重复发一遍；少了后者界面回放时 tool 结果凭空出现。
fn record_call_round(
    messages: &mut Vec<ChatMsg>,
    appended: &mut Vec<ChatMsg>,
    calls: &[ToolCallReq],
) {
    let msg = ChatMsg {
        role: "assistant".to_owned(),
        content: String::new(),
        tool_calls: Some(calls.to_vec()),
        tool_call_id: None,
    };
    messages.push(msg.clone());
    appended.push(msg);
}

/// 记一条工具结果。
///
/// `tool_call_id` 必填：OpenAI 用它把结果与调用配对，缺了会报 400。
fn push_tool_result(
    messages: &mut Vec<ChatMsg>,
    appended: &mut Vec<ChatMsg>,
    call: &ToolCallReq,
    content: &str,
) {
    let msg = ChatMsg {
        role: "tool".to_owned(),
        content: content.to_owned(),
        tool_calls: None,
        tool_call_id: Some(call.id.clone()),
    };
    messages.push(msg.clone());
    appended.push(msg);
}

/// 工具错误序列化成喂回模型的文本。
///
/// 带上机器码：模型看到 `BAD_MODE` 会去查可选值，看到一句中文「参数有误」只会
/// 换个说法再试一遍同样的参数。
fn error_payload(error: &AppError) -> String {
    format!(
        "{{\"error\":{{\"code\":\"{}\",\"message\":{}}}}}",
        error.code,
        // 走 serde 转义：中文 message 里可能有引号或换行，手拼会造出非法 JSON。
        serde_json::Value::String(error.message.clone())
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "测试里断言失败即测试失败"
)]
mod tests {
    use super::*;

    use route_core::models::{
        Crs, Endpoint, LatLng, ProviderId, Route, RoutePlan, Segment, SegmentKind, TravelMode,
    };

    fn call(id: &str, name: &str, args: &str) -> ToolCallReq {
        ToolCallReq {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: args.to_owned(),
        }
    }

    /// 只有文字、不带回灌载荷的工具结果。多数循环判据与载荷无关。
    fn outcome(summary: &str) -> ToolOutcome {
        ToolOutcome {
            summary: summary.to_owned(),
            payload: None,
        }
    }

    /// 一条带 `count` 个折线点的最小路线。只有点数是被断言的量,其余字段取满足
    /// 类型的最简值。
    fn plan_with_points(count: usize) -> RoutePlan {
        let points: Vec<LatLng> = (0..count)
            .map(|step| LatLng::new(30.5 + step as f64 * 1e-4, 114.4))
            .collect();
        let endpoint = |title: &str| Endpoint {
            title: title.to_owned(),
            location: LatLng::new(30.5, 114.4),
        };
        RoutePlan {
            origin: endpoint("光谷广场"),
            destination: endpoint("汉口火车站"),
            mode: TravelMode::Subway,
            routes: vec![Route {
                id: "r1".to_owned(),
                distance: 9_600,
                duration: 34,
                price: 3.0,
                bounds: [LatLng::new(30.5, 114.36), LatLng::new(30.54, 114.4)],
                segments: vec![Segment {
                    kind: SegmentKind::Transit,
                    distance: 9_000,
                    duration: 25,
                    points,
                    instruction: Vec::new(),
                    line: None,
                }],
            }],
            provider: ProviderId::Baidu,
            crs: Crs::Bd09,
            degraded: None,
        }
    }

    /// 一次循环的全部可观察产出。
    struct Driven {
        result: Result<TurnResult, AppError>,
        /// 工具**实际被执行**的名字，按顺序。
        ///
        /// 这是三条判据里最关键的观察点 —— 「整批作废」与「执行了但结果没被采用」
        /// 在消息列表上看不出差别，只有这里能分开。
        executed: Vec<String>,
        events: Vec<AgentEvent>,
    }

    /// 把一串预设的 `TurnOutcome` 依次回给循环。脚本用完后一律回一句最终回答，
    /// 免得测「轮次上限」以外的用例意外跑满 5 轮。
    fn drive(script: Vec<TurnOutcome>, tool_result: &Result<ToolOutcome, AppError>) -> Driven {
        use std::cell::RefCell;

        let mut turns = script.into_iter();
        let mut ask = move |_: &[ChatMsg], _: &mut dyn FnMut(AgentEvent)| {
            Ok(turns.next().unwrap_or(TurnOutcome::Text("收尾".to_owned())))
        };

        let executed = RefCell::new(Vec::new());
        let events = RefCell::new(Vec::new());
        let mut exec = |name: &str, _args: &str| {
            executed.borrow_mut().push(name.to_owned());
            tool_result.clone()
        };
        let mut on_event = |event: AgentEvent| events.borrow_mut().push(event);

        let result = run_loop(&mut ask, &mut exec, &[], "从孝陵卫到南京南", &mut on_event);
        Driven {
            result,
            executed: executed.into_inner(),
            events: events.into_inner(),
        }
    }

    #[test]
    fn 截断时整批工具调用一个都不执行() {
        // 判据第三条，也是整个 M3 唯一挡得住那个无声错误的东西。
        // 参数被截在 `{"from":"孝陵卫","to":"南京南` 仍能解析成合法 JSON，
        // 补全后是个真实地名，执行下去会算出一条去错地方的路线并被自信地报给用户。
        let driven = drive(
            vec![TurnOutcome::Truncated(vec![
                call("c1", "plan_route", r#"{"from":"孝陵卫","to":"南京南"#),
                call("c2", "search_place", r#"{"keyword":"南京"#),
            ])],
            &Ok(outcome("不该被看到")),
        );
        assert_eq!(driven.executed, Vec::<String>::new());

        // 而且两个调用都要作为失败回灌，不能悄悄丢掉 —— 模型得知道该重发。
        let failed: Vec<&str> = driven
            .events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolEnd {
                    tool, ok: false, ..
                } => Some(tool.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(failed, vec!["plan_route", "search_place"]);
    }

    #[test]
    fn 循环自己不发_delta_只发_done() {
        // `drive` 的假 `ask` 不发 Delta，所以循环若自己补一个，这里就会看到。
        // 破坏这条不变式的后果是界面上正文重复一遍：流式 `ask` 已经逐块发过了，
        // 循环再按最终文本发一次，用户看到同一段话出现两次。
        let driven = drive(
            vec![TurnOutcome::Text("建议走 2 号线".to_owned())],
            &Ok(outcome("")),
        );

        let deltas = driven
            .events
            .iter()
            .filter(|e| matches!(e, AgentEvent::Delta(_)))
            .count();
        assert_eq!(deltas, 0, "正文该由 ask 发，循环只宣告结束");
        assert_eq!(
            driven.events,
            vec![AgentEvent::Done {
                text: "建议走 2 号线".to_owned()
            }]
        );
    }

    #[test]
    fn 工具报错作为_tool_消息回灌而不中断本轮() {
        // 判据第二条。工具失败不是本轮失败：错误里带机器码，模型照着改参数重试。
        let driven = drive(
            vec![
                TurnOutcome::ToolCalls(vec![call("c1", "plan_route", r#"{"mode":"飞机"}"#)]),
                TurnOutcome::Text("已改用地铁方案".to_owned()),
            ],
            &Err(AppError::new("BAD_MODE", "不认识的出行方式「飞机」", 400)),
        );
        let turn = driven.result.unwrap();

        // 工具确实被执行了（判据一与判据三的区别就在这里）。
        assert_eq!(driven.executed, vec!["plan_route".to_owned()]);

        let tool_msgs: Vec<&ChatMsg> = turn.appended.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(
            (
                tool_msgs.len(),
                tool_msgs[0].content.contains("BAD_MODE"),
                tool_msgs[0].tool_call_id.as_deref(),
                turn.reply.as_str(),
            ),
            (1, true, Some("c1"), "已改用地铁方案")
        );
    }

    /// M4-5 的核心不变式:载荷与 summary 分两路,**载荷不受裁剪影响**。
    ///
    /// 只断言「事件带了 plan」会漏掉真正会出事的情况:把裁剪版误当载荷发出去。
    /// 那时地图照样画得出线,但折线点数少一截 —— 线歪了而不报错。所以这里比的是
    /// 点数:载荷里是原始的 600 点,而进 messages 的 summary 里没有点数组。
    #[test]
    fn 回灌载荷保留完整折线而_messages_里没有折线点() {
        let plan = plan_with_points(600);
        let original_points = plan.routes[0].segments[0].points.len();
        let summary = r#"{"routes":[{"segments":[{"pointCount":600}]}]}"#;

        let driven = drive(
            vec![
                TurnOutcome::ToolCalls(vec![call("c1", "plan_route", "{}")]),
                TurnOutcome::Text("建议走 2 号线".to_owned()),
            ],
            &Ok(ToolOutcome {
                summary: summary.to_owned(),
                payload: Some(ToolPayload::Plan(Box::new(plan))),
            }),
        );
        let turn = driven.result.unwrap();

        let carried = driven
            .events
            .iter()
            .find_map(|event| match event {
                AgentEvent::ToolEnd {
                    payload: Some(ToolPayload::Plan(plan)),
                    ..
                } => Some(plan.routes[0].segments[0].points.len()),
                _ => None,
            })
            .expect("plan_route 成功必须带回灌载荷");
        assert_eq!(
            (carried, original_points),
            (600, 600),
            "载荷折线点数必须与规划原结果一致,否则地图画出的线是残的"
        );

        // 另一路:喂回模型的那份不含折线点数组。
        let tool_msg = turn
            .appended
            .iter()
            .find(|m| m.role == "tool")
            .expect("工具结果必须落库");
        assert!(
            !tool_msg.content.contains("\"points\""),
            "裁剪版不该把折线点喂给模型"
        );
    }

    /// 失败与截断都不带载荷:没有结果可画。带了载荷就会把上一次的路线又画一遍,
    /// 用户以为这次算出来了。
    #[test]
    fn 工具失败与截断都不带回灌载荷() {
        let failed = drive(
            vec![TurnOutcome::ToolCalls(vec![call("c1", "plan_route", "{}")])],
            &Err(AppError::new("BAD_MODE", "不认识的出行方式", 400)),
        );
        let truncated = drive(
            vec![TurnOutcome::Truncated(vec![call(
                "c2",
                "plan_route",
                r#"{"from":"孝陵卫"#,
            )])],
            &Ok(outcome("不该被看到")),
        );

        let payloads: Vec<bool> = failed
            .events
            .iter()
            .chain(truncated.events.iter())
            .filter_map(|event| match event {
                AgentEvent::ToolEnd { payload, .. } => Some(payload.is_some()),
                _ => None,
            })
            .collect();
        assert_eq!(payloads, vec![false, false]);
    }

    #[test]
    fn 轮次上限触发时给出提示而不是继续循环() {
        // 判据第一条。脚本给足 MAX_ROUNDS + 3 轮「还要调工具」，循环必须自己停。
        let script: Vec<TurnOutcome> = (0..MAX_ROUNDS + 3)
            .map(|i| TurnOutcome::ToolCalls(vec![call(&format!("c{i}"), "search_place", "{}")]))
            .collect();
        let driven = drive(script, &Ok(outcome(r#"{"places":[]}"#)));
        let turn = driven.result.unwrap();

        assert_eq!(
            (driven.executed.len(), turn.reply.as_str()),
            (MAX_ROUNDS, ROUNDS_EXHAUSTED)
        );
    }

    #[test]
    fn 最终回答直接返回且只追加一条_assistant() {
        let driven = drive(
            vec![TurnOutcome::Text("地铁 2 号线直达，约 35 分钟".to_owned())],
            &Ok(outcome("")),
        );
        let turn = driven.result.unwrap();
        let roles: Vec<&str> = turn.appended.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            (roles, driven.executed.len(), turn.reply.as_str()),
            (vec!["user", "assistant"], 0, "地铁 2 号线直达，约 35 分钟")
        );
    }

    #[test]
    fn 上游失败时发_failed_事件并把错误原样抛出() {
        // 网络失败要让用户看到原因。吞掉错误换成一句「没能回答」等于把可排查的
        // 问题变成不可排查的。
        let mut ask = |_: &[ChatMsg], _: &mut dyn FnMut(AgentEvent)| {
            Err(AppError::new("UPSTREAM", "502 上游拒绝", 502))
        };
        let mut exec = |_: &str, _: &str| Ok(outcome(""));
        let mut events = Vec::new();
        // 作用域收住可变借用，出了块才能读 events。
        let result = {
            let mut on_event = |e: AgentEvent| events.push(e);
            run_loop(&mut ask, &mut exec, &[], "你好", &mut on_event)
        };

        assert_eq!(
            (
                result.err().map(|e| e.code),
                events
                    .iter()
                    .filter(|e| matches!(e, AgentEvent::Failed { .. }))
                    .count()
            ),
            (Some("UPSTREAM".to_owned()), 1)
        );
    }

    #[test]
    fn system_prompt_不进落库列表() {
        // 进了落库列表的后果：下一轮 history 里带着旧 prompt，改了 prompt 也不生效，
        // 而且界面上会渲染出一条 system 气泡。
        let driven = drive(vec![TurnOutcome::Text("好".to_owned())], &Ok(outcome("")));
        let turn = driven.result.unwrap();
        assert!(!turn.appended.iter().any(|m| m.role == "system"));
    }

    #[test]
    fn tool_defs_覆盖三个工具且参数非空() {
        let defs = tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["plan_route", "search_place", "life_circle"]);
        // schema 丢了 parameters 会让模型完全不知道该传什么，静默失败。
        assert!(defs.iter().all(|d| d.parameters.is_object()));
        assert!(defs.iter().all(|d| !d.description.is_empty()));
    }

    #[test]
    fn 工具结果消息带上_tool_call_id() {
        let mut messages = Vec::new();
        let mut appended = Vec::new();
        push_tool_result(
            &mut messages,
            &mut appended,
            &call("call_1", "plan_route", "{}"),
            "结果",
        );
        assert_eq!(
            (
                messages.len(),
                messages[0].role.as_str(),
                messages[0].tool_call_id.as_deref()
            ),
            (1, "tool", Some("call_1"))
        );
        // 两个列表必须同步，少一处就是上面注释里那两种病。
        assert_eq!(appended, messages);
    }

    #[test]
    fn assistant_调用轮同时进上下文与落库列表() {
        let mut messages = Vec::new();
        let mut appended = Vec::new();
        let calls = vec![call("c1", "plan_route", "{\"from\":\"a\"}")];
        record_call_round(&mut messages, &mut appended, &calls);
        assert_eq!(
            (
                messages.len(),
                appended.len(),
                messages[0].role.as_str(),
                messages[0].tool_calls.as_deref().map(<[_]>::len)
            ),
            (1, 1, "assistant", Some(1))
        );
    }

    #[test]
    fn 错误回灌带机器码且是合法_json() {
        let payload = error_payload(&AppError::new(
            "BAD_MODE",
            "不认识的出行方式「飞机」\n可选:transit / walking",
            400,
        ));
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            (
                parsed["error"]["code"].as_str(),
                parsed["error"]["message"]
                    .as_str()
                    .map(|m| m.contains("飞机")),
            ),
            (Some("BAD_MODE"), Some(true))
        );
    }

    #[test]
    fn 带引号与换行的错误文案不会造出非法_json() {
        // 手拼字符串在这里会炸，这条测的就是「走了 serde 转义」。
        let payload = error_payload(&AppError::new(
            "BAD_ARGS",
            "参数 \"from\" 缺失\t制表符也要能过",
            400,
        ));
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            parsed["error"]["message"].as_str(),
            Some("参数 \"from\" 缺失\t制表符也要能过")
        );
    }

    #[test]
    fn system_prompt_含坐标系硬约束() {
        // 这条约束是 §5.3 静默偏移的三道防线之一，改 prompt 时不能顺手删掉。
        assert_eq!(
            (
                SYSTEM_PROMPT.contains("经纬度"),
                SYSTEM_PROMPT.contains("名称指代"),
                SYSTEM_PROMPT.contains("一律来自工具"),
            ),
            (true, true, true)
        );
    }

    #[test]
    fn 轮次上限是有限的且提示语不空() {
        assert_eq!((MAX_ROUNDS > 0, ROUNDS_EXHAUSTED.is_empty()), (true, false));
    }
}
