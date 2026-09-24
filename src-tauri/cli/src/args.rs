//! 参数解析:`&[String]` → [`Command`]。
//!
//! # 为什么手写而不引 clap
//!
//! 不是为了少几个依赖(lock 里已有 521 个包,多 8 个不决定性)。真正的理由是
//! `--help` 的内容来源:合法值必须从 `TravelMode::ALL` 等常量打印,不许抄一份。
//! 用 clap 的 derive 做这件事要么给 `route-core` 的类型实现 `clap::ValueEnum`
//! (等于把 CLI 框架依赖装进算路内核),要么在这边写 custom value parser
//! (那已经把唯一有意思的部分手写完了)。见 §6.2。
//!
//! # 纯函数
//!
//! [`parse`] 不读 env、不碰磁盘,所以单测可以直接喂数组。

use route_core::error::AppError;
use route_core::models::{Crs, LifeCirclePackage, SortBy, TravelMode};

/// 一条已解析的命令。
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// 算路。
    Route {
        /// 起点地名或坐标。
        from: String,
        /// 终点地名或坐标。
        to: String,
        /// 出行方式。
        mode: TravelMode,
        /// 排序依据。
        sort: SortBy,
        /// 输出坐标系。
        out_crs: Crs,
        /// 是否输出 JSON。
        json: bool,
    },
    /// 地点联想。
    Suggest {
        /// 关键词。
        keyword: String,
        /// 返回条数上限。
        limit: usize,
        /// 输出坐标系。
        out_crs: Crs,
        /// 是否输出 JSON。
        json: bool,
    },
    /// 15 分钟生活圈。
    Circle {
        /// 中心点。
        at: route_core::models::LatLng,
        /// 入参坐标系。
        crs: Crs,
        /// 服务包。
        package: LifeCirclePackage,
        /// 输出坐标系。
        out_crs: Crs,
        /// 是否输出 JSON。
        json: bool,
    },
    /// 打印用法。
    Help,
    /// 打印版本号。
    Version,
}

/// 用法错误。`code` 用 `BAD_USAGE`,退出码归 2(见 [`exit_code`])。
fn usage(message: impl Into<String>) -> AppError {
    AppError::new("BAD_USAGE", message, 400)
}

/// 认不出的枚举值。合法值从 `ALL` 拼,不写字面量列表。
fn bad_value(code: &str, flag: &str, got: &str, allowed: &[&str]) -> AppError {
    AppError::new(
        code,
        format!("{flag} 不认识的值「{got}」,可选:{}", allowed.join(" / ")),
        400,
    )
}

/// 一趟分词的产物:flag → 值。
///
/// 只做分词,不认识哪个 flag 属于哪个子命令 —— 那是 [`parse`] 的事。
/// 这样 `--flag value` / `--flag=value` / `--` 三条规则只实现一次。
struct Flags {
    seen: Vec<(String, String)>,
}

impl Flags {
    /// 取一个 flag 的值;没给过返回 `None`。
    fn get(&self, name: &str) -> Option<&str> {
        self.seen
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// 取必填 flag,缺了就报错并**点名缺的那个** —— 只说「参数错误」要人猜。
    fn require(&self, name: &str) -> Result<&str, AppError> {
        self.get(name)
            .ok_or_else(|| usage(format!("缺少必填参数 {name}")))
    }

    /// 有没有给过这个开关。
    fn has(&self, name: &str) -> bool {
        self.seen.iter().any(|(key, _)| key == name)
    }
}

/// 不带值的开关。给它们记一个空串,`has` 据此判断。
const SWITCHES: [&str; 2] = ["--json", "--quiet"];

/// 分词。
///
/// 四条规则各只在这里实现一次:
/// - `--flag value` 与 `--flag=value` 都认
/// - `--` 之后的一个 token 按字面取,不当 flag(地名可能以 `-` 开头,
///   例如 `--from -- --怪地名`)
/// - 未知 flag 报错,不静默忽略
/// - 重复 flag 报错,不静默取最后一个
fn tokenize(argv: &[String], known: &[&str]) -> Result<Flags, AppError> {
    let mut flags = Flags { seen: Vec::new() };
    let mut index = 0;

    while index < argv.len() {
        let token = &argv[index];
        index += 1;

        if !token.starts_with("--") || token == "--" {
            return Err(usage(format!(
                "多余的参数「{token}」。所有输入都要带 flag,用 --help 看用法"
            )));
        }

        let (name, inline) = match token.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (token.clone(), None),
        };

        // 未知 flag 报错而不是忽略:静默忽略等于让打错字的人拿到默认参数的
        // 结果 —— 与 `_ => TravelMode::Subway` 那个已修掉的错误同一类。
        if !known.contains(&name.as_str()) {
            return Err(usage(format!("不认识的参数 {name}。用 --help 看用法")));
        }
        // 重复报错而不是取最后一个:取最后一个时写错的人不知道前一个被丢了。
        if flags.has(&name) {
            return Err(usage(format!("参数 {name} 给了多次")));
        }

        let value = if SWITCHES.contains(&name.as_str()) {
            if inline.is_some() {
                return Err(usage(format!("{name} 是开关,不接值")));
            }
            String::new()
        } else if let Some(value) = inline {
            value
        } else {
            let mut next = argv
                .get(index)
                .ok_or_else(|| usage(format!("{name} 后面缺一个值")))?;
            index += 1;
            // `--` 转义:后面那个 token 按字面取。地名以 `-` 开头时唯一的出路。
            if next == "--" {
                next = argv
                    .get(index)
                    .ok_or_else(|| usage(format!("{name} 的 -- 后面缺一个值")))?;
                index += 1;
            }
            next.clone()
        };
        flags.seen.push((name, value));
    }

