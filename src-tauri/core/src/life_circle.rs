//! 百度 POI 检索与真实步行时间组成的 15 分钟生活圈。
//!
//! 不按直线距离声称“可达”：每个保留的 POI 都经过百度步行路线验证。

use serde_json::Value;

use crate::error::AppError;
use crate::http::{encode_component, get_json};
use crate::models::{Crs, LatLng, LifeCircle, LifeCirclePackage, ReachablePoi};

const PLACE_URL: &str = "https://api.map.baidu.com/place/v2/search";
const WALK_URL: &str = "https://api.map.baidu.com/direction/v2/walking";
const RADIUS_M: u32 = 1_800;
const LIMIT_SECONDS: u32 = 15 * 60;
const CANDIDATES_PER_CATEGORY: usize = 3;

fn categories(package: LifeCirclePackage) -> &'static [&'static str] {
    match package {
        LifeCirclePackage::Care => &["医院", "药店", "卫生间"],
        LifeCirclePackage::Family => &["公园", "便利店", "地铁站"],
        LifeCirclePackage::Emergency => &["医院", "药店", "警务室"],
    }
}

fn point(value: &Value) -> Option<LatLng> {
    Some(LatLng::new(value.get("lat")?.as_f64()?, value.get("lng")?.as_f64()?))
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().filter(|text| !text.is_empty()).map(str::to_owned)
}

fn search(key: &str, origin: LatLng, category: &str) -> Result<Vec<(String, String, LatLng)>, AppError> {
    let url = format!(
        "{PLACE_URL}?query={}&location={:.6},{:.6}&radius={RADIUS_M}&scope=2&page_size={CANDIDATES_PER_CATEGORY}&output=json&ak={}",
        encode_component(category),
        origin.lat,
        origin.lng,
        encode_component(key),
    );
    let response: Value = get_json(&url)?;
    let status = response.get("status").and_then(Value::as_i64);
    if status != Some(0) {
        let detail = text(&response, "message").unwrap_or_else(|| "未知错误".to_string());
        // 数字状态码才是能查的东西:240 = 该 AK 没开通 Place API,302 = 配额用尽,
        // 210 = Referer 校验不通过。message 的中文在百度侧改过好几版。
        tracing::warn!(category, ?status, detail = %detail, "百度地点检索失败");
        return Err(AppError::upstream(
            "百度地点检索",
            &format!("{detail}(status={})", status.unwrap_or(-1)),
        ));
    }
    let found: Vec<(String, String, LatLng)> = response
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| Some((text(item, "name")?, text(item, "address").unwrap_or_default(), point(item.get("location")?)?)))
        .collect();
    tracing::debug!(category, count = found.len(), "百度地点检索返回候选");
    Ok(found)
}

fn walking(key: &str, origin: LatLng, destination: LatLng) -> Result<(u32, u32), AppError> {
    let url = format!(
        "{WALK_URL}?origin={:.6},{:.6}&destination={:.6},{:.6}&output=json&ak={}",
        origin.lat,
        origin.lng,
        destination.lat,
        destination.lng,
        encode_component(key),
    );
    let response: Value = get_json(&url)?;
    let status = response.get("status").and_then(Value::as_i64);
    if status != Some(0) {
        let detail = text(&response, "message").unwrap_or_else(|| "未知错误".to_string());
        tracing::debug!(?status, detail = %detail, "百度步行路线失败");
        return Err(AppError::upstream(
            "百度步行路线",
            &format!("{detail}(status={})", status.unwrap_or(-1)),
        ));
    }
    let route = response.pointer("/result/routes/0").ok_or_else(|| AppError::upstream("百度步行路线", "响应缺少路线"))?;
    let distance = route.get("distance").and_then(Value::as_u64).unwrap_or(0) as u32;
    let duration = route.get("duration").and_then(Value::as_u64).unwrap_or(0) as u32;
    Ok((distance, duration))
}

