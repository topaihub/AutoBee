//! 三个地图工具的 JSON Schema 声明与执行分派，供 Aether 的 `KitHost` 调用。
//!
//! schema 的 `enum` 值、错误文案里的可选值提示、`parse` 实现共用
//! `TravelMode::ALL` / `SortBy::ALL` / `LifeCirclePackage::ALL` ——
//! 三处同源才不会出现「schema 说能填、实现不认」的偏差。

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use route_core::config::Config;
use route_core::error::AppError;
use route_core::geo::convert;
use route_core::life_circle::life_circle as core_life_circle;
use route_core::models::{
    Crs, LatLng, LifeCircle, LifeCirclePackage, Place, ProviderId, Route, RoutePlan, Segment,
    SortBy, Stop, TransitLine, TravelMode,
};
use route_core::planner::{RouteQuery, plan_route as core_plan_route, suggest_places};

/// 工具名。schema 与 [`dispatch`] 都从这里取，改名只改一处。
const PLAN_ROUTE: &str = "plan_route";
const SEARCH_PLACE: &str = "search_place";
const LIFE_CIRCLE: &str = "life_circle";
const TOOL_NAMES: [&str; 3] = [PLAN_ROUTE, SEARCH_PLACE, LIFE_CIRCLE];

/// 单个工具返回值的字符上限。折线裁掉之后正常结果远低于此，这道闸只防异常长的上游数据
/// 把对话上下文挤爆。
const MAX_OUTPUT_CHARS: usize = 8000;

/// 截断提示。不加提示的话模型会把残缺 JSON 当完整结果解读。
const TRUNCATED_NOTE: &str = "（内容过长已截断）";

/// 报错时回显的上游原文长度。太短模型看不出自己错在哪，太长会淹没错误本身。
const ARGS_PREVIEW_CHARS: usize = 160;

/// 回退到换行的最大搜索距离（字节）。紧凑 JSON 根本没有换行，限距是防止一个很早的
/// 换行把整段内容削光。
const NEWLINE_BACKOFF_BYTES: usize = 512;

/// `search_place` 空结果的回话。回空数组模型只能自己猜原因，一句话能让它直接改关键词重试。
const EMPTY_SEARCH_HINT: &str = "没有找到匹配的地点，换个关键词再试";

/// 三个工具的声明，直接塞进请求体的 `tools` 字段。
#[must_use]
pub fn tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "name": PLAN_ROUTE,
            "description": "规划两点之间的公共交通或步行路线。起点与终点既可以写地名（如「武汉光谷广场」），\
        也可以写「纬度,经度」——顺序是纬度在前、经度在后（如「30.5,114.4」）。返回每条候选方案的总距离、\
        总耗时、票价与分段乘车信息；折线坐标不返回，每段只给点数（pointCount）与首末点。",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": {
                        "type": "string",
                        "description": "起点：地名，或「纬度,经度」（纬度在前）"
                    },
                    "to": {
                        "type": "string",
                        "description": "终点：地名，或「纬度,经度」（纬度在前）"
                    },
                    "mode": {
                        "type": "string",
                        "enum": TravelMode::ALL,
                        "description": "出行方式：subway 只坐地铁（可含步行换乘，不含公交），bus 公交优先（含公交+地铁），walking 全程步行"
                    },
                    "sort": {
                        "type": "string",
                        "enum": SortBy::ALL,
                        "description": "候选方案排序依据：time 按耗时最短，distance 按距离最短"
                    }
                },
                "required": ["from", "to", "mode", "sort"]
            }
        }),
        json!({
            "name": SEARCH_PLACE,
            "description": "按关键词搜索地点，拿到标准名称与坐标（纬度 lat、经度 lng）。\
        地名可能重名或写错，规划路线前先用它确认；搜不到时返回一句提示而不是报错，结果可能为空。",
            "parameters": {
                "type": "object",
                "properties": {
                    "keyword": {
                        "type": "string",
                        "description": "搜索关键词，如「光谷广场」「武汉大学」"
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 20,
                        "description": "返回条数上限，默认 8"
                    }
                },
                "required": ["keyword"]
            }
        }),
        json!({
            "name": LIFE_CIRCLE,
            "description": "查询一个坐标点周围真实步行 15 分钟内可达的生活服务点位（医院、药店、公园等），\
        按步行时间升序返回。坐标用纬度 lat 与经度 lng 两个数字分别给出。",
            "parameters": {
                "type": "object",
                "properties": {
                    "lat": {"type": "number", "description": "起点纬度"},
                    "lng": {"type": "number", "description": "起点经度"},
                    "package": {
                        "type": "string",
                        "enum": LifeCirclePackage::ALL,
                        "description": "服务包：care 医院/药店/卫生间，family 公园/便利店/地铁站，emergency 医院/药店/警务服务"
                    },
                    "crs": {
                        "type": "string",
                        "enum": Crs::ALL,
                        "description": "起点所用坐标系。省略即按系统输出坐标系解释，plan_route 与 search_place 返回的坐标可以直接填"
                    }
                },
                "required": ["lat", "lng", "package"]
            }
        }),
    ]
}

