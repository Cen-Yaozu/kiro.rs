//! Anthropic API Handler 函数

use std::convert::Infallible;

use crate::kiro::model::events::Event;
use crate::kiro::model::requests::kiro::KiroRequest;
use crate::kiro::parser::decoder::EventStreamDecoder;
use crate::kiro::provider::StreamResponse;
use crate::kiro::token_manager::ConnectionGuard;
use crate::token;
use axum::{
    Json as JsonExtractor,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
};
use bytes::Bytes;
use futures::{Stream, StreamExt, stream};
use serde_json::json;
use std::time::Duration;
use tokio::time::interval;
use uuid::Uuid;

use super::converter::{ConversionError, convert_request};
use super::middleware::AppState;
use super::stream::{SseEvent, StreamContext};
use super::types::{
    CountTokensRequest, CountTokensResponse, ErrorResponse, MessagesRequest, Model, ModelsResponse,
};
use super::websearch;

/// GET /v1/models
///
/// 返回可用的模型列表
pub async fn get_models() -> impl IntoResponse {
    tracing::info!("Received GET /v1/models request");

    let models = vec![
        Model {
            id: "claude-sonnet-4-5-20250929".to_string(),
            object: "model".to_string(),
            created: 1727568000,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 4.5".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
        },
        Model {
            id: "claude-opus-4-5-20251101".to_string(),
            object: "model".to_string(),
            created: 1730419200,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.5".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
        },
        Model {
            id: "claude-haiku-4-5-20251001".to_string(),
            object: "model".to_string(),
            created: 1727740800,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Haiku 4.5".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
        },
    ];

    Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    })
}

