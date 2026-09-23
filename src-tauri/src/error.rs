//! 应用侧错误。
//!
//! 直接复用核心层的 `AppError`：它已经带 code / message，而且实现了
//! `Serialize`（Tauri 的 command 要求错误可序列化）。另起一个类型只会让
//! command 多一层转换，而那一层转换里最容易丢的正是 code。

pub use route_core::error::AppError;
