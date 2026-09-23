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
