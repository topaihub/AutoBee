//! AutoBee 外壳层：凭据存储、大模型配置、日志、本机路径。
//!
//! 不依赖 Tauri，所以 GUI（`autobee`）与 CLI（`autobee-cli`）两个入口
//! 都能用同一套密钥与日志，不用各写一份。

pub mod credentials;
pub mod error;
pub mod llm;
pub mod logging;
pub mod logs;
pub mod paths;
