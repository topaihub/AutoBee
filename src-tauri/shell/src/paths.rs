//! 应用数据目录。
//!
//! 所有运行期数据都放在 exe 同目录的 `data` 下，便携版复制整个目录即可迁移。
//! 安装到受保护目录时需要该目录具备写权限。

use std::path::PathBuf;

/// 数据根目录：`<exe 目录>\data`。
///
/// `current_exe` 失败时退回当前工作目录；不使用系统 AppData，避免数据库和日志
/// 分散到用户看不见的位置。
#[must_use]
pub fn data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(std::env::temp_dir)
        .join("data")
}

/// 瓦片缓存目录。
#[must_use]
pub fn tile_cache_dir() -> PathBuf {
    data_dir().join("tiles")
}

/// 档案库路径。
#[must_use]
pub fn settings_db() -> PathBuf {
    data_dir().join("settings.db")
}

/// 日志文件路径。
#[must_use]
pub fn log_file() -> PathBuf {
    data_dir().join("logs").join("autobee.log")
}

#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
#[cfg(test)]
mod tests {
    use super::*;

    /// 便携版的全部意义在这一条:数据跟着 exe 走,不进用户看不见的系统目录。
    /// 一旦有人把它改回 `%APPDATA%\<identifier>\`,复制目录迁移就会悄悄丢档案库。
    #[test]
    fn 数据目录跟着_exe_而不是系统_appdata() {
        let exe = std::env::current_exe().unwrap();
        let exe_dir = exe.parent().unwrap();

        assert_eq!(
            data_dir(),
            exe_dir.join("data"),
            "数据根目录必须是 exe 同级的 data"
        );
        assert_eq!(settings_db(), exe_dir.join("data").join("settings.db"));
        assert_eq!(
            log_file(),
            exe_dir.join("data").join("logs").join("autobee.log")
        );
        assert_eq!(tile_cache_dir(), exe_dir.join("data").join("tiles"));
    }

    /// 光比对拼接结果还不够:万一 `data_dir` 改成读环境变量,上面那条在
    /// 恰好没设该变量的机器上仍会过。这里直接断言路径里不出现系统目录。
    #[test]
    fn 数据目录不落在_appdata_或_temp() {
        let path = settings_db();
        let text = path.to_string_lossy().to_ascii_lowercase();
        for var in ["APPDATA", "LOCALAPPDATA", "USERPROFILE"] {
            if let Ok(value) = std::env::var(var) {
                let value = value.to_ascii_lowercase();
                // 仓库自身就在用户目录下时 USERPROFILE 会是真前缀,那不算越界,
                // 只在它形如 `<USERPROFILE>\AppData\...` 时才是落回了系统目录。
                let escaped = text.starts_with(&value) && text.contains("\\appdata\\");
                assert!(!escaped, "{var} 下的系统目录:{text}");
            }
        }
    }
}
