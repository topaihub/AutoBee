//! 大模型的 command：入口 CRUD、拉模型列表、保存模型选择、会话读写、对话一轮。
//!
//! 通用约定（`spawn_blocking`、每次重新读配置）见父模块 [`crate::commands`]。

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use autobee_shell::credentials::store::Store;
use autobee_shell::credentials::{LlmModelInput, LlmProviderDto, LlmProviderInput};
use autobee_shell::error::AppError;
use autobee_shell::llm::agent::{AgentDeps, AgentEvent, run_turn_streaming};
use autobee_shell::llm::conversations::{self, ChatMessageDto, ConversationDto};
use autobee_shell::llm::mcp::McpRegistry;
use autobee_shell::llm::protocol::{ChatMsg, ToolCallReq, Upstream};
use autobee_shell::llm::tools::ToolPayload;
use autobee_shell::llm::{probe, store as llm_store};
use route_core::models::{LifeCircle, RoutePlan};

/// 后台任务本身崩了（panic）时的兜底。
fn join_failed(detail: &str) -> AppError {
    AppError::new("INTERNAL", format!("后台任务失败：{detail}"), 500)
}

/// 列出全部大模型服务入口（Key 已掩码，各自带上挑中的模型）。
///
/// # Errors
///
/// 配置库读不出来时返回错误。
#[tauri::command]
pub async fn list_llm_providers() -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(|| llm_store::list(Store::open()?.conn()))
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 保存一个入口。空 Key 表示不改动原 Key。
///
/// # Errors
///
/// 名称或 `base_url` 为空、密钥加密失败、SQL 失败时返回错误。
#[tauri::command]
pub async fn save_llm_provider(input: LlmProviderInput) -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        llm_store::save(store.conn(), &input)?;
        llm_store::list(store.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 删掉一个入口。它挑中的模型行由外键级联删掉。
///
/// # Errors
///
/// SQL 失败时返回错误。
#[tauri::command]
pub async fn delete_llm_provider(id: String) -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        llm_store::delete(store.conn(), &id)?;
        tracing::info!(provider = %id, "已删除大模型服务入口");
        llm_store::list(store.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 拉某个入口可用的模型列表。**不落库** —— 结果只供当次挑选。
///
/// # Errors
///
/// 入口不存在、没填 Key、网络或鉴权失败、返回空列表时返回错误。
#[tauri::command]
pub async fn fetch_llm_models(id: String) -> Result<Vec<String>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        let (base_url, protocol) = llm_store::endpoint_of(store.conn(), &id)?;
        let key = llm_store::secret_of(store.conn(), &id)?;
        probe::fetch_models(&base_url, protocol, key.as_deref())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 覆盖某个入口挑中的模型列表。
///
/// 是覆盖而不是追加：界面上那是一组多选框，取消勾选的必须真的消失。
///
/// # Errors
///
/// SQL 失败时返回错误。
#[tauri::command]
pub async fn save_llm_models(
    provider_id: String,
    models: Vec<LlmModelInput>,
) -> Result<Vec<LlmProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        llm_store::save_models(store.conn(), &provider_id, &models)?;
        llm_store::list(store.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 前端指定用哪个模型。两段都要：同一个模型标识可能挂在多个入口下。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub provider_id: String,
    pub model: String,
}

/// `llm_chat_start` 的立即返回值。
///
/// §6.2 原文写「返回 `String`(streamId)」。**多回两个字段**，理由是原文那个形状
/// 有个漏洞：会话是后端新建的，若 `convId` 只随成功事件回去，那么**上游失败时前端
/// 永远拿不到它** —— 用户那句话已经落库了，下一句又会新建一个会话，库里于是攒下
/// 一串各含一条孤立用户消息的会话。所以建会话与落用户消息放在 spawn 之前做完
/// （都是本地 SQLite，微秒级），`convId` 与那条消息的真实 id 一并同步返回。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStartHandle {
    /// 订阅 `llm:chat:<streamId>` 用，也是 `llm_chat_cancel` 的入参。
    pub stream_id: String,
    pub conv_id: String,
    /// 已落库的用户消息。前端拿它替换乐观气泡，省一次回读。
    pub user_message: ChatMessageDto,
}

/// 推给前端的流式事件，对应 `types.ts` 的 `ChatStreamEvent`。
///
/// `kind` 做外部标签而非内部 `type`：前端已按 `kind` 写好可辨识联合（6.3）。
///
/// 没有 `Eq`:`ToolEnd` 的路线载荷含 `f64`。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatStreamEvent {
    /// 新增的一段正文。前端**追加**而不是替换。
    Delta {
        text: String,
    },
    ToolStart {
        tool: String,
        args: String,
    },
    ToolEnd {
        tool: String,
        ok: bool,
        summary: String,
        /// `plan_route` 成功时的完整路线,前端据此写 `planner.plan` 并画折线（6.4.2）。
        ///
        /// 摊平成两个可选字段而不是一个带标签的对象:文档 6.4.2 的联合成员就是
        /// 这个形状,前端 `toolEnd` 分支直接取字段,不用再拆一层。
        #[serde(skip_serializing_if = "Option::is_none")]
        plan: Option<RoutePlan>,
        /// `life_circle` 成功时的完整生活圈,前端据此写 `planner.circleResults`。
        #[serde(skip_serializing_if = "Option::is_none")]
        circle: Option<LifeCircle>,
    },
    /// 本轮结束。`messages` 是本轮新增的 assistant 与 tool 行（**不含**用户那条，
    /// 它在 `ChatStartHandle` 里已经给过）。前端用它把累加出来的临时正文换成落库结果。
    Done {
        messages: Vec<ChatMessageDto>,
    },
    /// 本轮失败。必须带 `code`：前端按 code 分支，匹配中文会在改文案时静默失效。
    Error {
        code: String,
        message: String,
    },
}

