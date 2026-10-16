//! 对话的路线快照：会话里最后一次 `plan_route` 的完整结果。
//!
//! 完整折线不进模型上下文，Aether 的 chat.db 也只存工具摘要，所以重开会话时要从
//! 这里拿回整条路线重绘地图。同一会话只留最新一条，追问时不重复存几十 KB 坐标。

use route_core::models::RoutePlan;
use rusqlite::Connection;

use crate::error::AppError;

/// 写入（覆盖）一个会话的快照。
///
/// # Errors
///
/// JSON 序列化或 SQL 写入失败时返回错误。
pub fn save(conn: &Connection, conv_id: &str, plan: &RoutePlan) -> Result<(), AppError> {
    let json = serde_json::to_string(plan).map_err(|error| {
        AppError::new(
            "SERIALIZE_ERROR",
            format!("路线快照序列化失败：{error}"),
            500,
        )
    })?;
    conn.execute(
        "INSERT INTO route_snapshots (conv_id, plan_json, updated_at) VALUES (?1, ?2, unixepoch())
         ON CONFLICT(conv_id) DO UPDATE SET plan_json = excluded.plan_json, updated_at = excluded.updated_at",
        (conv_id, json),
    )
    .map_err(|error| sql("保存路线快照", &error))?;
    Ok(())
}

/// 读一个会话的快照。没有路线的会话返回 `Ok(None)`。
///
/// # Errors
///
/// SQL 查询失败或库内 JSON 损坏时返回错误。
pub fn read(conn: &Connection, conv_id: &str) -> Result<Option<RoutePlan>, AppError> {
    let json = match conn.query_row(
        "SELECT plan_json FROM route_snapshots WHERE conv_id = ?1",
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

fn sql(doing: &str, error: &rusqlite::Error) -> AppError {
    tracing::error!(%error, doing, "路线快照库操作失败");
    AppError::new("DB_ERROR", format!("{doing}失败：{error}"), 500)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;
    use crate::credentials::store::Store;

    fn plan(title: &str) -> RoutePlan {
        serde_json::from_value(serde_json::json!({
            "origin": { "title": title, "location": { "lat": 30.5, "lng": 114.4 } },
            "destination": { "title": "汉口站", "location": { "lat": 30.6, "lng": 114.2 } },
            "mode": "subway",
            "crs": "GCJ02",
            "provider": "baidu",
            "routes": []
        }))
        .unwrap()
    }

    #[test]
    fn 没存过返回空_再存是覆盖() {
        let store = Store::open_at(std::path::Path::new(":memory:")).unwrap();
        let conn = store.conn();
        assert_eq!(read(conn, "c1").unwrap(), None);

        save(conn, "c1", &plan("光谷")).unwrap();
        save(conn, "c1", &plan("街道口")).unwrap();

        assert_eq!(read(conn, "c1").unwrap(), Some(plan("街道口")));
        assert_eq!(read(conn, "c2").unwrap(), None, "会话之间不串");
    }
}