/// 一次工具调用的两份产物。
///
/// 存在两份是 6.4.2 的要求,不是冗余:发回模型的 JSON 不含折线点(省上下文),
/// 而地图要折线才画得出线。工具只执行一次,裁剪版回模型,完整版作为 Aether
/// `tool_result` 事件的 `payload` 回前端。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    /// 喂回模型的字符串,已按 [`MAX_OUTPUT_CHARS`] 裁剪。
    pub summary: String,
    /// 给地图的原始结构。`search_place` 没有 —— 它是中间步骤,
    /// 画一堆候选点只会干扰。
    pub payload: Option<ToolPayload>,
}

/// 可回灌进 `planner` 的结构化结果。序列化成 `{kind:"plan",plan}` /
/// `{kind:"circle",circle}`，前端按 `kind` 分派。
///
/// 用 enum 而不是两个 `Option` 字段:一次工具调用不可能同时产出路线和生活圈,
/// 两个 `Option` 会让「都是 Some」这种不可能状态可被表达。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ToolPayload {
    /// 写 `planner.plan` 与 `planner.selectedRouteId`。
    Plan { plan: Box<RoutePlan> },
    /// 写 `planner.circleResults`。
    Circle { circle: Box<LifeCircle> },
}

impl ToolOutcome {
    /// 只有文字、没有可回灌结构的结果。
    fn text(summary: String) -> Self {
        Self {
            summary,
            payload: None,
        }
    }
}

/// 执行一次工具调用。
///
/// # Errors
///
/// 工具名未知、`arguments` 不是合法 JSON、枚举参数不认识、缺少所需 Key,
/// 以及底层地图服务失败时返回错误。
pub fn dispatch(config: &Config, name: &str, arguments: &str) -> Result<ToolOutcome, AppError> {
    match name {
        PLAN_ROUTE => run_plan_route(config, arguments),
        SEARCH_PLACE => run_search_place(config, arguments),
        LIFE_CIRCLE => run_life_circle(config, arguments),
        _ => Err(AppError::new(
            "UNKNOWN_TOOL",
            format!("没有名为「{name}」的工具,可用:{}", TOOL_NAMES.join(" / ")),
            400,
        )),
    }
}

#[derive(Deserialize)]
struct PlanRouteArgs {
    from: String,
    to: String,
    mode: String,
    sort: String,
}

