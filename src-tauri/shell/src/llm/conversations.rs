//! 会话与消息的读写。对应 `llm_conversations` / `llm_messages` 两张表。
//!
//! 不存 tool 的原始返回值全文(可能几十 KB 折线)，只存裁剪后的摘要。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::llm::store::new_id;

#[cfg(test)]
use route_core::models::{Crs, Endpoint, LatLng, ProviderId, Route, RoutePlan, TravelMode};

/// 标题取首句前 20 **字**。按字节切会把一个汉字劈成两半（UTF-8 下 3 字节），
/// 得到的不是短标题而是乱码。
const TITLE_CHARS: usize = 20;

/// 空标题的回落。列表里空串会渲染成一行看不见、点不中的条目。
const DEFAULT_TITLE: &str = "新对话";

/// 下发给前端的一次对话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDto {
    pub id: String,
    pub title: String,
    /// unix 秒。
    pub created_at: i64,
    pub updated_at: i64,
}

/// 下发给前端的一条消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageDto {
    /// 库里是自增 INTEGER，下发成串 —— 前端的 `ChatMessage.id` 是 string，
    /// 它只拿来做列表 key，不做算术。
    pub id: String,
    pub conv_id: String,
    /// `system` | `user` | `assistant` | `tool`。
    pub role: String,
    pub content: String,
    /// assistant 轮的 tool_calls JSON 原文，`None` = 纯文本轮。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<String>,
    /// `role = "tool"` 时是哪个工具的结果。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// unix 秒。
    pub created_at: i64,
}

/// 最近聊过的排最前。同秒时再按 `created_at`、`id` 兜底：只按 `updated_at`
/// 的话同秒的两条会话每次刷新顺序都可能不同，列表会自己跳。
///
/// # Errors
///
/// 查询失败时返回错误。
pub fn list_conversations(conn: &Connection) -> Result<Vec<ConversationDto>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, updated_at FROM llm_conversations
             ORDER BY updated_at DESC, created_at DESC, id DESC",
        )
        .map_err(|error| sql("准备查询会话", &error))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ConversationDto {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(|error| sql("读会话列表", &error))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|error| sql("读一行会话", &error))?);
    }
    Ok(out)
}

/// 返回新会话 id。
///
/// # Errors
///
/// SQL 失败时返回错误。
pub fn create_conversation(conn: &Connection, title: &str) -> Result<String, AppError> {
    let trimmed = title.trim();
    let title = if trimmed.is_empty() {
        DEFAULT_TITLE
    } else {
        trimmed
    };
    let id = new_id();
    conn.execute(
        // `?3` 复用两次：两个时间戳必须是同一个值，各取一次 now 会在跨秒时差 1，
        // 界面上就成了「刚建好就已经被改过」。
        "INSERT INTO llm_conversations (id, title, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        (&id, title, now_secs()),
    )
    .map_err(|error| sql("新建会话", &error))?;
    Ok(id)
}

/// 按写入先后回放。第二排序键是 `id` 而不是只有 `created_at`：一次工具调用的
/// assistant 轮与 tool 轮几乎必然落在同一秒，只按秒排会把因果颠倒过来。
///
/// # Errors
///
/// 查询失败时返回错误。
pub fn read_conversation(conn: &Connection, id: &str) -> Result<Vec<ChatMessageDto>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, conv_id, role, content, tool_calls, tool_name, model, created_at
             FROM llm_messages WHERE conv_id = ?1 ORDER BY created_at, id",
        )
        .map_err(|error| sql("准备查询消息", &error))?;

    let rows = stmt
        .query_map([id], |row| {
            Ok(ChatMessageDto {
                id: row.get::<_, i64>(0)?.to_string(),
                conv_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                tool_calls: row.get(4)?,
                tool_name: row.get(5)?,
                model: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|error| sql("读消息", &error))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|error| sql("读一行消息", &error))?);
    }
    Ok(out)
}

/// 插入一条消息，同时把所属会话的 `updated_at` 刷成当前时间 —— 会话列表按它
/// 排序，不刷的话聊过的会话仍沉在底部。两条语句同一个事务：只成一半的话，
/// 消息在库里而列表顺序对不上。
///
/// # Errors
///
/// SQL 失败时返回错误；`conv_id` 不存在时外键会拒绝插入。
pub fn append_message(
    conn: &Connection,
    conv_id: &str,
    role: &str,
    content: &str,
    tool_calls: Option<&str>,
    tool_name: Option<&str>,
    model: Option<&str>,
) -> Result<ChatMessageDto, AppError> {
    let now = now_secs();
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| sql("开事务", &error))?;

    tx.execute(
        "INSERT INTO llm_messages (conv_id, role, content, tool_calls, tool_name, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (conv_id, role, content, tool_calls, tool_name, model, now),
    )
    .map_err(|error| sql("写入消息", &error))?;
    let id = tx.last_insert_rowid();

    tx.execute(
        "UPDATE llm_conversations SET updated_at = ?1 WHERE id = ?2",
        (now, conv_id),
    )
    .map_err(|error| sql("刷新会话时间", &error))?;

    tx.commit().map_err(|error| sql("提交消息", &error))?;

    Ok(ChatMessageDto {
        id: id.to_string(),
        conv_id: conv_id.to_owned(),
        role: role.to_owned(),
        content: content.to_owned(),
        tool_calls: tool_calls.map(str::to_owned),
        tool_name: tool_name.map(str::to_owned),
        model: model.map(str::to_owned),
        created_at: now,
    })
}

