//! 模型配置模块
//!
//! 定义不同Claude模型的context window大小和相关配置

/// 获取指定模型的context window大小（单位：tokens）
///
/// # 参数
/// * `model` - 模型ID字符串
///
/// # 返回
/// 返回该模型的context window大小（tokens）
///
/// # 支持的模型
/// - Claude Sonnet 4.5: 200,000 tokens
/// - Claude Opus 4.5: 200,000 tokens
/// - Claude Haiku 4.5: 200,000 tokens
///
/// # 注意
/// 虽然Sonnet 4.5通过API可以支持1M tokens（beta），
/// 但Kiro API目前统一使用200K作为标准限制
pub fn get_context_window_size(model: &str) -> i32 {
    // 标准化模型名称（转小写便于匹配）
    let model_lower = model.to_lowercase();

    // Claude 4.5系列统一使用200K context window
    if model_lower.contains("sonnet")
        || model_lower.contains("opus")
        || model_lower.contains("haiku") {
        200_000
    } else {
        // 默认值（兜底）
        200_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sonnet_context_window() {
        assert_eq!(get_context_window_size("claude-sonnet-4-5-20250929"), 200_000);
        assert_eq!(get_context_window_size("claude-3-5-sonnet-20241022"), 200_000);
    }

    #[test]
    fn test_opus_context_window() {
        assert_eq!(get_context_window_size("claude-opus-4-5-20251101"), 200_000);
    }

    #[test]
    fn test_haiku_context_window() {
        assert_eq!(get_context_window_size("claude-haiku-4-5-20251001"), 200_000);
    }

    #[test]
    fn test_unknown_model_default() {
        assert_eq!(get_context_window_size("unknown-model"), 200_000);
    }
}
