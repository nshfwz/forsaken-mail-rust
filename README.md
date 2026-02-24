# 即收即毁 (Rust)

按 `go-rewrite` 等价逻辑实现的 Rust 版本，包含：

- SMTP 收件服务（默认 `:25`）
- HTTP API（默认 `:3000`）
- 临时邮箱前端页面（`/`）
- 按邮箱查询邮件列表/正文 API
- 前端静态资源编译时内嵌到二进制（不依赖外部 `public` 目录）

## 当前版本

- `1.0.0`

## 启动

```bash
cargo run --release
```

访问：

- 前端：`http://127.0.0.1:3000`
- 健康检查：`http://127.0.0.1:3000/api/health`

## API 示例

```bash
curl "http://127.0.0.1:3000/api/mailboxes/demo/messages"
curl "http://127.0.0.1:3000/api/mailboxes/demo/messages/{message_id}"
curl "http://127.0.0.1:3000/api/messages?email=demo@example.com"
curl "http://127.0.0.1:3000/api/messages/{message_id}?email=demo@example.com"
```

## 邮件保留策略

- 内存保存（进程重启清空）
- 默认每邮箱最多 `200` 封
- 默认过期时间 `24h`（`MESSAGE_TTL_MINUTES=1440`）
- 默认每分钟清理一次过期邮件

## 环境变量

- `HTTP_ADDR`：HTTP 监听地址，默认 `:3000`
- `SMTP_ADDR`：SMTP 监听地址，默认 `:25`
- `MAIL_DOMAIN`：限制收件域名（可选）
- `MAILBOX_BLACKLIST`：邮箱前缀黑名单，逗号分隔
- `BANNED_SENDER_DOMAINS`：拒收发件域名，逗号分隔
- `MAX_MESSAGES_PER_MAILBOX`：每邮箱保留上限，默认 `200`
- `MESSAGE_TTL_MINUTES`：邮件过期分钟数，默认 `1440`
- `MAX_MESSAGE_BYTES`：单封邮件最大字节数，默认 `10485760`

## 构建

```bash
cargo build --release
```

二进制产物：

```text
target/release/forsaken-mail-rust
```

## 自动发布 Release

- GitHub Actions 监听 tag：`v*`
- 推送版本 tag（例如 `v1.0.0`）后会自动：
  - 构建多平台二进制（Linux/Windows/macOS Intel/macOS Apple Silicon）
  - 生成 `checksums.txt`
  - 上传到对应 GitHub Release