    Ok(flags)
}

/// 全局 flag,三个子命令都认。
const GLOBAL: [&str; 3] = ["--json", "--quiet", "--out-crs"];

/// 三个默认值。与图形界面一致(§4.3),`--help` 也打印它们 ——
/// 提成常量是为了让「默认是什么」只有一个来源:写两遍时改一处漏一处,
/// 现象是 help 说默认 subway 而实际走了别的。
///
/// `--out-crs` 的默认值不在这里:它要与 `Settings::default().out_crs` 同源,
/// 见 [`out_crs_of`]。
const DEFAULT_MODE: TravelMode = TravelMode::Subway;
/// 见 [`DEFAULT_MODE`]。
const DEFAULT_SORT: SortBy = SortBy::Time;
/// 见 [`DEFAULT_MODE`]。
const DEFAULT_PACKAGE: LifeCirclePackage = LifeCirclePackage::Care;

/// 取 `--out-crs`,缺省用 `Settings::default().out_crs`。
///
/// 默认值**不写字面量**:同一个设置的默认值只许有一个来源。本项目在这一点上
/// 真实踩过坑(GUI 侧 `resolve.rs` 的回落曾与 `Settings::default()` 分叉,
/// 现象是底图与路线差一个坐标系、偏移几百米且不报错),CLI 不许再开一个分叉。
fn out_crs_of(flags: &Flags) -> Result<Crs, AppError> {
    match flags.get("--out-crs") {
        None => Ok(route_core::config::Settings::default().out_crs),
        Some(text) => {
            Crs::parse(text).ok_or_else(|| bad_value("BAD_CRS", "--out-crs", text, &Crs::ALL))
        }
    }
}

/// 解析一条命令。纯函数:不读 env、不碰磁盘。
///
/// # Errors
///
/// 用法错误、认不出的枚举值、缺必填参数时返回错误。`--help` / `--version`
/// 是正常分支而不是错误 —— 它们返回 [`Command::Help`] / [`Command::Version`],
/// 退出码 0。返回非 0 会让 CI 把 `--help` 判成失败。
pub fn parse(argv: &[String]) -> Result<Command, AppError> {
    // help / version 在任何位置都优先,不要求带子命令。
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Command::Help);
    }
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        return Ok(Command::Version);
    }

    let Some((sub, rest)) = argv.split_first() else {
        return Ok(Command::Help);
    };

    match sub.as_str() {
        "route" => parse_route(rest),
        "suggest" => parse_suggest(rest),
        "circle" => parse_circle(rest),
        other => Err(usage(format!(
            "不认识的子命令「{other}」,可选:route / suggest / circle"
        ))),
    }
}

/// `route --from <地点> --to <地点> [--mode <方式>] [--sort <依据>]`
fn parse_route(argv: &[String]) -> Result<Command, AppError> {
    let mut known = vec!["--from", "--to", "--mode", "--sort"];
    known.extend_from_slice(&GLOBAL);
    let flags = tokenize(argv, &known)?;

    let mode = match flags.get("--mode") {
        None => DEFAULT_MODE,
        Some(text) => TravelMode::parse(text)
            .ok_or_else(|| bad_value("BAD_MODE", "--mode", text, &TravelMode::ALL))?,
    };
    let sort = match flags.get("--sort") {
        None => DEFAULT_SORT,
        Some(text) => SortBy::parse(text)
            .ok_or_else(|| bad_value("BAD_SORT", "--sort", text, &SortBy::ALL))?,
    };

    Ok(Command::Route {
        from: flags.require("--from")?.to_string(),
        to: flags.require("--to")?.to_string(),
        mode,
        sort,
        out_crs: out_crs_of(&flags)?,
        json: flags.has("--json"),
    })
}

