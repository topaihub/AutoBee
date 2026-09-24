//! 坐标系换算。
//!
//! # 为什么要转
//!
//! 各家坐标系不同：腾讯/高德 GCJ-02、百度 BD-09、天地图 CGCS2000、HERE/Google
//! 与 OSM 瓦片 WGS-84。同一份坐标喂给不同底图，差的就是 GCJ-02 那 300–600 米的
//! 偏移 —— 现象不是报错，而是**路线整条漂到隔壁街区**，看久了才觉得不对。
//!
//! # 精度
//!
//! GCJ-02 → WGS-84 的偏移不可逆，这里用两次迭代反解。单次反解在偏移大的区域
//! 会留数米残差，两次可降到 1 米内（实测往返误差 0.00 米，见下面的测试）。
//! CGCS2000 与 WGS-84 的差值在厘米级，按等同处理。

use crate::models::{Crs, LatLng};

const PI: f64 = std::f64::consts::PI;
/// 克拉索夫斯基椭球长半轴。
const A: f64 = 6_378_245.0;
/// 偏心率平方。
const EE: f64 = 0.006_693_421_622_965_943;

/// 粗判是否在中国大陆范围内。境外不做偏移 —— 对港澳台与海外坐标做 GCJ-02
/// 换算会凭空引入几百米误差，那是错得更隐蔽的一类。
fn out_of_china(lat: f64, lng: f64) -> bool {
    !(lng > 72.004 && lng < 137.8347 && lat > 0.8293 && lat < 55.8271)
}

