//! 读回自己写出的日志，供界面里的日志面板展示。
//!
//! # 为什么要在界面里读日志
//!
//! 这是个桌面程序，用户（和开发时的我们）看不到 stdout：WebView 里的
//! `console` 只有前端那一半，后端的 `tracing` 全进了文件。出问题时
//! 「前端弹了一句失败、日志在某个目录的某个文件里」等于没有日志。
//!
//! # 为什么是拉取而不是推送
//!
//! 推送要在 `tracing` 里挂一个自定义 Layer、把行缓存在内存里、再 emit
//! 事件给前端。而文件本来就在写，从尾部读回来就够了 —— 少一个 Layer、
//! 少一份内存副本，且崩溃前写下的那几行也读得到（内存副本会跟着丢）。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use serde::Serialize;

use crate::error::AppError;

/// 一次最多回读多少字节。
///
/// 日志按天累积没有上限，整文件读进内存在跑了几周的机器上是几十 MB。
/// 256 KB 大约是两三千行，足够覆盖「刚才那次操作为什么失败」。
const TAIL_BYTES: u64 = 256 * 1024;

/// 一条解析好的日志。
///
/// 时间、级别、目标分开给，前端才能按级别上色、按级别过滤。整行塞给前端
/// 的话过滤就得在前端拿正则再切一遍。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// 行号。同一次读取内唯一，前端拿它做列表 key。
    pub seq: usize,
    /// 时间戳原文（RFC 3339）。不解析成结构化时间 —— 前端只是显示。
    pub time: String,
    /// 级别：`ERROR` / `WARN` / `INFO` / `DEBUG` / `TRACE`。
    pub level: String,
    /// 发出这条日志的模块路径。
    pub target: String,
    /// 消息与结构化字段。
    pub message: String,
}

/// 把 `tracing` 默认 fmt 的一行切成结构化的一条。
///
/// 形如 `2026-09-23T02:37:59.100951Z  INFO route_planner_lib::x: 消息 k=v`。
/// 切不动的行（多行 panic 回溯、用户手改过的文件）不丢弃，整行当 message
/// 给出去 —— 排查时最需要看到的往往正是这种不合格式的行。
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
    // 级别与 target 之间是一个空格，target 后面跟冒号。
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

/// 读日志失败。`doing` 说明卡在哪一步 —— 「打不开」与「读到一半断了」
/// 要的处置不同（前者多半是权限，后者是磁盘）。
fn read_failed(doing: &str, error: &std::io::Error) -> AppError {
    AppError::new("LOG_READ_FAILED", format!("{doing}失败：{error}"), 500)
}

/// 读日志尾部，最多 `limit` 条，旧的在前。
///
/// # Errors
///
/// 文件存在但打不开、读不动时返回错误。
pub fn tail(limit: usize) -> Result<Vec<LogEntry>, AppError> {
    tail_of(&crate::paths::log_file(), limit)
}

/// [`tail`] 的实现。路径是参数而不是写死的，否则「文件不存在时给空列表」
/// 这条只能在真的没有日志的机器上验证 —— 那是跑一次就失效的测试。
///
/// 日志文件还不存在时返回空列表而不是错误 —— 干净装完还没触发过任何
/// 操作时就是这个状态，报错会让人以为坏了。
///
/// # Errors
///
/// 文件存在但打不开、读不动时返回错误。
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

    // 日志里有中文，按字节截断会切在 UTF-8 字符中间。`from_utf8_lossy`
    // 把那半个字符换成替换符，而不是整批读取失败。
    let text = String::from_utf8_lossy(&buffer);
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    // 从中间切进去时第一行大概率是半行，扔掉。文件本来就短（没被截断）时留着。
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
        // 干净装完、还没触发任何操作时就是这个状态。报错会让人以为坏了。
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
