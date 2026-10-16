//! 密钥掩码。日志本身由 `aether_observability` 初始化（见根包 `lib.rs`）。
//!
//! # 为什么必须有掩码
//!
//! 日志会被用户打包发给我们排查问题。而地图服务商的 Key 一旦进日志，
//! 那个包就等于附带了凭据。所以**任何一行写 Key 的地方都要先过
//! [`mask_secret`]** —— 包括「Key 为空」这种看似无害的诊断。

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
