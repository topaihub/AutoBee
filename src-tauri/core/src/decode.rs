//! polyline 解码与配色。
//!
//! # 腾讯的压缩格式（最容易写错的一处，已实测）
//!
//! **第 1 对是绝对经纬度（单位：度），从下标 2 开始每一对是增量，单位 1e-6 度。**
//!
//! 实测样本（武汉光谷大道 → 钟家村）开头是 `[30.506531, 114.423819, -2, -2, ...]`：
//! 前两个是浮点度数，后面是整数增量。
//!
//! **错误写法**是把首对也当增量从 0 累加 —— 整条线会缩到原点附近。
//! 那种现象不是报错，而是地图上出现一条从几内亚湾出发的线，
//! 第一眼很容易当成底图坏了。

use crate::models::LatLng;

/// 还原差分压缩的坐标。
///
/// 空数组或长度不足时返回空 —— 调用方按「没有折线」处理，
/// 而不是画一条只有起点的线。
#[must_use]
pub fn decode_polyline(pl: &[f64]) -> Vec<LatLng> {
    let mut out = Vec::with_capacity(pl.len() / 2 + 1);
    if pl.len() < 2 {
        return out;
    }
    let mut lat = pl[0];
    let mut lng = pl[1];
    out.push(LatLng::new(lat, lng));
    let mut i = 2;
    while i + 1 < pl.len() {
        lat += pl[i] / 1e6;
        lng += pl[i + 1] / 1e6;
        out.push(LatLng::new(lat, lng));
        i += 2;
    }
    out
}

/// 解析 `lat,lng` 文本。非法返回 `None`。
///
/// 允许两端与逗号两侧的空白；越界的经纬度返回 `None`（那是用户粘错了顺序，
/// 静默接受会得到一条跨越半个地球的路线）。
#[must_use]
pub fn parse_lat_lng(text: &str) -> Option<LatLng> {
    let (lat_s, lng_s) = text.trim().split_once(',')?;
    let lat: f64 = lat_s.trim().parse().ok()?;
    let lng: f64 = lng_s.trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return None;
    }
    Some(LatLng::new(lat, lng))
}

/// 地铁配色，按顺序轮流取用。
const SUBWAY_PALETTE: [&str; 7] = [
    "#5b6ee1", "#e8543f", "#2ba471", "#f2a33c", "#8e5bd4", "#00a0b8", "#d4488f",
];

/// 公交段的颜色。
pub const BUS_COLOR: &str = "#2ba471";
/// 步行段的颜色。
pub const WALK_COLOR: &str = "#9aa0a6";

/// 第 `index` 条地铁线路的颜色。
///
/// 服务端定色而不是前端算：换一次配色只改这里，前端与导出图不会各说一套。
#[must_use]
pub fn subway_color(index: usize) -> &'static str {
    SUBWAY_PALETTE[index % SUBWAY_PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 首对是绝对度数_其后是增量() {
        // 与实测样本同形：30.506531,114.423819 起，之后 -2,-2 表示各减 2e-6 度。
        let pl = [30.506_531_f64, 114.423_819, -2.0, -2.0];
        let out = decode_polyline(&pl);
        assert_eq!(out.len(), 2);
        assert!((out[0].lat - 30.506_531).abs() < 1e-9);
        assert!((out[1].lng - (114.423_819 - 2e-6)).abs() < 1e-9);
    }

    #[test]
    fn 空输入返回空而不是崩溃() {
        assert!(decode_polyline(&[]).is_empty());
        assert!(decode_polyline(&[1.0]).is_empty());
    }

    #[test]
    fn 奇数长度丢掉最后那个落单的值() {
        // 不能 panic：那会让一次算路整体失败，而现象只是少画一个点。
        assert_eq!(decode_polyline(&[1.0, 2.0, 3.0]).len(), 1);
    }

    #[test]
    fn 解析坐标文本() {
        assert_eq!(
            parse_lat_lng("39.90868,116.39530"),
            Some(LatLng::new(39.90868, 116.39530))
        );
        assert_eq!(
            parse_lat_lng(" 30.5 , 114.4 "),
            Some(LatLng::new(30.5, 114.4))
        );
    }

    #[test]
    fn 非法坐标返回_none() {
        assert_eq!(parse_lat_lng("武汉"), None);
        assert_eq!(parse_lat_lng("30.5"), None);
        assert_eq!(parse_lat_lng("999,114"), None, "越界的纬度");
        assert_eq!(parse_lat_lng(""), None);
    }

    #[test]
    fn 地铁配色循环取用() {
        assert_eq!(subway_color(0), "#5b6ee1");
        assert_eq!(subway_color(7), subway_color(0), "第 8 条回到第一个颜色");
    }
}