/// `suggest --keyword <关键词> [--limit <n>]`
fn parse_suggest(argv: &[String]) -> Result<Command, AppError> {
    let mut known = vec!["--keyword", "--limit"];
    known.extend_from_slice(&GLOBAL);
    let flags = tokenize(argv, &known)?;

    let limit = match flags.get("--limit") {
        None => 10,
        Some(text) => text
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| usage(format!("--limit 要一个正整数,收到「{text}」")))?,
    };

    Ok(Command::Suggest {
        keyword: flags.require("--keyword")?.to_string(),
        limit,
        out_crs: out_crs_of(&flags)?,
        json: flags.has("--json"),
    })
}

/// `circle --at <lat,lng> [--crs <坐标系>] [--package <服务包>]`
fn parse_circle(argv: &[String]) -> Result<Command, AppError> {
    let mut known = vec!["--at", "--crs", "--package"];
    known.extend_from_slice(&GLOBAL);
    let flags = tokenize(argv, &known)?;

    let at_text = flags.require("--at")?;
    // 用 route-core 的解析,不自己写:它已经挡掉了越界经纬度(粘错顺序时
    // 静默接受会得到一条跨越半个地球的结果)。
    let at = route_core::decode::parse_lat_lng(at_text)
        .ok_or_else(|| usage(format!("--at 要 `纬度,经度` 两个数,收到「{at_text}」")))?;

    let crs = match flags.get("--crs") {
        None => route_core::config::Settings::default().out_crs,
        Some(text) => {
            Crs::parse(text).ok_or_else(|| bad_value("BAD_CRS", "--crs", text, &Crs::ALL))?
        }
    };
    let package = match flags.get("--package") {
        None => DEFAULT_PACKAGE,
        Some(text) => LifeCirclePackage::parse(text)
            .ok_or_else(|| bad_value("BAD_PACKAGE", "--package", text, &LifeCirclePackage::ALL))?,
    };

    Ok(Command::Circle {
        at,
        crs,
        package,
        out_crs: out_crs_of(&flags)?,
        json: flags.has("--json"),
    })
}

/// 错误码 → 进程退出码。
///
/// **按 `code` 映射,不按 `status`。** `AppError.status` 不够分档:`BAD_MODE`
/// 与 `BAIDU_KEY_REQUIRED` 都是 400,前者要人改命令、后者要人去填 AK,
/// 落在同一档就把两件事混了。
///
/// 分档而不是「非 0 即错」,是因为调用方的处置完全不同:2 要改命令,
/// 4 要去设置页填 Key,5 值得重试,3 重试一万次也没用。
#[must_use]
pub fn exit_code(code: &str) -> i32 {
    match code {
        // 参数错(用法问题)
        "BAD_USAGE" | "BAD_MODE" | "BAD_SORT" | "BAD_CRS" | "BAD_PACKAGE" => 2,
        // 输入无解(用法没错)
        "RESOLVE_FAILED" | "NO_ROUTE" | "CROSS_BORDER" => 3,
        // 未配置:两者同属「去设置页填东西」,不许落到不同档
        "NO_PROVIDER" | "BAIDU_KEY_REQUIRED" => 4,
        // 上游失败,值得重试
        "UPSTREAM" | "NETWORK" | "ALL_PROVIDERS_FAILED" => 5,
        // 兜底
        _ => 1,
    }
}

/// 枚举值的小写名。四个 `parse` 都 trim + 忽略大小写,所以这与它们认的一致。
///
/// 用 `Debug` 而不是手写 match:手写的那份会在加枚举值时被忘掉,
/// 而 `Debug` 是编译器生成的。
fn lower(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}").to_ascii_lowercase()
}

