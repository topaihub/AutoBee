//! MCP server 的 command：server CRUD、连上去列工具。
//!
//! 通用约定（`spawn_blocking`、每次重新读配置）见父模块 [`crate::commands`]。
//!
//! # 为什么 server 地址要校验
//!
//! 这个 URL 是用户手填的，而 tool 调用的参数会原样发到它 —— 那里面可能有
//! 「我在哪」「我要去哪」。明文 HTTP 发到外网等于把行程摊在链路上给沿途每一跳看。
//! 所以只放过 HTTPS，以及指向本机的 HTTP（本机不出网卡，没有链路可窃听，
//! 而本地跑的 MCP server 大多没有证书）。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use autobee_shell::credentials::store::Store;
use autobee_shell::error::AppError;
use autobee_shell::llm::mcp::{McpClient, prefixed_name};

/// 后台任务本身崩了（panic）时的兜底。
fn join_failed(detail: &str) -> AppError {
    AppError::new("INTERNAL", format!("后台任务失败：{detail}"), 500)
}

/// 下发给前端的一个 MCP server。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    /// 上次成功 `tools/list` 的条数。表里没有这一列，所以始终是 `None` ——
    /// 前端据此显示「还没连上过」，真实条数由 `list_mcp_tools` 当场拿。
    pub tool_count: Option<usize>,
}

/// 前端提交的一个 MCP server。`id` 为 `None` 表示新建。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInput {
    pub id: Option<String>,
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

/// `list_mcp_tools` 的一项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    /// 带前缀的完整工具名，形如 `mcp_baidu_<tool>`。
    pub name: String,
    /// server 的 name，界面按它分组。
    pub server: String,
    pub description: String,
}

/// 本机的几种写法。IPv6 字面量在 URL 里带方括号，去掉括号后的裸 `::1`
/// 也认 —— 用户两种都可能手填。
const LOOPBACK_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "::1", "[::1]"];

/// 从 `http(s)://` 之后切出 host（去掉 userinfo、端口、路径）。
///
/// 不引 `url` crate：这里只需要 scheme 与 host 两段，而 `mcp.rs` 的
/// `McpClient::connect` 也是按前缀判断的，两处保持同一种做法。
fn host_of(rest: &str) -> &str {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // `user:pass@host` 里 host 在 `@` 之后；没有 `@` 时整段就是 host。
    let after_userinfo = authority.rsplit('@').next().unwrap_or_default();

    // IPv6 字面量形如 `[::1]:8931`，端口在右方括号之后，不能按第一个冒号切。
    if let Some(end) = after_userinfo.find(']') {
        return &after_userinfo[..=end];
    }
    after_userinfo.split(':').next().unwrap_or_default()
}

/// 校验并规范化 MCP server 地址：只放过 HTTPS 与指向本机的 HTTP（见模块文档）。
///
/// # Errors
///
/// 空串、非 http(s) 协议、缺主机名、明文 HTTP 指向非本机时返回 400。
fn validate_url(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(
            "MCP_URL_REQUIRED",
            "请填 MCP server 的地址。",
            400,
        ));
    }

    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(AppError::new(
            "MCP_URL_SCHEME",
            format!("MCP server 地址得以 https:// 开头，当前填的是「{trimmed}」。"),
            400,
        ));
    };

    // scheme 大小写不敏感（RFC 3986），`HTTPS://` 是合法写法。
    let scheme = scheme.to_ascii_lowercase();
    let host = host_of(rest).to_ascii_lowercase();

    if host.is_empty() {
        return Err(AppError::new(
            "MCP_URL_INVALID",
            format!("MCP server 地址「{trimmed}」没有主机名。"),
            400,
        ));
    }

    match scheme.as_str() {
        "https" => {}
        "http" if LOOPBACK_HOSTS.contains(&host.as_str()) => {}
        "http" => {
            return Err(AppError::new(
                "MCP_URL_INSECURE",
                format!(
                    "「{trimmed}」是明文 HTTP 且不在本机。工具参数可能含你的位置和行程，\
                     发到外网会被链路上每一跳看到。请改用 https://，或把 server 跑在本机。"
                ),
                400,
            ));
        }
        other => {
            return Err(AppError::new(
                "MCP_URL_SCHEME",
                format!("MCP server 地址只支持 http / https，收到「{other}」。"),
                400,
            ));
        }
    }

    Ok(trimmed.to_owned())
}