#[derive(Deserialize)]
struct SearchPlaceArgs {
    keyword: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct LifeCircleArgs {
    lat: f64,
    lng: f64,
    package: String,
    crs: Option<String>,
}

fn run_plan_route(config: &Config, arguments: &str) -> Result<ToolOutcome, AppError> {
    let args: PlanRouteArgs = parse_args(PLAN_ROUTE, arguments)?;
    let query = RouteQuery {
        from: args.from,
        to: args.to,
        mode: parse_mode(&args.mode)?,
        sort: parse_sort(&args.sort)?,
    };
    let plan = core_plan_route(config, &query)?;
    Ok(ToolOutcome {
        summary: encode(&trim_plan(&plan))?,
        payload: Some(ToolPayload::Plan {
            plan: Box::new(plan),
        }),
    })
}

fn run_search_place(config: &Config, arguments: &str) -> Result<ToolOutcome, AppError> {
    let args: SearchPlaceArgs = parse_args(SEARCH_PLACE, arguments)?;
    // 缺省 8 条与界面联想一致；上限 20 是防模型要一屏用不完的数据。
    let limit = args.limit.unwrap_or(8).clamp(1, 20);
    let hits = suggest_places(config, &args.keyword, limit);
    if hits.is_empty() {
        return Ok(ToolOutcome::text(EMPTY_SEARCH_HINT.to_string()));
    }
    // 不带 payload:候选点是中间步骤,画上地图只会干扰(6.4.2)。
    Ok(ToolOutcome::text(encode(&Value::Array(
        hits.iter().map(trim_place).collect(),
    ))?))
}

fn run_life_circle(config: &Config, arguments: &str) -> Result<ToolOutcome, AppError> {
    let args: LifeCircleArgs = parse_args(LIFE_CIRCLE, arguments)?;
    let package = parse_package(&args.package)?;
    // 省略 crs 时按系统输出坐标系解释:plan_route / search_place 给模型的坐标就是这个系,
    // 认错系统会让点位整体偏移几百米,而结果看上去仍然「正常」。
    let crs = match args.crs.as_deref() {
        Some(text) => parse_crs(text)?,
        None => config.settings.out_crs,
    };
    let key = config.key(ProviderId::Baidu).ok_or_else(|| {
        AppError::new(
            "BAIDU_KEY_REQUIRED",
            "请到设置页填写百度地图 Web 服务 AK",
            400,
        )
    })?;
    let mut circle = core_life_circle(
        key,
        convert(LatLng::new(args.lat, args.lng), crs, Crs::Bd09),
        package,
    )?;
    // 百度只吃 BD-09，回给模型要换回它给的那个系，否则它拿到的点和它自己的坐标对不上。
    for poi in &mut circle.pois {
        poi.location = convert(poi.location, Crs::Bd09, crs);
    }
    circle.crs = crs;
    Ok(ToolOutcome {
        summary: encode(&trim_circle(&circle))?,
        payload: Some(ToolPayload::Circle {
            circle: Box::new(circle),
        }),
    })
}

fn parse_args<T: DeserializeOwned>(tool: &str, arguments: &str) -> Result<T, AppError> {
    serde_json::from_str(arguments).map_err(|error| {
        AppError::new(
            "BAD_TOOL_ARGS",
            // 带上原文:模型只看到「参数不合法」时不知道自己写坏了哪一段,重试还是错。
            format!(
                "工具「{tool}」的参数不是合法 JSON:{error};收到的原文是:{}",
                preview(arguments)
            ),
            400,
        )
    })
}

/// 与界面命令同一套 `parse`：认不出就报错并列出合法值，绝不兜底成默认值 ——
/// 模型分辨不出自己拿到的是默认参数的结果，也就无法重试。
fn parse_mode(text: &str) -> Result<TravelMode, AppError> {
    TravelMode::parse(text).ok_or_else(|| {
        AppError::new(
            "BAD_MODE",
            format!(
                "不认识的出行方式「{text}」,可选:{}",
                TravelMode::ALL.join(" / ")
            ),
            400,
        )
    })
}

fn parse_sort(text: &str) -> Result<SortBy, AppError> {
    SortBy::parse(text).ok_or_else(|| {
        AppError::new(
            "BAD_SORT",
            format!(
                "不认识的排序依据「{text}」,可选:{}",
                SortBy::ALL.join(" / ")
            ),
            400,
        )
    })
}

fn parse_package(text: &str) -> Result<LifeCirclePackage, AppError> {
    LifeCirclePackage::parse(text).ok_or_else(|| {
        AppError::new(
            "BAD_PACKAGE",
            format!(
                "不认识的生活圈服务包「{text}」,可选:{}",
                LifeCirclePackage::ALL.join(" / ")
            ),
            400,
        )
    })
}

fn parse_crs(text: &str) -> Result<Crs, AppError> {
    Crs::parse(text).ok_or_else(|| {
        AppError::new(
            "BAD_CRS",
            format!("不认识的起点坐标系「{text}」,可选:{}", Crs::ALL.join(" / ")),
            400,
        )
    })
}

/// 紧凑 JSON：缩进在上下文里纯属浪费 token。
fn encode(value: &Value) -> Result<String, AppError> {
    let text = serde_json::to_string(value).map_err(|error| {
        AppError::new(
            "TOOL_ENCODE_FAILED",
            format!("工具结果序列化失败:{error}"),
            500,
        )
    })?;
    Ok(clip(text))
}

/// 按字符数硬切，切点先对齐字符边界再就近回退到换行。
///
/// 半个汉字切不出来（`str` 索引非边界会 panic），半行 JSON 会让模型把残句当数据读，
/// 所以两道对齐都要做。截断提示计入上限，保证返回长度不超 [`MAX_OUTPUT_CHARS`]。
fn clip(text: String) -> String {
    if text.chars().count() <= MAX_OUTPUT_CHARS {
        return text;
    }
    let keep = MAX_OUTPUT_CHARS - TRUNCATED_NOTE.chars().count();
    // char_indices 返回的就是字符边界，不会切出半个汉字。
    let mut end = text
        .char_indices()
        .nth(keep)
        .map_or_else(|| text.len(), |(index, _)| index);
    if let Some(newline) = text[..end].rfind('\n')
        && end - newline <= NEWLINE_BACKOFF_BYTES
    {
        end = newline;
    }
    let mut out = String::with_capacity(end + TRUNCATED_NOTE.len());
    out.push_str(&text[..end]);
    out.push_str(TRUNCATED_NOTE);
    out
}

fn preview(text: &str) -> String {
    match text.char_indices().nth(ARGS_PREVIEW_CHARS) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_string(),
    }
}

