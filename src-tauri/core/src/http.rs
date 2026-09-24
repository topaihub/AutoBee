//! HTTP 客户端。
//!
//! # 为什么是 blocking
//!
//! 算路就是「发一个请求、等一个响应」，没有并发扇出。引 async 会把
//! `async` 传染到每个 adapter 与每个单测，换不来任何吞吐。
//! 调用方（Tauri command）负责把它丢到阻塞线程池，不让 UI 卡住。
//!
//! # 为什么要一个共享 Client
//!
//! 每次请求新建一个 Client 会新建连接池，TCP 与 TLS 握手重来一遍：
//! 一次算路要连发三次请求（联想、解析、算路），那个延迟是能感觉出来的。

use std::sync::OnceLock;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::error::AppError;

/// 超时。地图厂商偶发慢响应，宁可早失败让上层降级到下一家，
/// 也不要让用户在转圈里等。
const TIMEOUT: Duration = Duration::from_secs(10);

/// 标识自己。厂商按 UA 统计来源，缺失时有些接口会直接拒绝。
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AutoBee/0.1";

fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

/// URL 组件编码（百分号编码）。
///
/// 字符一律按 UTF-8 字节编码 —— 中文地点名、空格、斜杠都要能安全放进查询串。
/// 保留 `-._~` 与字母数字：那些是 RFC 3986 的未保留字符，编不编都行，
/// 但不编更好读（`/ws/geocoder/v1` 这种路径不受影响）。
#[must_use]
pub fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(*byte));
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 取一个 URL 并解析成 JSON。
///
/// # Errors
///
/// 网络失败、超时、非 2xx、响应不是合法 JSON 时返回 [`AppError::network`]。
/// **不解析业务状态码** —— 那由各 adapter 自己判（各家字段名不同）。
pub fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, AppError> {
    let response = client()
        .get(url)
        .send()
        .map_err(|error| AppError::network(&error.to_string()))?;

    if !response.status().is_success() {
        return Err(AppError::network(&format!("HTTP {}", response.status())));
    }

    response
        .json::<T>()
        .map_err(|error| AppError::network(&error.to_string()))
}

/// 取一个 URL 并解析成 JSON，带上鉴权头。
///
/// 大模型服务的 `/models` 需要鉴权，而地图厂商一律把 Key 放在查询串里，
/// 所以 [`get_json`] 没有头部参数。这里分成两个函数而不是给前者加参数：
/// 地图侧几十个调用点不该为此各传一个 `None`。
///
/// # Errors
///
/// 网络失败、超时、非 2xx、响应不是合法 JSON 时返回 [`AppError::network`]。
/// 401/403 会被折成带原状态码的错误 —— Key 不对与网络不通对用户是两件事。
pub fn get_json_with_auth<T: DeserializeOwned>(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<T, AppError> {
    let mut request = client().get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }

    let response = request
        .send()
        .map_err(|error| AppError::network(&error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        // 带上响应体的前若干字节：中转商的 401 往往在 body 里写明
        // 「余额不足」还是「key 无效」，只报 HTTP 401 等于把线索丢了。
        let detail = response.text().unwrap_or_default();
        let detail: String = detail.chars().take(200).collect();
        return Err(AppError::network(&if detail.trim().is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status}：{}", detail.trim())
        }));
    }

    response
        .json::<T>()
        .map_err(|error| AppError::network(&error.to_string()))
}

/// 取二进制（瓦片图）。
///
/// # Errors
///
/// 网络失败、超时或非 2xx 时返回错误。返回空体的情况交给调用方判 ——
/// 瓦片源偶尔给 200 + 空体，那是「没有这块图」而不是错误。
pub fn get_bytes(url: &str) -> Result<Vec<u8>, AppError> {
    let response = client()
        .get(url)
        .send()
        .map_err(|error| AppError::network(&error.to_string()))?;

    if !response.status().is_success() {
        return Err(AppError::network(&format!("HTTP {}", response.status())));
    }

    Ok(response
        .bytes()
        .map_err(|error| AppError::network(&error.to_string()))?
        .to_vec())
}
