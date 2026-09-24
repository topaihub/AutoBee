//! 腾讯位置服务 adapter。
//!
//! # 已实测的行为（都是踩过的坑，务必保留）
//!
//! - **地址解析对 POI 式输入直接报 348**。「光谷大道地铁站」「铢家村-E1口」
//!   这类输入 `geocoder` 一律 `status:348 参数错误`，而 `suggestion` 不但能查到，
//!   还自带纠错（铢家村 → 钟家村）。所以编排层必须先联想、后解析。
//! - **`transit` 返回的是 `result.routes[]`**，不是文档里出现过的 `transits`。
//! - **TRANSIT 步骤自身没有 polyline**，坐标在线路 `lines[].polyline` 上。
//!   按步骤取会整段跳过，现象是路线缺了乘车那几段。
//! - **polyline 首对是绝对度数，其后是 1e-6 度增量**，见 [`crate::decode`]。
//! - **`price` 的单位是「分」**（500 = 5 元），归一化结构约定为「元」。

use serde::Deserialize;
use serde_json::Value;

use super::{RouteProvider, RouteRequest};
use crate::decode::{BUS_COLOR, decode_polyline, subway_color};
use crate::error::AppError;
use crate::http::{encode_component, get_json};
use crate::models::{
    Crs, Endpoint, LatLng, Place, ProviderId, Route, RoutePlan, Segment, SegmentKind, Stop,
    TransitLine, TravelMode, Vehicle,
};

/// 接口根地址。
const BASE: &str = "https://apis.map.qq.com";

/// 腾讯的响应信封。
///
/// `result` 在不同接口里有时是对象、有时是数组，所以统一收成 `Value`
/// 在具体函数里按接口取 —— 为六种形状各写一个 struct 不划算，
/// 而写错一个字段类型会让整个响应解析失败（serde 默认不宽容）。
#[derive(Debug, Deserialize)]
struct Envelope {
    /// 0 表示成功。
    #[serde(default)]
    status: i32,
    #[serde(default)]
    message: String,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    data: Option<Vec<Value>>,
}

/// 腾讯位置服务。
#[derive(Debug, Clone)]
pub struct TencentProvider {
    key: String,
}

