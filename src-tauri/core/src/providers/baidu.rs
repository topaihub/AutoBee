//! 百度地图 Web 服务 API adapter。
//!
//! 用**服务端 AK**。浏览器端那把调这里的接口会「请求通了但 `status` 非 0」,
//! 不是网络错误 —— 最容易被当成网络抖动而反复重试。
//!
//! # 照 `amap.rs` 改写时的三个坑
//!
//! 1. 请求参数坐标是 `纬度,经度`,与高德相反。抄错不报错,点落到另一个半球。
//! 2. `status` 是数字 `0` 表示成功(高德是字符串 `"1"`),详情在 `message`
//!    (高德在 `info`)。
//! 3. 坐标系是 BD-09,高德是 GCJ-02。填错的后果是整幅图偏移几百米而不报错。
//!
//! # 请求与响应的坐标顺序相反
//!
//! 请求 `origin` / `destination` 是 `纬度,经度`,响应折线 `path` 是 `经度,纬度`。
//! 所以 [`location`] 与 [`path`] 的顺序**故意是反的**。把其中一个「顺手统一」
//! 掉会得到一条经纬度对调的折线,而越界坐标被 [`LatLng`] 静默接受,
//! 地图上只是线跑到了南极附近。

use serde_json::Value;

use super::{RouteProvider, RouteRequest};
use crate::decode::{BUS_COLOR, subway_color};
use crate::error::AppError;
use crate::http::{encode_component, get_json};
use crate::models::{
    Crs, Endpoint, LatLng, Place, ProviderId, Route, RoutePlan, Segment, SegmentKind, SortBy, Stop,
    TransitLine, TravelMode, Vehicle,
};

const BASE: &str = "https://api.map.baidu.com";

/// `vehicle_info.type`：步行段。文档枚举 1 火车 2 飞机 3 公交 4 驾车 5 步行 6 大巴。
const VEHICLE_WALK: i64 = 5;
/// `vehicle_info.type`：市内公交类。**公交与地铁共用这个值**，区分在 `detail.type`。
const VEHICLE_TRANSIT: i64 = 3;
/// `vehicle_info.detail.type`：地铁 / 轻轨。0 是普通公交车。
const DETAIL_SUBWAY: i64 = 1;

#[derive(Debug, Clone)]
pub struct BaiduProvider {
    key: String,
}

