//! Aether 宿主：把 route-core 的三个地图工具交给 Aether 的对话循环。
//!
//! 模型调用、流式、会话、Skills、MCP、审批都在 `tauri-plugin-aether` 里；这里只做
//! 业务决策：系统提示词，以及工具的声明与执行。

use autobee_shell::credentials::resolve;
use autobee_shell::tools::{dispatch, tool_schemas};
use tauri_plugin_aether::{
    Approval, CancellationToken, HostTool, KitHost, RunRequest, ToolCall, ToolError, ToolOutput,
    ToolSource, ToolSpec,
};

/// 三条硬约束的来源：
///
/// 1. **路线与地点必须来自工具**：模型对本地公交线路的记忆既过时又自信，编出来的
///    时长与票价看不出真假（设计方案 §11.2 的「模型编造路线」）。
/// 2. **不念裸经纬度**：各家坐标系不同（腾讯/高德 GCJ-02、百度 BD-09、天地图
///    CGCS2000），模型把 BD-09 当 GPS 讲给用户会偏百米级，而且不报错、不崩溃。
/// 3. **工具报错要看 code**：照机器码改参数重试比换个说法重试有效。
const SYSTEM_PROMPT: &str = "你是 AutoBee 的出行助手，帮用户查地点、规划公共交通与步行路线、\
了解一个位置周边的生活设施。

硬性要求：

1. 路线、时长、票价、距离、地点信息**一律来自工具返回值**，绝不凭记忆作答。没调工具就别给具体数字。
2. **回答里不要出现经纬度数字。** 工具返回的坐标只用于你再次调用工具，地点一律用名称指代。\
不同地图服务商的坐标系不同，把坐标念给用户会造成百米级的误导且无法被发现。
3. 工具报错时错误信息里有机器码与中文说明，照它改参数重试；同一个参数不要重复试第二次。
4. 用中文回答，简洁。把工具给的分段信息讲成人能照着走的步骤，不要罗列原始字段。";

pub struct AutoBeeHost;

#[async_trait::async_trait]
impl KitHost for AutoBeeHost {
    fn system_prompt(&self, _req: &RunRequest) -> Option<String> {
        Some(SYSTEM_PROMPT.to_owned())
    }

    fn tools(&self, _req: &RunRequest) -> Vec<HostTool> {
        tool_schemas()
            .into_iter()
            .map(|schema| HostTool {
                spec: ToolSpec {
                    name: schema["name"].as_str().unwrap_or_default().to_owned(),
                    description: schema["description"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    parameters: schema["parameters"].clone(),
                    // 三个都是只读查询，不改任何东西，问用户只是白加一次点击。
                    approval: Approval::Auto,
                    source: ToolSource::Host,
                },
            })
            .collect()
    }

    async fn call_tool(
        &self,
        call: &ToolCall,
        _cancel: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let name = call.name.clone();
        let args = call.args.to_string();
        // route-core 是阻塞 HTTP；每次重新读配置，设置页改完 Key 下一轮就生效。
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            dispatch(&resolve::load_config()?, &name, &args)
        })
        .await
        .map_err(|error| ToolError {
            code: "INTERNAL".to_owned(),
            message: format!("后台任务失败：{error}"),
            unknown: false,
        })?
        .map_err(|error| {
            tracing::warn!(tool = %call.name, code = %error.code, "地图工具失败");
            ToolError {
                code: error.code,
                message: error.message,
                // 只读查询，没有副作用可言。
                unknown: false,
            }
        })?;
        Ok(ToolOutput {
            text: outcome.summary,
            payload: outcome
                .payload
                .and_then(|payload| serde_json::to_value(payload).ok()),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    /// Aether 对工具名有正则、对描述有 1024 字符上限；不合规的工具整轮被拒。
    #[test]
    fn 工具声明满足_aether_的约束() {
        let req: RunRequest = serde_json::from_value(serde_json::json!({
            "purpose": "chat", "skills": [], "input": "x", "regenerate": false, "language": "zh-CN"
        }))
        .unwrap();
        let tools = AutoBeeHost.tools(&req);
        let names: Vec<_> = tools.iter().map(|t| t.spec.name.as_str()).collect();
        assert_eq!(names, ["plan_route", "search_place", "life_circle"]);
        for tool in &tools {
            assert!(tool.spec.description.chars().count() <= 1024);
            assert_eq!(tool.spec.parameters["type"], "object");
        }
    }
}
