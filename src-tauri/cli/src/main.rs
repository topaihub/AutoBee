//! AutoBee 命令行入口。
//!
//! 三个子命令:`route`(算路)、`suggest`(地点联想)、`circle`(15 分钟生活圈)。
//! 只有这三个的产出对应用外的调用方有意义 —— 其余八个 IPC command 是配置动作
//! 或会把明文密钥打到 stdout,都不上 CLI(§4.1)。

// 显式写出来而不是靠默认:照抄 GUI 那份 `windows_subsystem = "windows"` 的话
// CLI 会一个字都不输出,而且不报错。这个手滑值得用一行挡住(§3.1 约束一)。
#![cfg_attr(not(debug_assertions), windows_subsystem = "console")]

mod args;
mod output;
mod run;

fn main() {
    // writer 必须是 stderr。默认是 stdout,不改就会把日志混进 `--json` 的
    // 输出里,`... --json | jq` 当场解析失败(§5.2 那条管道契约)。
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(match args::parse(&argv) {
        Ok(command) => dispatch(command),
        Err(error) => {
            eprintln!("{}: {}", error.code, error.message);
            args::exit_code(&error.code)
        }
    });
}

/// 执行一条已解析的命令,返回进程退出码。
fn dispatch(command: args::Command) -> i32 {
    match command {
        args::Command::Help => {
            print!("{}", args::help_text());
            0
        }
        args::Command::Version => {
            println!("autobee-cli {}", env!("CARGO_PKG_VERSION"));
            0
        }
        args::Command::Route {
            from,
            to,
            mode,
            sort,
            out_crs,
            json,
        } => run::report(
            run::route(&from, &to, mode, sort, out_crs),
            json,
            output::plan,
            output::degraded_note,
        ),
        args::Command::Suggest {
            keyword,
            limit,
            out_crs,
            json,
        } => run::report(
            run::suggest(&keyword, limit, out_crs),
            json,
            |places| output::places(places),
            // 地点联想没有降级概念:它不做多服务商回落。
            |_| None,
        ),
        args::Command::Circle {
            at,
            crs,
            package,
            out_crs,
            json,
        } => run::report(
            run::circle(at, crs, package, out_crs),
            json,
            output::life_circle,
            // 生活圈只走百度,没有回落链路。
            |_| None,
        ),
    }
}
