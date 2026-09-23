//! AutoBee desktop application entrypoint.

// Release 构建下不显示命令行窗口；调试构建仍保留 stdout/stderr。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    route_planner_lib::run()
}
