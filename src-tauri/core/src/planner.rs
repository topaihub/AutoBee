//! 编排层：解析起终点 → 选 provider → 精化 → 算路 → 坐标系统一。
//!
//! 对应 SPEC 11.3（两阶段解析）/ 11.4（顺序探测降级）/ 11.6（路由与降级）/ 11.7。
//! 前端不感知任何厂商细节，也不感知这里有几个阶段。
//!
//! # 顺序探测而不是「一次选对」
//!
//! 先按国家码挑候选链，再逐个尝试。挑得再准，那一家也可能刚好挂掉 ——
//! 顺序探测的代价是失败时多等一次超时，换来的是「只要有一家活着就能出结果」。

use std::sync::OnceLock;

use crate::circuit::CircuitBreaker;
use crate::config::Config;
use crate::decode::parse_lat_lng;
use crate::error::AppError;
use crate::factory::create_provider;
use crate::geo::{convert, convert_all};
use crate::models::{Crs, Endpoint, LatLng, Place, ProviderId, RoutePlan, SortBy, TravelMode};
use crate::providers::photon::global_locate;
use crate::providers::{RouteProvider, RouteRequest};
use crate::registry::{default_chain, resolve_providers};

/// 一次规划请求。起终点可以是坐标文本（`lat,lng`）也可以是地名。
#[derive(Debug, Clone, PartialEq)]
pub struct RouteQuery {
    /// 起点。
    pub from: String,
    /// 终点。
    pub to: String,
    /// 出行方式。
    pub mode: TravelMode,
    /// 排序依据。
    pub sort: SortBy,
}

/// 一个解析好的地点。
struct Resolved {
    location: LatLng,
    title: String,
    crs: Crs,
}

/// 进程内共享的熔断器。
///
/// 每次请求新建的话，熔断状态永远清零 —— 那这一层就只是摆设。
fn breaker() -> &'static CircuitBreaker {
    static BREAKER: OnceLock<CircuitBreaker> = OnceLock::new();
    BREAKER.get_or_init(CircuitBreaker::new)
}

/// 用坐标粗判是否在中国境内。
///
/// 矩形粗判，会包含部分邻国区域。只在阶段一（全球地理编码）不可用、
/// 或用户直接输入坐标时兜底 —— 那时宁可猜 CN，因为国内厂商是主力。
fn rough_country(l: LatLng) -> Option<String> {
    (l.lng > 72.004 && l.lng < 137.8347 && l.lat > 0.8293 && l.lat < 55.8271)
        .then(|| "CN".to_string())
}

/// 用某个 provider 解析地点文本。
///
/// **顺序必须是 suggestion → geocode**。geocoder 对 POI 式输入会报 348
/// （实测「光谷大道地铁站」「铢家村-E1口」都是这个码），先问它就白跑一趟。
fn resolve_on(provider: &dyn RouteProvider, text: &str) -> Result<Resolved, AppError> {
    let crs = crate::registry::get_meta(provider.id())
        .map(|m| m.coord_sys)
        .unwrap_or(Crs::Wgs84);

    let mut place: Option<Place> = provider
        .suggest(text, 1)
        .ok()
        .and_then(|hits| hits.into_iter().next());
    if place.is_none() {
        place = provider.geocode(text).ok().flatten();
    }
    let Some(place) = place else {
        return Err(AppError::resolve_failed(text));
    };

    Ok(Resolved {
        location: place.location,
        title: place.title,
        crs,
    })
}

/// 整份 [`RoutePlan`] 的坐标系转换。
#[must_use]
pub fn convert_plan(plan: RoutePlan, to: Crs) -> RoutePlan {
    let from = plan.crs;
    if from == to {
        return plan;
    }

    RoutePlan {
        origin: Endpoint {
            title: plan.origin.title,
            location: convert(plan.origin.location, from, to),
        },
        destination: Endpoint {
            title: plan.destination.title,
            location: convert(plan.destination.location, from, to),
        },
        crs: to,
        routes: plan
            .routes
            .into_iter()
            .map(|r| crate::models::Route {
                bounds: [
                    convert(r.bounds[0], from, to),
                    convert(r.bounds[1], from, to),
                ],
                segments: r
                    .segments
                    .into_iter()
                    .map(|s| crate::models::Segment {
                        points: convert_all(&s.points, from, to),
                        line: s.line.map(|l| crate::models::TransitLine {
                            get_on: crate::models::Stop {
                                location: convert(l.get_on.location, from, to),
                                ..l.get_on
                            },
                            get_off: crate::models::Stop {
                                location: convert(l.get_off.location, from, to),
                                ..l.get_off
                            },
                            ..l
                        }),
                        ..s
                    })
                    .collect(),
                ..r
            })
            .collect(),
        ..plan
    }
}

