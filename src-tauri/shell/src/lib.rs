//! AutoBee 外壳层：凭据存储、地图工具、路线快照、本机路径。
//!
//! 不依赖 Tauri，所以 GUI（`autobee`）与 CLI（`autobee-cli`）两个入口
//! 都能用同一套密钥，不用各写一份。大模型能力在 Aether，见根包 `host.rs`。

pub mod credentials;
pub mod error;
pub mod logging;
pub mod paths;
pub mod snapshots;
pub mod tools;