fn trim_plan(plan: &RoutePlan) -> Value {
    json!({
        "origin": trim_endpoint(&plan.origin.title, plan.origin.location),
        "destination": trim_endpoint(&plan.destination.title, plan.destination.location),
        "mode": plan.mode,
        "provider": plan.provider.as_str(),
        "crs": plan.crs,
        "routes": plan.routes.iter().map(trim_route).collect::<Vec<_>>(),
        "degraded": plan.degraded.as_ref().map(|degraded| json!({
            "tried": degraded.tried.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            "reason": degraded.reason,
        })),
    })
}

fn trim_endpoint(title: &str, location: LatLng) -> Value {
    json!({ "title": title, "lat": location.lat, "lng": location.lng })
}

fn trim_route(route: &Route) -> Value {
    json!({
        "id": route.id,
        "distanceM": route.distance,
        "durationMin": route.duration,
        "priceYuan": route.price,
        "segments": route.segments.iter().map(trim_segment).collect::<Vec<_>>(),
    })
}

fn trim_segment(segment: &Segment) -> Value {
    json!({
        "kind": segment.kind,
        "distanceM": segment.distance,
        "durationMin": segment.duration,
        // 折线一段就有几十 KB，模型既不画图也不算几何，只给点数与首末点足够它讲清走向。
        "pointCount": segment.points.len(),
        "firstPoint": segment.points.first().map(|point| trim_point(*point)),
        "lastPoint": segment.points.last().map(|point| trim_point(*point)),
        "instruction": segment.instruction,
        "line": segment.line.as_ref().map(trim_line),
    })
}

fn trim_point(point: LatLng) -> Value {
    json!({ "lat": point.lat, "lng": point.lng })
}

fn trim_line(line: &TransitLine) -> Value {
    json!({
        "vehicle": line.vehicle,
        "title": line.title,
        "stationCount": line.station_count,
        "getOn": trim_stop(&line.get_on),
        "getOff": trim_stop(&line.get_off),
        "direction": line.destination,
        "firstTime": line.first_time,
        "lastTime": line.last_time,
    })
}

fn trim_stop(stop: &Stop) -> Value {
    json!({
        "title": stop.title,
        "lat": stop.location.lat,
        "lng": stop.location.lng,
        "exit": stop.exit,
    })
}

