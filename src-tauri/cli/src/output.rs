//! 结果渲染。
//!
//! # 两条输出路径的分工
//!
//! `--json` 不走这里:它直接 `serde_json::to_string_pretty` 序列化 `route-core`
//! 的模型本身。这么做的理由是契约同源 —— GUI 的 IPC 返回的也是同一批结构,
//! 手写一份 JSON 投影会与前端拿到的字段漂开,而两边都号称是"AutoBee 的输出"。
//!
//! 本模块只负责给人看的那一路:带缩进、带单位、把秒折成分钟。
//!
//! # 为什么不加颜色
//!
//! CLI 的首要消费者是脚本与编码 agent(设计方案 §1.2)。ANSI 转义码进了
//! 管道就是垃圾字符,而判断"对面是不是 tty"要引依赖。不加色是这里更省的选择。

use route_core::models::{LifeCircle, Place, Route, RoutePlan, SegmentKind};

/// 秒 -> 分钟,向上取整到 1 分钟。
///
/// **名字带 `sec` 是刻意的。** `Route.duration` 的单位是分钟,`LifeCirclePoi`
/// 的是 `walk_duration_sec`(秒),两者混用过一次:算路摘要把 29 分钟再除以
/// 60,结果每条方案都显示「1 分钟」—— 11 公里地铁 1 分钟,而且不报错。
///
/// 向上而不是四舍五入:显示"0 分钟"的那一段会让人以为不用走。
fn sec_to_minutes(seconds: u32) -> u32 {
    seconds.div_ceil(60).max(1)
}

/// 米 -> 人能读的距离。
fn distance(meters: u32) -> String {
    if meters < 1000 {
        format!("{meters} 米")
    } else {
        format!("{:.1} 公里", f64::from(meters) / 1000.0)
    }
}

/// 一条方案的摘要行。
fn route_line(index: usize, route: &Route) -> String {
    let mut line = format!(
        "  [{index}] {} 分钟 · {}",
        // `Route.duration` 已经是分钟(`models.rs:304`),不要再换算。
        route.duration,
        distance(route.distance)
    );
    // 票价 0 有两种成因:步行(本来免费)与厂商没给。都不显示,免得
    // 把"未知"说成"免费"。
    if route.price > 0.0 {
        line.push_str(&format!(" · {:.1} 元", route.price));
    }
    line
}

/// 一条方案里的换乘明细。
fn segment_lines(route: &Route) -> Vec<String> {
    route
        .segments
        .iter()
        .map(|segment| match (&segment.kind, &segment.line) {
            (SegmentKind::Transit, Some(line)) => format!(
                "      {} {} → {}({} 站)",
                line.title, line.get_on.title, line.get_off.title, line.station_count
            ),
            // 步行段只说走多远:它的 instruction 是逐路口的转向指令,
            // 一条方案能有几十条,铺开就把摘要冲掉了。
            _ => format!("      步行 {}", distance(segment.distance)),
        })
        .collect()
}

/// 算路结果的文本形式。
#[must_use]
pub fn plan(plan: &RoutePlan) -> String {
    let mut out = vec![format!(
        "{} → {}",
        plan.origin.title, plan.destination.title
    )];
    // 数据来源与坐标系必须显示:同一对起终点在不同厂商那里方案数、时长、
    // 票价都不一样,不标来源的话两次结果不同会被当成 bug。坐标系不标则
    // 会有人把 GCJ02 的点直接画到 WGS84 底图上,整体偏移几百米。
    out.push(format!(
        "来源 {} · 坐标系 {:?} · {} 条方案",
        plan.provider.as_str(),
        plan.crs,
        plan.routes.len()
    ));
    for (index, route) in plan.routes.iter().enumerate() {
        out.push(route_line(index, route));
        out.extend(segment_lines(route));
    }
    out.join("\n")
}

/// 降级说明。**给 stderr 用,不进 stdout。**
///
/// §5.2:stdout 只有结果,降级说明属于「警告」那一档。它同时保留在 `--json`
/// 的 `degraded` 字段里 —— 人要被告知「主服务挂了换了一家」,脚本要能判断
/// 这件事,两个需求分别由 stderr 和 JSON 字段满足。
///
/// 返回 `None` 表示没降级,调用方据此决定打不打。
#[must_use]
pub fn degraded_note(plan: &RoutePlan) -> Option<String> {
    let degraded = plan.degraded.as_ref()?;
    let tried: Vec<&str> = degraded.tried.iter().map(|id| id.as_str()).collect();
    Some(format!(
        "已降级:依次试过 {}{}",
        tried.join(" → "),
        degraded
            .reason
            .as_deref()
            .map_or_else(String::new, |reason| format!(",原因:{reason}"))
    ))
}

