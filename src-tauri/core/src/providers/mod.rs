//! 各家厂商的 adapter。契约：每家实现 [`RouteProvider`]，把原始响应折成归一化的
//! [`RoutePlan`](crate::models::RoutePlan)。**归一化数据里不能出现厂商特有字段**
//! —— 出现一次前端就得为这家写一次分支。

pub mod amap;
pub mod baidu;
pub mod photon;
pub mod tencent;

use crate::error::AppError;
use crate::models::{LatLng, Place, ProviderId, RoutePlan, SortBy, TravelMode};

/// 起终点**必须已经是坐标**：文本 → 坐标的解析归编排层 [`crate::planner`]。
#[derive(Debug, Clone, PartialEq)]
pub struct RouteRequest {
    pub from: LatLng,
    pub to: LatLng,
    pub mode: TravelMode,
    pub sort: SortBy,
    /// 起点名称，取该 provider 解析出的名字（含自动纠错结果）。
    pub from_title: Option<String>,
    pub to_title: Option<String>,
}

pub trait RouteProvider {
    fn id(&self) -> ProviderId;

    /// # Errors
    ///
    /// 网络或上游失败时返回错误。**返回空数组不算错误**，调用方会退到地址解析。
    fn suggest(&self, keyword: &str, page_size: usize) -> Result<Vec<Place>, AppError>;

    /// # Errors
    ///
    /// 网络失败时返回错误。搜不到不是错误，返回 `Ok(None)`。
    fn geocode(&self, address: &str) -> Result<Option<Place>, AppError>;

    /// # Errors
    ///
    /// 网络失败、上游报错、或算不出任何方案时返回错误。
    fn plan_route(&self, req: &RouteRequest) -> Result<RoutePlan, AppError>;
}
