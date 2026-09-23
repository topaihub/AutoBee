//! 路线规划核心。
//!
//! # 这一层为什么独立于 Tauri
//!
//! 算路、坐标系换算、polyline 解码、provider 降级这些逻辑的正确性全靠单测守住，
//! 而它们与「这是个桌面应用」无关。独立成 crate 之后 `cargo test -p route-core`
//! 只需编译 reqwest 与 serde（几十秒），不必带上 tauri 那一整套（十几分钟）。
//!
//! # 分层
//!
//! - [`models`] —— 归一化契约，前端只消费这里的结构
//! - [`geo`] / [`decode`] —— 纯函数，最容易写错的两处（见各自模块文档）
//! - [`providers`] —— 每家厂商一个 adapter，把原始响应折成 [`models::RoutePlan`]
//! - [`registry`] / [`factory`] / [`circuit`] —— 选哪家、怎么降级
//! - [`planner`] —— 编排：解析 → 选 provider → 算路 → 坐标系统一
//! - [`tiles`] —— 底图瓦片的抓取与磁盘缓存

pub mod circuit;
pub mod config;
pub mod decode;
pub mod error;
pub mod factory;
pub mod geo;
pub mod http;
pub mod life_circle;
pub mod models;
pub mod planner;
pub mod providers;
pub mod registry;
pub mod tiles;