/// 地点联想结果的文本形式。
#[must_use]
pub fn places(places: &[Place]) -> String {
    if places.is_empty() {
        // 空结果也要说一句:什么都不打印时调用方分不清"没搜到"与"程序挂了"。
        return "没有匹配的地点".to_string();
    }
    places
        .iter()
        .enumerate()
        .map(|(index, place)| {
            format!(
                "[{index}] {} ({:.6},{:.6}) 来源 {}{}",
                place.title,
                place.location.lat,
                place.location.lng,
                place.provider.as_str(),
                place
                    .address
                    .as_deref()
                    .map_or_else(String::new, |address| format!("\n      {address}"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 生活圈结果的文本形式。
///
/// 不打印无障碍负担评分:那个算法在前端 `src/lib/accessibility.ts` 里,
/// Rust 侧没有,`LifeCircle` 也没有这个字段。CLI 只能给出它拿得到的东西。
#[must_use]
pub fn life_circle(circle: &LifeCircle) -> String {
    let mut out = vec![format!(
        "{:?} 服务包 · {} 分钟内 · 坐标系 {:?} · {} 个可达点",
        circle.package,
        circle.limit_minutes,
        circle.crs,
        circle.pois.len()
    )];
    for poi in &circle.pois {
        out.push(format!(
            "  {} {} · 步行 {} 分钟 · {}",
            poi.category,
            poi.name,
            sec_to_minutes(poi.walk_duration_sec),
            distance(poi.walk_distance)
        ));
    }
    out.join("\n")
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "测试里断言失败即测试失败"
)]
mod tests {
    use super::*;
    use route_core::models::{Crs, Degraded, Endpoint, LatLng, ProviderId, TravelMode};

    fn 降级过的空结果() -> RoutePlan {
        RoutePlan {
            origin: Endpoint {
                title: "南京南站".to_string(),
                location: LatLng::new(31.975, 118.804),
            },
            destination: Endpoint {
                title: "新街口".to_string(),
                location: LatLng::new(32.043, 118.784),
            },
            mode: TravelMode::Subway,
            routes: vec![],
            provider: ProviderId::Baidu,
            crs: Crs::Gcj02,
            degraded: Some(Degraded {
                tried: vec![ProviderId::Amap, ProviderId::Baidu],
                reason: Some("配额用尽".to_string()),
            }),
        }
    }

    /// `Route.duration` 的单位是**分钟**,不是秒。
    ///
    /// 这条挡的是真发生过的一次:摘要行套了一层 `秒 -> 分钟` 的换算,
    /// 于是 29 分钟的地铁方案显示成「1 分钟」。它不会 panic、不会报错,
    /// 只是把数字说错 —— 只有对着真实数据看输出才会发现,所以值得钉住。
    #[test]
    fn 方案耗时按分钟显示不再除一次六十() {
        let mut 结果 = 降级过的空结果();
        结果.routes = vec![Route {
            id: "r0".to_string(),
            distance: 11_886,
            duration: 29,
            price: 4.0,
            bounds: [LatLng::new(31.975, 118.804), LatLng::new(32.043, 118.784)],
            segments: vec![],
        }];

        let text = plan(&结果);
        assert!(text.contains("29 分钟"), "29 分钟的方案要显示 29:{text}");
        assert!(
            !text.contains("1 分钟"),
            "把分钟当秒再除 60 就会塌成 1 分钟:{text}"
        );
    }

    /// §5.2:stdout 只有结果,降级说明属于警告、归 stderr。
    ///
    /// 这条挡的是「顺手把降级说明拼进文本输出」—— 那样 `--json` 之外的
    /// stdout 里就混进了警告,而下游按行解析结果的脚本会多读到一行。
    #[test]
    fn 降级说明不进_stdout_文本() {
        let text = plan(&降级过的空结果());
        assert!(
            !text.contains("已降级"),
            "降级说明要走 stderr,不许出现在 stdout 的文本里:{text}"
        );
    }

    /// 同一份降级信息必须能从 stderr 那一路拿到,否则人就完全看不见了。
    #[test]
    fn 降级说明走_stderr_那一路() {
        let note = degraded_note(&降级过的空结果()).expect("降级了就要有旁白");
        assert!(note.contains("已降级"), "{note}");
        // 要点名试过哪几家:只说「已降级」的话,排查的人不知道谁挂了。
        assert!(note.contains(ProviderId::Amap.as_str()), "{note}");
        assert!(note.contains(ProviderId::Baidu.as_str()), "{note}");
        assert!(note.contains("配额用尽"), "{note}");

        // 没降级时不许打空行:stderr 上无端多一行会让人以为出了事。
        let mut clean = 降级过的空结果();
        clean.degraded = None;
        assert!(degraded_note(&clean).is_none());
    }

    /// §5.2 的另半边:降级信息**保留在 JSON 里**。脚本靠这个字段判断,
    /// 不能因为「已经打到 stderr 了」就把它从序列化里摘掉。
    #[test]
    fn 降级说明留在_json_里() {
        let json = serde_json::to_string(&降级过的空结果()).unwrap();
        assert!(json.contains("\"degraded\""), "{json}");
        assert!(json.contains("配额用尽"), "{json}");
    }
}
