//! 高德 Web服务 API adapter。
//!
//! 使用 **Web服务** Key：输入提示、地理编码、步行和公交/地铁路径规划均经
//! `restapi.amap.com` 的 HTTP 接口调用；Android 或 JS API Key 不能用于这里。

use serde_json::Value;

use super::{RouteProvider, RouteRequest};
use crate::decode::{BUS_COLOR, subway_color};
use crate::error::AppError;
use crate::http::{encode_component, get_json};
use crate::models::{
    Crs, Endpoint, LatLng, Place, ProviderId, Route, RoutePlan, Segment, SegmentKind, SortBy, Stop,
    TransitLine, TravelMode, Vehicle,
};

const BASE: &str = "https://restapi.amap.com";
/// 高德 Web服务 API adapter。
#[derive(Debug, Clone)]
pub struct AmapProvider {
    key: String,
}

impl AmapProvider {
    /// 用一个高德 Web服务 Key 建 adapter。
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(value) if !value.is_empty() && value != "[]" => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn number(value: &Value, key: &str) -> u32 {
    text(value, key)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
}

fn minutes(seconds: u32) -> u32 {
    seconds.div_ceil(60)
}

fn point(text: &str) -> Option<LatLng> {
    let (lng, lat) = text.split_once(',')?;
    Some(LatLng::new(
        lat.trim().parse().ok()?,
        lng.trim().parse().ok()?,
    ))
}

fn points(text: Option<&str>) -> Vec<LatLng> {
    text.unwrap_or_default()
        .split(';')
        .filter_map(point)
        .collect()
}

fn bounds(points: &[LatLng], fallback: [LatLng; 2]) -> [LatLng; 2] {
    let Some(first) = points.first().copied() else {
        return fallback;
    };
    let (mut min_lat, mut max_lat, mut min_lng, mut max_lng) =
        (first.lat, first.lat, first.lng, first.lng);
    for point in points.iter().skip(1) {
        min_lat = min_lat.min(point.lat);
        max_lat = max_lat.max(point.lat);
        min_lng = min_lng.min(point.lng);
        max_lng = max_lng.max(point.lng);
    }
    [LatLng::new(min_lat, min_lng), LatLng::new(max_lat, max_lng)]
}

fn location(point: LatLng) -> String {
    format!("{:.6},{:.6}", point.lng, point.lat)
}

fn ensure_ok(value: &Value, service: &str) -> Result<(), AppError> {
    if value.get("status").and_then(Value::as_str) == Some("1") {
        return Ok(());
    }
    Err(AppError::upstream(
        service,
        text(value, "info").as_deref().unwrap_or("未知错误"),
    ))
}

fn stop(value: Option<&Value>) -> Stop {
    let value = value.unwrap_or(&Value::Null);
    Stop {
        title: text(value, "name").unwrap_or_default(),
        location: text(value, "location")
            .and_then(|value| point(&value))
            .unwrap_or(LatLng::new(0.0, 0.0)),
        exit: None,
    }
}

fn city_code(key: &str, from: LatLng) -> Result<String, AppError> {
    let url = format!(
        "{BASE}/v3/geocode/regeo?location={}&key={}",
        location(from),
        key
    );
    let response: Value = get_json(&url)?;
    ensure_ok(&response, "高德逆地理编码")?;
    response
        .pointer("/regeocode/addressComponent/citycode")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::upstream("高德逆地理编码", "响应缺少起点城市编码"))
}

fn walking(response: &Value, sort: SortBy, fallback: [LatLng; 2]) -> Vec<Route> {
    let mut routes: Vec<Route> = response
        .pointer("/route/paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, path)| {
            let steps = path
                .get("steps")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let all_points: Vec<LatLng> = steps
                .iter()
                .flat_map(|step| points(text(step, "polyline").as_deref()))
                .collect();
            let instructions = steps
                .iter()
                .filter_map(|step| text(step, "instruction"))
                .collect();
            let distance = number(path, "distance");
            let duration = minutes(number(path, "duration"));
            Route {
                id: format!("r{index}"),
                distance,
                duration,
                price: 0.0,
                bounds: bounds(&all_points, fallback),
                segments: vec![Segment {
                    kind: SegmentKind::Walk,
                    distance,
                    duration,
                    points: all_points,
                    instruction: instructions,
                    line: None,
                }],
            }
        })
        .collect();
    routes.sort_by_key(|route| {
        if sort == SortBy::Distance {
            route.distance
        } else {
            route.duration
        }
    });
    routes
}