/// `--help` 的正文。
///
/// 合法值全部从 `ALL` 常量拼出来,一个字面量列表都不写 —— 抄一份的话它不会
/// 随 `route-core` 一起改,于是 help 里列的值与真正认的值会漂开。
#[must_use]
pub fn help_text() -> String {
    let default_crs =
        format!("{:?}", route_core::config::Settings::default().out_crs).to_ascii_uppercase();
    format!(
        "AutoBee 命令行 —— 算路、地点联想、15 分钟生活圈

用法:
  autobee-cli route   --from <地点> --to <地点> [--mode <方式>] [--sort <依据>]
  autobee-cli suggest --keyword <关键词> [--limit <n>]
  autobee-cli circle  --at <纬度,经度> [--crs <坐标系>] [--package <服务包>]

全局:
  --json              输出 JSON。stdout 只有结果,可直接接 jq
  --out-crs <坐标系>  输出坐标系,默认 {default_crs}
  --quiet             少打日志(日志一律走 stderr)
  --help, -h          打印这份用法
  --version, -V       打印版本号

合法值:
  --mode     {mode}(默认 {mode_default})
  --sort     {sort}(默认 {sort_default})
  --crs      {crs}
  --package  {package}(默认 {package_default})

退出码:
  0 成功   2 参数错   3 输入无解   4 未配置(去设置页填 Key)
  5 上游失败(可重试)   1 其他

密钥与图形界面共用同一份档案库,CLI 不读环境变量里的 Key。
DPAPI 是按 Windows 账户绑定的:换账户跑需要重填。
",
        mode = TravelMode::ALL.join(" / "),
        mode_default = lower(&DEFAULT_MODE),
        sort = SortBy::ALL.join(" / "),
        sort_default = lower(&DEFAULT_SORT),
        crs = Crs::ALL.join(" / "),
        package = LifeCirclePackage::ALL.join(" / "),
        package_default = lower(&DEFAULT_PACKAGE),
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "测试里断言失败即测试失败"
)]
mod tests {
    use super::*;

    /// 把字面量数组变成 `parse` 要的 `&[String]`。
    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn 两种写法都认() {
        // 两条分词路径(split_once('=') 与取下一个 token),容易只写对一条。
        let spaced = parse(&argv(&[
            "route", "--from", "a", "--to", "b", "--mode", "walking",
        ]))
        .unwrap();
        let inline = parse(&argv(&["route", "--from=a", "--to=b", "--mode=walking"])).unwrap();
        assert_eq!(spaced, inline);

        let Command::Route { mode, .. } = spaced else {
            panic!("应该解析成 route");
        };
        assert_eq!(mode, TravelMode::Walking);
    }

    #[test]
    fn 未知_flag_报错而不是忽略() {
        // 静默忽略等于让打错字的人拿到默认参数的结果,自己不知道参数没生效。
        let error = parse(&argv(&["route", "--from", "a", "--to", "b", "--nonsense"])).unwrap_err();
        assert_eq!(exit_code(&error.code), 2);
        assert!(
            error.message.contains("--nonsense"),
            "报错要点名是哪个参数:{}",
            error.message
        );
    }

    #[test]
    fn 重复_flag_报错() {
        // 静默取最后一个时,写错的人不知道前一个被丢了。
        let error = parse(&argv(&[
            "route", "--from", "a", "--to", "b", "--mode", "bus", "--mode", "walking",
        ]))
        .unwrap_err();
        assert_eq!(exit_code(&error.code), 2);
        assert!(error.message.contains("--mode"), "{}", error.message);
    }

    #[test]
    fn 双横线之后按字面取不当_flag() {
        // 地名可能以 `-` 开头,这是它唯一的出路。
        let command = parse(&argv(&["route", "--from", "--", "--怪地名", "--to", "b"])).unwrap();
        let Command::Route { from, to, .. } = command else {
            panic!("应该解析成 route");
        };
        assert_eq!(from, "--怪地名");
        assert_eq!(to, "b");
    }

    #[test]
    fn 缺必填参数报错并指出缺哪个() {
        // 只说「参数错误」的报错要人猜。
        let error = parse(&argv(&["route", "--from", "a"])).unwrap_err();
        assert_eq!(exit_code(&error.code), 2);
        assert!(
            error.message.contains("--to"),
            "要点名缺的是 --to:{}",
            error.message
        );
    }

    #[test]
    fn 不认识的枚举值报错并列出合法值() {
        // 这是 CLI 存在的那条纪律本身:不认识就报错,并且把可选值告诉调用方,
        // 而不是静默兜底成默认值。
        let error = parse(&argv(&[
            "route", "--from", "a", "--to", "b", "--mode", "transit",
        ]))
        .unwrap_err();
        assert_eq!(error.code, "BAD_MODE");
        assert_eq!(exit_code(&error.code), 2);
        for value in TravelMode::ALL {
            assert!(
                error.message.contains(value),
                "报错里要列出 {value}:{}",
                error.message
            );
        }

        let error =
            parse(&argv(&["circle", "--at", "32.06,118.79", "--package", "x"])).unwrap_err();
        assert_eq!(error.code, "BAD_PACKAGE");
        for value in LifeCirclePackage::ALL {
            assert!(error.message.contains(value), "{}", error.message);
        }
    }