fn trim_place(place: &Place) -> Value {
    json!({
        "title": place.title,
        "address": place.address,
        "lat": place.location.lat,
        "lng": place.location.lng,
        "provider": place.provider.as_str(),
    })
}

fn trim_circle(circle: &LifeCircle) -> Value {
    json!({
        "package": circle.package,
        "crs": circle.crs,
        "limitMinutes": circle.limit_minutes,
        "pois": circle.pois.iter().map(|poi| json!({
            "category": poi.category,
            "name": poi.name,
            "address": poi.address,
            "lat": poi.location.lat,
            "lng": poi.location.lng,
            "walkMinutes": poi.walk_duration_sec / 60,
            "walkDistanceM": poi.walk_distance,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    use route_core::models::{Endpoint, SegmentKind, Vehicle};

    fn schema(name: &str) -> Value {
        tool_schemas()
            .into_iter()
            .find(|tool| tool["name"] == name)
            .unwrap()
    }

    fn enum_of(tool: &str, field: &str) -> Vec<String> {
        schema(tool)["parameters"]["properties"][field]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect()
    }

    /// schema 与实现漂移的唯一防线：枚举值必须与 core 的 `ALL` 逐项相等。
    #[test]
    fn schema_enums_match_core_all() {
        assert_eq!(enum_of(PLAN_ROUTE, "mode"), TravelMode::ALL);
        assert_eq!(enum_of(PLAN_ROUTE, "sort"), SortBy::ALL);
        assert_eq!(enum_of(LIFE_CIRCLE, "package"), LifeCirclePackage::ALL);
        assert_eq!(enum_of(LIFE_CIRCLE, "crs"), Crs::ALL);
    }

    #[test]
    fn schema_covers_every_dispatchable_tool() {
        let names: Vec<String> = tool_schemas()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, TOOL_NAMES);
    }

    #[test]
    fn bad_json_reports_raw_text() {
        let error = dispatch(&Config::default(), PLAN_ROUTE, "{\"from\": 光谷,}").unwrap_err();
        assert_eq!(error.code, "BAD_TOOL_ARGS");
        assert!(
            error.message.contains("{\"from\": 光谷,}"),
            "文案要带原文，实际:{}",
            error.message
        );
    }

    #[test]
    fn bad_json_preview_is_bounded() {
        let long = format!("{{\"from\":\"{}\"", "汉".repeat(500));
        let error = dispatch(&Config::default(), PLAN_ROUTE, &long).unwrap_err();
        assert_eq!(error.code, "BAD_TOOL_ARGS");
        assert!(error.message.ends_with('…'));
        assert!(error.message.chars().count() < long.chars().count());
    }

    /// 模型最常写的非法值。必须失败并列出合法值，兜底成默认值等于给它一个假结果。
    #[test]
    fn transit_mode_is_rejected_with_choices() {
        let args = r#"{"from":"光谷广场","to":"武汉大学","mode":"transit","sort":"time"}"#;
        let error = dispatch(&Config::default(), PLAN_ROUTE, args).unwrap_err();
        assert_eq!(error.code, "BAD_MODE");
        for legal in TravelMode::ALL {
            assert!(
                error.message.contains(legal),
                "文案缺少合法值 {legal}:{}",
                error.message
            );
        }
    }

    #[test]
    fn bad_sort_and_package_are_rejected_with_choices() {
        let args = r#"{"from":"a","to":"b","mode":"bus","sort":"fastest"}"#;
        let error = dispatch(&Config::default(), PLAN_ROUTE, args).unwrap_err();
        assert_eq!(error.code, "BAD_SORT");
        assert!(SortBy::ALL.iter().all(|one| error.message.contains(one)));

        let args = r#"{"lat":30.5,"lng":114.4,"package":"medical"}"#;
        let error = dispatch(&Config::default(), LIFE_CIRCLE, args).unwrap_err();
        assert_eq!(error.code, "BAD_PACKAGE");
        assert!(
            LifeCirclePackage::ALL
                .iter()
                .all(|one| error.message.contains(one))
        );
    }

    #[test]
    fn unknown_tool_name_is_rejected() {
        let error = dispatch(&Config::default(), "get_weather", "{}").unwrap_err();
        assert_eq!(error.code, "UNKNOWN_TOOL");
        assert!(error.message.contains("get_weather"));
    }

    #[test]
    fn clip_keeps_char_boundary_and_marks_truncation() {
        let text = "汉".repeat(MAX_OUTPUT_CHARS + 1000);
        let out = clip(text);

        assert!(out.chars().count() <= MAX_OUTPUT_CHARS);
        assert!(out.ends_with(TRUNCATED_NOTE));
        let body = out.strip_suffix(TRUNCATED_NOTE).unwrap();
        // 全是完整汉字 = 切点没落在多字节字符中间（落中间的话上面的切片早就 panic 了）。
        assert!(body.chars().all(|one| one == '汉'));
        assert_eq!(
            body.chars().count(),
            MAX_OUTPUT_CHARS - TRUNCATED_NOTE.chars().count()
        );
    }

    #[test]
    fn clip_backs_off_to_nearest_newline() {
        let keep = MAX_OUTPUT_CHARS - TRUNCATED_NOTE.chars().count();
        let mut text = "汉".repeat(keep - 20);
        text.push('\n');
        text.push_str(&"尾".repeat(1000));

        let out = clip(text);
        assert!(out.ends_with(TRUNCATED_NOTE));
        // 回退到换行，换行之后那半行不该出现。
        assert!(!out.contains('尾'));
        assert!(!out.contains('\n'));
    }

    #[test]
    fn clip_leaves_short_text_alone() {
        let text = "短结果".to_string();
        assert_eq!(clip(text.clone()), text);
    }

    fn sample_plan() -> RoutePlan {
        let points: Vec<LatLng> = (0..600)
            .map(|step| LatLng::new(30.5 + f64::from(step) * 1e-4, 114.4))
            .collect();
        RoutePlan {
            origin: Endpoint {
                title: "光谷广场".to_string(),
                location: LatLng::new(30.5, 114.4),
            },
            destination: Endpoint {
                title: "武汉大学".to_string(),
                location: LatLng::new(30.54, 114.36),
            },
            mode: TravelMode::Subway,
            routes: vec![Route {
                id: "r1".to_string(),
                distance: 9_600,
                duration: 34,
                price: 3.0,
                bounds: [LatLng::new(30.5, 114.36), LatLng::new(30.54, 114.4)],
                segments: vec![
                    Segment {
                        kind: SegmentKind::Walk,
                        distance: 320,
                        duration: 5,
                        points: points.clone(),
                        instruction: vec!["向东步行 320 米".to_string()],
                        line: None,
                    },
                    Segment {
                        kind: SegmentKind::Transit,
                        distance: 9_000,
                        duration: 25,
                        points,
                        instruction: Vec::new(),
                        line: Some(TransitLine {
                            vehicle: Vehicle::Subway,
                            title: "地铁2号线".to_string(),
                            station_count: 7,
                            color: "#00A0E9".to_string(),
                            get_on: Stop {
                                title: "光谷广场".to_string(),
                                location: LatLng::new(30.5, 114.4),
                                exit: Some("A".to_string()),
                            },
                            get_off: Stop {
                                title: "街道口".to_string(),
                                location: LatLng::new(30.54, 114.36),
                                exit: None,
                            },
                            destination: Some("天河机场".to_string()),
                            first_time: Some("06:00".to_string()),
                            last_time: Some("23:00".to_string()),
                        }),
                    },
                ],
            }],
            provider: ProviderId::Tencent,
            crs: Crs::Gcj02,
            degraded: None,
        }
    }

    /// 折线是返回体里最大的一块，裁掉它是这个模块存在的理由之一。
    #[test]
    fn trimmed_plan_drops_polyline_keeps_point_count() {
        let out = encode(&trim_plan(&sample_plan())).unwrap();

        assert!(!out.contains("\"points\""), "折线不该出现:{out}");
        assert!(out.contains("\"pointCount\":600"));
        assert!(out.contains("\"firstPoint\""));
        assert!(out.contains("\"lastPoint\""));
        // 该留的业务字段还在。
        assert!(out.contains("地铁2号线"));
        assert!(out.contains("向东步行 320 米"));
        assert!(out.contains("\"durationMin\":34"));
        // 紧凑 JSON：不带缩进。
        assert!(!out.contains("\n  "));
    }

    #[test]
    fn trimmed_plan_is_far_smaller_than_raw() {
        let plan = sample_plan();
        let raw = serde_json::to_string(&plan).unwrap();
        let trimmed = encode(&trim_plan(&plan)).unwrap();
        assert!(
            trimmed.len() * 20 < raw.len(),
            "裁剪后 {} 字节 vs 原始 {} 字节",
            trimmed.len(),
            raw.len()
        );
    }

    #[test]
    fn trimmed_plan_reports_degradation() {
        let mut plan = sample_plan();
        plan.degraded = Some(route_core::models::Degraded {
            tried: vec![ProviderId::Amap],
            reason: Some("配额用尽".to_string()),
        });
        let out = encode(&trim_plan(&plan)).unwrap();
        assert!(out.contains("amap"));
        assert!(out.contains("配额用尽"));
    }

    /// 空数组会让模型自己猜原因（没配 Key？关键词错？），一句话直接指路。
    #[test]
    fn empty_search_returns_hint_not_empty_array() {
        // 没有任何 Key 的配置必然搜不到，正好覆盖空结果分支且不发网络请求。
        let out = dispatch(&Config::default(), SEARCH_PLACE, r#"{"keyword":"光谷"}"#).unwrap();
        assert_eq!(out.summary, EMPTY_SEARCH_HINT);
        // 候选点不回灌地图（6.4.2）。
        assert_eq!(out.payload, None);
    }

    #[test]
    fn trimmed_place_has_flat_coordinates() {
        let place = Place {
            title: "光谷广场".to_string(),
            address: Some("武汉市洪山区".to_string()),
            location: LatLng::new(30.5, 114.4),
            adcode: Some("420111".to_string()),
            provider: ProviderId::Tencent,
            id: Some("x".to_string()),
        };
        let out = serde_json::to_string(&trim_place(&place)).unwrap();
        assert!(out.contains("\"lat\":30.5"));
        assert!(out.contains("\"lng\":114.4"));
        assert!(out.contains("tencent"));
    }

    #[test]
    fn life_circle_rejects_unknown_crs_before_touching_network() {
        let args = r#"{"lat":30.5,"lng":114.4,"package":"care","crs":"EPSG:4326"}"#;
        let error = dispatch(&Config::default(), LIFE_CIRCLE, args).unwrap_err();
        assert_eq!(error.code, "BAD_CRS");
        assert!(Crs::ALL.iter().all(|one| error.message.contains(one)));
    }

    /// 参数都合法但没有百度 AK 时，报缺 Key 而不是空结果 —— 现象和原因差太远。
    #[test]
    fn life_circle_requires_baidu_key() {
        let args = r#"{"lat":30.5,"lng":114.4,"package":"care"}"#;
        let error = dispatch(&Config::default(), LIFE_CIRCLE, args).unwrap_err();
        assert_eq!(error.code, "BAIDU_KEY_REQUIRED");
    }

    #[test]
    fn trimmed_circle_converts_seconds_to_minutes() {
        let circle = LifeCircle {
            package: LifeCirclePackage::Care,
            crs: Crs::Gcj02,
            limit_minutes: 15,
            pois: vec![route_core::models::ReachablePoi {
                category: "药店".to_string(),
                name: "某药房".to_string(),
                address: "关山大道".to_string(),
                location: LatLng::new(30.5, 114.4),
                walk_duration_sec: 420,
                walk_distance: 500,
            }],
        };
        let out = serde_json::to_string(&trim_circle(&circle)).unwrap();
        assert!(out.contains("\"walkMinutes\":7"));
        assert!(out.contains("\"limitMinutes\":15"));
        assert!(out.contains("care"));
    }
}
