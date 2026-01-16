//! Admin API 业务逻辑服务

use std::sync::Arc;

use crate::kiro::model::credentials::KiroCredentials;
use crate::kiro::token_manager::MultiTokenManager;

use super::error::AdminServiceError;
use super::types::{
    AddCredentialRequest, AddCredentialResponse, BalanceResponse, BatchImportRequest,
    BatchImportResponse, BatchImportResultItem, CredentialStatusItem, CredentialsStatusResponse,
};

/// Admin 服务
///
/// 封装所有 Admin API 的业务逻辑
pub struct AdminService {
    token_manager: Arc<MultiTokenManager>,
}

impl AdminService {
    pub fn new(token_manager: Arc<MultiTokenManager>) -> Self {
        Self { token_manager }
    }

    /// 获取所有凭据状态
    pub fn get_all_credentials(&self) -> CredentialsStatusResponse {
        let snapshot = self.token_manager.snapshot();

        let mut credentials: Vec<CredentialStatusItem> = snapshot
            .entries
            .into_iter()
            .map(|entry| CredentialStatusItem {
                id: entry.id,
                priority: entry.priority,
                disabled: entry.disabled,
                failure_count: entry.failure_count,
                is_current: entry.id == snapshot.current_id,
                expires_at: entry.expires_at,
                auth_method: entry.auth_method,
                has_profile_arn: entry.has_profile_arn,
            })
            .collect();

        // 按优先级排序（数字越小优先级越高）
        credentials.sort_by_key(|c| c.priority);

        CredentialsStatusResponse {
            total: snapshot.total,
            available: snapshot.available,
            current_id: snapshot.current_id,
            credentials,
        }
    }

    /// 设置凭据禁用状态
    pub fn set_disabled(&self, id: u64, disabled: bool) -> Result<(), AdminServiceError> {
        // 先获取当前凭据 ID，用于判断是否需要切换
        let snapshot = self.token_manager.snapshot();
        let current_id = snapshot.current_id;

        self.token_manager
            .set_disabled(id, disabled)
            .map_err(|e| self.classify_error(e, id))?;

        // 只有禁用的是当前凭据时才尝试切换到下一个
        if disabled && id == current_id {
            let _ = self.token_manager.switch_to_next();
        }
        Ok(())
    }

