//! 地图能力的 command：服务商档案、界面偏好、地点联想、算路、底图瓦片、
//! 生活圈、日志回读。
//!
//! 通用约定（`spawn_blocking`、每次重新读配置）见父模块 [`crate::commands`]。

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use route_core::models::{
    Crs, LatLng, LifeCircle, LifeCirclePackage, ProviderId, RoutePlan, SortBy, TravelMode,
};
use route_core::planner::{RouteQuery, plan_route as core_plan_route, suggest_places};
use route_core::{factory, life_circle, models::Place};

use autobee_shell::credentials::resolve;
use autobee_shell::credentials::store::Store;
use autobee_shell::credentials::{MapProviderDto, MapProviderInput, SettingsDto, TestResult};
use autobee_shell::error::AppError;

/// 后台任务本身崩了（panic）时的兜底。
fn join_failed(detail: &str) -> AppError {
    AppError::new("INTERNAL", format!("后台任务失败：{detail}"), 500)
}

/// 列出全部服务商档案（密钥已掩码）。
///
/// # Errors
///
/// 档案库读不出来时返回错误。
#[tauri::command]
pub async fn list_map_providers() -> Result<Vec<MapProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(|| Store::open()?.list())
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 保存一条档案。
///
/// # Errors
///
/// 密钥加密失败或 SQL 失败时返回错误。
#[tauri::command]
pub async fn save_map_provider(input: MapProviderInput) -> Result<Vec<MapProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        store.save(&input)?;
        store.list()
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 删掉一条档案。内置的六家删不掉。
///
/// # Errors
///
/// 试图删预置档案或 SQL 失败时返回错误。
#[tauri::command]
pub async fn delete_map_provider(slug: String) -> Result<Vec<MapProviderDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open()?;
        store.delete(&slug)?;
        tracing::info!(provider = %slug, "已删除服务商档案");
        store.list()
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 测试某家的 Key 能不能用：真发一次请求。
///
/// # 为什么真的发请求
///
/// 「填了 Key」不等于「Key 能用」—— 配额打满、权限没开、白名单没配都会让它
/// 在算路时才失败。而那个失败发生在用户填完 Key 几分钟之后，他已经忘了这回事。
///
/// # Errors
///
/// 档案库读不出来时返回错误。Key 不可用**不算错误**，返回 `ok: false`。
#[tauri::command]
pub async fn test_map_provider(slug: String) -> Result<TestResult, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(id) = ProviderId::parse(&slug) else {
            return Ok(TestResult {
                ok: false,
                message: "不认识的服务商".to_string(),
            });
        };
        if !factory::is_implemented(id) {
            return Ok(TestResult {
                ok: false,
                message: "这家的接入还没实现，填了 Key 也用不上".to_string(),
            });
        }

        let config = resolve::load_config()?;
        let provider = match factory::create_provider(id, &config) {
            Ok(p) => p,
            Err(_) => {
                return Ok(TestResult {
                    ok: false,
                    message: "还没填 Key".to_string(),
                });
            }
        };

        let outcome = match provider.suggest("天安门", 1) {
            Ok(hits) if !hits.is_empty() => TestResult {
                ok: true,
                message: format!("Key 可用，测试查询返回「{}」", hits[0].title),
            },
            Ok(_) => TestResult {
                ok: false,
                message: "Key 可能没开 WebService 权限：查询成功但没有结果".to_string(),
            },
            Err(error) => TestResult {
                ok: false,
                message: error.message,
            },
        };
        // 不记 Key 本身。这里连掩码都不用 —— slug 就够定位是哪一家。
        if outcome.ok {
            tracing::info!(provider = %slug, "Key 测试通过");
        } else {
            tracing::warn!(provider = %slug, reason = %outcome.message, "Key 测试不通过");
        }
        Ok(outcome)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 读界面偏好。