/// 把候选 POI 按真实步行时长筛成生活圈。
///
/// `walk` 抽成参数只为可测:真实实现是一次百度请求,测试传一个假的。
fn assemble(
    package: LifeCirclePackage,
    candidates: Vec<(&'static str, String, String, LatLng)>,
    mut walk: impl FnMut(LatLng) -> Result<(u32, u32), AppError>,
) -> LifeCircle {
    let mut pois = Vec::new();
    for (category, name, address, location) in candidates {
        // 单个 POI 的步行路线失败不能带崩整个生活圈 —— 百度对隔水域、
        // 园区内部点这类目的地会返回非 0,那只说明「这一个点走不到」。
        let (walk_distance, walk_duration_sec) = match walk(location) {
            Ok(pair) => pair,
            Err(error) => {
                tracing::debug!(category, poi = %name, %error, "步行路线算不出来，跳过这个点");
                continue;
            }
        };
        if walk_duration_sec <= LIMIT_SECONDS {
            pois.push(ReachablePoi {
                category: category.to_string(),
                name,
                address,
                location,
                walk_duration_sec,
                walk_distance,
            });
        }
    }
    pois.sort_by_key(|poi| poi.walk_duration_sec);
    tracing::info!(?package, kept = pois.len(), "生活圈完成");
    LifeCircle {
        package,
        crs: Crs::Bd09,
        limit_minutes: LIMIT_SECONDS / 60,
        pois,
    }
}

/// 搜索服务 POI，使用真实百度步行路线过滤为 15 分钟生活圈。
///
/// # Errors
///
/// 百度地点检索失败时返回错误（AK 未开通、配额用尽这类问题换类别也一样失败）。
/// 单个 POI 的步行路线失败只跳过那个点。
pub fn life_circle(key: &str, origin: LatLng, package: LifeCirclePackage) -> Result<LifeCircle, AppError> {
    let mut candidates = Vec::new();
    for category in categories(package) {
        for (name, address, location) in search(key, origin, category)? {
            candidates.push((*category, name, address, location));
        }
    }
    Ok(assemble(package, candidates, |destination| {
        walking(key, origin, destination)
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    fn candidate(name: &str) -> (&'static str, String, String, LatLng) {
        ("医院", name.to_string(), "某路 1 号".to_string(), LatLng::new(39.9, 116.4))
    }

    #[test]
    fn 单个点步行算不出来时其余点照常保留() {
        let candidates = vec![candidate("走不到的点"), candidate("能走到的点")];
        let mut first = true;
        let circle = assemble(LifeCirclePackage::Care, candidates, |_| {
            if std::mem::take(&mut first) {
                Err(AppError::upstream("百度步行路线", "步行不可达"))
            } else {
                Ok((600, 480))
            }
        });
        assert_eq!(circle.pois.len(), 1);
        assert_eq!(circle.pois[0].name, "能走到的点");
    }

    #[test]
    fn 超过十五分钟的点被筛掉() {
        let circle = assemble(LifeCirclePackage::Care, vec![candidate("太远了")], |_| {
            Ok((3000, LIMIT_SECONDS + 1))
        });
        assert!(circle.pois.is_empty());
    }

    #[test]
    fn 正好十五分钟算可达() {
        let circle = assemble(LifeCirclePackage::Care, vec![candidate("刚好")], |_| {
            Ok((1200, LIMIT_SECONDS))
        });
        assert_eq!(circle.pois.len(), 1);
    }

    #[test]
    fn 结果按步行时长升序() {
        let candidates = vec![candidate("远"), candidate("近")];
        let mut durations = [600u32, 120u32].into_iter();
        let circle = assemble(LifeCirclePackage::Care, candidates, |_| {
            Ok((500, durations.next().unwrap()))
        });
        assert_eq!(circle.pois[0].name, "近");
        assert_eq!(circle.pois[1].name, "远");
    }
}
