# kiro-rs

一个用 Rust 编写的 Anthropic Claude API 兼容代理服务，将 Anthropic API 请求转换为 Kiro API 请求。

## 免责声明
本项目仅供研究使用, Use at your own risk, 使用本项目所导致的任何后果由使用人承担, 与本项目无关。
本项目与 AWS/KIRO/Anthropic/Claude 等官方无关, 本项目不代表官方立场。

## 注意！
因 TLS 默认从 native-tls 切换至 rustls，你可能需要专门安装证书后才能配置 HTTP 代理。可通过 `config.json` 的 `tlsBackend` 切回 `native-tls`。
如果遇到请求报错, 尤其是无法刷新 token, 或者是直接返回 error request, 请尝试切换 tls 后端为 `native-tls`, 一般即可解决。

**Write Failed/会话卡死**: 如果遇到持续的 Write File / Write Failed 并导致会话不可用，参考 Issue [#22](https://github.com/hank9999/kiro.rs/issues/22) 和 [#49](https://github.com/hank9999/kiro.rs/issues/49) 的说明与临时解决方案（通常与输出过长被截断有关，可尝试调低输出相关 token 上限）

## 功能特性

- **Anthropic API 兼容**: 完整支持 Anthropic Claude API 格式
- **流式响应**: 支持 SSE (Server-Sent Events) 流式输出
- **Token 自动刷新**: 自动管理和刷新 OAuth Token
- **多凭据支持**: 支持配置多个凭据，按优先级自动故障转移
- **智能重试**: 单凭据最多重试 3 次，单请求最多重试 9 次
- **凭据回写**: 多凭据格式下自动回写刷新后的 Token
- **Thinking 模式**: 支持 Claude 的 extended thinking 功能
- **工具调用**: 完整支持 function calling / tool use
- **多模型支持**: 支持 Sonnet、Opus、Haiku 系列模型

## 支持的 API 端点

| 端点 | 方法 | 描述          |
|------|------|-------------|
| `/v1/models` | GET | 获取可用模型列表    |
| `/v1/messages` | POST | 创建消息（对话）    |
| `/v1/messages/count_tokens` | POST | 估算 Token 数量 |

## 快速开始

> **前置步骤**：编译前需要先构建前端 Admin UI（用于嵌入到二进制中）：
> ```bash
> cd admin-ui && pnpm install && pnpm build
> ```

### 1. 编译项目

```bash
cargo build --release
```

### 2. 配置文件

创建 `config.json` 配置文件：

```json
{
   "host": "127.0.0.1",   // 必配, 监听地址
   "port": 8990,  // 必配, 监听端口
   "apiKey": "sk-kiro-rs-qazWSXedcRFV123456",  // 必配, 请求的鉴权 token
   "region": "us-east-1",  // 必配, 区域, 一般保持默认即可
   "tlsBackend": "rustls", // 可选, TLS 后端: rustls / native-tls
   "kiroVersion": "0.8.0",  // 可选, 用于自定义请求特征, 不需要请删除: kiro ide 版本
   "machineId": "如果你需要自定义机器码请将64位机器码填到这里", // 可选, 用于自定义请求特征, 不需要请删除: 机器码
   "systemVersion": "darwin#24.6.0",  // 可选, 用于自定义请求特征, 不需要请删除: 系统版本
   "nodeVersion": "22.21.1",  // 可选, 用于自定义请求特征, 不需要请删除: node 版本
   "proxyUrl": "http://127.0.0.1:7890", // 可选, HTTP/SOCK5代理, 不需要请删除
   "proxyUsername": "user",  // 可选, HTTP/SOCK5代理用户名, 不需要请删除
   "proxyPassword": "pass",  // 可选, HTTP/SOCK5代理密码, 不需要请删除
   "adminApiKey": "sk-admin-your-secret-key"  // 可选, Admin API 密钥, 用于启用凭据管理 API, 填写后才会启用web管理， 不需要请删除
}
```
最小启动配置为: 
```json
{
   "host": "127.0.0.1",
   "port": 8990,
   "apiKey": "sk-kiro-rs-qazWSXedcRFV123456",
   "region": "us-east-1",
   "tlsBackend": "rustls"
}
```
### 3. 凭证文件

创建 `credentials.json` 凭证文件（从 Kiro IDE 获取）。支持两种格式：

#### 单凭据格式（旧格式，向后兼容）

```json
{
   "accessToken": "这里是请求token 一般有效期一小时",  // 可选, 不需要请删除, 可以自动刷新
   "refreshToken": "这里是刷新token 一般有效期7-30天不等",  // 必配, 根据实际填写
   "profileArn": "这是profileArn, 如果没有请你删除该字段， 配置应该像这个 arn:aws:codewhisperer:us-east-1:111112222233:profile/QWER1QAZSDFGH",  // 可选, 不需要请删除
   "expiresAt": "这里是请求token过期时间, 一般格式是这样2025-12-31T02:32:45.144Z, 在过期前 kirors 不会请求刷新请求token",  // 必配, 不确定你需要写一个已经过期的UTC时间
   "authMethod": "这里是认证方式 social / idc",  // 必配, IdC/Builder-ID/IAM 三类用户统一填写 idc
   "clientId": "如果你是 IdC 登录 需要配置这个",  // 可选, 不需要请删除
   "clientSecret": "如果你是 IdC 登录 需要配置这个"  // 可选, 不需要请删除
}
```

#### 多凭据格式（新格式，支持故障转移和自动回写）

```json
[
   {
      "refreshToken": "第一个凭据的刷新token",
      "expiresAt": "2025-12-31T02:32:45.144Z",
      "authMethod": "social",
      "priority": 0
   },
   {
      "refreshToken": "第二个凭据的刷新token",
      "expiresAt": "2025-12-31T02:32:45.144Z",
      "authMethod": "idc",
      "clientId": "xxxxxxxxx",
      "clientSecret": "xxxxxxxxx",
      "region": "us-east-2",
      "priority": 1
   }
]
```

> **多凭据特性说明**：
> - 按 `priority` 字段排序，数字越小优先级越高（默认为 0）
> - 单凭据最多重试 3 次，单请求最多重试 9 次
> - 自动故障转移到下一个可用凭据
> - 多凭据格式下 Token 刷新后自动回写到源文件
> - 可选的 `region` 字段：用于 OIDC token 刷新时指定 endpoint 区域，未配置时回退到 config.json 的 region
> - 可选的 `machineId` 字段：凭据级机器码；未配置时回退到 config.json 的 machineId；都未配置时由 refreshToken 派生

最小启动配置(social):
```json
{
   "refreshToken": "XXXXXXXXXXXXXXXX",
   "expiresAt": "2025-12-31T02:32:45.144Z",
   "authMethod": "social"
}
```

最小启动配置(idc):
```json
{
   "refreshToken": "XXXXXXXXXXXXXXXX",
   "expiresAt": "2025-12-31T02:32:45.144Z",
   "authMethod": "idc",
   "clientId": "xxxxxxxxx",
   "clientSecret": "xxxxxxxxx"
}
```
### 4. 启动服务

```bash
./target/release/kiro-rs
```

或指定配置文件路径：

```bash
./target/release/kiro-rs -c /path/to/config.json --credentials /path/to/credentials.json
```

### 5. 使用 API

```bash
curl http://127.0.0.1:8990/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: sk-your-custom-api-key" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "Hello, Claude!"}
    ]
  }'
```

## 配置说明

### config.json

| 字段 | 类型 | 默认值 | 描述                      |
|------|------|--------|-------------------------|
| `host` | string | `127.0.0.1` | 服务监听地址                  |
| `port` | number | `8080` | 服务监听端口                  |
| `apiKey` | string | - | 自定义 API Key（用于客户端认证，必配） |
| `region` | string | `us-east-1` | AWS 区域                  |
| `kiroVersion` | string | `0.8.0` | Kiro 版本号                |
| `machineId` | string | - | 自定义机器码（64位十六进制）不定义则自动生成 |
| `systemVersion` | string | 随机 | 系统版本标识                  |
| `nodeVersion` | string | `22.21.1` | Node.js 版本标识            |
| `tlsBackend` | string | `rustls` | TLS 后端：`rustls` 或 `native-tls` |
| `countTokensApiUrl` | string | - | 外部 count_tokens API 地址（可选） |
| `countTokensApiKey` | string | - | 外部 count_tokens API 密钥（可选） |
| `countTokensAuthType` | string | `x-api-key` | 外部 API 认证类型：`x-api-key` 或 `bearer` |
| `proxyUrl` | string | - | HTTP/SOCKS5 代理地址（可选） |
| `proxyUsername` | string | - | 代理用户名（可选） |
| `proxyPassword` | string | - | 代理密码（可选） |
| `adminApiKey` | string | - | Admin API 密钥，配置后启用凭据管理 API, 填写后才会启用web管理（可选） |

### credentials.json

支持单对象格式（向后兼容）或数组格式（多凭据）。

| 字段 | 类型 | 描述                      |
|------|------|-------------------------|
| `id` | number | 凭据唯一 ID（可选，仅用于 Admin API 管理；手写文件可不填） |
| `accessToken` | string | OAuth 访问令牌（可选，可自动刷新）    |
| `refreshToken` | string | OAuth 刷新令牌              |
| `profileArn` | string | AWS Profile ARN（可选，登录时返回） |
| `expiresAt` | string | Token 过期时间 (RFC3339)    |
| `authMethod` | string | 认证方式（`social` / `idc`） |
| `clientId` | string | IdC 登录的客户端 ID（可选）      |
| `clientSecret` | string | IdC 登录的客户端密钥（可选）      |
| `priority` | number | 凭据优先级，数字越小越优先，默认为 0（多凭据格式时有效）|
| `region` | string | 凭据级 region（可选），用于 OIDC token 刷新时指定 endpoint 的区域。未配置时回退到 config.json 的 region。注意：API 调用始终使用 config.json 的 region |
| `machineId` | string | 凭据级机器码（可选，64位十六进制）。未配置时回退到 config.json 的 machineId；都未配置时由 refreshToken 派生 |

说明：
- IdC / Builder-ID / IAM 在本项目里属于同一种登录方式，配置时统一使用 `authMethod: "idc"`
- 为兼容旧配置，`builder-id` / `iam` 仍可被识别，但会按 `idc` 处理

## 模型映射

| Anthropic 模型 | Kiro 模型 |
|----------------|-----------|
| `*sonnet*` | `claude-sonnet-4.5` |
| `*opus*` | `claude-opus-4.5` |
| `*haiku*` | `claude-haiku-4.5` |

## 项目结构

```
kiro-rs/
├── src/
│   ├── main.rs                 # 程序入口
│   ├── model/                  # 配置和参数模型
│   │   ├── config.rs           # 应用配置
│   │   └── arg.rs              # 命令行参数
│   ├── anthropic/              # Anthropic API 兼容层
│   │   ├── router.rs           # 路由配置
│   │   ├── handlers.rs         # 请求处理器
│   │   ├── middleware.rs       # 认证中间件
│   │   ├── types.rs            # 类型定义
│   │   ├── converter.rs        # 协议转换器
│   │   ├── stream.rs           # 流式响应处理
│   │   └── token.rs            # Token 估算
│   └── kiro/                   # Kiro API 客户端
│       ├── provider.rs         # API 提供者
│       ├── token_manager.rs    # Token 管理
│       ├── machine_id.rs       # 设备指纹生成
│       ├── model/              # 数据模型
│       │   ├── credentials.rs  # OAuth 凭证
│       │   ├── events/         # 响应事件类型
│       │   ├── requests/       # 请求类型
│       │   └── common/         # 共享类型
│       └── parser/             # AWS Event Stream 解析器
│           ├── decoder.rs      # 流式解码器
│           ├── frame.rs        # 帧解析
│           ├── header.rs       # 头部解析
│           └── crc.rs          # CRC 校验
├── Cargo.toml                  # 项目配置
├── config.example.json         # 配置示例
├── admin-ui/                   # Admin UI 前端工程（构建产物会嵌入二进制）
├── tools/                      # 辅助工具
└── Dockerfile                  # Docker 构建文件
```

## 技术栈

- **Web 框架**: [Axum](https://github.com/tokio-rs/axum) 0.8
- **异步运行时**: [Tokio](https://tokio.rs/)
- **HTTP 客户端**: [Reqwest](https://github.com/seanmonstar/reqwest)
- **序列化**: [Serde](https://serde.rs/)
- **日志**: [tracing](https://github.com/tokio-rs/tracing)
- **命令行**: [Clap](https://github.com/clap-rs/clap)

## 高级功能

### Thinking 模式

支持 Claude 的 extended thinking 功能：

```json
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 16000,
  "thinking": {
    "type": "enabled",
    "budget_tokens": 10000
  },
  "messages": [...]
}
```

### 工具调用

完整支持 Anthropic 的 tool use 功能：

```json
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "tools": [
    {
      "name": "get_weather",
      "description": "获取指定城市的天气",
      "input_schema": {
        "type": "object",
        "properties": {
          "city": {"type": "string"}
        },
        "required": ["city"]
      }
    }
  ],
  "messages": [...]
}
```

### 流式响应

设置 `stream: true` 启用 SSE 流式响应：

```json
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "stream": true,
  "messages": [...]
}
```

## 认证方式

支持两种 API Key 认证方式：

1. **x-api-key Header**
   ```
   x-api-key: sk-your-api-key
   ```

2. **Authorization Bearer**
   ```
   Authorization: Bearer sk-your-api-key
   ```

## 环境变量

可通过环境变量配置日志级别：

```bash
RUST_LOG=debug ./target/release/kiro-rs
```

## Token 计数

### Token 计数方法

kiro.rs 使用三层降级策略来确保准确的 token 计数：

1. **Claude 官方 Tokenizer**（推荐，~98% 准确度）
   - 使用 Hugging Face tokenizers 库和 Claude 官方 tokenizer
   - 零成本、零延迟、本地计算
   - 自动加载，无需配置

2. **外部 count_tokens API**（可选，100% 准确度）
   - 调用 Anthropic 官方 count_tokens API
   - 需要配置 API 密钥和 URL
   - 有网络延迟和 API 成本

3. **简单估算**（回退方案，~85% 准确度）
   - 基于字符数的简单估算
   - 当 tokenizer 加载失败时自动启用

### 为什么准确的 Token 计数很重要？

当使用 Claude Code 等客户端时，准确的 token 计数至关重要：

- Claude Code 依赖 token 计数来触发自动对话压缩（auto-compact）
- 不准确的计数会导致对话超过 200K token 限制后才报错
- 准确的计数确保在接近限制前自动压缩，提升用户体验

### 默认行为（推荐）

kiro.rs 默认使用 Claude 官方 tokenizer，无需任何配置：

1. 启动时自动加载 `tokenizers/claude-tokenizer.json`
2. 如果加载成功，日志会显示：
   ```
   INFO kiro_rs::token: 成功加载 Claude tokenizer: tokenizers/claude-tokenizer.json
   ```
3. 所有 token 计数请求自动使用官方 tokenizer

**优势**：
- ✅ 零配置
- ✅ 零成本
- ✅ 零延迟
- ✅ ~98% 准确度
- ✅ 完全离线工作

### 可选：配置外部 API（高级用户）

如果需要 100% 准确度，可以配置 Anthropic 官方 count_tokens API：

```json
{
  "countTokensApiUrl": "https://api.anthropic.com/v1/messages/count_tokens",
  "countTokensApiKey": "sk-ant-your-anthropic-api-key",
  "countTokensAuthType": "x-api-key"
}
```

**注意**：
- 需要 Anthropic API 密钥（从 [console.anthropic.com](https://console.anthropic.com) 获取）
- 每次调用会产生 API 费用
- 增加约 100-200ms 延迟
- 优先级高于本地 tokenizer

### 验证 Token 计数

测试 token 计数功能：

```bash
curl http://127.0.0.1:8990/v1/messages/count_tokens \
  -H "Content-Type: application/json" \
  -H "x-api-key: sk-your-custom-api-key" \
  -d '{
    "model": "claude-sonnet-4-5-20250514",
    "messages": [
      {"role": "user", "content": "Hello, Claude!"}
    ]
  }'
```

**预期响应**：
```json
{"input_tokens": 8}
```

**日志输出**：
- 使用官方 tokenizer：`INFO kiro_rs::token: 成功加载 Claude tokenizer`
- 使用外部 API：`DEBUG kiro_rs::token: 远程 count_tokens API 返回: 8`
- 使用简单估算：`WARN kiro_rs::token: 无法加载 Claude tokenizer，将使用简单估算`

### 故障排查

#### 问题：日志显示 "无法加载 Claude tokenizer"

**可能原因**：
- `tokenizers/claude-tokenizer.json` 文件缺失或损坏
- 文件路径不正确

**解决方法**：
1. 检查 `tokenizers/` 目录是否存在
2. 确认 `claude-tokenizer.json` 文件存在且大小约 1.7MB
3. 如果文件缺失，从项目仓库重新下载
4. 系统会自动降级到简单估算，不影响正常使用

#### 问题：Token 计数不准确

**可能原因**：
- Tokenizer 加载失败，使用了简单估算
- 外部 API 配置错误

**解决方法**：
1. 检查日志确认使用的是哪种计数方法
2. 如果使用简单估算，确保 tokenizer 文件完整
3. 如果配置了外部 API，验证 API 密钥和 URL 是否正确

#### 问题：Claude Code 仍然在超过 200K token 后报错

**可能原因**：
- Token 计数方法不准确（使用了简单估算）
- 上游 Kiro API 返回的 token 计数不准确

**解决方法**：
1. 确认 tokenizer 已成功加载（查看启动日志）
2. 测试 `/v1/messages/count_tokens` 端点验证准确性
3. 如果问题持续，考虑配置外部 API 获得 100% 准确度

### 性能对比

| 方法 | 准确度 | 延迟 | 成本 | 网络依赖 |
|------|--------|------|------|----------|
| Claude Tokenizer | ~98% | <1ms | 免费 | 否 |
| 外部 API | 100% | ~150ms | 按调用计费 | 是 |
| 简单估算 | ~85% | <1ms | 免费 | 否 |

**推荐**：使用默认的 Claude Tokenizer，在准确度、性能和成本之间达到最佳平衡。

## 注意事项

1. **凭证安全**: 请妥善保管 `credentials.json` 文件，不要提交到版本控制
2. **Token 刷新**: 服务会自动刷新过期的 Token，无需手动干预
3. **WebSearch 工具**: 当 `tools` 列表仅包含一个 `web_search` 工具时，会走内置 WebSearch 转换逻辑
4. **Token 计数 API 密钥**: 如果配置了 `countTokensApiKey`，请同样妥善保管，不要泄露

## Admin（可选）

当 `config.json` 配置了非空 `adminApiKey` 时，会启用：

- **Admin API（认证同 API Key）**
  - `GET /api/admin/credentials` - 获取所有凭据状态
  - `POST /api/admin/credentials` - 添加新凭据
  - `DELETE /api/admin/credentials/:id` - 删除凭据
  - `POST /api/admin/credentials/:id/disabled` - 设置凭据禁用状态
  - `POST /api/admin/credentials/:id/priority` - 设置凭据优先级
  - `POST /api/admin/credentials/:id/reset` - 重置失败计数
  - `GET /api/admin/credentials/:id/balance` - 获取凭据余额

- **Admin UI**
  - `GET /admin` - 访问管理页面（需要在编译前构建 `admin-ui/dist`）

## License

MIT

## 致谢

本项目的实现离不开前辈的努力:  
 - [kiro2api](https://github.com/caidaoli/kiro2api)
 - [proxycast](https://github.com/aiclientproxy/proxycast)

本项目部分逻辑参考了以上的项目, 再次由衷的感谢!