/// POST /v1/messages
///
/// 创建消息（对话）
pub async fn post_messages(
    State(state): State<AppState>,
    JsonExtractor(payload): JsonExtractor<MessagesRequest>,
) -> Response {
    tracing::info!(
        model = %payload.model,
        max_tokens = %payload.max_tokens,
        stream = %payload.stream,
        message_count = %payload.messages.len(),
        "Received POST /v1/messages request"
    );
    // 检查 KiroProvider 是否可用
    let provider = match &state.kiro_provider {
        Some(p) => p.clone(),
        None => {
            tracing::error!("KiroProvider 未配置");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse::new(
                    "service_unavailable",
                    "Kiro API provider not configured",
                )),
            )
                .into_response();
        }
    };

    // 检查是否为 WebSearch 请求
    if websearch::has_web_search_tool(&payload) {
        tracing::info!("检测到 WebSearch 工具，路由到 WebSearch 处理");

        // 估算输入 tokens
        let input_tokens = token::count_all_tokens(
            payload.model.clone(),
            payload.system.clone(),
            payload.messages.clone(),
            payload.tools.clone(),
        ) as i32;

        return websearch::handle_websearch_request(provider, &payload, input_tokens).await;
    }

    // 转换请求
    let conversion_result = match convert_request(&payload) {
        Ok(result) => result,
        Err(e) => {
            let (error_type, message) = match &e {
                ConversionError::UnsupportedModel(model) => {
                    ("invalid_request_error", format!("模型不支持: {}", model))
                }
                ConversionError::EmptyMessages => {
                    ("invalid_request_error", "消息列表为空".to_string())
                }
            };
            tracing::warn!("请求转换失败: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(error_type, message)),
            )
                .into_response();
        }
    };

    // 构建 Kiro 请求
    let kiro_request = KiroRequest {
        conversation_state: conversion_result.conversation_state,
        profile_arn: state.profile_arn.clone(),
    };

    let request_body = match serde_json::to_string(&kiro_request) {
        Ok(body) => {
            let body_size = body.len();
            let body_size_kb = body_size as f64 / 1024.0;

            // 分析请求体内容组成
            let message_count = payload.messages.len();
            let system_size = payload.system.as_ref()
                .map(|s| serde_json::to_string(s).unwrap_or_default().len())
                .unwrap_or(0);
            let tools_size = payload.tools.as_ref()
                .map(|t| serde_json::to_string(t).unwrap_or_default().len())
                .unwrap_or(0);

            // 计算消息内容的平均大小
            let avg_message_size = if message_count > 0 {
                body_size / message_count
            } else {
                0
            };

            tracing::info!(
                "📊 请求体分析 - 总大小: {} bytes ({:.2} KB), 消息数: {}, 平均每条: {} bytes, system: {} bytes, tools: {} bytes",
                body_size,
                body_size_kb,
                message_count,
                avg_message_size,
                system_size,
                tools_size
            );

            // 警告阈值检查
            if body_size > 2_000_000 {
                tracing::error!(
                    "❌ 请求体过大: {:.2} MB，超过 Kiro API 可能的限制（~2MB）",
                    body_size as f64 / 1024.0 / 1024.0
                );

                // 分析消息大小分布
                if message_count > 0 {
                    let mut message_sizes: Vec<(usize, usize)> = payload.messages.iter()
                        .enumerate()
                        .map(|(idx, msg)| {
                            let size = serde_json::to_string(msg).unwrap_or_default().len();
                            (idx, size)
                        })
                        .collect();

                    // 按大小排序，找出最大的几条消息
                    message_sizes.sort_by(|a, b| b.1.cmp(&a.1));

                    tracing::error!("📋 最大的 5 条消息:");
                    for (idx, size) in message_sizes.iter().take(5) {
                        tracing::error!("  消息 #{}: {:.2} KB", idx + 1, *size as f64 / 1024.0);
                    }
                }
            } else if body_size > 1_500_000 {
                tracing::warn!(
                    "⚠️  请求体接近限制: {:.2} MB，建议使用 /compact 压缩上下文",
                    body_size as f64 / 1024.0 / 1024.0
                );
            } else if body_size > 1_000_000 {
                tracing::warn!(
                    "⚠️  请求体较大: {:.2} MB",
                    body_size as f64 / 1024.0 / 1024.0
                );
            }

            body
        }
        Err(e) => {
            tracing::error!("序列化请求失败: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(
                    "internal_error",
                    format!("序列化请求失败: {}", e),
                )),
            )
                .into_response();
        }
    };

    tracing::debug!("Kiro request body: {}", request_body);

    // 估算输入 tokens
    let input_tokens = token::count_all_tokens(
        payload.model.clone(),
        payload.system.clone(),
        payload.messages.clone(),
        payload.tools.clone(),
    ) as i32;

    tracing::info!(
        "Token 计数 - 消息数: {}, 输入 tokens: {}",
        payload.messages.len(),
        input_tokens
    );

    // 获取模型的context window大小
    let context_window_size = super::model_config::get_context_window_size(&payload.model);

    // 提前检查：input_tokens + max_tokens 是否超过context window
    let total_tokens = input_tokens + payload.max_tokens;
    if total_tokens > context_window_size {
        tracing::warn!(
            "请求被拦截: input_tokens({}) + max_tokens({}) = {} > context_window({})",
            input_tokens,
            payload.max_tokens,
            total_tokens,
            context_window_size
        );

        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "invalid_request_error",
                format!(
                    "input length and max_tokens exceed context limit: {} + {} > {}, decrease input length or max_tokens and try again. Suggestion: 1) Use /compact command to reduce context 2) Reduce conversation history 3) Decrease max_tokens parameter",
                    input_tokens,
                    payload.max_tokens,
                    context_window_size
                ),
            )),
        )
            .into_response();
    }

    // 检查是否启用了thinking
    let thinking_enabled = payload
        .thinking
        .as_ref()
        .map(|t| t.thinking_type == "enabled")
        .unwrap_or(false);

    if payload.stream {
        // 流式响应
        handle_stream_request(
            provider,
            &request_body,
            &payload.model,
            input_tokens,
            thinking_enabled,
        )
        .await
    } else {
        // 非流式响应
        handle_non_stream_request(provider, &request_body, &payload.model, input_tokens).await
    }
}

