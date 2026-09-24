//! 三个子命令的执行体,与 `src-tauri/src/commands/map.rs` 的 command 共用同一份
//! `route-core` 调用与 `credentials::resolve`。CLI 不接受 `--key`:密钥只有 DPAPI
//! 加密后的 `settings.db` 一个来源。

use route_core::config::Config;
use route_core::error::AppError;
use route_core::models::{
    Crs, LatLng, LifeCircle, LifeCirclePackage, Place, ProviderId, RoutePlan, SortBy, TravelMode,
};
use route_core::planner::RouteQuery;

/// `plan_route` 内部用 `settings.out_crs` 做最后一次换算,所以 `--out-crs` 必须在
/// 调用**前**落到 settings 上;在结果上再转一次会换算两遍。
fn config_with(out_crs: Crs) -> Result<Config, AppError> {
    let mut config = autobee_shell::credentials::resolve::load_config()?;
    config.settings.out_crs = out_crs;
    Ok(config)
}

/// # Errors
///
/// 地点解析不出、跨境、没有可用服务商、所有候选都失败时返回错误。
pub fn route(
    from: &str,
    to: &str,
    mode: TravelMode,
    sort: SortBy,
    out_crs: Crs,
) -> Result<RoutePlan, AppError> {
    let config = config_with(out_crs)?;
    tracing::info!(from, to, ?mode, ?sort, "开始规划路线");
    route_core::planner::plan_route(
        &config,
        &RouteQuery {
            from: from.to_string(),
            to: to.to_string(),
            mode,
            sort,
        },
    )
}

/// # Errors
///
/// 一个 Key 都没配时返回错误。搜不到结果不是错误 —— 返回空列表、退出码 0。
pub fn suggest(keyword: &str, limit: usize, out_crs: Crs) -> Result<Vec<Place>, AppError> {
    let config = config_with(out_crs)?;
    // `suggest_places` 一个 Key 都没配时也返回空 Vec,拿不到"为什么空"。用
    // `default_chain` 而不是数 `config.keys`:候选链还受 disabled 影响。
    if route_core::registry::default_chain(&config).is_empty() {
        return Err(AppError::no_provider());
    }
    tracing::info!(keyword, limit, "开始地点联想");
    let mut places = route_core::planner::suggest_places(&config, keyword, limit);
    // 每家返回的是自己的原生坐标系(GCJ02 / BD09 / CGCS2000 / WGS84),`suggest_places`
    // 不做换算。CLI 的调用方拿坐标去自己用,不统一到 --out-crs 会偏几百米。
    for place in &mut places {
        let native =
            route_core::registry::get_meta(place.provider).map_or(out_crs, |meta| meta.coord_sys);
        place.location = route_core::geo::convert(place.location, native, out_crs);
    }
    Ok(places)
}

/// # Errors
///
/// 没配百度 AK(`BAIDU_KEY_REQUIRED`)、百度地点检索失败时返回错误。
pub fn circle(
    at: LatLng,
    crs: Crs,
    package: LifeCirclePackage,
    out_crs: Crs,
) -> Result<LifeCircle, AppError> {
    let config = config_with(out_crs)?;
    let key = config.key(ProviderId::Baidu).ok_or_else(|| {
        tracing::warn!("未配置百度 Web 服务 AK,生活圈无法查询");
        AppError::new(
            "BAIDU_KEY_REQUIRED",
            "请到设置页填写百度地图 Web 服务 AK",
            400,
        )
    })?;
    tracing::info!(?package, ?crs, "开始查询生活圈");
    // 百度的接口只认 BD09,入口先转进去,出口再转到 --out-crs。
    let mut circle = route_core::life_circle::life_circle(
        key,
        route_core::geo::convert(at, crs, Crs::Bd09),
        package,
    )?;
    for poi in &mut circle.pois {
        poi.location = route_core::geo::convert(poi.location, Crs::Bd09, out_crs);
    }
    circle.crs = out_crs;
    Ok(circle)
}

/// 错误与 `note` 走 stderr,结果走 stdout:`... --json | jq` 要求 stdout 里只有 JSON。
/// `note` 在 `--json` 时也要打 —— JSON 里的 `degraded` 字段是给脚本的,人还得看见一句话。
pub fn report<T: serde::Serialize>(
    result: Result<T, AppError>,
    json: bool,
    render: impl FnOnce(&T) -> String,
    note: impl FnOnce(&T) -> Option<String>,
) -> i32 {
    match result {
        Ok(value) => {
            if let Some(text) = note(&value) {
                eprintln!("{text}");
            }
            if json {
                match serde_json::to_string_pretty(&value) {
                    Ok(text) => println!("{text}"),
                    // 不 unwrap:panic 的退出码 101 会落到脚本按 exit code 分档的未知档。
                    Err(error) => {
                        eprintln!("SERIALIZE_FAILED: 结果序列化失败:{error}");
                        return 1;
                    }
                }
            } else {
                println!("{}", render(&value));
            }
            0
        }
        Err(error) => {
            eprintln!("{}: {}", error.code, error.message);
            crate::args::exit_code(&error.code)
        }
    }
}
