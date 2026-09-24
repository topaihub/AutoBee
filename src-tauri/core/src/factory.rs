//! Provider 工厂：`ProviderId` → 一个 adapter。接新厂商只需实现 adapter 并在
//! `create_provider` 加一个 case。

use crate::config::Config;
use crate::error::AppError;
use crate::models::ProviderId;
use crate::providers::RouteProvider;
use crate::providers::amap::AmapProvider;
use crate::providers::baidu::BaiduProvider;
use crate::providers::tencent::TencentProvider;

/// 设置页据此标「待接入」，否则用户填了 Key 却永远选不中它、界面上也没有解释。
#[must_use]
pub fn is_implemented(id: ProviderId) -> bool {
    matches!(
        id,
        ProviderId::Tencent | ProviderId::Amap | ProviderId::Baidu
    )
}

/// # Errors
///
/// 没配 Key，或 adapter 还没实现时返回错误。两者处置一样（跳过、试下一家），共用一个码。
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
        ProviderId::Baidu => Ok(Box::new(BaiduProvider::new(key))),
        other => Err(AppError::not_implemented(other.as_str())),
    }
}