/// 规划一条路线。
///
/// # Errors
///
/// 跨境（[`AppError::cross_border`]）、没有可用服务商（[`AppError::no_provider`]）、
/// 所有候选都失败（[`AppError::all_failed`]）时返回错误。
pub fn plan_route(config: &Config, query: &RouteQuery) -> Result<RoutePlan, AppError> {
    let settings = &config.settings;
    let from_ll = parse_lat_lng(&query.from);
    let to_ll = parse_lat_lng(&query.to);

    /* ---------- 阶段一：判定归属（只用于选 provider） ---------- */
    let mut cc_from = from_ll.and_then(rough_country);
    let mut cc_to = to_ll.and_then(rough_country);

    if settings.auto_provider {
        if cc_from.is_none() {
            cc_from = global_locate(&query.from).and_then(|h| h.country_code);
        }
        if cc_to.is_none() {
            cc_to = global_locate(&query.to).and_then(|h| h.country_code);
        }
    }

    if let (Some(from_cc), Some(to_cc)) = (&cc_from, &cc_to)
        && from_cc != to_cc
        && !settings.allow_cross_border
    {
        return Err(AppError::cross_border(from_cc, to_cc));
    }
    let country = cc_from.as_deref().or(cc_to.as_deref());

    /* ---------- 选 provider 链 ---------- */
    let mut chain = if settings.auto_provider {
        resolve_providers(country, query.mode, config)
    } else {
        Vec::new()
    };
    if chain.is_empty() {
        chain = default_chain(config);
    }
    if chain.is_empty() {
        return Err(AppError::no_provider());
    }

    /* ---------- 逐个尝试 + 降级 ---------- */
    let mut tried: Vec<String> = Vec::new();
    let mut last_error = String::new();

    for id in chain {
        if breaker().is_open(id) {
            tried.push(id.as_str().to_string());
            continue;
        }

        let provider = match create_provider(id, config) {
            Ok(p) => p,
            Err(_) => {
                // 没配 Key 或没实现 adapter：不算失败，也不该熔断。
                continue;
            }
        };

        let result = (|| -> Result<RoutePlan, AppError> {
            let target_crs = crate::registry::get_meta(id)
                .map(|m| m.coord_sys)
                .unwrap_or(Crs::Wgs84);

            let from = match from_ll {
                Some(ll) => Resolved {
                    location: ll,
                    title: "起点".to_string(),
                    crs: Crs::Wgs84,
                },
                None => resolve_on(provider.as_ref(), &query.from)?,
            };
            let to = match to_ll {
                Some(ll) => Resolved {
                    location: ll,
                    title: "终点".to_string(),
                    crs: Crs::Wgs84,
                },
                None => resolve_on(provider.as_ref(), &query.to)?,
            };

            // 用户直接粘的坐标按 WGS-84 处理（那是 GPS 的通用写法），
            // 要换算到该 provider 自己的坐标系才能发请求。
            let plan = provider.plan_route(&RouteRequest {
                from: convert(from.location, from.crs, target_crs),
                to: convert(to.location, to.crs, target_crs),
                mode: query.mode,
                sort: query.sort,
                from_title: Some(from.title),
                to_title: Some(to.title),
            })?;
            Ok(plan)
        })();

        match result {
            Ok(plan) => {
                breaker().record_success(id);
                let out = convert_plan(plan, settings.out_crs);
                if tried.is_empty() {
                    return Ok(out);
                }
                return Ok(RoutePlan {
                    degraded: Some(crate::models::Degraded {
                        tried: tried.iter().filter_map(|s| ProviderId::parse(s)).collect(),
                        reason: Some(if last_error.is_empty() {
                            "主服务不可用".to_string()
                        } else {
                            last_error
                        }),
                    }),
                    ..out
                });
            }
            Err(error) => {
                last_error = error.message.clone();
                if error.code != "RESOLVE_FAILED" {
                    // 解析失败多半是「这家不认识这个地名」（海外地名问国内厂商），
                    // 换一家还有机会；那是正常的探测成本，不该熔断。
                    breaker().record_failure(id);
                }
                tried.push(id.as_str().to_string());
            }
        }
    }

    Err(AppError::all_failed(&tried, &last_error))
}