fn transit(response: &Value, mode: TravelMode, sort: SortBy, fallback: [LatLng; 2]) -> Vec<Route> {
    let mut routes: Vec<Route> = response
        .pointer("/route/transits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, transit)| {
            let mut segments = Vec::new();
            let mut subway_index = 0;
            for raw in transit
                .get("segments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let walking = raw.get("walking").unwrap_or(&Value::Null);
                let walking_points: Vec<LatLng> = walking
                    .get("steps")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .flat_map(|step| points(text(step, "polyline").as_deref()))
                    .collect();
                let instructions = walking
                    .get("steps")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|step| text(step, "instruction"))
                    .collect();
                if !walking_points.is_empty() || number(walking, "distance") > 0 {
                    segments.push(Segment {
                        kind: SegmentKind::Walk,
                        distance: number(walking, "distance"),
                        duration: minutes(number(walking, "duration")),
                        points: walking_points,
                        instruction: instructions,
                        line: None,
                    });
                }
                for line in raw
                    .pointer("/bus/buslines")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let is_subway = text(line, "type").is_some_and(|value| value.contains("地铁"));
                    let color = if is_subway {
                        let color = subway_color(subway_index).to_string();
                        subway_index += 1;
                        color
                    } else {
                        BUS_COLOR.to_string()
                    };
                    segments.push(Segment {
                        kind: SegmentKind::Transit,
                        distance: number(line, "distance"),
                        duration: minutes(number(line, "duration")),
                        points: points(text(line, "polyline").as_deref()),
                        instruction: Vec::new(),
                        line: Some(TransitLine {
                            vehicle: if is_subway {
                                Vehicle::Subway
                            } else {
                                Vehicle::Bus
                            },
                            title: text(line, "name").unwrap_or_default(),
                            station_count: number(line, "via_num"),
                            color,
                            get_on: stop(line.get("departure_stop")),
                            get_off: stop(line.get("arrival_stop")),
                            destination: None,
                            first_time: text(line, "start_time"),
                            last_time: text(line, "end_time"),
                        }),
                    });
                }
            }
            let all_points: Vec<LatLng> = segments
                .iter()
                .flat_map(|segment| segment.points.iter().copied())
                .collect();
            Route {
                id: format!("r{index}"),
                distance: segments.iter().map(|segment| segment.distance).sum(),
                duration: minutes(number(transit, "duration")),
                price: text(transit, "cost")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0.0),
                bounds: bounds(&all_points, fallback),
                segments,
            }
        })
        .collect();
    routes.retain(|route| match mode {
        TravelMode::Subway => route.segments.iter().any(|segment| {
            segment
                .line
                .as_ref()
                .is_some_and(|line| line.vehicle == Vehicle::Subway)
        }),
        TravelMode::Bus => route.segments.iter().any(|segment| {
            segment
                .line
                .as_ref()
                .is_some_and(|line| line.vehicle == Vehicle::Bus)
        }),
        TravelMode::Walking => false,
    });
    routes.sort_by_key(|route| {
        if sort == SortBy::Distance {
            route.distance
        } else {
            route.duration
        }
    });
    routes
}

impl RouteProvider for AmapProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Amap
    }

    fn suggest(&self, keyword: &str, page_size: usize) -> Result<Vec<Place>, AppError> {
        let url = format!(
            "{BASE}/v3/assistant/inputtips?keywords={}&datatype=poi&key={}",
            encode_component(keyword),
            self.key
        );
        let response: Value = get_json(&url)?;
        ensure_ok(&response, "高德输入提示")?;
        Ok(response
            .get("tips")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tip| {
                Some(Place {
                    title: text(tip, "name")?,
                    address: text(tip, "address").or_else(|| text(tip, "district")),
                    location: text(tip, "location").and_then(|value| point(&value))?,
                    adcode: text(tip, "adcode"),
                    provider: ProviderId::Amap,
                    id: text(tip, "id"),
                })
            })
            .take(page_size)
            .collect())
    }

    fn geocode(&self, address: &str) -> Result<Option<Place>, AppError> {
        let url = format!(
            "{BASE}/v3/geocode/geo?address={}&key={}",
            encode_component(address),
            self.key
        );
        let response: Value = get_json(&url)?;
        ensure_ok(&response, "高德地理编码")?;
        let Some(item) = response
            .get("geocodes")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
        else {
            return Ok(None);
        };
        Ok(Some(Place {
            title: address.to_string(),
            address: text(item, "formatted_address"),
            location: text(item, "location")
                .and_then(|value| point(&value))
                .ok_or_else(|| AppError::upstream("高德地理编码", "响应缺少坐标"))?,
            adcode: text(item, "adcode"),
            provider: ProviderId::Amap,
            id: None,
        }))
    }

    fn plan_route(&self, req: &RouteRequest) -> Result<RoutePlan, AppError> {
        let fallback = [req.from, req.to];
        let from = location(req.from);
        let to = location(req.to);
        let routes = if req.mode == TravelMode::Walking {
            let url = format!(
                "{BASE}/v3/direction/walking?origin={from}&destination={to}&key={}",
                self.key
            );
            let response: Value = get_json(&url)?;
            ensure_ok(&response, "高德步行路径规划")?;
            walking(&response, req.sort, fallback)
        } else {
            let city = city_code(&self.key, req.from)?;
            let strategy = if req.mode == TravelMode::Subway { 0 } else { 5 };
            let url = format!(
                "{BASE}/v3/direction/transit/integrated?origin={from}&destination={to}&city={city}&strategy={strategy}&extensions=all&key={}",
                self.key
            );
            let response: Value = get_json(&url)?;
            ensure_ok(&response, "高德公交路径规划")?;
            transit(&response, req.mode, req.sort, fallback)
        };
        if routes.is_empty() {
            return Err(AppError::no_route());
        }
        Ok(RoutePlan {
            origin: Endpoint {
                title: req.from_title.clone().unwrap_or_else(|| "起点".to_string()),
                location: req.from,
            },
            destination: Endpoint {
                title: req.to_title.clone().unwrap_or_else(|| "终点".to_string()),
                location: req.to,
            },
            mode: req.mode,
            routes,
            provider: ProviderId::Amap,
            crs: Crs::Gcj02,
            degraded: None,
        })
    }
}