    #[test]
    fn 错误码映射到分档_exit_code() {
        // 脚本按档位分支:映射错了重试逻辑就错。
        assert_eq!(exit_code("BAD_MODE"), 2);
        assert_eq!(exit_code("BAD_USAGE"), 2);
        assert_eq!(exit_code("NO_ROUTE"), 3);
        assert_eq!(exit_code("CROSS_BORDER"), 3);
        // 这两个同属「去设置页填东西」,不许落到不同档。
        assert_eq!(exit_code("NO_PROVIDER"), 4);
        assert_eq!(exit_code("BAIDU_KEY_REQUIRED"), 4);
        // 这三个值得重试。
        assert_eq!(exit_code("NETWORK"), 5);
        assert_eq!(exit_code("UPSTREAM"), 5);
        assert_eq!(exit_code("ALL_PROVIDERS_FAILED"), 5);
        // 没列过的落兜底档,不是 0 —— 0 会让脚本以为成功了。
        assert_eq!(exit_code("SOMETHING_NEW"), 1);
    }

    #[test]
    fn help_与_version_不是错误分支() {
        // `--help` 是正常用法,返回非 0 会让 CI 判失败。
        assert_eq!(parse(&argv(&["--help"])).unwrap(), Command::Help);
        assert_eq!(parse(&argv(&["-h"])).unwrap(), Command::Help);
        assert_eq!(parse(&argv(&["--version"])).unwrap(), Command::Version);
        assert_eq!(parse(&argv(&["-V"])).unwrap(), Command::Version);
        // 子命令后面给 --help 也要认,而不是去抱怨缺 --from。
        assert_eq!(parse(&argv(&["route", "--help"])).unwrap(), Command::Help);
        // 不给参数时打用法,也不是错误。
        assert_eq!(parse(&argv(&[])).unwrap(), Command::Help);
    }

    #[test]
    fn help_里的合法值与_all_同源() {
        // 抄一份字面量列表的话,它不会随 route-core 一起改,于是 help 里
        // 列的值与真正认的值会漂开。这条测试就是那个约束。
        let text = help_text();
        for value in TravelMode::ALL {
            assert!(text.contains(value), "help 里缺 {value}");
        }
        for value in SortBy::ALL {
            assert!(text.contains(value), "help 里缺 {value}");
        }
        for value in Crs::ALL {
            assert!(text.contains(value), "help 里缺 {value}");
        }
        for value in LifeCirclePackage::ALL {
            assert!(text.contains(value), "help 里缺 {value}");
        }
    }

    #[test]
    fn 默认值与图形界面同源() {
        let command = parse(&argv(&["route", "--from", "a", "--to", "b"])).unwrap();
        let Command::Route {
            mode,
            sort,
            out_crs,
            json,
            ..
        } = command
        else {
            panic!("应该解析成 route");
        };
        assert_eq!(mode, DEFAULT_MODE);
        assert_eq!(sort, DEFAULT_SORT);
        assert!(!json, "不给 --json 时默认输出给人看的文本");
        // 不写 Gcj02 字面量:这一条的意义是「CLI 不许开第二个默认值来源」,
        // 写死的话改 Settings::default() 时这里不会响。
        assert_eq!(out_crs, route_core::config::Settings::default().out_crs);
    }

    #[test]
    fn 坐标与数值参数拒绝坏输入() {
        // --at 走 route-core 的 parse_lat_lng,它挡掉越界值:粘错顺序时
        // 静默接受会得到一条跨越半个地球的结果。
        assert!(parse(&argv(&["circle", "--at", "118.79,32.06"])).is_err());
        assert!(parse(&argv(&["circle", "--at", "不是坐标"])).is_err());
        // --limit 要正整数:0 条等于什么都不返回,而调用方会以为是没搜到。
        assert!(parse(&argv(&["suggest", "--keyword", "a", "--limit", "0"])).is_err());
        assert!(parse(&argv(&["suggest", "--keyword", "a", "--limit", "x"])).is_err());
    }

    #[test]
    fn 开关不接值且缺值时报错() {
        assert!(parse(&argv(&["route", "--from", "a", "--to", "b", "--json=1"])).is_err());
        // `--mode` 在末尾没有值:取 argv[index] 会越界,必须报错而不是 panic。
        assert!(parse(&argv(&["route", "--from", "a", "--to", "b", "--mode"])).is_err());
    }
}
