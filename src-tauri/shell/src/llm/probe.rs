//! 拉取某个服务入口可用的模型列表。
//!
//! 结果只进内存不落库：缓存了用户就看不到中转商新上的模型，也无处刷新。

use serde::Deserialize;

use crate::credentials::LlmProtocol;
use crate::error::AppError;

/// OpenAI 兼容的 `/models` 响应。只取 `id`，其余字段各家给的都不一样。
#[derive(Debug, Deserialize)]
struct OpenaiModels {
    data: Vec<OpenaiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenaiModel {
    id: String,
}

/// Anthropic 的 `/v1/models` 响应：形状同 OpenAI，鉴权用 `x-api-key` + 版本头（缺则 400）。
#[derive(Debug, Deserialize)]
struct AnthropicModels {
    data: Vec<OpenaiModel>,
}

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// # Errors
///
/// `base_url` 形如 `https://xxx/v1`。网络、鉴权、响应形状不符时返回错误；
/// 空列表也算错误，空的多选框会让用户以为程序卡住了。
pub fn fetch_models(
    base_url: &str,
    protocol: LlmProtocol,
    api_key: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let key = api_key.map(str::trim).unwrap_or_default();
    if key.is_empty() {
        return Err(AppError::new(
            "LLM_KEY_REQUIRED",
            "这个入口还没填 API Key，先填上再获取模型列表。",
            400,
        ));
    }

    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let bearer = format!("Bearer {key}");

    let mut models = match protocol {
        LlmProtocol::AnthropicMessages => {
            let body: AnthropicModels = route_core::http::get_json_with_auth(
                &url,
                &[("x-api-key", key), ("anthropic-version", ANTHROPIC_VERSION)],
            )?;
            body.data.into_iter().map(|m| m.id).collect::<Vec<_>>()
        }
        LlmProtocol::OpenaiChat | LlmProtocol::OpenaiResponses => {
            let body: OpenaiModels =
                route_core::http::get_json_with_auth(&url, &[("Authorization", &bearer)])?;
            body.data.into_iter().map(|m| m.id).collect::<Vec<_>>()
        }
    };

    models.retain(|m| !m.trim().is_empty());
    models.sort_unstable();
    models.dedup();

    if models.is_empty() {
        return Err(AppError::new(
            "LLM_NO_MODELS",
            "这个入口返回了空的模型列表。确认 base_url 是否带 /v1，以及该 Key 是否开通了模型。",
            502,
        ));
    }
    Ok(models)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]
mod tests {
    use super::*;

    #[test]
    fn 没填_key_时不发请求就报错() {
        let error =
            fetch_models("https://api.example.com/v1", LlmProtocol::OpenaiChat, None).unwrap_err();
        assert_eq!(error.code, "LLM_KEY_REQUIRED");

        let blank = fetch_models(
            "https://api.example.com/v1",
            LlmProtocol::OpenaiChat,
            Some("  "),
        )
        .unwrap_err();
        assert_eq!(blank.code, "LLM_KEY_REQUIRED");
    }

    #[test]
    fn 响应体按_data_id_解析并去重排序() {
        let body: OpenaiModels = serde_json::from_str(
            r#"{"data":[{"id":"gpt-4o"},{"id":"claude-3"},{"id":"gpt-4o"},{"id":""}]}"#,
        )
        .unwrap();
        let mut ids: Vec<String> = body
            .data
            .into_iter()
            .map(|m| m.id)
            .filter(|m| !m.trim().is_empty())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids, ["claude-3", "gpt-4o"]);
    }

    #[test]
    fn 多余字段不影响解析() {
        // 中转商爱往里加字段，多一个就解不开的话每换一家都要改代码。
        let body: OpenaiModels = serde_json::from_str(
            r#"{"object":"list","data":[{"id":"m1","object":"model","created":1,"owned_by":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(body.data.len(), 1);
    }
}
