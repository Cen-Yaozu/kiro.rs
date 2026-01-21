//! Token 计算模块
//!
//! 提供文本 token 数量计算功能。
//!
//! # 计算方法
//! - 优先使用 Hugging Face tokenizers（Claude 官方 tokenizer）
//! - 如果 tokenizer 加载失败，回退到简单估算
//! - 支持远程 API 调用（可选）

use crate::anthropic::types::{
    CountTokensRequest, CountTokensResponse, Message, SystemMessage, Tool,
};
use crate::http_client::{ProxyConfig, build_client};
use crate::model::config::TlsBackend;
use std::sync::OnceLock;
use tokenizers::Tokenizer;

/// Count Tokens API 配置
#[derive(Clone, Default)]
pub struct CountTokensConfig {
    /// 外部 count_tokens API 地址
    pub api_url: Option<String>,
    /// count_tokens API 密钥
    pub api_key: Option<String>,
    /// count_tokens API 认证类型（"x-api-key" 或 "bearer"）
    pub auth_type: String,
    /// 代理配置
    pub proxy: Option<ProxyConfig>,

    pub tls_backend: TlsBackend,
}

/// 全局配置存储
static COUNT_TOKENS_CONFIG: OnceLock<CountTokensConfig> = OnceLock::new();

/// 全局 Claude tokenizer
static CLAUDE_TOKENIZER: OnceLock<Option<Tokenizer>> = OnceLock::new();

/// 初始化 count_tokens 配置
///
/// 应在应用启动时调用一次
pub fn init_config(config: CountTokensConfig) {
    let _ = COUNT_TOKENS_CONFIG.set(config);
}

/// 初始化 Claude tokenizer
///
/// 尝试从文件加载 tokenizer，如果失败则返回 None
fn init_tokenizer() -> Option<Tokenizer> {
    // 尝试从多个可能的路径加载 tokenizer
    let paths = vec![
        "tokenizers/claude-tokenizer.json",
        "./tokenizers/claude-tokenizer.json",
        "../tokenizers/claude-tokenizer.json",
    ];

    for path in paths {
        match Tokenizer::from_file(path) {
            Ok(tokenizer) => {
                tracing::info!("成功加载 Claude tokenizer: {}", path);
                return Some(tokenizer);
            }
            Err(e) => {
                tracing::debug!("无法从 {} 加载 tokenizer: {}", path, e);
            }
        }
    }

    tracing::warn!("无法加载 Claude tokenizer，将使用简单估算");
    None
}

/// 获取 Claude tokenizer
fn get_tokenizer() -> Option<&'static Tokenizer> {
    CLAUDE_TOKENIZER
        .get_or_init(init_tokenizer)
        .as_ref()
}

/// 获取配置
fn get_config() -> Option<&'static CountTokensConfig> {
    COUNT_TOKENS_CONFIG.get()
}

/// 计算文本的 token 数量
///
/// 优先使用 Claude tokenizer，失败时回退到简单估算
pub fn count_tokens(text: &str) -> u64 {
    // 尝试使用 Claude tokenizer
    if let Some(tokenizer) = get_tokenizer() {
        match tokenizer.encode(text, false) {
            Ok(encoding) => {
                let count = encoding.get_ids().len() as u64;
                return count;
            }
            Err(e) => {
                tracing::warn!("Tokenizer 编码失败，回退到简单估算: {}", e);
            }
        }
    }

    // 回退到简单估算
    count_tokens_fallback(text)
}

/// 简单估算（回退方法）
///
/// 基于字符数的简单估算：
/// - 英文：约 4 个字符 = 1 token
/// - 中文：约 1.5 个字符 = 1 token
fn count_tokens_fallback(text: &str) -> u64 {
    let char_count = text.chars().count() as f64;

    // 检测文本类型
    let non_ascii_count = text.chars().filter(|c| !c.is_ascii()).count() as f64;
    let non_ascii_ratio = non_ascii_count / char_count.max(1.0);

    // 根据非 ASCII 字符比例调整估算
    let tokens = if non_ascii_ratio > 0.5 {
        // 主要是中文/日文/韩文等
        char_count / 1.5
    } else {
        // 主要是英文
        char_count / 4.0
    };

    // 添加 10% 安全边际
    (tokens * 1.1).ceil() as u64
}

/// 估算请求的输入 tokens
///
/// 优先级：远程 API > Claude tokenizer > 简单估算
pub(crate) fn count_all_tokens(
    model: String,
    system: Option<Vec<SystemMessage>>,
    messages: Vec<Message>,
    tools: Option<Vec<Tool>>,
) -> u64 {
    // 检查是否配置了远程 API
    if let Some(config) = get_config() {
        if let Some(api_url) = &config.api_url {
            // 尝试调用远程 API
            let result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(call_remote_count_tokens(
                    api_url, config, model, &system, &messages, &tools,
                ))
            });

            match result {
                Ok(tokens) => {
                    tracing::debug!("远程 count_tokens API 返回: {}", tokens);
                    return tokens;
                }
                Err(e) => {
                    tracing::warn!("远程 count_tokens API 调用失败，回退到本地计算: {}", e);
                }
            }
        }
    }

    // 本地计算（使用 Claude tokenizer 或简单估算）
    count_all_tokens_local(system, messages, tools)
}