    /// 设置凭据优先级
    pub fn set_priority(&self, id: u64, priority: u32) -> Result<(), AdminServiceError> {
        self.token_manager
            .set_priority(id, priority)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 重置失败计数并重新启用
    pub fn reset_and_enable(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .reset_and_enable(id)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 强制刷新指定凭据的 Token
    pub async fn refresh_token(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .force_refresh_token(id)
            .await
            .map_err(|e| self.classify_balance_error(e, id))
    }

    /// 获取凭据余额
    pub async fn get_balance(&self, id: u64) -> Result<BalanceResponse, AdminServiceError> {
        let usage = self
            .token_manager
            .get_usage_limits_for(id)
            .await
            .map_err(|e| self.classify_balance_error(e, id))?;

        let current_usage = usage.current_usage();
        let usage_limit = usage.usage_limit();
        let remaining = (usage_limit - current_usage).max(0.0);
        let usage_percentage = if usage_limit > 0.0 {
            (current_usage / usage_limit * 100.0).min(100.0)
        } else {
            0.0
        };

        Ok(BalanceResponse {
            id,
            subscription_title: usage.subscription_title().map(|s| s.to_string()),
            current_usage,
            usage_limit,
            remaining,
            usage_percentage,
            next_reset_at: usage.next_date_reset,
        })
    }

    /// 添加新凭据
    pub async fn add_credential(
        &self,
        req: AddCredentialRequest,
    ) -> Result<AddCredentialResponse, AdminServiceError> {
        // 构建凭据对象
        let new_cred = KiroCredentials {
            id: None,
            access_token: None,
            refresh_token: Some(req.refresh_token),
            profile_arn: None,
            expires_at: None,
            auth_method: Some(req.auth_method),
            client_id: req.client_id,
            client_secret: req.client_secret,
            priority: req.priority,
            region: req.region,
            machine_id: req.machine_id,
        };

        // 调用 token_manager 添加凭据
        let credential_id = self
            .token_manager
            .add_credential(new_cred)
            .await
            .map_err(|e| self.classify_add_error(e))?;

        Ok(AddCredentialResponse {
            success: true,
            message: format!("凭据添加成功，ID: {}", credential_id),
            credential_id,
        })
    }

    /// 删除凭据
    pub fn delete_credential(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .delete_credential(id)
            .map_err(|e| self.classify_delete_error(e, id))
    }

    /// 批量导入凭据
    pub async fn batch_import_credentials(
        &self,
        req: BatchImportRequest,
    ) -> Result<BatchImportResponse, AdminServiceError> {
        // 限制：最多 1000 个 token
        const MAX_BATCH_SIZE: usize = 1000;
        // 限制：单个 token 最大 4KB
        const MAX_TOKEN_LENGTH: usize = 4096;
        // Token 最小长度（refresh_token 通常 > 100 字符）
        const MIN_TOKEN_LENGTH: usize = 100;

        if req.tokens.len() > MAX_BATCH_SIZE {
            return Err(AdminServiceError::InvalidCredential(format!(
                "批量导入数量超限：最多支持 {} 个，实际 {} 个",
                MAX_BATCH_SIZE,
                req.tokens.len()
            )));
        }

        // 获取现有凭据的 refresh_token 用于去重
        let existing_tokens: std::collections::HashSet<String> = self
            .token_manager
            .snapshot()
            .entries
            .iter()
            .filter_map(|e| {
                // 提取 refresh_token 的前 64 字符作为指纹（避免存储完整 token）
                self.token_manager
                    .get_refresh_token_fingerprint(e.id)
            })
            .collect();

        // 预处理：解析并验证所有 token
        struct ParsedToken {
            line: usize,
            token: String,
        }
        let mut parsed_tokens: Vec<ParsedToken> = Vec::new();
        let mut results = Vec::new();
        let mut skipped = 0usize;
        let mut failed = 0usize;

        // 用于检测批次内重复
        let mut seen_fingerprints: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (index, raw_token) in req.tokens.iter().enumerate() {
            let line = index + 1;
            let token = raw_token.trim();

            // 跳过空行
            if token.is_empty() {
                skipped += 1;
                continue;
            }

            // 验证：单 token 最大长度
            if token.len() > MAX_TOKEN_LENGTH {
                let err_msg = format!("Token 过长：{} 字符，最大允许 {} 字符", token.len(), MAX_TOKEN_LENGTH);
                if req.skip_invalid {
                    failed += 1;
                    results.push(BatchImportResultItem {
                        line,
                        status: "failed".to_string(),
                        credential_id: None,
                        error: Some(err_msg),
                    });
                    continue;
                } else {
                    return Err(AdminServiceError::InvalidCredential(format!(
                        "第 {} 行 {}", line, err_msg
                    )));
                }
            }

            // 验证：必须包含 : 分隔符
            if !token.contains(':') {
                let err_msg = "Token 格式无效：缺少签名部分（应包含 : 分隔符）".to_string();
                if req.skip_invalid {
                    failed += 1;
                    results.push(BatchImportResultItem {
                        line,
                        status: "failed".to_string(),
                        credential_id: None,
                        error: Some(err_msg),
                    });
                    continue;
                } else {
                    return Err(AdminServiceError::InvalidCredential(format!(
                        "第 {} 行 {}", line, err_msg
                    )));
                }
            }

            // 验证：最小长度
            if token.len() < MIN_TOKEN_LENGTH {
                let err_msg = format!(
                    "Token 过短：{} 字符，有效 token 通常超过 {} 字符",
                    token.len(), MIN_TOKEN_LENGTH
                );
                if req.skip_invalid {
                    failed += 1;
                    results.push(BatchImportResultItem {
                        line,
                        status: "failed".to_string(),
                        credential_id: None,
                        error: Some(err_msg),
                    });
                    continue;
                } else {
                    return Err(AdminServiceError::InvalidCredential(format!(
                        "第 {} 行 {}", line, err_msg
                    )));
                }
            }

            // 检测重复：使用前 64 字符作为指纹
            let fingerprint: String = token.chars().take(64).collect();

            // 检测批次内重复
            if seen_fingerprints.contains(&fingerprint) {
                let err_msg = "Token 重复：与本批次中的其他 token 重复".to_string();
                if req.skip_invalid {
                    failed += 1;
                    results.push(BatchImportResultItem {
                        line,
                        status: "failed".to_string(),
                        credential_id: None,
                        error: Some(err_msg),
                    });
                    continue;
                } else {
                    return Err(AdminServiceError::InvalidCredential(format!(
                        "第 {} 行 {}", line, err_msg
                    )));
                }
            }

            // 检测与现有凭据重复
            if existing_tokens.contains(&fingerprint) {
                let err_msg = "Token 重复：该凭据已存在于系统中".to_string();
                if req.skip_invalid {
                    failed += 1;
                    results.push(BatchImportResultItem {
                        line,
                        status: "failed".to_string(),
                        credential_id: None,
                        error: Some(err_msg),
                    });
                    continue;
                } else {
                    return Err(AdminServiceError::InvalidCredential(format!(
                        "第 {} 行 {}", line, err_msg
                    )));
                }
            }

            seen_fingerprints.insert(fingerprint);
            parsed_tokens.push(ParsedToken {
                line,
                token: token.to_string(),
            });
        }

        // 如果 skipInvalid=false 且有验证失败，前面已经返回错误
        // 到这里说明所有 token 都通过了基本验证

        // 执行导入
        let mut imported = 0usize;
        for parsed in parsed_tokens {
            let new_cred = KiroCredentials {
                id: None,
                access_token: None,
                refresh_token: Some(parsed.token),
                profile_arn: None,
                expires_at: None,
                auth_method: Some(req.auth_method.clone()),
                client_id: None,
                client_secret: None,
                priority: 0,
                region: None,
                machine_id: None,
            };

            match self.token_manager.add_credential(new_cred).await {
                Ok(credential_id) => {
                    imported += 1;
                    results.push(BatchImportResultItem {
                        line: parsed.line,
                        status: "success".to_string(),
                        credential_id: Some(credential_id),
                        error: None,
                    });
                }
                Err(e) => {
                    if req.skip_invalid {
                        failed += 1;
                        results.push(BatchImportResultItem {
                            line: parsed.line,
                            status: "failed".to_string(),
                            credential_id: None,
                            error: Some(e.to_string()),
                        });
                    } else {
                        return Err(AdminServiceError::InvalidCredential(format!(
                            "第 {} 行导入失败: {}",
                            parsed.line, e
                        )));
                    }
                }
            }
        }

        // 按行号排序结果
        results.sort_by_key(|r| r.line);

        let total = req.tokens.len();
        let success = imported > 0 || (failed == 0 && skipped == total);
        let message = if imported > 0 {
            format!("批量导入完成，成功 {} 个", imported)
        } else if failed > 0 {
            "批量导入失败，无有效凭据".to_string()
        } else {
            "无有效 token 可导入".to_string()
        };

        Ok(BatchImportResponse {
            success,
            message,
            total,
            imported,
            failed,
            skipped,
            results,
        })
    }

    /// 分类简单操作错误（set_disabled, set_priority, reset_and_enable）
    fn classify_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();
        if msg.contains("不存在") {
            AdminServiceError::NotFound { id }
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类余额查询错误（可能涉及上游 API 调用）
    fn classify_balance_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();

        // 1. 凭据不存在
        if msg.contains("不存在") {
            return AdminServiceError::NotFound { id };
        }

        // 2. 上游服务错误特征：HTTP 响应错误或网络错误
        let is_upstream_error =
            // HTTP 响应错误（来自 refresh_*_token 的错误消息）
            msg.contains("凭证已过期或无效") ||
            msg.contains("权限不足") ||
            msg.contains("已被限流") ||
            msg.contains("服务器错误") ||
            msg.contains("Token 刷新失败") ||
            msg.contains("暂时不可用") ||
            // 网络错误（reqwest 错误）
            msg.contains("error trying to connect") ||
            msg.contains("connection") ||
            msg.contains("timeout") ||
            msg.contains("timed out");

        if is_upstream_error {
            AdminServiceError::UpstreamError(msg)
        } else {
            // 3. 默认归类为内部错误（本地验证失败、配置错误等）
            // 包括：缺少 refreshToken、refreshToken 已被截断、无法生成 machineId 等
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类添加凭据错误
    fn classify_add_error(&self, e: anyhow::Error) -> AdminServiceError {
        let msg = e.to_string();

        // 凭据验证失败（refreshToken 无效、格式错误等）
        let is_invalid_credential = msg.contains("缺少 refreshToken")
            || msg.contains("refreshToken 为空")
            || msg.contains("refreshToken 已被截断")
            || msg.contains("凭证已过期或无效")
            || msg.contains("权限不足")
            || msg.contains("已被限流");

        if is_invalid_credential {
            AdminServiceError::InvalidCredential(msg)
        } else if msg.contains("error trying to connect")
            || msg.contains("connection")
            || msg.contains("timeout")
        {
            AdminServiceError::UpstreamError(msg)
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类删除凭据错误
    fn classify_delete_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();
        if msg.contains("不存在") {
            AdminServiceError::NotFound { id }
        } else if msg.contains("只能删除已禁用的凭据") {
            AdminServiceError::InvalidCredential(msg)
        } else {
            AdminServiceError::InternalError(msg)
        }
    }
}