/// 正在跑的流的取消标志表。
///
/// `llm_chat_start` 每起一条流就登记一个标志，流结束时**无论成败都摘掉**
/// （见 `finish_stream`）—— 不摘会让这张表随会话数无界增长。
#[derive(Default)]
pub struct StreamRegistry(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

/// 列出全部会话，最近聊过的排最前。
///
/// # Errors
///
/// 配置库读不出来时返回错误。
#[tauri::command]
pub async fn list_conversations() -> Result<Vec<ConversationDto>, AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        conversations::list_conversations(Store::open()?.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 回读一个会话的全部消息，按写入先后。
///
/// # Errors
///
/// 配置库读不出来时返回错误。
#[tauri::command]
pub async fn read_conversation(id: String) -> Result<Vec<ChatMessageDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        conversations::read_conversation(Store::open()?.conn(), &id)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 读取某个会话最后一次完整路线快照。旧会话没有快照时返回 `None`。
///
/// # Errors
///
/// 配置库读不出来或快照损坏时返回错误。
#[tauri::command]
pub async fn read_conversation_route(id: String) -> Result<Option<RoutePlan>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        conversations::read_route_snapshot(Store::open()?.conn(), &id)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 删掉一个会话。消息行由外键级联删掉。
///
/// # Errors
///
/// SQL 失败时返回错误。
#[tauri::command]
pub async fn delete_conversation(id: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        conversations::delete_conversation(Store::open()?.conn(), &id)?;
        tracing::info!(conversation = %id, "已删除会话");
        Ok(())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 起一轮流式对话：落库用户输入 → 立即返回 → 正文与工具进展走 event。
///
/// `conv_id` 为 `None` 时新建会话，标题取用户首句。
///
/// # 为什么先落库再发请求
///
/// 顺序反了的话，请求失败时用户刚打的那句话不在库里，界面看起来像根本没发出去 ——
/// 而他已经按过发送键了。先落库则失败后重开会话还能看到自己说了什么。
///
/// # 为什么建会话与落库在 spawn 之前
///
/// 它们要的结果（`convId` 与用户消息的真实 id）是**同步返回值**的一部分，
/// 见 [`ChatStartHandle`] 的文档。两步都是本地 SQLite，不会让这个 command 卡住。
///
/// # Errors
///
/// 空输入、入口不存在、没填 Key、SQL 失败时返回错误。**上游与网络失败不在这里** ——
/// 那时流已经起来了，错误走 `error` 事件。
#[tauri::command]
pub async fn llm_chat_start(
    conv_id: Option<String>,
    text: String,
    model: ModelRef,
    app: tauri::AppHandle,
    registry: tauri::State<'_, StreamRegistry>,
) -> Result<ChatStartHandle, AppError> {
    if text.trim().is_empty() {
        // 在建会话之前挡掉：否则库里会多一个标题「新对话」、只有一条空消息的会话，
        // 而用户并没有真的问什么。
        return Err(AppError::new("EMPTY_MESSAGE", "请先输入要问的内容。", 400));
    }

    // 准备工作放 spawn_blocking：Store::open 与三次 SQL 都是阻塞 IO，
    // 在 command 的 async 上下文里直接做会占住事件循环线程。
    let prepared = {
        let text = text.clone();
        let model = model.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let store = Store::open()?;
            let conn = store.conn();

            // Key 与入口先查：缺 Key 时直接失败，不留下一个空会话。
            let (base_url, protocol) = llm_store::endpoint_of(conn, &model.provider_id)?;
            let api_key = require_key(conn, &model.provider_id)?;

            let conv_id = match conv_id.filter(|id| !id.is_empty()) {
                Some(id) => id,
                None => conversations::create_conversation(
                    conn,
                    &conversations::title_from_first_message(&text),
                )?,
            };

            // 历史要在落用户那条**之前**读：`run_turn_streaming` 自己会把 `user_text`
            // 加进去，读在后面会让这一句在请求体里出现两遍。
            let history = history_as_chat_msgs(&conversations::read_conversation(conn, &conv_id)?)?;
            let user_row =
                conversations::append_message(conn, &conv_id, "user", &text, None, None, None)?;

            Ok::<_, AppError>(Prepared {
                base_url,
                protocol,
                api_key,
                conv_id,
                history,
                user_row,
            })
        })
        .await
        .map_err(|error| join_failed(&error.to_string()))??
    };

    let stream_id = llm_store::new_id();
    let cancelled = Arc::new(AtomicBool::new(false));
    registry
        .0
        .lock()
        .map_err(lock_poisoned)?
        .insert(stream_id.clone(), Arc::clone(&cancelled));

    let handle = ChatStartHandle {
        stream_id: stream_id.clone(),
        conv_id: prepared.conv_id.clone(),
        // clone 而不是移出：`prepared` 整个要交给后台线程，部分移动会借用检查失败。
        user_message: prepared.user_row.clone(),
    };

    // 真正的回合循环扔到后台线程，command 立刻返回，前端这时就能渲染用户气泡。
    tauri::async_runtime::spawn_blocking(move || {
        run_stream(&app, &stream_id, &cancelled, prepared, &model, &text);
    });

    Ok(handle)
}

/// `llm_chat_start` 在起流之前查好的东西。
///
/// 单独一个结构体而不是元组：六个字段的元组在 `spawn_blocking` 边界两侧各写一遍
/// `.0 .1 .2`，改顺序时静默错位。
struct Prepared {
    base_url: String,
    protocol: autobee_shell::credentials::LlmProtocol,
    api_key: String,
    conv_id: String,
    history: Vec<ChatMsg>,
    user_row: ChatMessageDto,
}

/// 后台跑完一轮，把进展与结果发成 `llm:chat:<stream_id>` 事件。
///
/// 不返回 `Result`：这里已经没有调用方能接错误了，失败必须变成 `error` 事件 ——
/// 否则前端会一直停在「正在思考」。
fn run_stream(
    app: &tauri::AppHandle,
    stream_id: &str,
    cancelled: &AtomicBool,
    prepared: Prepared,
    model: &ModelRef,
    text: &str,
) {
    let config = match autobee_shell::credentials::resolve::load_config() {
        Ok(config) => config,
        Err(error) => {
            emit(app, stream_id, &ChatStreamEvent::from(&error));
            finish_stream(app, stream_id);
            return;
        }
    };
    let mut mcp = match Store::open().and_then(|store| McpRegistry::load_enabled(store.conn())) {
        Ok(registry) => registry,
        Err(error) => {
            tracing::warn!(code = %error.code, %error, "跳过 MCP 工具：读取配置失败");
            McpRegistry::default()
        }
    };
    let mut deps = AgentDeps {
        config: &config,
        up: Upstream {
            base_url: &prepared.base_url,
            protocol: prepared.protocol,
            api_key: &prepared.api_key,
            model: &model.model,
        },
        mcp: &mut mcp,
    };

    let mut sink = |event: AgentEvent| {
        // 完整折线只在 tool 事件里存在；收到成功路线就立即独立保存，不能等整轮
        // 最终回答。用户可能在模型总结时关闭窗口，但地图已经显示过，快照仍应可恢复。
        if let AgentEvent::ToolEnd {
            payload: Some(ToolPayload::Plan(plan)),
            ok: true,
            ..
        } = &event
        {
            match Store::open().and_then(|store| {
                conversations::save_route_snapshot(store.conn(), &prepared.conv_id, plan)
            }) {
                Ok(()) => tracing::info!(
                    conversation = %prepared.conv_id,
                    "路线地图快照已保存"
                ),
                Err(error) => tracing::error!(
                    %error,
                    conversation = %prepared.conv_id,
                    "路线地图快照保存失败，重启后无法恢复该地图"
                ),
            }
        }
        // `Done` 不在这里发：它要带落库后的消息，而落库发生在 `run_turn_streaming`
        // 返回之后。在这里转发会让前端先收到 `done` 再收到内容。
        if let Some(payload) = to_stream_event(event) {
            emit(app, stream_id, &payload);
        }
    };
    let outcome = run_turn_streaming(&mut deps, &prepared.history, text, cancelled, &mut sink);

    let payload = match outcome {
        Ok(turn) => match persist_turn(&prepared.conv_id, &turn.appended, &model.model) {
            Ok(messages) => {
                tracing::info!(
                    conversation = %prepared.conv_id,
                    model = %model.model,
                    appended = messages.len(),
                    "一轮对话完成"
                );
                ChatStreamEvent::Done { messages }
            }
            Err(error) => ChatStreamEvent::from(&error),
        },
        Err(error) => ChatStreamEvent::from(&error),
    };
    emit(app, stream_id, &payload);
    finish_stream(app, stream_id);
}

/// 取消一条正在跑的流。
///
/// 只置标志位，不等它真的停下来：流所在的线程可能正堵在 `read` 上，等它返回
/// 会让这个 command 跟着卡住。`stream_once` 每读完一块检查一次，所以「立刻不再发
/// event」由那一侧保证。
///
/// # Errors
///
/// `stream_id` 不在注册表里时返回 404 —— 流已经自己结束了，或者这个 id 是假的。
#[tauri::command]
pub async fn llm_chat_cancel(
    stream_id: String,
    registry: tauri::State<'_, StreamRegistry>,
) -> Result<(), AppError> {
    let found = registry
        .0
        .lock()
        .map_err(lock_poisoned)?
        .get(&stream_id)
        .map(|flag| {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .is_some();

    if !found {
        return Err(AppError::new(
            "STREAM_NOT_FOUND",
            format!("找不到这次回复（{stream_id}），它可能已经结束了。"),
            404,
        ));
    }
    tracing::info!(stream = %stream_id, "已请求取消回复");
    Ok(())
}

/// 锁中毒说明持锁的线程 panic 了。报错而不是继续用可能不一致的表。
fn lock_poisoned<T>(error: std::sync::PoisonError<T>) -> AppError {
    AppError::new("INTERNAL", format!("取消表锁不可用：{error}"), 500)
}

/// 把一条事件推给前端。
///
/// 发送失败只记日志：窗口已经关掉是最常见的原因，那时没有任何人在等这条事件，
/// 把它升级成错误只会在关窗时刷一串无人关心的 ERROR。
fn emit(app: &tauri::AppHandle, stream_id: &str, payload: &ChatStreamEvent) {
    use tauri::Emitter as _;
    let name = format!("llm:chat:{stream_id}");
    if let Err(error) = app.emit(&name, payload) {
        tracing::debug!(%error, event = %name, "推送流式事件失败，窗口可能已关闭");
    }
}

/// 流结束后把取消标志摘掉。
///
/// **无论成功、失败还是被取消都要摘**：留着会让这张表随会话数无界增长，而且
/// 同一个 `streamId` 的第二次取消会「成功」——对一条已经结束的流。
fn finish_stream(app: &tauri::AppHandle, stream_id: &str) {
    use tauri::Manager as _;
    let Some(registry) = app.try_state::<StreamRegistry>() else {
        return;
    };
    match registry.0.lock() {
        Ok(mut map) => {
            map.remove(stream_id);
        }
        Err(error) => tracing::error!(%error, stream = %stream_id, "取消表锁不可用，条目未摘除"),
    }
}

/// `AgentEvent` → 前端事件。`Done` 返回 `None`：它由 [`run_stream`] 在落库之后自己发，
/// 带上真实的消息 id。转发这里的 `Done` 会让前端先收到结束再收到内容。
fn to_stream_event(event: AgentEvent) -> Option<ChatStreamEvent> {
    match event {
        AgentEvent::Delta(text) => Some(ChatStreamEvent::Delta { text }),
        AgentEvent::ToolStart { tool, args } => Some(ChatStreamEvent::ToolStart { tool, args }),
        AgentEvent::ToolEnd {
            tool,
            ok,
            summary,
            payload,
        } => {
            // enum → 两个可选字段。内部用 enum 是为了让「同时有路线和生活圈」
            // 不可表达,线上摊平是为了对齐文档 6.4.2 的联合形状。
            let (plan, circle) = match payload {
                Some(ToolPayload::Plan(plan)) => (Some(*plan), None),
                Some(ToolPayload::Circle(circle)) => (None, Some(*circle)),
                None => (None, None),
            };
            Some(ChatStreamEvent::ToolEnd {
                tool,
                ok,
                summary,
                plan,
                circle,
            })
        }
        AgentEvent::Done { .. } => None,
        AgentEvent::Failed { code, message } => Some(ChatStreamEvent::Error { code, message }),
    }
}

/// 本轮新增的消息落库。自己开库连接：后台线程与起流那次的连接不共用
/// （`Store` 不是 `Send`，而且流可能跑了几十秒，一直占着连接没有好处）。
fn persist_turn(
    conv_id: &str,
    appended: &[ChatMsg],
    model: &str,
) -> Result<Vec<ChatMessageDto>, AppError> {
    let store = Store::open()?;
    persist_appended(store.conn(), conv_id, appended, model)
}

impl From<&AppError> for ChatStreamEvent {
    /// 用户主动取消不是错误，但仍走 `error` 事件（前端按 `code` 分辨）：
    /// 界面需要一个明确的终止信号才能把「正在思考」收掉，静默结束会让它一直转。
    fn from(error: &AppError) -> Self {
        Self::Error {
            code: error.code.clone(),
            message: error.message.clone(),
        }
    }
}

/// 取某个入口的明文 Key，空的当错误。
///
/// 不拿空 Key 去发请求：上游会回一个语焉不详的 401，用户查不出是自己没填 Key。
fn require_key(conn: &rusqlite::Connection, provider_id: &str) -> Result<String, AppError> {
    let key = llm_store::secret_of(conn, provider_id)?
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty());

    key.ok_or_else(|| {
        AppError::new(
            "LLM_KEY_REQUIRED",
            "这个大模型服务入口还没填 API Key，请到设置页填好再对话。",
            400,
        )
    })
}

/// 库里的消息转成发给模型的上下文。
///
/// `role = "tool"` 的那些**必须带回 `tool_call_id`**：OpenAI 用它把结果与调用配对，
/// 缺了就是一条孤立的 tool 消息，上游直接 400。库里没有单独的 id 列，它在 assistant
/// 轮的 `tool_calls` JSON 里 —— 所以按顺序记住上一条 assistant 要求的调用，
/// 依次配给紧随其后的 tool 消息。
fn history_as_chat_msgs(rows: &[ChatMessageDto]) -> Result<Vec<ChatMsg>, AppError> {
    let mut out = Vec::with_capacity(rows.len());
    // 上一条 assistant 轮要求的调用，按发出顺序排队等结果回填。
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();

    for row in rows {
        match row.role.as_str() {
            "assistant" => {
                let calls = row
                    .tool_calls
                    .as_deref()
                    .filter(|raw| !raw.trim().is_empty())
                    .map(parse_tool_calls)
                    .transpose()?;
                pending.clear();
                if let Some(calls) = &calls {
                    pending.extend(calls.iter().map(|call| call.id.clone()));
                }
                out.push(ChatMsg {
                    role: row.role.clone(),
                    content: row.content.clone(),
                    tool_calls: calls,
                    tool_call_id: None,
                });
            }
            "tool" => out.push(ChatMsg {
                role: row.role.clone(),
                content: row.content.clone(),
                tool_calls: None,
                tool_call_id: pending.pop_front(),
            }),
            _ => out.push(ChatMsg::text(&row.role, row.content.clone())),
        }
    }
    Ok(out)
}

/// `tool_calls` 列里存的是 JSON 串。
fn parse_tool_calls(raw: &str) -> Result<Vec<ToolCallReq>, AppError> {
    serde_json::from_str(raw).map_err(|error| {
        tracing::error!(%error, "历史消息里的 tool_calls 解不开");
        AppError::new(
            "BAD_HISTORY",
            format!("这个会话里有一条工具调用记录读不出来：{error}"),
            500,
        )
    })
}

/// 把 `run_turn` 新增的消息落库，返回下发给前端的 DTO。
///
/// **只跳过开头那条 user**：它在调 `run_turn` 之前就已经落库了（见 `llm_chat_start`
/// 的「为什么先落库」），再插一次会让界面上同一句话出现两遍。后面若还出现 user 轮
/// 则照常落库 —— 那会是 `run_turn` 自己插的补充说明，丢掉它会让回放缺一段上下文。
fn persist_appended(
    conn: &rusqlite::Connection,
    conv_id: &str,
    appended: &[ChatMsg],
    model: &str,
) -> Result<Vec<ChatMessageDto>, AppError> {
    let mut out = Vec::new();

    for msg in skip_echoed_user(appended) {
        let row = row_for(msg, model)?;
        out.push(conversations::append_message(
            conn,
            conv_id,
            &msg.role,
            &msg.content,
            row.tool_calls.as_deref(),
            row.tool_name,
            row.model,
        )?);
    }

    Ok(out)
}

/// 落库时要跳过的开头那条 user（见 [`persist_appended`]）。
fn skip_echoed_user(appended: &[ChatMsg]) -> &[ChatMsg] {
    match appended.split_first() {
        Some((first, rest)) if first.role == "user" => rest,
        _ => appended,
    }
}

/// 一条消息落库时那三个可空列的取值。
struct MsgRow<'a> {
    tool_calls: Option<String>,
    tool_name: Option<&'a str>,
    model: Option<&'a str>,
}

/// 决定一条消息的三个可空列该填什么。
///
/// 单独一个函数是为了能不带数据库地测：判据在「哪个角色填哪一列」，而不在 SQL。
fn row_for<'a>(msg: &'a ChatMsg, model: &'a str) -> Result<MsgRow<'a>, AppError> {
    let tool_calls = msg
        .tool_calls
        .as_ref()
        .map(|calls| {
            serde_json::to_string(calls).map_err(|error| {
                AppError::new("INTERNAL", format!("工具调用序列化失败：{error}"), 500)
            })
        })
        .transpose()?;

    Ok(MsgRow {
        tool_calls,
        // tool 轮的工具名 `ChatMsg` 不带 —— 用它配对的调用 id 兜底，界面折叠显示时
        // 至少能区分是哪一次调用。
        tool_name: if msg.role == "tool" {
            msg.tool_call_id.as_deref()
        } else {
            None
        },
        // model 只标在 assistant 轮：tool 轮的内容是本机执行出来的，与模型无关。
        model: if msg.role == "assistant" {
            Some(model)
        } else {
            None
        },
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    fn row(role: &str, content: &str, tool_calls: Option<&str>) -> ChatMessageDto {
        ChatMessageDto {
            id: "1".to_owned(),
            conv_id: "c1".to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            tool_calls: tool_calls.map(str::to_owned),
            tool_name: None,
            model: None,
            created_at: 0,
        }
    }

    #[test]
    fn tool_history_carries_its_call_id() {
        let calls = r#"[{"id":"call_a","name":"plan_route","arguments":"{}"}]"#;
        let rows = vec![
            row("user", "怎么去武大", None),
            row("assistant", "", Some(calls)),
            row("tool", "两条线路", None),
            row("assistant", "坐地铁 2 号线", None),
        ];

        let msgs = history_as_chat_msgs(&rows).unwrap();

        // tool 那条必须带上 id，否则下一轮请求里它是孤立的，OpenAI 报 400。
        let ids: Vec<_> = msgs
            .iter()
            .map(|msg| (msg.role.as_str(), msg.tool_call_id.as_deref()))
            .collect();
        assert_eq!(
            ids,
            vec![
                ("user", None),
                ("assistant", None),
                ("tool", Some("call_a")),
                ("assistant", None),
            ]
        );
        assert_eq!(
            msgs[1].tool_calls.as_deref().map(<[ToolCallReq]>::len),
            Some(1)
        );
    }

    #[test]
    fn parallel_tool_results_pair_in_order() {
        let calls = r#"[{"id":"call_a","name":"search_place","arguments":"{}"},
                        {"id":"call_b","name":"life_circle","arguments":"{}"}]"#;
        let rows = vec![
            row("assistant", "", Some(calls)),
            row("tool", "地点结果", None),
            row("tool", "生活圈结果", None),
        ];

        let msgs = history_as_chat_msgs(&rows).unwrap();

        assert_eq!(
            msgs.iter()
                .filter(|msg| msg.role == "tool")
                .map(|msg| msg.tool_call_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("call_a"), Some("call_b")]
        );
    }

    #[test]
    fn plain_text_assistant_has_no_tool_calls() {
        let msgs = history_as_chat_msgs(&[row("assistant", "你好", None)]).unwrap();
        assert_eq!(
            (msgs[0].tool_calls.clone(), msgs[0].tool_call_id.clone()),
            (None, None)
        );
    }

    #[test]
    fn broken_tool_calls_json_is_reported() {
        let error = history_as_chat_msgs(&[row("assistant", "", Some("{不是 JSON"))]).unwrap_err();
        assert_eq!((error.code.as_str(), error.status), ("BAD_HISTORY", 500));
    }

    #[test]
    fn leading_user_is_skipped_only_once() {
        let msgs = vec![
            ChatMsg::text("user", "问题"),
            ChatMsg::text("assistant", "回答"),
        ];
        // 开头那条在 run_turn 之前已落库，再插一次界面上会出现两遍同样的话。
        assert_eq!(
            skip_echoed_user(&msgs)
                .iter()
                .map(|m| m.role.as_str())
                .collect::<Vec<_>>(),
            vec!["assistant"]
        );

        // 开头不是 user 时一条都不能少。
        let assistant_first = vec![ChatMsg::text("assistant", "回答")];
        assert_eq!(skip_echoed_user(&assistant_first).len(), 1);
    }

    #[test]
    fn tool_round_stores_call_id_and_no_model() {
        let tool = ChatMsg {
            role: "tool".to_owned(),
            content: "两条线路".to_owned(),
            tool_calls: None,
            tool_call_id: Some("call_a".to_owned()),
        };
        let row = row_for(&tool, "gpt-4o").unwrap();
        assert_eq!(
            (row.tool_calls, row.tool_name, row.model),
            (None, Some("call_a"), None)
        );
    }

    #[test]
    fn assistant_round_stores_tool_calls_json_and_model() {
        let assistant = ChatMsg {
            role: "assistant".to_owned(),
            content: String::new(),
            tool_calls: Some(vec![ToolCallReq {
                id: "call_a".to_owned(),
                name: "plan_route".to_owned(),
                arguments: "{}".to_owned(),
            }]),
            tool_call_id: None,
        };
        let row = row_for(&assistant, "gpt-4o").unwrap();
        assert_eq!((row.tool_name, row.model), (None, Some("gpt-4o")));

        // 落库的是 JSON 串，且下一轮 history_as_chat_msgs 能原样读回来。
        let restored = parse_tool_calls(&row.tool_calls.unwrap()).unwrap();
        assert_eq!(restored, assistant.tool_calls.unwrap());
    }
}
