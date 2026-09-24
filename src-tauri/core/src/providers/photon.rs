//! 阶段一：全球地理编码 —— 只回答「这个地点属于哪个国家」。
//!
//! 选 provider 要先知道坐标属于哪国，而「文本 → 坐标」本身就得调 provider，国内厂商
//! 又只覆盖中国大陆；故拆两阶段，阶段二按国家码选定 provider 后用它的地点搜索精化
//! （地铁站、出入口只有本土服务解析得准）。Photon 落点可能不准，坐标只用于判国家。

use serde::Deserialize;
use serde_json::Value;

use crate::http::{encode_component, get_json};
use crate::models::LatLng;

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalHit {
    /// 坐标（WGS-84）。
    pub location: LatLng,
    /// 国家码，ISO 3166-1 alpha-2，大写。查不到是 `None`。
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub name: Option<String>,
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

// User-Agent 由 `http::client` 统一设置成浏览器风格；Nominatim 见到自定义 UA 返回 403。

fn try_photon(query: &str) -> Option<GlobalHit> {
    let url = format!(
        "https://photon.komoot.io/api/?q={}&limit=1",
        encode_component(query)
    );
    let response: PhotonResponse = get_json(&url).ok()?;
    let feature = response.features.into_iter().next()?;
    let coords = feature.geometry?.coordinates;
    if coords.len() < 2 {
        return None;
    }
    let (lng, lat) = (coords[0], coords[1]);
    let props = feature.properties?;
    Some(GlobalHit {
        location: LatLng::new(lat, lng),
        country_code: props
            .countrycode
            .map(|c| c.to_uppercase())
            .filter(|c| !c.is_empty()),
        country: props.country,
        name: props.name.or(props.city),
        source: "photon",
    })
}

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
        name: first
            .get("display_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        source: "nominatim",
    })
}

/// 查一个地点属于哪个国家。两个服务都失败返回 `None`，调用方应退化为顺序探测。
#[must_use]
pub fn global_locate(query: &str) -> Option<GlobalHit> {
    try_photon(query).or_else(|| try_nominatim(query))
}