/// 不引 `uuid`：与 `llm::store::new_id` 同样的理由，只要求同一个库里不重复。
fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&nanos, &mut hasher);
    std::hash::Hash::hash(&std::thread::current().id(), &mut hasher);
    let salt = std::hash::Hasher::finish(&hasher);
    format!("{nanos:x}-{salt:x}")
}

fn sql(doing: &str, error: &rusqlite::Error) -> AppError {
    tracing::error!(%error, doing, "MCP 配置库操作失败");
    AppError::new("DB_ERROR", format!("MCP server {doing}失败：{error}"), 500)
}

/// 读出全部 server，`sort_order` 升序。
///
/// # Errors
///
/// 查询失败时返回错误。
fn list_all(conn: &Connection) -> Result<Vec<McpServer>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, name, url, enabled FROM mcp_servers ORDER BY sort_order, name")
        .map_err(|error| sql("准备查询", &error))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(McpServer {
                id: row.get(0)?,
                name: row.get(1)?,
                url: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
                tool_count: None,
            })
        })
        .map_err(|error| sql("读取", &error))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|error| sql("读取", &error))?);
    }
    Ok(out)
}

/// 新建或更新一条。返回落库后的完整列表。
///
/// # Errors
///
/// 名称为空、URL 不合法、SQL 失败时返回错误。
fn save(conn: &Connection, input: &McpServerInput) -> Result<Vec<McpServer>, AppError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::new(
            "MCP_NAME_REQUIRED",
            "请给这个 MCP server 起个名字，工具名前缀取自它。",
            400,
        ));
    }
    let url = validate_url(&input.url)?;
    let enabled = i64::from(input.enabled);

    match input.id.as_deref().filter(|id| !id.is_empty()) {
        Some(id) => {
            let changed = conn
                .execute(
                    "UPDATE mcp_servers SET name = ?2, url = ?3, enabled = ?4 WHERE id = ?1",
                    rusqlite::params![id, name, url, enabled],
                )
                .map_err(|error| sql("更新", &error))?;
            if changed == 0 {
                return Err(AppError::new(
                    "MCP_SERVER_NOT_FOUND",
                    "这个 MCP server 不存在，可能已经被删掉了。",
                    404,
                ));
            }
        }
        None => {
            // 新建的排在末尾：用户刚加的应该出现在他上次看到的列表下面，而不是插队到中间。
            let next_order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM mcp_servers",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| sql("排序", &error))?;
            conn.execute(
                "INSERT INTO mcp_servers (id, name, url, enabled, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![new_id(), name, url, enabled, next_order],
            )
            .map_err(|error| sql("新建", &error))?;
        }
    }

    list_all(conn)
}

