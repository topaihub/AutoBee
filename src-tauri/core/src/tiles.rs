//! 底图瓦片的抓取与磁盘缓存。
//!
//! # 为什么走 Rust 而不是让 WebView 直接拉
//!
//! 1. **要能缓存**。街道瓦片服务的使用政策要求客户端做缓存、别反复拉同一块，
//!    而浏览器缓存不归我们管（清一次就没了）。落盘后离线也能看到走过的区域。
//! 2. **要能标识自己**。瓦片服务按 UA 统计与限流，`http::client` 统一设了 UA。
//! 3. **不引 CSP 例外**。走 IPC 取图，前端不必放开对外的 `img-src`。
//! 4. **Key 不外泄**。当前源免 Key，同一条通道将来换成要鉴权的源时不用改架构。
//!
//! # 缓存为什么不做过期
//!
//! 瓦片内容几乎不变（路网几个月才更新一次）。做 TTL 只会让用户重复下载。
//! 真要换底图源时目录名带源名，互不干扰。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::http::get_bytes;

/// 可用的无需额外 Key 的瓦片源：`slug` → 名称。
pub const TILE_SOURCES: &[(&str, &str)] = &[("arcgis-street", "ArcGIS 街道")];

/// 默认瓦片源。
pub const DEFAULT_TILE_SOURCE: &str = "arcgis-street";

/// 瓦片的 URL 模板。返回 `None` 表示不认识这个源。
fn template(source: &str) -> Option<&'static str> {
    match source {
        "arcgis-street" => Some(
            "https://server.arcgisonline.com/ArcGIS/rest/services/World_Street_Map/MapServer/tile/{z}/{y}/{x}",
        ),
        _ => None,
    }
}
/// 拼一个瓦片的 URL。
///
/// # Errors
///
/// 源名不认识、或 z/x/y 越界时返回错误。越界要拦在发请求之前 ——
/// 不然会对着一个不存在的瓦片发一堆 404，而那些 404 看起来像服务挂了。
pub fn tile_url(source: &str, z: u8, x: u32, y: u32) -> Result<String, AppError> {
    let tpl = template(source).ok_or_else(|| {
        AppError::new(
            "UNKNOWN_TILE_SOURCE",
            format!("不认识的瓦片源 {source}"),
            400,
        )
    })?;

    if z > 19 {
        return Err(AppError::new(
            "TILE_OUT_OF_RANGE",
            format!("缩放级别 {z} 超出范围"),
            400,
        ));
    }
    let max = 1u32 << z;
    if x >= max || y >= max {
        return Err(AppError::new(
            "TILE_OUT_OF_RANGE",
            format!("瓦片坐标 {x},{y} 超出了 z={z} 的范围"),
            400,
        ));
    }
    Ok(tpl
        .replace("{z}", &z.to_string())
        .replace("{x}", &x.to_string())
        .replace("{y}", &y.to_string()))
}

/// 缓存文件的路径。
fn cache_path(cache_dir: &Path, source: &str, z: u8, x: u32, y: u32) -> PathBuf {
    cache_dir
        .join(source)
        .join(z.to_string())
        .join(format!("{x}_{y}.png"))
}

/// 取一块瓦片。命中磁盘缓存就直接返回，否则下载后写入缓存。
///
/// # Errors
///
/// 参数非法、下载失败、或下载到空内容时返回错误。
/// 写缓存失败**不算错误** —— 缓存只是加速，写不进（只读目录、磁盘满）时
/// 照样要能把图返回给用户。
pub fn fetch_tile(
    cache_dir: &Path,
    source: &str,
    z: u8,
    x: u32,
    y: u32,
) -> Result<Vec<u8>, AppError> {
    let path = cache_path(cache_dir, source, z, x, y);
    if let Ok(bytes) = fs::read(&path)
        && !bytes.is_empty()
    {
        return Ok(bytes);
    }

    let url = tile_url(source, z, x, y)?;
    let bytes = get_bytes(&url)?;
    if bytes.is_empty() {
        return Err(AppError::network("瓦片返回了空内容"));
    }

    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        tracing::debug!(%error, "建不了瓦片缓存目录，本次不缓存");
        return Ok(bytes);
    }
    if let Err(error) = fs::write(&path, &bytes) {
        tracing::debug!(%error, "写瓦片缓存失败，本次不缓存");
    }
    Ok(bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 拼出_arcgis_街道_url() {
        let url = tile_url("arcgis-street", 12, 3349, 1683).unwrap();
        assert_eq!(
            url,
            "https://server.arcgisonline.com/ArcGIS/rest/services/World_Street_Map/MapServer/tile/12/1683/3349"
        );
    }

    #[test]
    fn z_越界被拦下() {
        assert!(tile_url("arcgis-street", 20, 0, 0).is_err());
        assert!(tile_url("arcgis-street", 1, 2, 0).is_err());
        assert!(tile_url("arcgis-street", 1, 0, 0).is_ok());
    }

    #[test]
    fn 不认识的瓦片源报错而不是拼出坏_url() {
        assert!(tile_url("nope", 1, 0, 0).is_err());
    }

    #[test]
    fn 缓存命中时不发起请求() {
        let dir = std::env::temp_dir().join(format!("route-tile-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = cache_path(&dir, "arcgis-street", 3, 1, 2);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"PNG-FAKE").unwrap();

        // 命中缓存时不应联网。
        let bytes = fetch_tile(&dir, "arcgis-street", 3, 1, 2).unwrap();
        assert_eq!(bytes, b"PNG-FAKE");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 唯一的默认瓦片源已注册() {
        assert_eq!(TILE_SOURCES.len(), 1);
        assert!(tile_url(TILE_SOURCES[0].0, 0, 0, 0).is_ok());
    }
}