impl TencentProvider {
    /// 用一个 Key 建 adapter。
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

/* ---------------- 取字段的小工具 ---------------- */

/// 取字符串字段。数字也收（adcode 有时是数字、有时是字符串）。
fn str_of(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn f64_of(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn u32_of(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .map_or(0, |v| v.max(0.0) as u32)
}

fn latlng_of(value: &Value) -> Option<LatLng> {
    Some(LatLng::new(f64_of(value, "lat")?, f64_of(value, "lng")?))
}

fn polyline_of(value: &Value) -> Vec<f64> {
    value
        .get("polyline")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

/// 该方案用到的交通工具集合。
fn vehicles_of(route: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(steps) = route.get("steps").and_then(Value::as_array) else {
        return out;
    };
    for step in steps {
        let Some(lines) = step.get("lines").and_then(Value::as_array) else {
            continue;
        };
        for line in lines {
            if let Some(v) = str_of(line, "vehicle")
                && !out.contains(&v)
            {
                out.push(v);
            }
        }
    }
    out
}

/// 折线的外接矩形。没有点时退回起终点 —— 那比 `[0,0]` 好，
/// 前端据此缩放至少能落在正确的城市。
fn bounds_of(points: &[LatLng], fallback: [LatLng; 2]) -> [LatLng; 2] {
    if points.is_empty() {
        return fallback;
    }
    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lng = f64::MAX;
    let mut max_lng = f64::MIN;
    for p in points {
        min_lat = min_lat.min(p.lat);
        max_lat = max_lat.max(p.lat);
        min_lng = min_lng.min(p.lng);
        max_lng = max_lng.max(p.lng);
    }
    [LatLng::new(min_lat, min_lng), LatLng::new(max_lat, max_lng)]
}

fn stop_of(value: Option<&Value>) -> Stop {
    let Some(v) = value else {
        return Stop {
            title: String::new(),
            location: LatLng::new(0.0, 0.0),
            exit: None,
        };
    };
    let exit = v
        .get("exit")
        .map(|e| match e {
            Value::String(s) => s.clone(),
            other => str_of(other, "title").unwrap_or_default(),
        })
        .filter(|s| !s.is_empty());
    Stop {
        title: str_of(v, "title").unwrap_or_default(),
        location: latlng_of(v).unwrap_or(LatLng::new(0.0, 0.0)),
        exit,
    }
}

/* ---------------- 归一化 ---------------- */

/// 公交/地铁方案。
fn normalize_transit(raw: &[Value], mode: TravelMode, sort: crate::models::SortBy) -> Vec<Route> {
    // 方案过滤：地铁模式要「有地铁且没公交」的；一条纯地铁都没有时
    // 放宽到「含地铁」—— 宁可给出带一段接驳公交的方案，也不要空白。
    let pool: Vec<&Value> = if mode == TravelMode::Subway {
        let pure: Vec<&Value> = raw
            .iter()
            .filter(|r| {
                let v = vehicles_of(r);
                v.iter().any(|x| x == "SUBWAY") && !v.iter().any(|x| x == "BUS")
            })
            .collect();
        if pure.is_empty() {
            raw.iter()
                .filter(|r| vehicles_of(r).iter().any(|x| x == "SUBWAY"))
                .collect()
        } else {
            pure
        }
    } else {
        let with_bus: Vec<&Value> = raw
            .iter()
            .filter(|r| vehicles_of(r).iter().any(|x| x == "BUS"))
            .collect();
        if with_bus.is_empty() {
            raw.iter().collect()
        } else {
            with_bus
        }
    };

    let mut sorted: Vec<&Value> = pool;
    sorted.sort_by_key(|r| {
        if sort == crate::models::SortBy::Distance {
            u32_of(r, "distance")
        } else {
            u32_of(r, "duration")
        }
    });

    sorted
        .iter()
        .enumerate()
        .map(|(index, r)| {
            let mut segments: Vec<Segment> = Vec::new();
            let mut line_index = 0usize;

            if let Some(steps) = r.get("steps").and_then(Value::as_array) {
                for step in steps {
                    if str_of(step, "mode").as_deref() == Some("WALKING") {
                        segments.push(Segment {
                            kind: SegmentKind::Walk,
                            distance: u32_of(step, "distance"),
                            duration: u32_of(step, "duration"),
                            points: decode_polyline(&polyline_of(step)),
                            instruction: step
                                .get("steps")
                                .and_then(Value::as_array)
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|s| str_of(s, "instruction"))
                                        .filter(|s| !s.is_empty())
                                        .collect()
                                })
                                .unwrap_or_default(),
                            line: None,
                        });
                        continue;
                    }

                    let Some(lines) = step.get("lines").and_then(Value::as_array) else {
                        continue;
                    };
                    for line in lines {
                        let is_subway = str_of(line, "vehicle").as_deref() == Some("SUBWAY");
                        let color = if is_subway {
                            subway_color(line_index).to_string()
                        } else {
                            BUS_COLOR.to_string()
                        };
                        if is_subway {
                            line_index += 1;
                        }
                        segments.push(Segment {
                            kind: SegmentKind::Transit,
                            distance: u32_of(line, "distance").max(u32_of(step, "distance")),
                            duration: u32_of(line, "duration").max(u32_of(step, "duration")),
                            points: decode_polyline(&polyline_of(line)),
                            instruction: Vec::new(),
                            line: Some(TransitLine {
                                vehicle: if is_subway {
                                    Vehicle::Subway
                                } else {
                                    Vehicle::Bus
                                },
                                title: str_of(line, "title").unwrap_or_default(),
                                station_count: u32_of(line, "station_count"),
                                color,
                                get_on: stop_of(line.get("geton")),
                                get_off: stop_of(line.get("getoff")),
                                destination: line
                                    .get("destination")
                                    .and_then(|d| str_of(d, "title")),
                                first_time: str_of(line, "start_time"),
                                last_time: str_of(line, "end_time"),
                            }),
                        });
                    }
                }
            }

            let all_points: Vec<LatLng> = segments
                .iter()
                .flat_map(|s| s.points.iter().copied())
                .collect();
            let fallback = [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)];

            Route {
                id: format!("r{index}"),
                distance: u32_of(r, "distance"),
                duration: u32_of(r, "duration"),
                // 腾讯给的是「分」，归一化约定是「元」。
                price: f64_of(r, "price").unwrap_or(0.0) / 100.0,
                bounds: bounds_of(&all_points, fallback),
                segments,
            }
        })
        .collect()
}

