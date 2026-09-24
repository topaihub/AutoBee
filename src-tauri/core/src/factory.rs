//! Provider 工厂：`ProviderId` → 一个 adapter。
//!
//! 接第二家（比如高德）时：实现 adapter，然后在这里加一行 case。
//! 编排层与前端都不用改。

use crate::config::Config;
use crate::error::AppError;
use crate::models::ProviderId;
use crate::providers::RouteProvider;
use crate::providers::amap::AmapProvider;
use crate::providers::tencent::TencentProvider;

/// 这家的 adapter 是否已经实现。
///
/// 设置页要据此标「待接入」：不标的话用户填了 Key 却永远选不中它，
/// 而界面上没有任何解释（它只是静默地不参与候选）。
#[must_use]
pub fn is_implemented(id: ProviderId) -> bool {
    matches!(id, ProviderId::Tencent | ProviderId::Amap)
}

/// 造一个 adapter。
///
/// # Errors
///
/// 这家没配 Key，或它的 adapter 还没实现时返回错误。
/// 两种情况的处置是一样的（跳过它，试下一家），所以共用一个码。
pub fn create_provider(
    id: ProviderId,
    config: &Config,
) -> Result<Box<dyn RouteProvider>, AppError> {
    let key = config
        .key(id)
        .ok_or_else(|| AppError::not_implemented(id.as_str()))?;
    match id {
        ProviderId::Tencent => Ok(Box::new(TencentProvider::new(key))),
        ProviderId::Amap => Ok(Box::new(AmapProvider::new(key))),
        // baidu / tianditu / here / google 陆续接入
        other => Err(AppError::not_implemented(other.as_str())),
    }
}
