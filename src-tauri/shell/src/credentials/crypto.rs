//! 密钥的落盘加密：Windows DPAPI。`settings.db` 是磁盘上一个路径固定的 SQLite 文件，
//! 明文存 Key 等于把凭据抄了一份到用户不知道的地方。DPAPI 的密钥由 Windows 按登录
//! 凭据管、per-user 绑定，别的账户拿到 db 文件也解不开。
//!
//! db 从别的机器或账户拷过来时必然解不开，那时要能正常打开设置页让用户重填，
//! 所以 [`unprotect`] 返回 `Option`。

#[cfg(windows)]
use windows::Win32::Foundation::{HLOCAL, LocalFree};
#[cfg(windows)]
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData,
};

/// 加密一个密钥，得到可以落盘的字节。返回 `None` 表示 DPAPI 不可用，此时调用方
/// **不要退而存明文** —— 那会让「加密存储」变成有时成立，而用户无从知道是哪一次。
#[cfg(windows)]
#[allow(unsafe_code, reason = "DPAPI 没有安全封装，块内写了 SAFETY")]
pub(crate) fn protect(secret: &str) -> Option<Vec<u8>> {
    if secret.is_empty() {
        return Some(Vec::new());
    }

    let mut input = blob(secret.as_bytes());
    let mut output = CRYPT_INTEGER_BLOB::default();

    let result =
        // SAFETY: `input` 指向本函数栈上的 blob，它借用的 `secret` 在调用期间有效。
        // 其余四个可选参数传 None（不加额外熵、无提示界面）。`output` 由 DPAPI
        // 分配，成功后必须 LocalFree —— 下面 take_blob 负责。
        unsafe {
            CryptProtectData(&raw mut input, None, None, None, None, 0, &raw mut output)
        };

    match result {
        Ok(()) => Some(take_blob(&mut output)),
        Err(error) => {
            // 不记密钥本身，也不记长度 —— 长度也是信息。
            tracing::warn!(%error, "DPAPI 加密失败，密钥不会被保存");
            None
        }
    }
}

/// 解回明文。解不开返回 `None`（见模块文档）。
#[cfg(windows)]
#[allow(unsafe_code, reason = "DPAPI 没有安全封装，块内写了 SAFETY")]
pub(crate) fn unprotect(cipher: &[u8]) -> Option<String> {
    if cipher.is_empty() {
        return Some(String::new());
    }

    let mut input = blob(cipher);
    let mut output = CRYPT_INTEGER_BLOB::default();

    let result =
        // SAFETY: 同 protect。`cipher` 在调用期间有效，`output` 成功后要 LocalFree。
        unsafe { CryptUnprotectData(&raw mut input, None, None, None, None, 0, &raw mut output) };

    match result {
        Ok(()) => {
            let plain = take_blob(&mut output);
            // 解出来不是 UTF-8 说明这不是我们写的密文（或者 db 坏了），当「没有密钥」处理。
            String::from_utf8(plain).ok()
        }
        Err(error) => {
            // 预期内的失败：db 换了机器或账户。
            tracing::debug!(%error, "DPAPI 解密失败，当作没填密钥");
            None
        }
    }
}

/// 借一段字节做 DPAPI 的输入 blob。只借不拥有。
#[cfg(windows)]
fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).unwrap_or(u32::MAX),
        // 转成 *mut 是 API 的形状要求；DPAPI 不写输入 blob。
        pbData: data.as_ptr().cast_mut(),
    }
}

/// 把 DPAPI 分配的 blob 拷进 Vec 并释放它。不释放是每次读写档案泄漏几十字节。
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "读 DPAPI 分配的 blob 并 LocalFree，块内写了 SAFETY"
)]
fn take_blob(out: &mut CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    let copied =
        // SAFETY: DPAPI 成功返回时保证 pbData 指向 cbData 字节的有效内存。
        unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    // SAFETY: pbData 由 DPAPI 用 LocalAlloc 分配，配对的释放就是 LocalFree。
    let _ = unsafe { LocalFree(Some(HLOCAL(out.pbData.cast()))) };
    out.pbData = std::ptr::null_mut();
    copied
}

/// 非 Windows 下不提供加密：返回 `None` 而不是存明文。留桩是为了 `cargo check` 能过。
#[cfg(not(windows))]
pub(crate) fn protect(_secret: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
pub(crate) fn unprotect(_cipher: &[u8]) -> Option<String> {
    None
}

#[cfg(all(test, windows))]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 加密再解密拿回原文() {
        let secret = "FAKE-KEY-FOR-TEST";
        let cipher = protect(secret).unwrap();
        assert_eq!(unprotect(&cipher).unwrap(), secret);
    }

    #[test]
    fn 密文与明文不同且更长() {
        // 防的是「加密」被实现成直接拷字节。
        let secret = "FAKE-KEY-FOR-TEST";
        let cipher = protect(secret).unwrap();
        assert_ne!(cipher.as_slice(), secret.as_bytes());
        assert!(cipher.len() > secret.len(), "DPAPI 会加上自己的头");
    }

    #[test]
    fn 空串往返得到空串() {
        // 「还没填密钥」是正常状态，不该报错也不该走一趟 DPAPI。
        assert_eq!(protect("").unwrap(), Vec::<u8>::new());
        assert_eq!(unprotect(&[]).unwrap(), "");
    }

    #[test]
    fn 乱数据解不开时返回_none() {
        // db 从别的机器拷来就是这个现象。必须不 panic。
        assert!(unprotect("这不是 DPAPI 密文".as_bytes()).is_none());
        assert!(unprotect(&[0_u8; 64]).is_none());
    }

    #[test]
    fn 连续两次加密同一个密钥得到不同密文() {
        // DPAPI 自带盐。相同的话说明退化成了确定性加密，db 里就能看出这两条用的是同一个密钥。
        let first = protect("same-key").unwrap();
        let second = protect("same-key").unwrap();
        assert_ne!(first, second);
        assert_eq!(unprotect(&first).unwrap(), "same-key");
        assert_eq!(unprotect(&second).unwrap(), "same-key");
    }
}
