//! 归一化数据契约（SPEC 4.5 / 11.5）—— 前端只消费这里定义的结构，不感知任何
//! 厂商细节。**新增厂商 = 新增一个 adapter，不动这里。**

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLng {
    pub lat: f64,
    pub lng: f64,
}

impl LatLng {
    #[must_use]
    pub fn new(lat: f64, lng: f64) -> Self {
        Self { lat, lng }
    }

    /// 两点间的球面距离（米）。不参与算路，只用于验收断言与前端兜底。
    #[must_use]
    pub fn haversine_m(self, other: Self) -> f64 {
        const R: f64 = 6_371_008.8;
        let d_lat = (other.lat - self.lat).to_radians();
        let d_lng = (other.lng - self.lng).to_radians();
        let a = (d_lat / 2.0).sin().powi(2)
            + self.lat.to_radians().cos()
                * other.lat.to_radians().cos()
                * (d_lng / 2.0).sin().powi(2);
        2.0 * R * a.sqrt().asin()
    }
}

/// 各家不一样：腾讯/高德 GCJ-02、百度 BD-09、天地图 CGCS2000、HERE/Google WGS-84。
/// CGCS2000 与 WGS-84 的差值在厘米级，实用上按等同处理（见 [`geo`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Crs {
    Wgs84,
    Gcj02,
    Bd09,
    Cgcs2000,
}

impl Crs {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "WGS84" => Some(Self::Wgs84),
            "GCJ02" => Some(Self::Gcj02),
            "BD09" => Some(Self::Bd09),
            "CGCS2000" => Some(Self::Cgcs2000),
            _ => None,
        }
    }

    pub const ALL: [&'static str; 4] = ["WGS84", "GCJ02", "BD09", "CGCS2000"];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Tencent,
    Amap,
    Baidu,
    Tianditu,
    Here,
    Google,
}

impl ProviderId {
    /// 档案库里的 slug，也是前端下拉的 value。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tencent => "tencent",
            Self::Amap => "amap",
            Self::Baidu => "baidu",
            Self::Tianditu => "tianditu",
            Self::Here => "here",
            Self::Google => "google",
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Tencent => "腾讯位置服务",
            Self::Amap => "高德地图",
            Self::Baidu => "百度地图",
            Self::Tianditu => "天地图",
            Self::Here => "HERE",
            Self::Google => "Google Maps",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "tencent" => Some(Self::Tencent),
            "amap" => Some(Self::Amap),
            "baidu" => Some(Self::Baidu),
            "tianditu" => Some(Self::Tianditu),
            "here" => Some(Self::Here),
            "google" => Some(Self::Google),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TravelMode {
    /// 只要地铁（含地铁+步行的组合，不含公交）。
    Subway,
    /// 公交优先（含公交+地铁的组合）。
    Bus,
    Walking,
}

impl TravelMode {
    /// 不认识返回 `None` 而不是猜默认值:调用方是大模型时,分辨不出猜错,无法重试。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "subway" => Some(Self::Subway),
            "bus" => Some(Self::Bus),
            "walking" => Some(Self::Walking),
            _ => None,
        }
    }

    pub const ALL: [&'static str; 3] = ["subway", "bus", "walking"];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    Time,
    Distance,
}

