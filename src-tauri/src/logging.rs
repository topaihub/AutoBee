//! 日志初始化与密钥掩码。
//!
//! # 为什么必须有掩码
//!
//! 日志会被用户打包发给我们排查问题。而地图服务商的 Key 一旦进日志，
//! 那个包就等于附带了凭据。所以**任何一行写 Key 的地方都要先过
//! [`mask_secret`]** —— 包括「Key 为空」这种看似无害的诊断。

use std::fs::OpenOptions;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;

/// 掩码一个密钥：留头尾各 4 个字符。
///
/// 按字符而不是字节切 —— 密钥理论上都是 ASCII，但真有人把中文粘进来时
/// 按字节切会 panic 在字符边界上。短于 12 个字符的全遮。
#[must_use]
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() < 12 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}********{tail}")
}

/// 初始化日志。默认 `info`，可用 `RUST_LOG` 覆盖；日志写入 exe 同目录的 `data\autobee.log`。
///
/// 失败不 panic：日志写不出来时程序该照常跑，而不是启动即崩。
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let path = crate::paths::log_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let writer = OpenOptions::new().create(true).append(true).open(path);
    match writer {
        Ok(file) => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(file.with_max_level(tracing::Level::TRACE))
                .try_init();
        }
        Err(_) => {
            let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 长密钥留头尾各四位() {
        assert_eq!(mask_secret("FAKEKEYFORTESTONLY00"), "FAKE********LY00");
    }

    #[test]
    fn 短密钥全遮且不泄漏长度外的信息() {
        let masked = mask_secret("abc");
        assert_eq!(masked, "***");
        assert!(!masked.contains('a'));
    }

    #[test]
    fn 空密钥得到空串() {
        assert_eq!(mask_secret(""), "");
    }

    #[test]
    fn 中文密钥不会切在字符中间() {
        // 按字节切这里会 panic。
        let masked = mask_secret("密钥密钥密钥密钥密钥密钥");
        assert_eq!(masked.chars().count(), 16);
        assert!(masked.starts_with("密钥密钥"));
    }
}