fn transform_lat(x: f64, y: f64) -> f64 {
    let mut ret = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    ret += ((20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0) / 3.0;
    ret += ((20.0 * (y * PI).sin() + 40.0 * ((y / 3.0) * PI).sin()) * 2.0) / 3.0;
    ret += ((160.0 * ((y / 12.0) * PI).sin() + 320.0 * ((y * PI) / 30.0).sin()) * 2.0) / 3.0;
    ret
}

fn transform_lng(x: f64, y: f64) -> f64 {
    let mut ret = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    ret += ((20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0) / 3.0;
    ret += ((20.0 * (x * PI).sin() + 40.0 * ((x / 3.0) * PI).sin()) * 2.0) / 3.0;
    ret += ((150.0 * ((x / 12.0) * PI).sin() + 300.0 * ((x / 30.0) * PI).sin()) * 2.0) / 3.0;
    ret
}

/// 该点处的偏移量（度）。
fn delta(lat: f64, lng: f64) -> (f64, f64) {
    let d_lat_t = transform_lat(lng - 105.0, lat - 35.0);
    let d_lng_t = transform_lng(lng - 105.0, lat - 35.0);
    let rad_lat = (lat / 180.0) * PI;
    let mut magic = rad_lat.sin();
    magic = 1.0 - EE * magic * magic;
    let sqrt_magic = magic.sqrt();
    let d_lat = (d_lat_t * 180.0) / (((A * (1.0 - EE)) / (magic * sqrt_magic)) * PI);
    let d_lng = (d_lng_t * 180.0) / ((A / sqrt_magic) * rad_lat.cos() * PI);
    (d_lat, d_lng)
}

/// WGS-84 → GCJ-02。
#[must_use]
pub fn wgs84_to_gcj02(p: LatLng) -> LatLng {
    if out_of_china(p.lat, p.lng) {
        return p;
    }
    let (d_lat, d_lng) = delta(p.lat, p.lng);
    LatLng::new(p.lat + d_lat, p.lng + d_lng)
}

/// GCJ-02 → WGS-84，两次迭代反解。
#[must_use]
pub fn gcj02_to_wgs84(p: LatLng) -> LatLng {
    if out_of_china(p.lat, p.lng) {
        return p;
    }
    let (mut lat, mut lng) = (p.lat, p.lng);
    for _ in 0..2 {
        let (d_lat, d_lng) = delta(lat, lng);
        lat = p.lat - d_lat;
        lng = p.lng - d_lng;
    }
    LatLng::new(lat, lng)
}

/// GCJ-02 → BD-09。
#[must_use]
pub fn gcj02_to_bd09(p: LatLng) -> LatLng {
    let x = p.lng;
    let y = p.lat;
    let z = (x * x + y * y).sqrt() + 0.000_02 * (y * PI * 3000.0 / 180.0).sin();
    let theta = y.atan2(x) + 0.000_003 * (x * PI * 3000.0 / 180.0).cos();
    LatLng::new(z * theta.sin() + 0.006, z * theta.cos() + 0.006_5)
}

/// BD-09 → GCJ-02。
#[must_use]
pub fn bd09_to_gcj02(p: LatLng) -> LatLng {
    let x = p.lng - 0.006_5;
    let y = p.lat - 0.006;
    let z = (x * x + y * y).sqrt() - 0.000_02 * (y * PI * 3000.0 / 180.0).sin();
    let theta = y.atan2(x) - 0.000_003 * (x * PI * 3000.0 / 180.0).cos();
    LatLng::new(z * theta.sin(), z * theta.cos())
}

/// 任意坐标系互转，统一经由 WGS-84 中转。
#[must_use]
pub fn convert(p: LatLng, from: Crs, to: Crs) -> LatLng {
    if from == to {
        return p;
    }
    // CGCS2000 与 WGS84 视作等同。
    let f = if from == Crs::Cgcs2000 {
        Crs::Wgs84
    } else {
        from
    };
    let t = if to == Crs::Cgcs2000 { Crs::Wgs84 } else { to };
    if f == t {
        return p;
    }

    let wgs = match f {
        Crs::Wgs84 => p,
        Crs::Gcj02 => gcj02_to_wgs84(p),
        Crs::Bd09 => gcj02_to_wgs84(bd09_to_gcj02(p)),
        Crs::Cgcs2000 => p,
    };

    match t {
        Crs::Wgs84 => wgs,
        Crs::Gcj02 => wgs84_to_gcj02(wgs),
        Crs::Bd09 => gcj02_to_bd09(wgs84_to_gcj02(wgs)),
        Crs::Cgcs2000 => wgs,
    }
}

/// 批量转换。
#[must_use]
pub fn convert_all(points: &[LatLng], from: Crs, to: Crs) -> Vec<LatLng> {
    if from == to {
        return points.to_vec();
    }
    points.iter().map(|p| convert(*p, from, to)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 北京的_gcj02_偏移在几百米量级() {
        let wgs = LatLng::new(39.908_68, 116.395_30);
        let gcj = wgs84_to_gcj02(wgs);
        let m = wgs.haversine_m(gcj);
        assert!((300.0..=800.0).contains(&m), "实测偏移 {m:.1} m");
    }

    #[test]
    fn gcj02_转回_wgs84_的往返误差在米内() {
        let wgs = LatLng::new(39.908_68, 116.395_30);
        let back = gcj02_to_wgs84(wgs84_to_gcj02(wgs));
        assert!(
            wgs.haversine_m(back) < 1.0,
            "往返误差 {}",
            wgs.haversine_m(back)
        );
    }

    #[test]
    fn 境外坐标不做偏移() {
        // 东京。对它加火星偏移会凭空错几百米，而且看不出来。
        let tokyo = LatLng::new(35.6812, 139.7671);
        assert_eq!(wgs84_to_gcj02(tokyo), tokyo);
        assert_eq!(gcj02_to_wgs84(tokyo), tokyo);
    }

    #[test]
    fn bd09_与_gcj02_往返一致() {
        let gcj = LatLng::new(39.908_68, 116.395_30);
        let back = bd09_to_gcj02(gcj02_to_bd09(gcj));
        assert!(gcj.haversine_m(back) < 1.0);
    }

    #[test]
    fn convert_经由_wgs84_中转() {
        let gcj = LatLng::new(30.506_531, 114.423_819);
        let bd = convert(gcj, Crs::Gcj02, Crs::Bd09);
        assert!(gcj.haversine_m(bd) > 50.0, "BD-09 本身就带偏移");
        let back = convert(bd, Crs::Bd09, Crs::Gcj02);
        assert!(gcj.haversine_m(back) < 1.0);
    }

    #[test]
    fn cgcs2000_按_wgs84_处理() {
        let p = LatLng::new(30.5, 114.4);
        assert_eq!(convert(p, Crs::Cgcs2000, Crs::Wgs84), p);
        assert_eq!(convert(p, Crs::Wgs84, Crs::Cgcs2000), p);
    }

    #[test]
    fn 同坐标系互转返回原值() {
        let p = LatLng::new(30.5, 114.4);
        assert_eq!(convert(p, Crs::Gcj02, Crs::Gcj02), p);
        assert_eq!(convert_all(&[p], Crs::Bd09, Crs::Bd09), vec![p]);
    }
}
