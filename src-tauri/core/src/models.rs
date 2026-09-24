//! 归一化数据契约 —— 前端只消费这里定义的结构。
//!
//! 对应 SPEC 4.5 / 11.5。所有 provider adapter 都把各自的原始响应折成这里的
//! 形状，前端不感知任何厂商细节。**新增厂商 = 新增一个 adapter，不动这里。**

use serde::{Deserialize, Serialize};

/// 经纬度。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLng {
    /// 纬度。
    pub lat: f64,
    /// 经度。
    pub lng: f64,
}

impl LatLng {
    /// 构造一个坐标。
    #[must_use]
    pub fn new(lat: f64, lng: f64) -> Self {
        Self { lat, lng }
    }

    /// 两点间的球面距离（米）。
    ///
    /// 用来做验收断言（解码出来的末点是不是落在终点附近）与前端的兜底计算，
    /// 不参与算路本身。
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

/// 坐标系。
///
/// 各家不一样：腾讯/高德 GCJ-02、百度 BD-09、天地图 CGCS2000、HERE/Google WGS-84。
/// CGCS2000 与 WGS-84 的差值在厘米级，实用上按等同处理（见 [`geo`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Crs {
    /// 世界大地坐标系，GPS 与 OSM 瓦片用的就是这个。
    Wgs84,
    /// 国测局坐标系（火星坐标），腾讯与高德。
    Gcj02,
    /// 百度坐标系，在 GCJ-02 上又加了一层偏移。
    Bd09,
    /// 中国大地坐标系 2000，天地图。
    Cgcs2000,
}

impl Crs {
    /// 解析前端或配置里的字符串。认不出返回 `None`。
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
}

/// 地图服务商。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// 腾讯位置服务。
    Tencent,
    /// 高德。
    Amap,
    /// 百度地图。
    Baidu,
    /// 天地图。
    Tianditu,
    /// HERE（海外）。
    Here,
    /// Google Maps（海外）。
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

    /// 中文显示名。
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

    /// 解析 slug。认不出返回 `None`（调用方应当跳过而不是猜测）。
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

/// 出行方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TravelMode {
    /// 只要地铁（含地铁+步行的组合，不含公交）。
    Subway,
    /// 公交优先（含公交+地铁的组合）。
    Bus,
    /// 步行。
    Walking,
}

impl TravelMode {
    /// 解析前端或模型给出的字符串。
    ///
    /// 不认识就返回 `None` 而不是猜一个默认值:调用方是大模型时,猜出来的
    /// 结果它分辨不出错,也就无法重试。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "subway" => Some(Self::Subway),
            "bus" => Some(Self::Bus),
            "walking" => Some(Self::Walking),
            _ => None,
        }
    }

    /// 全部合法值,用于错误信息里告诉调用方该填什么。
    pub const ALL: [&'static str; 3] = ["subway", "bus", "walking"];
}

/// 方案排序依据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    /// 时间最短。
    Time,
    /// 距离最短。
    Distance,
}

impl SortBy {
    /// 解析前端或模型给出的字符串。理由同 [`TravelMode::parse`]。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "time" => Some(Self::Time),
            "distance" => Some(Self::Distance),
            _ => None,
        }
    }

    /// 全部合法值。
    pub const ALL: [&'static str; 2] = ["time", "distance"];
}

/// 交通工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Vehicle {
    /// 地铁。
    Subway,
    /// 公交。
    Bus,
}

/// 一个地点（POI 或解析结果）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// 名称。
    pub title: String,
    /// 详细地址，可能缺失。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// 坐标。
    pub location: LatLng,
    /// 行政区划代码。中国境内是 6 位数字，用它判境内/境外。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adcode: Option<String>,
    /// 这条结果由哪家给出。
    pub provider: ProviderId,
    /// 该 provider 内部的 POI id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// 上下车站点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    /// 站名。
    pub title: String,
    /// 坐标。
    pub location: LatLng,
    /// 建议进站口，只有部分线路会给。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
}