impl SortBy {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "time" => Some(Self::Time),
            "distance" => Some(Self::Distance),
            _ => None,
        }
    }

    pub const ALL: [&'static str; 2] = ["time", "distance"];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Vehicle {
    Subway,
    Bus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    pub location: LatLng,
    /// 行政区划代码。中国境内是 6 位数字，用它判境内/境外。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adcode: Option<String>,
    pub provider: ProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub title: String,
    pub location: LatLng,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitLine {
    pub vehicle: Vehicle,
    pub title: String,
    pub station_count: u32,
    pub color: String,
    pub get_on: Stop,
    pub get_off: Stop,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub kind: SegmentKind,
    /// 距离，米。
    pub distance: u32,
    /// 耗时，分钟。
    pub duration: u32,
    pub points: Vec<LatLng>,
    /// 步行指令，乘车段为空。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instruction: Vec<String>,
    /// 乘车段的线路信息，步行段为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<TransitLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    Walk,
    Transit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub id: String,
    /// 总距离，米。
    pub distance: u32,
    /// 总耗时，分钟。
    pub duration: u32,
    /// 票价，**元**。注意腾讯原始单位是「分」，换算在 adapter 里做。
    pub price: f64,
    pub bounds: [LatLng; 2],
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlan {
    pub origin: Endpoint,
    pub destination: Endpoint,
    pub mode: TravelMode,
    /// 候选方案，已按 [`SortBy`] 排好序。
    pub routes: Vec<Route>,
    /// 这次结果来自哪家。不同厂商的方案数、时长、票价都可能不同，前端应显示「数据来源」。
    pub provider: ProviderId,
    /// 本份结果所用的坐标系。前端底图必须与之匹配，否则整体偏移几百米。
    pub crs: Crs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded: Option<Degraded>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifeCirclePackage {
    /// 医院、药店、卫生间。
    Care,
    /// 公园、便利店、地铁站。
    Family,
    /// 医院、药店、警务服务。
    Emergency,
}

impl LifeCirclePackage {
    /// 解析前端或命令行给出的字符串。认不出返回 `None`。trim + 忽略大小写,
    /// 与 [`TravelMode::parse`] / [`SortBy::parse`] / [`Crs::parse`] 一致。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "care" => Some(Self::Care),
            "family" => Some(Self::Family),
            "emergency" => Some(Self::Emergency),
            _ => None,
        }
    }

    /// 全部合法值,与 [`Self::parse`] 一一对应。理由同 [`Crs::ALL`]。
    pub const ALL: [&'static str; 3] = ["care", "family", "emergency"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReachablePoi {
    /// 服务类别，例如“药店”。
    pub category: String,
    pub name: String,
    pub address: String,
    /// 百度 BD-09 坐标。
    pub location: LatLng,
    pub walk_duration_sec: u32,
    /// 从起点步行距离，米。
    pub walk_distance: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifeCircle {
    pub package: LifeCirclePackage,
    /// 生活圈点位输出的坐标系，和请求起点保持一致。
    pub crs: Crs,
    /// 固定 15 分钟，前端据此展示约束。
    pub limit_minutes: u32,
    /// 按步行时间升序的可达 POI。
    pub pois: Vec<ReachablePoi>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    /// 名称（含自动纠错的结果，如「铢家村」→「钟家村」）。
    pub title: String,
    pub location: LatLng,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Degraded {
    pub tried: Vec<ProviderId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 北京到上海的距离约一千公里() {
        let beijing = LatLng::new(39.9087, 116.3975);
        let shanghai = LatLng::new(31.2304, 121.4737);
        let km = beijing.haversine_m(shanghai) / 1000.0;
        assert!((1050.0..=1090.0).contains(&km), "实测 {km:.1} km");
    }

    #[test]
    fn 相邻两点距离很小() {
        // 验收解码时会用到：15 米以内算「落在同一个点」。
        let a = LatLng::new(30.506_531, 114.423_819);
        let b = LatLng::new(30.506_600, 114.423_900);
        assert!(a.haversine_m(b) < 15.0);
    }

    #[test]
    fn provider_slug_往返() {
        for id in [
            ProviderId::Tencent,
            ProviderId::Amap,
            ProviderId::Baidu,
            ProviderId::Tianditu,
            ProviderId::Here,
            ProviderId::Google,
        ] {
            assert_eq!(ProviderId::parse(id.as_str()), Some(id));
        }
        assert_eq!(ProviderId::parse("TENCENT"), Some(ProviderId::Tencent));
        assert_eq!(ProviderId::parse("nope"), None);
    }

    #[test]
    fn crs_的_serde_名与前端一致() {
        // 前端按大写字符串判等，写错的现象是底图整体偏移而不报错。
        assert_eq!(serde_json::to_string(&Crs::Wgs84).unwrap(), "\"WGS84\"");
        assert_eq!(serde_json::to_string(&Crs::Gcj02).unwrap(), "\"GCJ02\"");
        assert_eq!(serde_json::to_string(&Crs::Bd09).unwrap(), "\"BD09\"");
        assert_eq!(
            serde_json::to_string(&Crs::Cgcs2000).unwrap(),
            "\"CGCS2000\""
        );
    }

    #[test]
    fn 出行方式与排序的_serde_名() {
        assert_eq!(
            serde_json::to_string(&TravelMode::Subway).unwrap(),
            "\"subway\""
        );
        assert_eq!(
            serde_json::to_string(&SortBy::Distance).unwrap(),
            "\"distance\""
        );
        assert_eq!(
            serde_json::to_string(&Vehicle::Subway).unwrap(),
            "\"SUBWAY\""
        );
    }

    #[test]
    fn 出行方式与排序拒绝不认识的值而不是猜一个() {
        // 猜默认值会让调用方（大模型）静默拿到错结果，自己无从重试。
        assert_eq!(TravelMode::parse("transit"), None);
        assert_eq!(TravelMode::parse("地铁"), None);
        assert_eq!(SortBy::parse("fastest"), None);
        assert_eq!(SortBy::parse(""), None);

        // serde 名即合法值，两者必须能对上。
        for (text, mode) in [
            ("subway", TravelMode::Subway),
            ("bus", TravelMode::Bus),
            ("walking", TravelMode::Walking),
        ] {
            assert_eq!(TravelMode::parse(text), Some(mode));
            assert!(TravelMode::ALL.contains(&text));
        }
        assert_eq!(SortBy::parse(" Distance "), Some(SortBy::Distance));
    }

    /// `ALL` 与 `parse` 漂开的现象是:报错说「可选:a / b」,而填 b 被拒。
    #[test]
    fn 四个枚举的_all_与_parse_一一对应() {
        for text in TravelMode::ALL {
            assert!(
                TravelMode::parse(text).is_some(),
                "TravelMode 认不出 {text}"
            );
        }
        for text in SortBy::ALL {
            assert!(SortBy::parse(text).is_some(), "SortBy 认不出 {text}");
        }
        for text in Crs::ALL {
            assert!(Crs::parse(text).is_some(), "Crs 认不出 {text}");
        }
        for text in LifeCirclePackage::ALL {
            assert!(
                LifeCirclePackage::parse(text).is_some(),
                "LifeCirclePackage 认不出 {text}"
            );
        }

        // 反向:大小写与空白不该改变结果(四个 parse 都 trim + 归一化)。
        assert_eq!(Crs::parse(" gcj02 "), Crs::parse("GCJ02"));
        assert_eq!(
            LifeCirclePackage::parse(" Care "),
            LifeCirclePackage::parse("care")
        );
    }
}
