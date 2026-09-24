//! 读回自己写出的日志，供界面里的日志面板展示 —— 桌面程序里用户看不到 stdout。
//! 从文件尾部拉取而不是挂一个 Layer 往前端推：少一份内存副本，且崩溃前写下的
//! 那几行也读得到。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use serde::Serialize;

use crate::error::AppError;

/// 一次最多回读多少字节。整文件读进内存在跑了几周的机器上是几十 MB。
const TAIL_BYTES: u64 = 256 * 1024;

/// 一条解析好的日志。时间、级别、目标分开给，前端才能按级别上色、按级别过滤。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub seq: usize,
    pub time: String,
    /// 级别：`ERROR` / `WARN` / `INFO` / `DEBUG` / `TRACE`。
    pub level: String,
    pub target: String,
    pub message: String,
}

/// 把 `tracing` 默认 fmt 的一行（`2026-09-23T02:37:59.100951Z  INFO x::y: 消息 k=v`）
/// 切成结构化的一条。切不动的行（多行 panic 回溯）整行当 message 给出去。
fn parse_line(seq: usize, line: &str) -> LogEntry {
    let unparsed = |message: &str| LogEntry {
        seq,
        time: String::new(),
        level: "INFO".to_string(),
        target: String::new(),
        message: message.to_string(),
    };

    let mut parts = line.splitn(2, "  ");
    let Some(time) = parts.next().filter(|t| t.ends_with('Z')) else {
        return unparsed(line);
    };
    let Some(rest) = parts.next() else {
        return unparsed(line);
    };
    let mut rest = rest.splitn(2, ' ');
    let Some(level) = rest.next() else {
        return unparsed(line);
    };
    let Some(tail) = rest.next() else {
        return unparsed(line);
    };
    let (target, message) = tail.split_once(": ").unwrap_or((tail, ""));

    LogEntry {
        seq,
        time: time.to_string(),
        level: level.to_string(),
        target: target.to_string(),
        message: message.to_string(),
    }
}

fn read_failed(doing: &str, error: &std::io::Error) -> AppError {
    AppError::new("LOG_READ_FAILED", format!("{doing}失败：{error}"), 500)
}

/// 最多 `limit` 条，旧的在前。
///
/// # Errors
///
/// 文件存在但打不开、读不动时返回错误。
pub fn tail(limit: usize) -> Result<Vec<LogEntry>, AppError> {
    tail_of(&crate::paths::log_file(), limit)
}

/// [`tail`] 的实现。日志文件还不存在时返回空列表而不是错误 —— 干净装完还没触发过
/// 任何操作时就是这个状态。
fn tail_of(path: &std::path::Path, limit: usize) -> Result<Vec<LogEntry>, AppError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(read_failed("打开日志", &error)),
    };

    let size = file
        .metadata()
        .map_err(|error| read_failed("读取日志大小", &error))?
        .len();
    let from = size.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(from))
        .map_err(|error| read_failed("定位日志尾部", &error))?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|error| read_failed("读取日志", &error))?;

    // 按字节截断会切在 UTF-8 字符中间，`from_utf8_lossy` 只把那半个字符换成替换符。
    let text = String::from_utf8_lossy(&buffer);
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    // 从中间切进去时第一行大概率是半行，扔掉。
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..]
        .iter()
        .enumerate()
        .map(|(index, line)| parse_line(start + index, line))
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 切开一行标准日志() {
        let entry = parse_line(
            0,
            "2026-09-23T02:37:59.100951Z  WARN route_planner_lib::credentials::store: 已保存服务商密钥 provider=tencent",
        );
        assert_eq!(entry.time, "2026-09-23T02:37:59.100951Z");
        assert_eq!(entry.level, "WARN");
        assert_eq!(entry.target, "route_planner_lib::credentials::store");
        assert_eq!(entry.message, "已保存服务商密钥 provider=tencent");
    }

    #[test]
    fn 不合格式的行整行留下而不是丢掉() {
        // panic 回溯就是这个形状。丢掉它等于排查时最该看的那几行不见了。
        let entry = parse_line(3, "stack backtrace:");
        assert_eq!(entry.message, "stack backtrace:");
        assert_eq!(entry.seq, 3);
    }

    #[test]
    fn 日志文件不存在时给空列表而不是报错() {
        let missing = std::env::temp_dir().join("autobee-没有这个日志文件.log");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(tail_of(&missing, 50).unwrap(), Vec::new());
    }

    #[test]
    fn 只回读最后若干条且顺序是旧的在前() {
        let path = std::env::temp_dir().join("autobee-tail-测试.log");
        let lines: Vec<String> = (0..10)
            .map(|i| format!("2026-09-23T02:37:5{i}.0Z  INFO t: 第{i}行"))
            .collect();
        std::fs::write(&path, lines.join("\n")).unwrap();

        let entries = tail_of(&path, 3).unwrap();
        let messages: Vec<&str> = entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, vec!["第7行", "第8行", "第9行"]);

        let _ = std::fs::remove_file(&path);
    }
}
