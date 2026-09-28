//! 大模型服务入口的配置层。
//!
//! 配置模型是两层：`llm_providers` 一行 = 一个可用的服务入口（名称 + base_url
//! + 协议 + Key），`llm_models` 一行 = 用户从该入口挑中的一个模型。
//!
//! 没有「同一家多条线路负载均衡」那一层：AutoBee 不需要，合并后少一张表和一组 CRUD。

pub mod agent;
pub mod chat;
pub mod conversations;
pub mod mcp;
pub mod probe;
pub mod protocol;
pub mod sse;
pub mod store;
pub mod tools;
