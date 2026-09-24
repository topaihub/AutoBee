//! 各家厂商的 adapter。
//!
//! # 契约
//!
//! 每家实现同一个 [`RouteProvider`]，把自家的原始响应折成归一化的
//! [`RoutePlan`](crate::models::RoutePlan)。**归一化数据里不能出现任何厂商特有
//! 的字段** —— 出现一次，前端就得为这家写一次分支，抽象层就白做了。

pub mod amap;
pub mod baidu;
pub mod photon;
pub mod tencent;

use crate::error::AppError;
use crate::models::{LatLng, Place, ProviderId, RoutePlan, SortBy, TravelMode};

/// 一次算路请求。起终点**必须已经是坐标** ——
/// 文本 → 坐标的解析由编排层（[`crate::planner`]）负责，因为那份逻辑要按
/// 「先联想后地址解析」的顺序试，且失败时要换下一家重试。
#[derive(Debug, Clone, PartialEq)]
pub struct RouteRequest {
    /// 起点坐标。
    pub from: LatLng,
    /// 终点坐标。
    pub to: LatLng,
    /// 出行方式。
    pub mode: TravelMode,
    /// 排序依据。
    pub sort: SortBy,
    /// 起点名称。用「该 provider 解析出来的名字」更贴近实际（含自动纠错结果）。
    pub from_title: Option<String>,
    /// 终点名称。
    pub to_title: Option<String>,
}

/// 一家厂商的接入。
pub trait RouteProvider {
    /// 这家是谁。
    fn id(&self) -> ProviderId;

    /// 地点联想 / POI 搜索。
    ///
    /// # Errors
    ///
    /// 网络失败或上游返回错误时返回错误。**返回空数组不算错误** —— 搜不到
    /// 是正常的，调用方会退到地址解析。
    fn suggest(&self, keyword: &str, page_size: usize) -> Result<Vec<Place>, AppError>;

    /// 结构化地址解析。搜不到时返回 `Ok(None)`。
    ///
    /// # Errors
    ///
    /// 网络失败时返回错误。
    fn geocode(&self, address: &str) -> Result<Option<Place>, AppError>;

    /// 算路。
    ///
    /// # Errors
    ///
    /// 网络失败、上游报错、或算不出任何方案时返回错误。
    fn plan_route(&self, req: &RouteRequest) -> Result<RoutePlan, AppError>;
}