/// 一条公交线路 / 地铁线路。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitLine {
    /// 地铁还是公交。
    pub vehicle: Vehicle,
    /// 线路名，如「2号线」。
    pub title: String,
    /// 乘坐站数。
    pub station_count: u32,
    /// 服务端定好的颜色，前端不再计算（配色规则只有一处实现）。
    pub color: String,
    /// 上车站点。
    pub get_on: Stop,
    /// 下车站点。
    pub get_off: Stop,
    /// 开往方向。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    /// 首班车时间。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_time: Option<String>,
    /// 末班车时间。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_time: Option<String>,
}

/// 一段行程：要么步行，要么乘车。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// 这一段是走路还是坐车。
    pub kind: SegmentKind,
    /// 距离，米。
    pub distance: u32,
    /// 耗时，分钟。
    pub duration: u32,
    /// 已解码的折线点。
    pub points: Vec<LatLng>,
    /// 步行指令。乘车段没有这个。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instruction: Vec<String>,
    /// 乘车段的线路信息。步行段没有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<TransitLine>,
}

/// 段的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    /// 步行。
    Walk,
    /// 乘车（地铁或公交）。
    Transit,
}

/// 一个候选方案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    /// 方案 id，前端选中态用。
    pub id: String,
    /// 总距离，米。
    pub distance: u32,
    /// 总耗时，分钟。
    pub duration: u32,
    /// 票价，**元**。注意腾讯原始单位是「分」，换算在 adapter 里做。
    pub price: f64,
    /// 外接矩形，前端据此自适应缩放。
    pub bounds: [LatLng; 2],
    /// 分段。
    pub segments: Vec<Segment>,
}

/// 一次规划的完整结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlan {
    /// 起点。
    pub origin: Endpoint,
    /// 终点。
    pub destination: Endpoint,
    /// 出行方式。
    pub mode: TravelMode,
    /// 候选方案，已按 [`SortBy`] 排好序。
    pub routes: Vec<Route>,
    /// 这次结果来自哪家。前端应当显示「数据来源」—— 同一对起终点在不同
    /// 厂商那里给出的方案数、时长、票价都可能不一样。
    pub provider: ProviderId,
    /// 本份结果所用的坐标系。前端底图必须与之匹配，否则整体偏移几百米。
    pub crs: Crs,
    /// 降级说明：主 provider 失败时记录尝试过的链路。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded: Option<Degraded>,
}

/// 15 分钟生活圈服务包。
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
    /// 解析前端字符串。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "care" => Some(Self::Care),
            "family" => Some(Self::Family),
            "emergency" => Some(Self::Emergency),
            _ => None,
        }
    }
}

/// 生活圈内一个经真实步行路线验证的 POI。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReachablePoi {
    /// 服务类别，例如“药店”。
    pub category: String,
    /// POI 名称。
    pub name: String,
    /// 人类可读地址。
    pub address: String,
    /// 百度 BD-09 坐标。
    pub location: LatLng,
    /// 从起点步行所需秒数。
    pub walk_duration_sec: u32,
    /// 从起点步行距离，米。
    pub walk_distance: u32,
}

/// 一个服务包的 15 分钟可达结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifeCircle {
    /// 本次服务包。
    pub package: LifeCirclePackage,
    /// 生活圈点位输出的坐标系，和请求起点保持一致。
    pub crs: Crs,
    /// 固定 15 分钟，用于前端明确展示约束。
    pub limit_minutes: u32,
    /// 按步行时间升序的可达 POI。
    pub pois: Vec<ReachablePoi>,
}

/// 起点或终点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    /// 名称（含自动纠错的结果，如「铢家村」→「钟家村」）。
    pub title: String,
    /// 坐标。
    pub location: LatLng,
}

/// 降级记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Degraded {
    /// 尝试过哪几家。
    pub tried: Vec<ProviderId>,
    /// 最后一次失败的原因。
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
        // 曾经的行为是 `_ => Subway` / else Time：模型写 "transit" 会静默
        // 得到地铁+耗时的结果，自己不知道参数错了，也就无从重试。
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
}