/// 地点联想：走默认链里第一个可用的 provider。
///
/// 返回空数组而不是错误 —— 输入框每敲一下都会调它，报错会刷屏。
#[must_use]
pub fn suggest_places(config: &Config, keyword: &str, page_size: usize) -> Vec<Place> {
    for id in default_chain(config) {
        if breaker().is_open(id) {
            continue;
        }
        if let Ok(provider) = create_provider(id, config) {
            match provider.suggest(keyword, page_size) {
                Ok(hits) => {
                    breaker().record_success(id);
                    return hits;
                }
                Err(_) => breaker().record_failure(id),
            }
        }
    }
    Vec::new()
}

/// 地址解析兜底。
#[must_use]
pub fn geocode_place(config: &Config, address: &str) -> Option<Place> {
    for id in default_chain(config) {
        if breaker().is_open(id) {
            continue;
        }
        if let Ok(provider) = create_provider(id, config)
            && let Ok(Some(place)) = provider.geocode(address)
        {
            return Some(place);
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 坐标粗判中国境内() {
        assert_eq!(
            rough_country(LatLng::new(30.5, 114.4)),
            Some("CN".to_string())
        );
        assert_eq!(
            rough_country(LatLng::new(35.68, 139.76)),
            None,
            "东京不在框内"
        );
    }

    #[test]
    fn 坐标转_plan_会换掉所有点() {
        let plan = RoutePlan {
            origin: Endpoint {
                title: "a".into(),
                location: LatLng::new(30.5, 114.4),
            },
            destination: Endpoint {
                title: "b".into(),
                location: LatLng::new(30.6, 114.5),
            },
            mode: TravelMode::Subway,
            routes: vec![crate::models::Route {
                id: "r0".into(),
                distance: 100,
                duration: 10,
                price: 5.0,
                bounds: [LatLng::new(30.5, 114.4), LatLng::new(30.6, 114.5)],
                segments: vec![crate::models::Segment {
                    kind: crate::models::SegmentKind::Walk,
                    distance: 100,
                    duration: 10,
                    points: vec![LatLng::new(30.5, 114.4)],
                    instruction: vec![],
                    line: None,
                }],
            }],
            provider: ProviderId::Tencent,
            crs: Crs::Gcj02,
            degraded: None,
        };

        let out = convert_plan(plan.clone(), Crs::Wgs84);
        assert_eq!(out.crs, Crs::Wgs84);
        // GCJ-02 → WGS-84 的偏移方向随位置变化（往东南或往西北都可能），
        // 能稳定断言的是偏移量本身在这个量级 —— 写死方向的断言在别处会翻车。
        let shift = out.origin.location.haversine_m(plan.origin.location);
        assert!((100.0..=1000.0).contains(&shift), "偏移 {shift:.1} m");
        assert_eq!(plan.origin.location.lat, 30.5, "原对象没被改动");
    }

    #[test]
    fn 同坐标系转换返回原值() {
        let plan = RoutePlan {
            origin: Endpoint {
                title: "a".into(),
                location: LatLng::new(30.5, 114.4),
            },
            destination: Endpoint {
                title: "b".into(),
                location: LatLng::new(30.6, 114.5),
            },
            mode: TravelMode::Walking,
            routes: vec![],
            provider: ProviderId::Tencent,
            crs: Crs::Wgs84,
            degraded: None,
        };
        assert_eq!(convert_plan(plan.clone(), Crs::Wgs84), plan);
    }

    #[test]
    fn 一个_key_都没配时报_no_provider() {
        let config = Config::default();
        let error = plan_route(
            &config,
            &RouteQuery {
                from: "30.5,114.4".into(),
                to: "30.6,114.5".into(),
                mode: TravelMode::Subway,
                sort: SortBy::Time,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "NO_PROVIDER");
    }
}