///
/// # Errors
///
/// 档案库读不出来时返回错误。
#[tauri::command]
pub async fn get_settings() -> Result<SettingsDto, AppError> {
    tauri::async_runtime::spawn_blocking(resolve::load_settings)
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 写界面偏好。
///
/// # Errors
///
/// 档案库写不进去时返回错误。
#[tauri::command]
pub async fn save_settings(settings: SettingsDto) -> Result<SettingsDto, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        tracing::info!(
            auto_provider = settings.auto_provider,
            out_crs = %settings.out_crs,
            tile_source = %settings.tile_source,
            show_logs = settings.show_logs,
            "保存界面偏好"
        );
        resolve::save_settings(&settings)?;
        resolve::load_settings()
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 地点联想。
///
/// 返回空数组而不是错误 —— 输入框每敲一下都会调它，报错会刷屏。
///
/// # Errors
///
/// 档案库读不出来时返回错误。
#[tauri::command]
pub async fn suggest(keyword: String) -> Result<Vec<Place>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = resolve::load_config()?;
        let hits = suggest_places(&config, &keyword, 8);
        // debug 而不是 info：每敲一个字符一条，info 会把有用的行冲掉。
        tracing::debug!(keyword = %keyword, hits = hits.len(), "地点联想");
        Ok(hits)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 规划路线。
///
/// # Errors
///
/// 跨境、没有可用服务商、所有候选都失败时返回错误。
#[tauri::command]
pub async fn plan_route(
    from: String,
    to: String,
    mode: String,
    sort: String,
) -> Result<RoutePlan, AppError> {
    // 不认识就报错而不是兜底猜:调用方是大模型时,它得知道自己填错了才能重试。
    let mode = TravelMode::parse(&mode).ok_or_else(|| {
        AppError::new(
            "BAD_MODE",
            format!(
                "不认识的出行方式「{mode}」,可选:{}",
                TravelMode::ALL.join(" / ")
            ),
            400,
        )
    })?;
    let sort = SortBy::parse(&sort).ok_or_else(|| {
        AppError::new(
            "BAD_SORT",
            format!(
                "不认识的排序依据「{sort}」,可选:{}",
                SortBy::ALL.join(" / ")
            ),
            400,
        )
    })?;

    tauri::async_runtime::spawn_blocking(move || {
        tracing::info!(from = %from, to = %to, ?mode, ?sort, "开始规划路线");
        let config = resolve::load_config()?;
        let plan = core_plan_route(
            &config,
            &RouteQuery {
                from,
                to,
                mode,
                sort,
            },
        );
        match &plan {
            Ok(out) => tracing::info!(
                provider = out.provider.as_str(),
                routes = out.routes.len(),
                degraded = out.degraded.is_some(),
                "规划完成"
            ),
            // error 而不是 warn：走到这里用户看到的就是一句失败，没有下一步了。
            Err(error) => tracing::error!(code = %error.code, %error, "规划失败"),
        }
        plan
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 取一块底图瓦片，返回 base64 图片。
///
/// 走 IPC 而不是让 WebView 直接拉，是为了缓存与 UA 可控，见
/// `route_core::tiles` 的模块文档。
///
/// # Errors
///
/// 参数越界、源名不认识、下载失败时返回错误。
#[tauri::command]
pub async fn get_tile(z: u8, x: u32, y: u32) -> Result<String, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = resolve::load_config()?;
        let bytes = route_core::tiles::fetch_tile(
            &autobee_shell::paths::tile_cache_dir(),
            &config.settings.tile_source,
            z,
            x,
            y,
        )
        // warn 而不是 error：单块瓦片拉不到只是图上缺一格，路线还在。
        // 不记成功的那条 —— 拖一次地图就是几十块，info 会被冲掉。
        .inspect_err(|error| {
            tracing::warn!(source = %config.settings.tile_source, z, x, y, %error, "瓦片获取失败");
        })?;
        Ok(BASE64.encode(bytes))
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 取百度 JS API GL 的 **AK 明文**,仅供本地 WebView 加载百度底图。
///
/// # 为什么这条 command 下发明文,而 `list_map_providers` 只给掩码
///
/// JS API GL 是浏览器内渲染,脚本 URL 里必须带明文 AK,没有代理掉的办法
/// (脚本在百度的域,请求由浏览器直接发)。其余厂商都只在 Rust 侧调 HTTP,
/// 明文不必离开进程 —— 那条掩码约定不变,`store.rs` 里有测试钉着。
/// 泄露风险与取舍见 [`resolve::baidu_sdk_key`]。
///
/// 原先这里返回的是腾讯的 key(矢量底图),M1-4 把主地图换成百度 JS API GL
/// 之后底图来自百度,继续下发腾讯 key 只会让前端加载一个用不上的 SDK。
///
/// # Errors
///
/// 档案库读不出来时返回错误。没填 AK 或用户停用了百度时返回 `Ok(None)` ——
/// 前端据此显示「请先在设置里填百度 AK」而不是错误态。
#[tauri::command]
pub async fn get_map_sdk_key() -> Result<Option<String>, AppError> {
    tauri::async_runtime::spawn_blocking(resolve::baidu_sdk_key)
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}

/// 查询起点周围真实步行 15 分钟内的百度 POI 服务。
///
/// # Errors
///
/// 未配置百度 Web 服务 AK、参数非法或百度服务失败时返回错误。
#[tauri::command]
pub async fn get_life_circle(
    lat: f64,
    lng: f64,
    crs: String,
    package: String,
) -> Result<LifeCircle, AppError> {
    let crs =
        Crs::parse(&crs).ok_or_else(|| AppError::new("BAD_CRS", "不认识的起点坐标系", 400))?;
    let package = LifeCirclePackage::parse(&package)
        .ok_or_else(|| AppError::new("BAD_PACKAGE", "不认识的生活圈服务包", 400))?;
    tauri::async_runtime::spawn_blocking(move || {
        tracing::info!(?package, ?crs, "开始查询生活圈");
        let config = resolve::load_config()?;
        let key = config.key(ProviderId::Baidu).ok_or_else(|| {
            // 这条最值得留：现象是「生活圈一直出不来」，而原因只是没填 AK。
            tracing::warn!("未配置百度 Web 服务 AK，生活圈无法查询");
            AppError::new(
                "BAIDU_KEY_REQUIRED",
                "请到设置页填写百度地图 Web 服务 AK",
                400,
            )
        })?;
        let mut circle = life_circle::life_circle(
            key,
            route_core::geo::convert(LatLng::new(lat, lng), crs, Crs::Bd09),
            package,
        )
        .inspect_err(
            |error| tracing::error!(?package, code = %error.code, %error, "生活圈查询失败"),
        )?;
        for poi in &mut circle.pois {
            poi.location = route_core::geo::convert(poi.location, Crs::Bd09, crs);
        }
        circle.crs = crs;
        Ok(circle)
    })
    .await
    .map_err(|error| join_failed(&error.to_string()))?
}

/// 回读后端日志尾部，供界面里的日志面板展示。
///
/// `limit` 是条数上限；传 0 按 200 处理 —— 前端漏传参数时给一屏可用的内容，
/// 比给一个空面板好排查。
///
/// # Errors
///
/// 日志文件存在但读不动时返回错误。
#[tauri::command]
pub async fn read_logs(limit: usize) -> Result<Vec<autobee_shell::logs::LogEntry>, AppError> {
    let limit = if limit == 0 { 200 } else { limit.min(2000) };
    tauri::async_runtime::spawn_blocking(move || autobee_shell::logs::tail(limit))
        .await
        .map_err(|error| join_failed(&error.to_string()))?
}
