//! 阶段一：全球地理编码 —— 只用来回答「这个地点属于哪个国家」。
//!
//! # 为什么要有这一跳
//!
//! 选 provider 需要知道坐标属于哪国，可「文本 → 坐标」本身就得调某个 provider，
//! 而国内厂商基本只覆盖中国大陆。这是个死循环，拆成两阶段才解得开：
//!
//! 1. **这里**：用一个覆盖全球的免 Key 服务拿到 `lat,lng + country_code`
//! 2. **阶段二**：按国家码选中 provider 后，**再用它自己的地点搜索精化一次**
//!
//! 第二跳不能省：地铁站、出入口这类 POI 只有本土服务解析得准 ——
//! 「铢家村-E1口」在全球服务里查不到，在腾讯 suggestion 里会被纠正成「钟家村-E1口」。
//!
//! # 实测要点
//!
//! - Photon 的 `properties.countrycode` 是 ISO 3166-1 alpha-2（武汉→CN、Tokyo Station→JP）。
//! - Photon 对 POI 的落点可能不准（「光谷大道地铁站」会落到附近别的地方），
//!   所以这里的坐标**只用于判国家**，不用于算路。
//! - Nominatim 作备份，但它的 User-Agent 必须伪装成浏览器，自定义 UA 直接 403。

use serde::Deserialize;
use serde_json::Value;

use crate::http::{encode_component, get_json};
use crate::models::LatLng;

/// 一次全球定位的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalHit {
    /// 坐标（WGS-84）。
    pub location: LatLng,
    /// 国家码，大写。查不到是 `None`。
    pub country_code: Option<String>,
    /// 国家名。
    pub country: Option<String>,
    /// 命中的地名。
    pub name: Option<String>,
    /// 来自哪个服务。
    pub source: &'static str,
}

#[derive(Debug, Deserialize)]
struct PhotonResponse {
    #[serde(default)]
    features: Vec<PhotonFeature>,
}

#[derive(Debug, Deserialize)]
struct PhotonFeature {
    #[serde(default)]
    geometry: Option<PhotonGeometry>,
    #[serde(default)]
    properties: Option<PhotonProperties>,
}

#[derive(Debug, Deserialize)]
struct PhotonGeometry {
    /// GeoJSON 的顺序是 `[lng, lat]`，与前端习惯的 `lat,lng` 相反。
    #[serde(default)]
    coordinates: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct PhotonProperties {
    #[serde(default)]
    countrycode: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    city: Option<String>,
}

// User-Agent 由 `http::client` 统一设置成浏览器风格。不要在请求里另设一个：
// Nominatim 见到自定义 UA 直接 403，而那个 403 看起来像「服务挂了」，
// 排查方向会完全跑偏。

/// Photon 主查询。任何一步失败都返回 `None` —— 这是「判不出国家」而不是错误，
/// 调用方会退化成顺序探测。
fn try_photon(query: &str) -> Option<GlobalHit> {
    let url = format!("https://photon.komoot.io/api/?q={}&limit=1", encode_component(query));
    let response: PhotonResponse = get_json(&url).ok()?;
    let feature = response.features.into_iter().next()?;
    let coords = feature.geometry?.coordinates;
    if coords.len() < 2 {
        return None;
    }
    // GeoJSON: [lng, lat]。写反的现象是坐标跑到完全错误的半球，
    // 而国家码还是对的 —— 判国家不受影响，所以只有算路时才会暴露，很难查。
    let (lng, lat) = (coords[0], coords[1]);
    let props = feature.properties?;
    Some(GlobalHit {
        location: LatLng::new(lat, lng),
        country_code: props.countrycode.map(|c| c.to_uppercase()).filter(|c| !c.is_empty()),
        country: props.country,
        name: props.name.or(props.city),
        source: "photon",
    })
}

/// Nominatim 备份。
fn try_nominatim(query: &str) -> Option<GlobalHit> {
    let url = format!(
        "https://nominatim.openstreetmap.org/search?q={}&format=json&limit=1",
        encode_component(query)
    );
    let items: Vec<Value> = get_json(&url).ok()?;
    let first = items.into_iter().next()?;
    let lat = first.get("lat")?.as_str()?.parse::<f64>().ok()?;
    let lng = first.get("lon")?.as_str()?.parse::<f64>().ok()?;
    Some(GlobalHit {
        location: LatLng::new(lat, lng),
        country_code: first
            .get("address")
            .and_then(|a| a.get("country_code"))
            .and_then(Value::as_str)
            .map(str::to_uppercase)
            .filter(|c| !c.is_empty()),
        country: first
            .get("address")
            .and_then(|a| a.get("country"))
            .and_then(Value::as_str)
            .map(str::to_string),
        name: first.get("display_name").and_then(Value::as_str).map(str::to_string),
        source: "nominatim",
    })
}

/// 查一个地点属于哪个国家。Photon 主，Nominatim 备。
///
/// 两个都失败返回 `None`：调用方应退化为「按已配置的顺序探测」，
/// 而不是报「查不到这个地点」—— 那句话会让人以为地名写错了。
#[must_use]
pub fn global_locate(query: &str) -> Option<GlobalHit> {
    try_photon(query).or_else(|| try_nominatim(query))
}
