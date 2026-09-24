//! Tauri command：只做参数校验与转发，业务逻辑全在核心层。
//!
//! # 为什么都要走 `spawn_blocking`
//!
//! 核心层是阻塞的（HTTP + SQLite）。直接在 command 里调用会卡住 WebView
//! 所在的线程，表现为「输入时界面卡顿、拖动地图不跟手」。
//! 丢到阻塞线程池之后，界面线程只等结果。
//!
//! # 为什么每次都重新读配置
//!
//! 用户在设置页改完 Key，下一次算路必须立刻生效。见
//! `autobee_shell::credentials::resolve`。
//!
//! # 为什么这里只有 `pub mod` 与 `pub use`
//!
//! `lib.rs` 的 `generate_handler!` 按路径引用每个 command。把它们从
//! `commands::xxx` 挪进 `commands::map::xxx` 会让那份清单跟着改，
//! 而清单的每一行都是前端 `invoke` 的字符串契约。这里重新导出之后
//! 路径不变，拆文件对 `lib.rs` 和前端都是无感的。
//!
//! # 为什么是 glob 而不是逐名导出
//!
//! `#[tauri::command]` 除了函数本身，还在它旁边生成两个隐藏项
//! （`__cmd__xxx` 与 `__tauri_command_name_xxx`），`generate_handler!`
//! 展开后引用的是它们。逐名 `pub use map::{a, b, …}` 带不走隐藏项，
//! 编译会报 24 个 `could not find __cmd__xxx in commands` —— 而报错里
//! 一个字都没提宏生成。别把下面这行「整理」成显式清单。

/// 地图能力：服务商档案、界面偏好、联想、算路、瓦片、生活圈、日志。
pub mod map;

pub use map::*;