/// 消息行由外键级联删掉（见 `schema::prepare` 里的 `PRAGMA`）。
///
/// # Errors
///
/// SQL 失败时返回错误。
pub fn delete_conversation(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM llm_conversations WHERE id = ?1", [id])
        .map_err(|error| sql("删除会话", &error))?;
    Ok(())
}

/// 保存一个会话最后一次成功路线工具调用的完整结果。
///
/// 完整折线不属于模型上下文，独立表存它才能在重启后重绘历史地图；同一会话只保留
/// 最新路线，避免每次追问都复制一份几十 KB 的坐标数组。
///
/// # Errors
///
/// JSON 序列化或 SQL 写入失败时返回错误。
pub fn save_route_snapshot(
    conn: &Connection,
    conv_id: &str,
    plan: &route_core::models::RoutePlan,
) -> Result<(), AppError> {
    let plan_json = serde_json::to_string(plan).map_err(|error| {
        AppError::new(
            "SERIALIZE_ERROR",
            format!("路线快照序列化失败：{error}"),
            500,
        )
    })?;
    conn.execute(
        "INSERT INTO llm_route_snapshots (conv_id, plan_json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(conv_id) DO UPDATE SET plan_json = excluded.plan_json, updated_at = excluded.updated_at",
        (conv_id, plan_json, now_secs()),
    )
    .map_err(|error| sql("保存路线快照", &error))?;
    Ok(())
}

/// 读取会话最后一次完整路线快照。旧会话没有快照是正常的，返回 `Ok(None)`。
///
/// # Errors
///
/// SQL 查询失败或库内 JSON 损坏时返回错误。
pub fn read_route_snapshot(
    conn: &Connection,
    conv_id: &str,
) -> Result<Option<route_core::models::RoutePlan>, AppError> {
    let json = match conn.query_row(
        "SELECT plan_json FROM llm_route_snapshots WHERE conv_id = ?1",
        [conv_id],
        |row| row.get::<_, String>(0),
    ) {
        Ok(json) => json,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(sql("读取路线快照", &error)),
    };
    serde_json::from_str(&json).map(Some).map_err(|error| {
        AppError::new(
            "BAD_ROUTE_SNAPSHOT",
            format!("路线快照读不出来：{error}"),
            500,
        )
    })
}