/// 调用远程 count_tokens API
async fn call_remote_count_tokens(
    api_url: &str,
    config: &CountTokensConfig,
    model: String,
    system: &Option<Vec<SystemMessage>>,
    messages: &Vec<Message>,
    tools: &Option<Vec<Tool>>,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let client = build_client(config.proxy.as_ref(), 300, config.tls_backend)?;

    // 构建请求体
    let request = CountTokensRequest {
        model,
        messages: messages.clone(),
        system: system.clone(),
        tools: tools.clone(),
    };

    // 构建请求
    let mut req_builder = client.post(api_url);

    // 设置认证头
    if let Some(api_key) = &config.api_key {
        if config.auth_type == "bearer" {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
        } else {
            req_builder = req_builder.header("x-api-key", api_key);
        }
    }

    // 发送请求
    let response = req_builder
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("API 返回错误状态: {}", response.status()).into());
    }

    let result: CountTokensResponse = response.json().await?;
    Ok(result.input_tokens as u64)
}

/// 本地计算请求的输入 tokens
fn count_all_tokens_local(
    system: Option<Vec<SystemMessage>>,
    messages: Vec<Message>,
    tools: Option<Vec<Tool>>,
) -> u64 {
    let mut total = 0;

    // 系统消息
    if let Some(ref system) = system {
        for msg in system {
            let tokens = count_tokens(&msg.text);
            total += tokens;
            tracing::debug!("系统消息 tokens: {}", tokens);
        }
        // 系统消息额外开销
        total += 10;
    }

    // 用户消息
    tracing::debug!("开始计算 {} 条消息的 tokens", messages.len());
    for (idx, msg) in messages.iter().enumerate() {
        // 每条消息的结构开销
        total += 4;

        let msg_tokens = if let serde_json::Value::String(s) = &msg.content {
            count_tokens(s)
        } else if let serde_json::Value::Array(arr) = &msg.content {
            let mut content_tokens = 0;
            for item in arr {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    content_tokens += count_tokens(text);
                }
            }
            content_tokens
        } else {
            0
        };

        total += msg_tokens;

        if idx < 5 || idx >= messages.len() - 5 {
            tracing::debug!("消息 #{} ({}) tokens: {}", idx + 1, msg.role, msg_tokens);
        } else if idx == 5 {
            tracing::debug!("... 省略中间消息 ...");
        }
    }

    // 工具定义
    if let Some(ref tools) = tools {
        for tool in tools {
            total += count_tokens(&tool.name);
            total += count_tokens(&tool.description);
            let input_schema_json = serde_json::to_string(&tool.input_schema).unwrap_or_default();
            total += count_tokens(&input_schema_json);
            // 每个工具的结构开销
            total += 10;
        }
        tracing::debug!("工具定义 tokens: {} 个工具", tools.len());
    }

    tracing::info!(
        "Token 计数完成 - 总计: {} tokens (消息: {}, 系统: {}, 工具: {})",
        total,
        messages.len(),
        system.as_ref().map(|s| s.len()).unwrap_or(0),
        tools.as_ref().map(|t| t.len()).unwrap_or(0)
    );

    total.max(1)
}

/// 估算输出 tokens
pub(crate) fn estimate_output_tokens(content: &[serde_json::Value]) -> i32 {
    let mut total = 0;

    for block in content {
        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
            let tokens = count_tokens(text) as i32;
            total += tokens;
            tracing::debug!("文本块 tokens: {}", tokens);
        }
        if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
            // 工具调用开销
            if let Some(input) = block.get("input") {
                let input_str = serde_json::to_string(input).unwrap_or_default();
                let tokens = count_tokens(&input_str) as i32;
                total += tokens;
                tracing::debug!("工具调用 tokens: {}", tokens);
            }
            total += 10; // 工具调用结构开销
        }
    }

    tracing::info!("输出 tokens 估算: {} tokens ({} 个内容块)", total.max(1), content.len());

    total.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens_english() {
        let text = "Hello, world!";
        let count = count_tokens(text);
        // 应该在 3-5 个 token 之间
        assert!(count >= 3 && count <= 5, "English text token count: {}", count);
    }

    #[test]
    fn test_count_tokens_chinese() {
        let text = "你好，世界！";
        let count = count_tokens(text);
        // 中文应该在 5-8 个 token 之间
        assert!(count >= 5 && count <= 8, "Chinese text token count: {}", count);
    }

    #[test]
    fn test_count_tokens_mixed() {
        let text = "Hello 你好 world 世界";
        let count = count_tokens(text);
        // 混合文本应该在 8-15 个 token 之间
        assert!(count >= 8 && count <= 15, "Mixed text token count: {}", count);
    }

    #[test]
    fn test_count_tokens_empty() {
        let text = "";
        let count = count_tokens(text);
        assert_eq!(count, 0, "Empty text should have 0 tokens");
    }
}