/// 步行方案。
fn normalize_walking(raw: &[Value], sort: crate::models::SortBy) -> Vec<Route> {
    let mut sorted: Vec<&Value> = raw.iter().collect();
    sorted.sort_by_key(|r| {
        if sort == crate::models::SortBy::Distance {
            u32_of(r, "distance")
        } else {
            u32_of(r, "duration")
        }
    });

    sorted
        .iter()
        .enumerate()
        .map(|(index, r)| {
            let points = decode_polyline(&polyline_of(r));
            let bounds = bounds_of(&points, [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)]);
            Route {
                id: format!("r{index}"),
                distance: u32_of(r, "distance"),
                duration: u32_of(r, "duration"),
                price: 0.0,
                bounds,
                segments: vec![Segment {
                    kind: SegmentKind::Walk,
                    distance: u32_of(r, "distance"),
                    duration: u32_of(r, "duration"),
                    points,
                    instruction: r
                        .get("steps")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|s| str_of(s, "instruction"))
                                .filter(|s| !s.is_empty())
                                .collect()
                        })
                        .unwrap_or_default(),
                    line: None,
                }],
            }
        })
        .collect()
}

impl RouteProvider for TencentProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Tencent
    }

    fn suggest(&self, keyword: &str, page_size: usize) -> Result<Vec<Place>, AppError> {
        let url = format!(
            "{BASE}/ws/place/v1/suggestion?keyword={}&page_size={page_size}&key={}",
            encode_component(keyword),
            self.key
        );
        let envelope: Envelope = get_json(&url)?;
        if envelope.status != 0 {
            return Err(AppError::upstream("腾讯位置服务", &envelope.message));
        }
        let items = envelope.data.unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|item| {
                Some(Place {
                    title: str_of(item, "title")?,
                    address: str_of(item, "address"),
                    location: latlng_of(item.get("location")?)?,
                    adcode: str_of(item, "adcode"),
                    provider: ProviderId::Tencent,
                    id: str_of(item, "id"),
                })
            })
            .collect())
    }

    fn geocode(&self, address: &str) -> Result<Option<Place>, AppError> {
        let url = format!(
            "{BASE}/ws/geocoder/v1?address={}&key={}",
            encode_component(address),
            self.key
        );
        let envelope: Envelope = get_json(&url)?;
        if envelope.status != 0 {
            // 348 是常态：POI 式输入（地铁站名、出入口）一律这个码。
            // 不记 error 级别 —— 那会把正常降级路径刷成一片红。
            tracing::debug!(status = envelope.status, "腾讯地址解析没命中");
            return Ok(None);
        }
        let Some(result) = envelope.result else {
            return Ok(None);
        };
        let location = result
            .get("location")
            .and_then(latlng_of)
            .ok_or_else(|| AppError::upstream("腾讯位置服务", "响应里没有坐标"))?;
        Ok(Some(Place {
            title: str_of(&result, "title").unwrap_or_else(|| address.to_string()),
            address: str_of(&result, "address"),
            location,
            adcode: result.get("ad_info").and_then(|a| str_of(a, "adcode")),
            provider: ProviderId::Tencent,
            id: str_of(&result, "id"),
        }))
    }

    fn plan_route(&self, req: &RouteRequest) -> Result<RoutePlan, AppError> {
        let from = format!("{},{}", req.from.lat, req.from.lng);
        let to = format!("{},{}", req.to.lat, req.to.lng);
        let origin = Endpoint {
            title: req.from_title.clone().unwrap_or_else(|| "起点".to_string()),
            location: req.from,
        };
        let destination = Endpoint {
            title: req.to_title.clone().unwrap_or_else(|| "终点".to_string()),
            location: req.to,
        };

        let routes = if req.mode == TravelMode::Walking {
            let url = format!(
                "{BASE}/ws/direction/v1/walking?from={from}&to={to}&key={}",
                self.key
            );
            let envelope: Envelope = get_json(&url)?;
            if envelope.status != 0 {
                return Err(AppError::upstream("腾讯步行路线规划", &envelope.message));
            }
            let raw = envelope
                .result
                .as_ref()
                .and_then(|r| r.get("routes"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            normalize_walking(&raw, req.sort)
        } else {
            let url = format!(
                "{BASE}/ws/direction/v1/transit?from={from}&to={to}&policy=LEAST_TIME&key={}",
                self.key
            );
            let envelope: Envelope = get_json(&url)?;
            if envelope.status != 0 {
                return Err(AppError::upstream("腾讯公交路线规划", &envelope.message));
            }
            let raw = envelope
                .result
                .as_ref()
                .and_then(|r| r.get("routes"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            normalize_transit(&raw, req.mode, req.sort)
        };

        if routes.is_empty() {
            return Err(AppError::no_route());
        }

        Ok(RoutePlan {
            origin,
            destination,
            mode: req.mode,
            routes,
            provider: ProviderId::Tencent,
            crs: Crs::Gcj02,
            degraded: None,
        })
    }
}
