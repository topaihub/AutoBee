//! 三个子命令的执行体。
//!
//! # 与 GUI 的关系
//!
//! 这里是 `src-tauri/src/commands/map.rs` 里那三个 command 的同胞:同一份
//! `route-core` 调用、同一份 `autobee_shell::credentials::resolve` 读配置,
//! 区别只在外壳 —— 那边包一层 `#[tauri::command]` + `spawn_blocking`,
//! 这边是同步直调。
//!
//! **同一份配置、同一个 `settings.db`。** CLI 不自带 Key,也不接受 `--key`:
//! 密钥只有一个来源(DPAPI 加密后的 `settings.db`),多一个来源就多一处泄漏面,
//! 而且会出现"GUI 能算、CLI 算不出来"这种无从排查的分叉(设计方案 §7.1)。
//!
//! # 为什么不用 async
//!
//! GUI 那边 `spawn_blocking` 是为了不卡 WebView 的 UI 线程。CLI 是一次性进程,
//! 算完就退出,没有要让出的线程 —— 引 tokio 只为把同步调用包起来再 `await`
//! 一次是纯开销。

use route_core::config::Config;
use route_core::error::AppError;
use route_core::models::{
    Crs, LatLng, LifeCircle, LifeCirclePackage, Place, ProviderId, RoutePlan, SortBy, TravelMode,
};
use route_core::planner::RouteQuery;

/// 读配置,并把输出坐标系换成命令行要的那个。
///
/// `plan_route` 内部用 `settings.out_crs` 做最后一次换算(`planner.rs:241`),
/// 所以 `--out-crs` 要在**调用前**落到 settings 上。在结果上再转一次的写法
/// 会换算两遍:第一遍已经把坐标带去了 settings 里存的那个系。
fn config_with(out_crs: Crs) -> Result<Config, AppError> {
    let mut config = autobee_shell::credentials::resolve::load_config()?;
    config.settings.out_crs = out_crs;
    Ok(config)
}

/// 算路。
///
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

/// 地点联想。
///
/// # Errors
///
/// 一个 Key 都没配时返回错误。
///
/// 搜不到结果**不是**错误:返回空列表,退出码 0。空结果是一个有效答案
/// ("这个词没有匹配的地点"),而退出码非 0 会让脚本以为查询本身失败了。
pub fn suggest(keyword: &str, limit: usize, out_crs: Crs) -> Result<Vec<Place>, AppError> {
    let config = config_with(out_crs)?;
    // `suggest_places` 返回裸 Vec,拿不到"为什么空"。一个 Key 都没配时它也
    // 返回空,那种情况必须报错而不是装作搜不到 —— 后者会让人一直换关键词。
    // 用 `default_chain` 而不是自己数 `config.keys`:候选链还受 disabled 与
    // default_providers 影响,自己数会与 planner 真正走的链路判断不一致。
    if route_core::registry::default_chain(&config).is_empty() {
        return Err(AppError::no_provider());
    }
    tracing::info!(keyword, limit, "开始地点联想");
    let mut places = route_core::planner::suggest_places(&config, keyword, limit);
    // 每家返回的是自己的原生坐标系(高德/腾讯 GCJ02、百度 BD09、天地图
    // CGCS2000、Photon/Google WGS84),`suggest_places` 不做换算。GUI 那边
    // 不换是因为它只把点画在同样是 GCJ02 的底图上;CLI 的调用方拿到坐标要
    // 自己用,不标一个统一的系就会偏几百米,所以这里必须换到 --out-crs。
    for place in &mut places {
        let native =
            route_core::registry::get_meta(place.provider).map_or(out_crs, |meta| meta.coord_sys);
        place.location = route_core::geo::convert(place.location, native, out_crs);
    }
    Ok(places)
}

/// 15 分钟生活圈。
///
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

/// 把结果打到 stdout,返回退出码。
///
/// `--json` 序列化 `route-core` 的模型本身而不是手写投影:GUI 的 IPC 返回的是
/// 同一批结构,手写一份会与前端拿到的字段漂开。
///
/// 错误一律走 stderr。`... --json | jq` 这条管道要求 stdout 里只有 JSON:
/// 把报错混进去的话,失败时 jq 当场解析失败,看不到真正的原因(§5.2)。
///
/// `note` 是**两条路径都要打到 stderr** 的旁白(目前只有降级说明)。它不能
/// 写进 `render` 的返回值:那份返回值走 stdout,而 §5.2 把警告划给 stderr。
/// `--json` 时它也要打 —— JSON 里的 `degraded` 字段是给脚本的,人还得看见一句话。
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
                    // 序列化不该失败(模型里没有 NaN 以外的坑),但不能 unwrap:
                    // panic 的退出码是 101,脚本按 exit code 分档时会落到未知档。
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