/// 列出全部 MCP server。
///
/// # Errors
///
/// 配置库读不出来时返回错误。
#[tauri::command]
pub async fn list_mcp_servers() -> Result<Vec<McpServer>, AppError> {
    tauri::async_runtime::spawn_blocking(|| list_all(Store::open()?.conn()))
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 保存一个 MCP server（`input.id` 为空则新建）。
///
/// # Errors
///
/// 名称为空、URL 不合法或不安全、SQL 失败时返回错误。
#[tauri::command]
pub async fn save_mcp_server(input: McpServerInput) -> Result<Vec<McpServer>, AppError> {
    tauri::async_runtime::spawn_blocking(move || save(Store::open()?.conn(), &input))
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 删掉一个 MCP server。
///
/// # Errors
///
/// SQL 失败时返回错误。
#[tauri::command]
pub async fn delete_mcp_server(id: String) -> Result<Vec<McpServer>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        store
            .conn()
            .execute("DELETE FROM mcp_servers WHERE id = ?1", [&id])
            .map_err(|error| sql("删除", &error))?;
        tracing::info!(server = %id, "已删除 MCP server");
        list_all(store.conn())
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 连上去跑一次 `tools/list`，**不落库**。
///
/// `id` 给了就只连那一个，没给就遍历全部 `enabled` 的。
///
/// # 为什么单个失败不算整体失败
///
/// 用户配了三个 server，其中一个的进程没起来。整体报错的话工具面板全空，
/// 他会以为 MCP 整个坏了；跳过那一个则另两个照常可用，日志里有 warn 可查。
///
/// # Errors
///
/// 配置库读不出来时返回错误；指定 `id` 且那一个连不上时返回它的错误。
#[tauri::command]
pub async fn list_mcp_tools(id: Option<String>) -> Result<Vec<McpToolInfo>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let servers = list_all(Store::open()?.conn())?;
        let mut out = Vec::new();

        match id {
            // 指定了某一个：用户点的是那一行的「测试」，连不上必须如实报出来。
            Some(id) => {
                let server = servers
                    .into_iter()
                    .find(|one| one.id == id)
                    .ok_or_else(|| {
                        AppError::new(
                            "MCP_SERVER_NOT_FOUND",
                            "这个 MCP server 不存在，可能已经被删掉了。",
                            404,
                        )
                    })?;
                out.extend(tools_of(&server)?);
            }
            None => {
                for server in servers.into_iter().filter(|one| one.enabled) {
                    match tools_of(&server) {
                        Ok(tools) => out.extend(tools),
                        Err(error) => tracing::warn!(
                            server = %server.name,
                            url = %server.url,
                            code = %error.code,
                            %error,
                            "跳过这个 MCP server：列工具失败"
                        ),
                    }
                }
            }
        }

        Ok(out)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 连一个 server 并列出它的工具，名字已加前缀。
fn tools_of(server: &McpServer) -> Result<Vec<McpToolInfo>, AppError> {
    let mut client = McpClient::connect(&server.url)?;
    let tools = client.list_tools()?;
    tracing::info!(server = %server.name, tools = tools.len(), "MCP 工具列表已取回");
    Ok(tools
        .into_iter()
        .map(|tool| McpToolInfo {
            name: prefixed_name(&server.name, &tool.name),
            server: server.name.clone(),
            description: tool.description,
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn https_is_allowed() {
        assert_eq!(
            validate_url("https://mcp.example.com/sse").unwrap(),
            "https://mcp.example.com/sse"
        );
    }

    #[test]
    fn plain_http_to_public_host_is_rejected() {
        let error = validate_url("http://mcp.example.com/sse").unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.status),
            ("MCP_URL_INSECURE", 400)
        );
    }

    #[test]
    fn plain_http_to_loopback_is_allowed() {
        for raw in [
            "http://127.0.0.1:8931/mcp",
            "http://localhost:3000/sse",
            "http://[::1]:8931/mcp",
        ] {
            assert_eq!(validate_url(raw).unwrap(), raw);
        }
    }

    #[test]
    fn non_http_scheme_and_empty_are_rejected() {
        let scheme = validate_url("ws://127.0.0.1:8931/mcp").unwrap_err();
        assert_eq!(
            (scheme.code.as_str(), scheme.status),
            ("MCP_URL_SCHEME", 400)
        );

        let empty = validate_url("   ").unwrap_err();
        assert_eq!(
            (empty.code.as_str(), empty.status),
            ("MCP_URL_REQUIRED", 400)
        );

        // 没有 `://` 的一律当 scheme 缺失，与 `McpClient::connect` 的判断一致。
        let bare = validate_url("mcp.example.com/sse").unwrap_err();
        assert_eq!((bare.code.as_str(), bare.status), ("MCP_URL_SCHEME", 400));

        let hostless = validate_url("https:///sse").unwrap_err();
        assert_eq!(
            (hostless.code.as_str(), hostless.status),
            ("MCP_URL_INVALID", 400)
        );
    }

    #[test]
    fn userinfo_and_ipv6_port_do_not_fool_the_host_check() {
        // `@` 之前的部分是 userinfo，不是 host。按第一段切会把外网地址当本机放过。
        let spoofed = validate_url("http://127.0.0.1@evil.example.com/mcp").unwrap_err();
        assert_eq!(
            (spoofed.code.as_str(), spoofed.status),
            ("MCP_URL_INSECURE", 400)
        );

        assert_eq!(host_of("[::1]:8931/mcp"), "[::1]");
        assert_eq!(host_of("127.0.0.1:8931/mcp?x=1"), "127.0.0.1");
        assert_eq!(host_of("mcp.example.com/sse"), "mcp.example.com");
    }

    #[test]
    fn scheme_and_host_are_case_insensitive() {
        let raw = "HTTP://LocalHost:3000/sse";
        assert_eq!(validate_url(raw).unwrap(), raw);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            validate_url("  https://mcp.example.com/sse  ").unwrap(),
            "https://mcp.example.com/sse"
        );
    }

    /// 手写的 SQL 要真跑一遍：列名写错、`enabled` 的整数与 bool 没对上这类问题
    /// 只在运行时暴露，而那时表现是「保存完列表是空的」。
    ///
    /// 用真实文件而不是内存库：`schema::prepare` 在 shell 里是 `pub(crate)`，
    /// 从 command 层拿不到，`Store::open_at` 是唯一能建出带表的连接的入口。
    #[test]
    fn crud_round_trips_through_real_sql() {
        let path = std::env::temp_dir().join(format!(
            "autobee-mcp-cmd-test-{}-{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = Store::open_at(&path).unwrap();
        let conn = store.conn();

        assert_eq!(list_all(conn).unwrap(), Vec::new());

        let created = save(
            conn,
            &McpServerInput {
                id: None,
                name: "  百度地图  ".into(),
                url: "  https://mcp.example.com/sse  ".into(),
                enabled: true,
            },
        )
        .unwrap();
        assert_eq!(created.len(), 1);
        // 名字与 URL 都去掉了首尾空白：前缀取自 name，带空格会拼出非法工具名。
        assert_eq!(
            (
                created[0].name.as_str(),
                created[0].url.as_str(),
                created[0].enabled,
                created[0].tool_count
            ),
            ("百度地图", "https://mcp.example.com/sse", true, None)
        );

        // 更新走 id 分支：条数不变，字段变了。
        let updated = save(
            conn,
            &McpServerInput {
                id: Some(created[0].id.clone()),
                name: "本机 Playwright".into(),
                url: "http://127.0.0.1:8931/mcp".into(),
                enabled: false,
            },
        )
        .unwrap();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            (
                updated[0].id.as_str(),
                updated[0].name.as_str(),
                updated[0].enabled
            ),
            (created[0].id.as_str(), "本机 Playwright", false)
        );

        // 不存在的 id 不能静默变成新建 —— 那会让「编辑」悄悄多出一行。
        let missing = save(
            conn,
            &McpServerInput {
                id: Some("没有这个".into()),
                name: "x".into(),
                url: "https://x.example.com/".into(),
                enabled: true,
            },
        )
        .unwrap_err();
        assert_eq!(
            (missing.code.as_str(), missing.status),
            ("MCP_SERVER_NOT_FOUND", 404)
        );

        // 空名字拒绝：工具名前缀取自它。
        let unnamed = save(
            conn,
            &McpServerInput {
                id: None,
                name: "   ".into(),
                url: "https://x.example.com/".into(),
                enabled: true,
            },
        )
        .unwrap_err();
        assert_eq!(
            (unnamed.code.as_str(), unnamed.status),
            ("MCP_NAME_REQUIRED", 400)
        );

        // 新建的排在后面，不插队。
        save(
            conn,
            &McpServerInput {
                id: None,
                name: "第二个".into(),
                url: "https://two.example.com/sse".into(),
                enabled: true,
            },
        )
        .unwrap();
        let listed = list_all(conn).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|one| one.name.as_str())
                .collect::<Vec<_>>(),
            vec!["本机 Playwright", "第二个"]
        );

        conn.execute("DELETE FROM mcp_servers WHERE id = ?1", [&listed[0].id])
            .unwrap();
        assert_eq!(
            list_all(conn)
                .unwrap()
                .iter()
                .map(|one| one.name.as_str())
                .collect::<Vec<_>>(),
            vec!["第二个"]
        );

        drop(store);
        let _ = std::fs::remove_file(&path);
    }
}
