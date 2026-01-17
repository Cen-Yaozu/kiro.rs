# Token Counting Configuration - Testing Guide

This guide helps you verify that the count_tokens API configuration is working correctly.

## Prerequisites

- kiro.rs is installed and configured
- You have a valid Anthropic API key
- `config.json` includes count_tokens configuration

## Test Scenarios

### Scenario 1: Without Configuration (Baseline)

**Purpose**: Verify that local estimation works as fallback

**Steps**:
1. Remove or comment out count_tokens fields from `config.json`:
   ```json
   {
     "host": "127.0.0.1",
     "port": 8990,
     "apiKey": "sk-kiro-rs-qazWSXedcRFV123456",
     "region": "us-east-1"
   }
   ```

2. Start kiro.rs:
   ```bash
   ./target/release/kiro-rs
   ```

3. Send a test request:
   ```bash
   curl http://127.0.0.1:8990/v1/messages/count_tokens \
     -H "Content-Type: application/json" \
     -H "x-api-key: sk-kiro-rs-qazWSXedcRFV123456" \
     -d '{
       "model": "claude-sonnet-4-20250514",
       "messages": [
         {"role": "user", "content": "Hello, Claude! How are you today?"}
       ]
     }'
   ```

**Expected Result**:
- Request succeeds
- Returns token count (estimated locally)
- No "远程 count_tokens API" messages in logs

---

### Scenario 2: With Valid Configuration

**Purpose**: Verify that remote API is called successfully

**Steps**:
1. Add count_tokens configuration to `config.json`:
   ```json
   {
     "host": "127.0.0.1",
     "port": 8990,
     "apiKey": "sk-kiro-rs-qazWSXedcRFV123456",
     "region": "us-east-1",
     "countTokensApiUrl": "https://api.anthropic.com/v1/messages/count_tokens",
     "countTokensApiKey": "sk-ant-your-actual-api-key",
     "countTokensAuthType": "x-api-key"
   }
   ```

2. Start kiro.rs with debug logging:
   ```bash
   RUST_LOG=debug ./target/release/kiro-rs
   ```

3. Send the same test request as Scenario 1

**Expected Result**:
- Request succeeds
- Returns accurate token count from Anthropic API
- Log shows: `DEBUG kiro_rs::token: 远程 count_tokens API 返回: <number>`

**Example log output**:
```
2026-01-17T10:30:45.123456Z  INFO kiro_rs::anthropic::handlers: Received POST /v1/messages/count_tokens request model=claude-sonnet-4-20250514 message_count=1
2026-01-17T10:30:45.234567Z DEBUG kiro_rs::token: 远程 count_tokens API 返回: 15
```

---

### Scenario 3: With Invalid API Key

**Purpose**: Verify graceful fallback to local estimation

**Steps**:
1. Configure with an invalid API key:
   ```json
   {
     "countTokensApiUrl": "https://api.anthropic.com/v1/messages/count_tokens",
     "countTokensApiKey": "sk-ant-invalid-key-12345",
     "countTokensAuthType": "x-api-key"
   }
   ```

2. Start kiro.rs with warning level logging:
   ```bash
   RUST_LOG=warn ./target/release/kiro-rs
   ```

3. Send a test request

**Expected Result**:
- Request still succeeds (fallback to local estimation)
- Log shows warning: `WARN kiro_rs::token: 远程 count_tokens API 调用失败，回退到本地计算: API 返回错误状态: 401`
- Returns estimated token count

---

### Scenario 4: End-to-End with Claude Code

**Purpose**: Verify that Claude Code's auto-compact works correctly

**Prerequisites**:
- Claude Code CLI installed
- kiro.rs configured with valid count_tokens API

**Steps**:
1. Configure Claude Code to use kiro.rs as API endpoint:
   ```bash
   export ANTHROPIC_API_KEY="sk-kiro-rs-qazWSXedcRFV123456"
   export ANTHROPIC_BASE_URL="http://127.0.0.1:8990"
   ```

2. Start a long conversation with Claude Code:
   ```bash
   claude-code
   ```

3. Continue the conversation until it approaches 200K tokens

**Expected Result**:
- Claude Code automatically triggers `/compact` before hitting 200K limit
- No "400 Bad Request" errors occur
- Conversation continues smoothly after compaction

**Monitoring**:
Watch kiro.rs logs for token count updates:
```bash
tail -f /path/to/kiro-rs.log | grep "count_tokens"
```

---

## Validation Checklist

- [ ] Scenario 1: Local estimation works without configuration
- [ ] Scenario 2: Remote API is called with valid configuration
- [ ] Scenario 3: Graceful fallback occurs with invalid key
- [ ] Scenario 4: Claude Code auto-compact works correctly
- [ ] Documentation is clear and complete
- [ ] Configuration example is valid JSON
- [ ] All fields are properly explained

---

## Troubleshooting

### Issue: "远程 count_tokens API 调用失败"

**Check**:
1. API key validity:
   ```bash
   curl https://api.anthropic.com/v1/messages/count_tokens \
     -H "x-api-key: sk-ant-your-api-key" \
     -H "Content-Type: application/json" \
     -d '{"model":"claude-sonnet-4-20250514","messages":[{"role":"user","content":"test"}]}'
   ```

2. Network connectivity:
   ```bash
   ping api.anthropic.com
   ```

3. Proxy configuration (if using proxy):
   - Verify `proxyUrl` is correct
   - Test proxy connection

### Issue: Token counts don't match expectations

**Check**:
1. Verify remote API is being used:
   ```bash
   grep "远程 count_tokens API 返回" /path/to/kiro-rs.log
   ```

2. Compare with Anthropic's official count:
   - Use Anthropic Console's token counter
   - Or call the API directly (see above)

3. If using local estimation, consider configuring remote API

### Issue: High latency

**Expected**: Remote API adds ~100-200ms latency

**If latency is unacceptable**:
- Remove count_tokens configuration to use local estimation
- Or optimize network path (use closer proxy, better network)

---

## Performance Benchmarks

### Local Estimation
- Latency: < 1ms
- Accuracy: ~85-95% (varies by content type)

### Remote API
- Latency: ~100-200ms
- Accuracy: 100% (official Anthropic counting)

### Recommendation

Use remote API when:
- Using Claude Code or similar clients that rely on accurate token counts
- Conversations frequently approach 200K token limit
- Accuracy is more important than latency

Use local estimation when:
- Latency is critical
- Token counts are only used for rough estimates
- Not using auto-compact features

---

## Next Steps

After validation:
1. Monitor logs for any issues
2. Collect user feedback
3. Consider adding metrics for token counting accuracy
4. Document any edge cases discovered

---

## Support

If you encounter issues:
1. Check logs for error messages
2. Verify configuration against this guide
3. Test with minimal configuration first
4. Report issues with full logs and configuration (redact sensitive keys)