/// 用户首句当标题。
#[must_use]
pub fn title_from_first_message(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return DEFAULT_TITLE.to_owned();
    }
    let mut out: String = trimmed.chars().take(TITLE_CHARS).collect();
    if trimmed.chars().nth(TITLE_CHARS).is_some() {
        out.push('…');
    }
    out
}

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn sql(doing: &str, error: &rusqlite::Error) -> AppError {
    tracing::error!(%error, doing, "会话记录库操作失败");
    AppError::new("DB_ERROR", format!("会话记录{doing}失败：{error}"), 500)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::credentials::schema::prepare(&conn).unwrap();
        conn
    }

    /// 把时间戳钉成固定值，好让顺序断言不依赖「这一秒跑得够快」。
    fn 钉住会话时间(conn: &Connection, id: &str, secs: i64) {
        conn.execute(
            "UPDATE llm_conversations SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
            (secs, id),
        )
        .unwrap();
    }

    #[test]
    fn 回放顺序与写入顺序一致() {
        let conn = memory();
        let conv = create_conversation(&conn, "路线").unwrap();

        append_message(&conn, &conv, "user", "从北京到天津", None, None, None).unwrap();
        append_message(
            &conn,
            &conv,
            "assistant",
            "好的",
            None,
            None,
            Some("gpt-4o"),
        )
        .unwrap();
        append_message(&conn, &conv, "user", "改走高速", None, None, None).unwrap();

        let rows = read_conversation(&conn, &conv).unwrap();
        let 内容: Vec<&str> = rows.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(内容, ["从北京到天津", "好的", "改走高速"]);
        assert_eq!(rows[1].model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn 追加消息把会话顶到最前() {
        let conn = memory();
        let a = create_conversation(&conn, "A").unwrap();
        let b = create_conversation(&conn, "B").unwrap();
        钉住会话时间(&conn, &a, 1_000);
        钉住会话时间(&conn, &b, 2_000);
        assert_eq!(
            list_conversations(&conn).unwrap()[0].id,
            b,
            "B 更新，先排 B"
        );

        append_message(&conn, &a, "user", "还在吗", None, None, None).unwrap();

        let list = list_conversations(&conn).unwrap();
        assert_eq!(list[0].id, a, "刚聊过的 A 必须顶到最前");
        assert!(list[0].updated_at > 1_000, "updated_at 得被刷新");
        assert_eq!(list[0].created_at, 1_000, "created_at 不许跟着动");
    }

    #[test]
    fn 删会话时消息由外键级联清掉() {
        let conn = memory();
        let conv = create_conversation(&conn, "待删").unwrap();
        append_message(&conn, &conv, "user", "一", None, None, None).unwrap();
        append_message(&conn, &conv, "assistant", "二", None, None, None).unwrap();
        let 留下 = create_conversation(&conn, "留下").unwrap();
        append_message(&conn, &留下, "user", "三", None, None, None).unwrap();

        delete_conversation(&conn, &conv).unwrap();

        let 剩余: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM llm_messages WHERE conv_id = ?1",
                [&conv],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(剩余, 0, "级联没生效，孤儿消息会一直堆着");
        assert_eq!(read_conversation(&conn, &留下).unwrap().len(), 1, "别连坐");
    }

    #[test]
    fn 路线快照跨重开可读且删会话级联清掉() {
        let conn = memory();
        let conv = create_conversation(&conn, "路线快照").unwrap();
        let plan = RoutePlan {
            origin: Endpoint {
                title: "光谷广场".to_owned(),
                location: LatLng::new(30.5, 114.4),
            },
            destination: Endpoint {
                title: "汉口火车站".to_owned(),
                location: LatLng::new(30.62, 114.26),
            },
            mode: TravelMode::Subway,
            routes: vec![Route {
                id: "r0".to_owned(),
                distance: 18_800,
                duration: 45,
                price: 5.0,
                bounds: [LatLng::new(30.5, 114.26), LatLng::new(30.62, 114.4)],
                segments: Vec::new(),
            }],
            provider: ProviderId::Baidu,
            crs: Crs::Bd09,
            degraded: None,
        };

        save_route_snapshot(&conn, &conv, &plan).unwrap();
        assert_eq!(read_route_snapshot(&conn, &conv).unwrap(), Some(plan));

        delete_conversation(&conn, &conv).unwrap();
        assert_eq!(read_route_snapshot(&conn, &conv).unwrap(), None);
    }

    #[test]
    fn 标题按字取前二十个() {
        let 长句 = "从北京市海淀区中关村出发到天津市滨海新区开发区然后再回来";
        let title = title_from_first_message(长句);

        assert_eq!(title.chars().count(), TITLE_CHARS + 1, "20 字 + 省略号");
        assert_eq!(title, "从北京市海淀区中关村出发到天津市滨海新区…");
        assert_eq!(
            title_from_first_message("  \n 短句 \t"),
            "短句",
            "首尾空白去掉"
        );
        assert_eq!(title_from_first_message(" \t\n "), DEFAULT_TITLE);
        assert_eq!(title_from_first_message(""), DEFAULT_TITLE);
        // 刚好 20 字：不该补省略号，否则用户会以为标题被截了。
        let 刚好 = "刚好二十个字的标题不应该补省略号的哦哦哦";
        assert_eq!(title_from_first_message(刚好), 刚好);
    }

    #[test]
    fn 同秒的_assistant_轮与_tool_轮不乱序() {
        let conn = memory();
        let conv = create_conversation(&conn, "工具").unwrap();
        let 调用 = append_message(
            &conn,
            &conv,
            "assistant",
            "",
            Some(r#"[{"id":"c1","function":{"name":"plan_route"}}]"#),
            None,
            Some("gpt-4o"),
        )
        .unwrap();
        let 结果 = append_message(
            &conn,
            &conv,
            "tool",
            "总长 137 公里",
            None,
            Some("plan_route"),
            None,
        )
        .unwrap();

        // 真实情况就是同秒；这里显式压成同一秒，免得偶然跨秒让测试失去意义。
        conn.execute(
            "UPDATE llm_messages SET created_at = 1700000000 WHERE conv_id = ?1",
            [&conv],
        )
        .unwrap();

        let rows = read_conversation(&conn, &conv).unwrap();
        assert_eq!(rows[0].id, 调用.id, "assistant 轮在前");
        assert_eq!(rows[1].id, 结果.id, "tool 轮在后");
        assert_eq!(rows[0].role, "assistant");
        assert_eq!(rows[1].tool_name.as_deref(), Some("plan_route"));
        assert!(
            rows[0]
                .tool_calls
                .as_deref()
                .unwrap()
                .contains("plan_route")
        );
    }
}