impl BaiduProvider {
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn number(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn minutes(seconds: u32) -> u32 {
    seconds.div_ceil(60)
}

fn point(value: &Value) -> Option<LatLng> {
    Some(LatLng::new(
        value.get("lat")?.as_f64()?,
        value.get("lng")?.as_f64()?,
    ))
}

/// 解折线串。点间 `;`，点内 **`经度,纬度`**（与请求参数相反，见模块文档）。
fn path(text: Option<&str>) -> Vec<LatLng> {
    text.unwrap_or_default()
        .split(';')
        .filter_map(|pair| {
            let (lng, lat) = pair.split_once(',')?;
            Some(LatLng::new(
                lat.trim().parse().ok()?,
                lng.trim().parse().ok()?,
            ))
        })
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

/// 拼请求参数里的坐标：**`纬度,经度`**（与响应折线相反，见模块文档）。
fn location(point: LatLng) -> String {
    format!("{:.6},{:.6}", point.lat, point.lng)
}

/// 错误信息里带上**数字码**:`message` 的中文百度侧改过好几版,码才是能查的,
/// 码表见 `.claude/skills/baidu-map-webapi/references/constants.md`。详情要读
/// `message` 与 `msg` 两个字段:`geocoding/v3` 的「无相关结果」只有 `msg`,
/// 只读 `message` 会把唯一的线索换成一句「未知错误」。
fn ensure_ok(value: &Value, service: &str) -> Result<(), AppError> {
    let status = value.get("status").and_then(Value::as_i64);
    if status == Some(0) {
        return Ok(());
    }
    let detail = text(value, "message")
        .or_else(|| text(value, "msg"))
        .unwrap_or_else(|| "未知错误".to_string());
    tracing::warn!(service, ?status, detail = %detail, "百度接口返回失败");
    Err(AppError::upstream(
        service,
        &format!("{detail}(status={})", status.unwrap_or(-1)),
    ))
}

/// 百度把「无相关结果」塞进 `status=1`，而官方码表里 `1` 是「服务器内部错误」，
/// 真故障用同一个码 —— 只能靠 `msg` 文案分开。匹配文案脆弱，但坏的方向安全：
/// 百度改了这句中文就退回返回 Err，两处调用方都用 `.ok()` 兜住。
fn is_no_result(value: &Value) -> bool {
    value.get("status").and_then(Value::as_i64) == Some(1)
        && text(value, "msg").is_some_and(|msg| msg.contains("无相关结果"))
}

/// 步行方案**没有整条折线字段**，整条线必须把各 step 的 `path` 拼起来。
fn walking(response: &Value, sort: SortBy, fallback: [LatLng; 2]) -> Vec<Route> {
    let mut routes: Vec<Route> = response
        .pointer("/result/routes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, route)| {
            let steps = route
                .get("steps")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let all_points: Vec<LatLng> = steps
                .iter()
                .flat_map(|step| path(text(step, "path").as_deref()))
                .collect();
            // 字段名是 instructions（复数），不是高德的 instruction。
            let instructions = steps
                .iter()
                .filter_map(|step| text(step, "instructions"))
                .collect();
            let distance = number(route, "distance");
            let duration = minutes(number(route, "duration"));
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

/// 百度公交的 `steps` 是**二维**的：`steps[i]` 是数组，`steps[i][j]` 才是分段对象。
/// 官方返回参数表把内层命名为 `schemes`，响应示例里却是匿名数组，两种都接受。
fn sub_steps(step: &Value) -> Vec<&Value> {
    if let Some(items) = step.as_array() {
        return items.iter().collect();
    }
    if let Some(items) = step.get("schemes").and_then(Value::as_array) {
        return items.iter().collect();
    }
    // 再退一步：万一哪天百度把它拍平成对象，按「自己就是一个分段」处理。
    if step.get("vehicle_info").is_some() {
        return vec![step];
    }
    Vec::new()
}

/// `on_station` / `off_station` 只有站名**不带坐标**；坐标取 `start_info/start_location`
/// 与 `end_info/end_location`，缺失时回落到分段自己的 `start_location` / `end_location`。
fn stop(detail: &Value, info_key: &str, name_key: &str, fallback: Option<&Value>) -> Stop {
    let info = detail.get(info_key).unwrap_or(&Value::Null);
    let location_key = if info_key == "start_info" {
        "start_location"
    } else {
        "end_location"
    };
    Stop {
        title: text(detail, name_key)
            .or_else(|| {
                text(
                    info,
                    if info_key == "start_info" {
                        "start_name"
                    } else {
                        "end_name"
                    },
                )
            })
            .unwrap_or_default(),
        location: info
            .get(location_key)
            .and_then(point)
            .or_else(|| fallback.and_then(point))
            .unwrap_or(LatLng::new(0.0, 0.0)),
        exit: None,
    }
}

fn transit(response: &Value, mode: TravelMode, sort: SortBy, fallback: [LatLng; 2]) -> Vec<Route> {
    let mut routes: Vec<Route> = response
        .pointer("/result/routes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, route)| {
            let mut segments = Vec::new();
            let mut subway_index = 0;
            for step in route
                .get("steps")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(sub_steps)
            {
                let info = step.get("vehicle_info").unwrap_or(&Value::Null);
                let kind = info.get("type").and_then(Value::as_i64);
                let distance = number(step, "distance");
                let duration = minutes(number(step, "duration"));
                let points = path(text(step, "path").as_deref());
                if kind == Some(VEHICLE_WALK) {
                    segments.push(Segment {
                        kind: SegmentKind::Walk,
                        distance,
                        duration,
                        points,
                        instruction: text(step, "instructions").into_iter().collect(),
                        line: None,
                    });
                    continue;
                }
                // 火车 1 / 飞机 2 / 大巴 6 只在跨城出现，detail 是另一套字段，跳过。
                if kind != Some(VEHICLE_TRANSIT) {
                    continue;
                }
                let detail = info.get("detail").unwrap_or(&Value::Null);
                let is_subway = detail.get("type").and_then(Value::as_i64) == Some(DETAIL_SUBWAY);
                let color = if is_subway {
                    let color = subway_color(subway_index).to_string();
                    subway_index += 1;
                    color
                } else {
                    BUS_COLOR.to_string()
                };
                segments.push(Segment {
                    kind: SegmentKind::Transit,
                    distance,
                    duration,
                    points,
                    instruction: Vec::new(),
                    line: Some(TransitLine {
                        vehicle: if is_subway {
                            Vehicle::Subway
                        } else {
                            Vehicle::Bus
                        },
                        title: text(detail, "name").unwrap_or_default(),
                        station_count: number(detail, "stop_num"),
                        color,
                        get_on: stop(
                            detail,
                            "start_info",
                            "on_station",
                            step.get("start_location"),
                        ),
                        get_off: stop(detail, "end_info", "off_station", step.get("end_location")),
                        // 百度没有「开往方向」字段，方向只以自然语言嵌在 instructions 里。
                        destination: None,
                        first_time: text(detail, "first_time"),
                        last_time: text(detail, "last_time"),
                    }),
                });
            }
            let all_points: Vec<LatLng> = segments
                .iter()
                .flat_map(|segment| segment.points.iter().copied())
                .collect();
            Route {
                id: format!("r{index}"),
                distance: segments.iter().map(|segment| segment.distance).sum(),
                duration: minutes(number(route, "duration")),
                // price 单位是元：官方表格没标，从跨城示例的数量级反推（北京→上海 631）。
                price: route.get("price").and_then(Value::as_f64).unwrap_or(0.0),
                bounds: bounds(&all_points, fallback),
                segments,
            }
        })
        .collect();
    // 百度没有原生的地铁/公交过滤（registry 里 subway_filter: false），只能本地筛。
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

impl RouteProvider for BaiduProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Baidu
    }

    fn suggest(&self, keyword: &str, page_size: usize) -> Result<Vec<Place>, AppError> {
        // 用「行政区划区域检索」而不是 place/v2/suggestion：suggestion 固定 10 条、没有
        // 条数参数，接不住 page_size。`region=全国` 不在文档列举的值里，收紧的话表现是
        // `status=2` 而不是空结果。extensions_adcode=true 才召回编排层判境内外用的
        // adcode；scope=2 换来 detail_info。
        let url = format!(
            "{BASE}/place/v2/search?query={}&region={}&page_size={}&scope=2&extensions_adcode=true&output=json&ak={}",
            encode_component(keyword),
            encode_component("全国"),
            page_size.clamp(1, 20),
            encode_component(&self.key),
        );
        let response: Value = get_json(&url)?;
        ensure_ok(&response, "百度地点检索")?;
        Ok(response
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(Place {
                    title: text(item, "name")?,
                    address: text(item, "address").or_else(|| text(item, "area")),
                    location: point(item.get("location")?)?,
                    // 百度的 adcode 是数字（高德是字符串），text() 会转成 6 位数字文本。
                    adcode: text(item, "adcode"),
                    provider: ProviderId::Baidu,
                    id: text(item, "uid"),
                })
            })
            .take(page_size)
            .collect())
    }

    fn geocode(&self, address: &str) -> Result<Option<Place>, AppError> {
        // output=json 必须显式传：默认输出是 **xml**，不传只会得到「响应不是合法 JSON」。
        // extension_poi_infos=true 才返回 formatted_address 与 adcode，且在 /poi_infos 下。
        let url = format!(
            "{BASE}/geocoding/v3/?address={}&extension_poi_infos=true&output=json&ak={}",
            encode_component(address),
            encode_component(&self.key),
        );
        let response: Value = get_json(&url)?;
        // 「无相关结果」先接住，它是 status=1 而不是 0，否则会被 ensure_ok 抛成错误。
        if is_no_result(&response) {
            return Ok(None);
        }
        ensure_ok(&response, "百度地理编码")?;
        // 结构不完整（空 result、缺 location）也算搜不到，编排层才能换下一家。
        let Some(location) = response.pointer("/result/location").and_then(point) else {
            return Ok(None);
        };
        let poi = response.pointer("/poi_infos/0").unwrap_or(&Value::Null);
        Ok(Some(Place {
            title: address.to_string(),
            address: text(poi, "formatted_address"),
            location,
            adcode: text(poi, "adcode"),
            provider: ProviderId::Baidu,
            id: text(poi, "uid"),
        }))
    }

    fn plan_route(&self, req: &RouteRequest) -> Result<RoutePlan, AppError> {
        let fallback = [req.from, req.to];
        let from = location(req.from);
        let to = location(req.to);
        let routes = if req.mode == TravelMode::Walking {
            let url = format!(
                "{BASE}/direction/v2/walking?origin={from}&destination={to}&ak={}",
                encode_component(&self.key),
            );
            let response: Value = get_json(&url)?;
            ensure_ok(&response, "百度步行路径规划")?;
            walking(&response, req.sort, fallback)
        } else {
            // 百度公交不需要城市参数。tactics_incity=5 是「地铁优先」、1 是「少换乘」。
            let tactics = if req.mode == TravelMode::Subway { 5 } else { 1 };
            let url = format!(
                "{BASE}/direction/v2/transit?origin={from}&destination={to}&tactics_incity={tactics}&ak={}",
                encode_component(&self.key),
            );
            let response: Value = get_json(&url)?;
            ensure_ok(&response, "百度公交路径规划")?;
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
            provider: ProviderId::Baidu,
            crs: Crs::Bd09,
            degraded: None,
        })
    }
}