/// 根据上游错误信息判断应返回的状态码
fn determine_error_status(error_msg: &str) -> (StatusCode, &'static str) {
    if error_msg.contains("400 Bad Request") {
        (StatusCode::BAD_REQUEST, "invalid_request_error")
    } else if error_msg.contains("429") {
        (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error")
    } else if error_msg.contains("401") || error_msg.contains("403") {
        (StatusCode::UNAUTHORIZED, "authentication_error")
    } else {
        (StatusCode::BAD_GATEWAY, "api_error")
    }
}

/// 检查错误信息是否为token超限错误
fn is_token_limit_error(error_msg: &str) -> bool {
    error_msg.contains("Input is too long")
        || error_msg.contains("too long")
        || error_msg.contains("exceeds")
        || error_msg.contains("CONTENT_LENGTH_EXCEEDS_THRESHOLD")
        || error_msg.contains("context limit")
}

/// 生成友好的token超限错误信息
fn create_token_limit_error(input_tokens: i32, max_tokens: i32, context_window: i32) -> ErrorResponse {
    ErrorResponse::new(
        "invalid_request_error",
        format!(
            "Prompt is too long (server-side context limit reached). Input tokens: {}, Max tokens: {}, Context window: {}. Suggestion: 1) Use /compact command to reduce context 2) Reduce conversation history 3) Decrease max_tokens parameter",
            input_tokens,
            max_tokens,
            context_window
        ),
    )
}

/// 处理流式请求
async fn handle_stream_request(
    provider: std::sync::Arc<crate::kiro::provider::KiroProvider>,
    request_body: &str,
    model: &str,
    input_tokens: i32,
    thinking_enabled: bool,
) -> Response {
    tracing::info!(
        "开始处理流式请求 - model: {}, input_tokens: {}, thinking: {}",
        model,
        input_tokens,
        thinking_enabled
    );

    // 调用 Kiro API（支持多凭据故障转移）
    let stream_response = match provider.call_api_stream(request_body).await {
        Ok(resp) => resp,
        Err(e) => {
            let error_msg = e.to_string();
            tracing::error!("Kiro API 调用失败: {}", error_msg);

            // 检查是否为token超限错误
            if is_token_limit_error(&error_msg) {
                let context_window = super::model_config::get_context_window_size(model);
                // 从request_body解析max_tokens（简化处理，使用默认值）
                let max_tokens = 8192; // 默认值，实际应该从payload获取
                return (
                    StatusCode::BAD_REQUEST,
                    Json(create_token_limit_error(input_tokens, max_tokens, context_window)),
                )
                    .into_response();
            }

            let (status, error_type) = determine_error_status(&error_msg);
            return (
                status,
                Json(ErrorResponse::new(
                    error_type,
                    format!("上游 API 调用失败: {}", error_msg),
                )),
            )
                .into_response();
        }
    };

    // 解构 StreamResponse，获取 response 和 guard
    let StreamResponse { response, guard } = stream_response;

    // 创建流处理上下文
    let mut ctx = StreamContext::new_with_thinking(model, input_tokens, thinking_enabled);

    // 生成初始事件
    let initial_events = ctx.generate_initial_events();

    // 创建 SSE 流，传入 guard 以保持其生命周期
    let stream = create_sse_stream(response, ctx, initial_events, guard);

    // 返回 SSE 响应
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// Ping 事件间隔（25秒）
const PING_INTERVAL_SECS: u64 = 25;

/// 创建 ping 事件的 SSE 字符串
fn create_ping_sse() -> Bytes {
    Bytes::from("event: ping\ndata: {\"type\": \"ping\"}\n\n")
}

/// 创建 SSE 事件流
///
/// guard 参数用于保持 ConnectionGuard 的生命周期，确保 active_connections 计数
/// 在流完全结束后才递减
fn create_sse_stream(
    response: reqwest::Response,
    ctx: StreamContext,
    initial_events: Vec<SseEvent>,
    guard: ConnectionGuard,
) -> impl Stream<Item = Result<Bytes, Infallible>> {
    // 先发送初始事件
    let initial_stream = stream::iter(
        initial_events
            .into_iter()
            .map(|e| Ok(Bytes::from(e.to_sse_string()))),
    );

    // 然后处理 Kiro 响应流，同时每25秒发送 ping 保活
    let body_stream = response.bytes_stream();

    // guard 被移入闭包状态，随流一起存活
    let processing_stream = stream::unfold(
        (body_stream, ctx, EventStreamDecoder::new(), false, interval(Duration::from_secs(PING_INTERVAL_SECS)), Some(guard)),
        |(mut body_stream, mut ctx, mut decoder, finished, mut ping_interval, guard)| async move {
            if finished {
                // 流结束时 guard 会被 drop，active_connections 递减
                drop(guard);
                return None;
            }

            // 使用 select! 同时等待数据和 ping 定时器
            tokio::select! {
                // 处理数据流
                chunk_result = body_stream.next() => {
                    match chunk_result {
                        Some(Ok(chunk)) => {
                            // 解码事件
                            if let Err(e) = decoder.feed(&chunk) {
                                tracing::warn!("缓冲区溢出: {}", e);
                            }

                            let mut events = Vec::new();
                            for result in decoder.decode_iter() {
                                match result {
                                    Ok(frame) => {
                                        if let Ok(event) = Event::from_frame(frame) {
                                            let sse_events = ctx.process_kiro_event(&event);
                                            events.extend(sse_events);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("解码事件失败: {}", e);
                                    }
                                }
                            }

                            // 转换为 SSE 字节流
                            let bytes: Vec<Result<Bytes, Infallible>> = events
                                .into_iter()
                                .map(|e| Ok(Bytes::from(e.to_sse_string())))
                                .collect();

                            Some((stream::iter(bytes), (body_stream, ctx, decoder, false, ping_interval, guard)))
                        }
                        Some(Err(e)) => {
                            tracing::error!("读取响应流失败: {}", e);
                            // 发送最终事件并结束
                            let final_events = ctx.generate_final_events();
                            let bytes: Vec<Result<Bytes, Infallible>> = final_events
                                .into_iter()
                                .map(|e| Ok(Bytes::from(e.to_sse_string())))
                                .collect();
                            Some((stream::iter(bytes), (body_stream, ctx, decoder, true, ping_interval, guard)))
                        }
                        None => {
                            // 流结束，发送最终事件
                            let final_events = ctx.generate_final_events();
                            let bytes: Vec<Result<Bytes, Infallible>> = final_events
                                .into_iter()
                                .map(|e| Ok(Bytes::from(e.to_sse_string())))
                                .collect();
                            Some((stream::iter(bytes), (body_stream, ctx, decoder, true, ping_interval, guard)))
                        }
                    }
                }
                // 发送 ping 保活
                _ = ping_interval.tick() => {
                    tracing::trace!("发送 ping 保活事件");
                    let bytes: Vec<Result<Bytes, Infallible>> = vec![Ok(create_ping_sse())];
                    Some((stream::iter(bytes), (body_stream, ctx, decoder, false, ping_interval, guard)))
                }
            }
        },
    )
    .flatten();

    initial_stream.chain(processing_stream)
}

/// 处理非流式请求
async fn handle_non_stream_request(
    provider: std::sync::Arc<crate::kiro::provider::KiroProvider>,
    request_body: &str,
    model: &str,
    input_tokens: i32,
) -> Response {
    // 调用 Kiro API（支持多凭据故障转移）
    let response = match provider.call_api(request_body).await {
        Ok(resp) => resp,
        Err(e) => {
            let error_msg = e.to_string();
            tracing::error!("Kiro API 调用失败: {}", error_msg);

            // 检查是否为token超限错误
            if is_token_limit_error(&error_msg) {
                let context_window = super::model_config::get_context_window_size(model);
                let max_tokens = 8192; // 默认值
                return (
                    StatusCode::BAD_REQUEST,
                    Json(create_token_limit_error(input_tokens, max_tokens, context_window)),
                )
                    .into_response();
            }

            let (status, error_type) = determine_error_status(&error_msg);
            return (
                status,
                Json(ErrorResponse::new(
                    error_type,
                    format!("上游 API 调用失败: {}", error_msg),
                )),
            )
                .into_response();
        }
    };

    // 读取响应体
    let body_bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("读取响应体失败: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse::new(
                    "api_error",
                    format!("读取响应失败: {}", e),
                )),
            )
                .into_response();
        }
    };

    // 解析事件流
    let mut decoder = EventStreamDecoder::new();
    if let Err(e) = decoder.feed(&body_bytes) {
        tracing::warn!("缓冲区溢出: {}", e);
    }

    let mut text_content = String::new();
    let mut tool_uses: Vec<serde_json::Value> = Vec::new();
    let mut has_tool_use = false;
    let mut stop_reason = "end_turn".to_string();
    // 从 contextUsageEvent 计算的实际输入 tokens
    let mut context_input_tokens: Option<i32> = None;

    // 收集工具调用的增量 JSON
    let mut tool_json_buffers: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for result in decoder.decode_iter() {
        match result {
            Ok(frame) => {
                if let Ok(event) = Event::from_frame(frame) {
                    match event {
                        Event::AssistantResponse(resp) => {
                            text_content.push_str(&resp.content);
                        }
                        Event::ToolUse(tool_use) => {
                            has_tool_use = true;

                            // 累积工具的 JSON 输入
                            let buffer = tool_json_buffers
                                .entry(tool_use.tool_use_id.clone())
                                .or_insert_with(String::new);
                            buffer.push_str(&tool_use.input);

                            // 如果是完整的工具调用，添加到列表
                            if tool_use.stop {
                                let input: serde_json::Value = serde_json::from_str(buffer)
                                    .unwrap_or_else(|e| {
                                        tracing::warn!(
                                            "工具输入 JSON 解析失败: {}, tool_use_id: {}, 原始内容: {}",
                                            e, tool_use.tool_use_id, buffer
                                        );
                                        serde_json::json!({})
                                    });

                                tool_uses.push(json!({
                                    "type": "tool_use",
                                    "id": tool_use.tool_use_id,
                                    "name": tool_use.name,
                                    "input": input
                                }));
                            }
                        }
                        Event::ContextUsage(context_usage) => {
                            // 从上下文使用百分比计算实际的 input_tokens
                            // 获取模型的context window大小
                            let context_window_size = super::model_config::get_context_window_size(model);
                            let actual_input_tokens = (context_usage.context_usage_percentage
                                * (context_window_size as f64)
                                / 100.0)
                                as i32;
                            context_input_tokens = Some(actual_input_tokens);
                            tracing::info!(
                                "📊 收到 contextUsageEvent - 百分比: {:.2}%, 计算得出 input_tokens: {} (累积值), context_window: {}",
                                context_usage.context_usage_percentage,
                                actual_input_tokens,
                                context_window_size
                            );
                        }
                        Event::Exception { exception_type, .. } => {
                            if exception_type == "ContentLengthExceededException" {
                                stop_reason = "max_tokens".to_string();
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                tracing::warn!("解码事件失败: {}", e);
            }
        }
    }

    // 确定 stop_reason
    if has_tool_use && stop_reason == "end_turn" {
        stop_reason = "tool_use".to_string();
    }

    // 构建响应内容
    let mut content: Vec<serde_json::Value> = Vec::new();

    if !text_content.is_empty() {
        content.push(json!({
            "type": "text",
            "text": text_content
        }));
    }

    content.extend(tool_uses);

    // 估算输出 tokens
    let output_tokens = token::estimate_output_tokens(&content);

    // 使用从 contextUsageEvent 计算的 input_tokens，如果没有则使用估算值
    let final_input_tokens = context_input_tokens.unwrap_or(input_tokens);

    tracing::info!(
        "构建非流式响应 - input_tokens: {}, output_tokens: {}, context_input_tokens: {:?}",
        final_input_tokens,
        output_tokens,
        context_input_tokens
    );

    // 构建 Anthropic 响应
    let response_body = json!({
        "id": format!("msg_{}", Uuid::new_v4().to_string().replace('-', "")),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": final_input_tokens,
            "output_tokens": output_tokens
        }
    });

    tracing::debug!("响应 usage 字段: {{ input_tokens: {}, output_tokens: {} }}", final_input_tokens, output_tokens);

    (StatusCode::OK, Json(response_body)).into_response()
}

/// POST /v1/messages/count_tokens
///
/// 计算消息的 token 数量
pub async fn count_tokens(
    JsonExtractor(payload): JsonExtractor<CountTokensRequest>,
) -> impl IntoResponse {
    tracing::info!(
        model = %payload.model,
        message_count = %payload.messages.len(),
        "Received POST /v1/messages/count_tokens request"
    );

    let total_tokens = token::count_all_tokens(
        payload.model,
        payload.system,
        payload.messages,
        payload.tools,
    ) as i32;

    Json(CountTokensResponse {
        input_tokens: total_tokens.max(1) as i32,
    })
}