#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn walking_response() -> Value {
        json!({
            "status": 0,
            "message": "ok",
            "result": {
                "routes": [{
                    "distance": 1240,
                    "duration": 953,
                    "steps": [
                        {
                            "distance": 610,
                            "duration": 470,
                            "instructions": "沿上地十街向南步行610米",
                            "path": "116.339966,40.011156;116.340006,40.010546;116.340017,40.010166"
                        },
                        {
                            "distance": 630,
                            "duration": 483,
                            "instructions": "向东步行630米到达终点",
                            "path": "116.340067,40.009366;116.346067,40.009300"
                        }
                    ]
                }]
            }
        })
    }

    #[test]
    fn 步行折线按经度在前解析而不是纬度在前() {
        let routes = walking(
            &walking_response(),
            SortBy::Time,
            [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)],
        );
        let points = &routes[0].segments[0].points;
        // 串里第一个数字是经度；按 `lat,lng` 读会得到 lat=116，越界不报错只是线跑到极区。
        assert!((points[0].lat - 40.011_156).abs() < 1e-9);
        assert!((points[0].lng - 116.339_966).abs() < 1e-9);
        // 两个 step 的 path 要拼成一条线，而不是只取第一段。
        assert_eq!(points.len(), 5);
    }

    #[test]
    fn 步行秒进分且取整向上() {
        let routes = walking(
            &walking_response(),
            SortBy::Time,
            [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)],
        );
        assert_eq!(routes[0].distance, 1240);
        // 953 秒 = 15.88 分：取整向下会报「15 分钟」，时间类指标宁多勿少。
        assert_eq!(routes[0].duration, 16);
        assert_eq!(routes[0].segments[0].instruction.len(), 2);
    }

    #[test]
    fn 外接矩形覆盖全部折线点() {
        let routes = walking(
            &walking_response(),
            SortBy::Time,
            [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)],
        );
        let [min, max] = routes[0].bounds;
        assert!((min.lat - 40.009_300).abs() < 1e-9);
        assert!((max.lng - 116.346_067).abs() < 1e-9);
    }

    #[test]
    fn 没有折线时外接矩形回落到起终点() {
        let fallback = [LatLng::new(31.0, 118.0), LatLng::new(32.0, 119.0)];
        let response = json!({"status": 0, "result": {"routes": [{"distance": 1, "duration": 1}]}});
        let routes = walking(&response, SortBy::Time, fallback);
        // 回落成起终点，而不是留一个 [0,0] 的矩形把视野拉到几内亚湾。
        assert_eq!(routes[0].bounds, fallback);
    }

    fn transit_response() -> Value {
        json!({
            "status": 0,
            "message": "ok",
            "result": {
                "routes": [{
                    "distance": 8200,
                    "duration": 2400,
                    "price": 5.0,
                    "steps": [
                        [{
                            "distance": 700,
                            "duration": 596,
                            "instructions": "步行700米",
                            "path": "116.30822,40.056885;116.308096,40.057182",
                            "vehicle_info": {"type": 5, "detail": null}
                        }],
                        [{
                            "distance": 7000,
                            "duration": 1500,
                            "instructions": "西二旗站(A1北口)乘地铁13号线(西直门方向)经过6站到西直门站",
                            "path": "116.31258,40.059046;116.35000,40.050000",
                            "start_location": {"lng": 116.312_588, "lat": 40.059_046},
                            "end_location": {"lng": 116.350_00, "lat": 40.050_00},
                            "vehicle_info": {
                                "type": 3,
                                "detail": {
                                    "name": "地铁13号线",
                                    "type": 1,
                                    "stop_num": 6,
                                    "on_station": "西二旗站(A1北口)",
                                    "off_station": "西直门站",
                                    "first_time": "05:10",
                                    "last_time": "23:26",
                                    "start_info": {
                                        "start_name": "西二旗站(A1北口)",
                                        "start_location": {"lng": 116.312_588, "lat": 40.059_046}
                                    },
                                    "end_info": {
                                        "end_name": "西直门站",
                                        "end_location": {"lng": 116.350_00, "lat": 40.050_00}
                                    }
                                }
                            }
                        }]
                    ]
                }]
            }
        })
    }

    #[test]
    fn 公交嵌套数组能解出步行段与地铁段() {
        let routes = transit(
            &transit_response(),
            TravelMode::Subway,
            SortBy::Time,
            [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)],
        );
        // 按高德那样平铺解会一段都取不到，表现是「算不出路线」而不是解析报错。
        let segments = &routes[0].segments;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].kind, SegmentKind::Walk);
        assert_eq!(segments[1].kind, SegmentKind::Transit);
    }

    #[test]
    fn detail_type_为_1_判成地铁而不是公交() {
        let routes = transit(
            &transit_response(),
            TravelMode::Subway,
            SortBy::Time,
            [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)],
        );
        let line = routes[0].segments[1].line.as_ref().unwrap();
        // 只看共用的 vehicle_info.type=3 会把每条地铁标成公交，配色与徽标全错。
        assert_eq!(line.vehicle, Vehicle::Subway);
        assert_eq!(line.title, "地铁13号线");
        assert_eq!(line.station_count, 6);
        assert_eq!(line.get_on.title, "西二旗站(A1北口)");
        assert_eq!(line.get_off.title, "西直门站");
        assert!((line.get_on.location.lat - 40.059_046).abs() < 1e-9);
        assert_eq!(line.first_time.as_deref(), Some("05:10"));
    }

    #[test]
    fn 只坐公交时地铁方案被筛掉() {
        // 过滤只存在于本地；漏了它，选「公交」会拿到一串地铁方案。
        let routes = transit(
            &transit_response(),
            TravelMode::Bus,
            SortBy::Time,
            [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)],
        );
        assert!(routes.is_empty());
    }

    #[test]
    fn 文档所述的_schemes_形状也能解出分段() {
        // 官方参数表叫 schemes、响应示例里是匿名数组；赌错一边会静默返回空方案。
        let mut response = transit_response();
        let steps = response.pointer_mut("/result/routes/0/steps").unwrap();
        let wrapped: Vec<Value> = steps
            .as_array()
            .unwrap()
            .iter()
            .map(|step| json!({"schemes": step}))
            .collect();
        *steps = Value::Array(wrapped);
        let routes = transit(
            &response,
            TravelMode::Subway,
            SortBy::Time,
            [LatLng::new(0.0, 0.0), LatLng::new(0.0, 0.0)],
        );
        assert_eq!(routes[0].segments.len(), 2);
    }

    #[test]
    fn 非零状态码的错误里带上数字码() {
        let response = json!({"status": 240, "message": "APP 服务被禁用"});
        let error = ensure_ok(&response, "百度公交路径规划").unwrap_err();
        // message 的中文百度侧改过好几版，只贴中文等于把线索丢了。
        assert!(format!("{error}").contains("240"));
    }

    #[test]
    fn 请求参数的坐标是纬度在前() {
        // 与响应折线相反，是百度自己的不一致，不是笔误。
        assert_eq!(
            location(LatLng::new(40.056_878, 116.308_15)),
            "40.056878,116.308150"
        );
    }

    #[test]
    fn 地理编码的无相关结果不算错误() {
        // 百度把「没搜到」塞进 status=1；当错误抛的话 geocode 永远不返回 Ok(None)。
        let response = json!({
            "status": 1,
            "msg": "Internal Service Error:无相关结果",
            "results": []
        });
        assert!(is_no_result(&response));
    }

    #[test]
    fn 真的服务器错误仍然是错误() {
        // 同样 status=1 但没有那句「无相关结果」，不能一起咽掉。
        let response = json!({"status": 1, "msg": "Internal Service Error"});
        assert!(!is_no_result(&response));
        assert!(ensure_ok(&response, "百度地理编码").is_err());
    }

    #[test]
    fn 错误详情在_msg_字段时也能读到() {
        // geocoding/v3 的 status=1 只有 msg 没有 message，只读 message 会丢线索。
        let response = json!({"status": 1, "msg": "Internal Service Error"});
        let error = ensure_ok(&response, "百度地理编码").unwrap_err();
        let text = format!("{error}");
        assert!(text.contains("Internal Service Error"), "实际:{text}");
        assert!(text.contains('1'), "实际:{text}");
    }
}
