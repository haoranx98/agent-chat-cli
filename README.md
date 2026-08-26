# agent-chat

一个使用 Rust 编写的 OpenAI 兼容接口 CLI 对话客户端。默认请求：

```text
http://127.0.0.1:8031/v1/chat/completions
```

## 使用

```bash
cargo run -- --ip 127.0.0.1 --port 8031 chat
```

也可以省略 `chat`：

```bash
agent-chat -i 192.168.1.20 -p 8031 -m Qwen2.5-0.5B-Instruct
```

也可以通过 YAML 文件配置：

```bash
agent-chat --config config.yaml chat
```

配置模板见 [`config.example.yaml`](config.example.yaml)。参数优先级为：命令行参数 > YAML 配置文件 > 内置默认值。YAML 中未填写的参数会继续使用内置默认值。

支持的 YAML 配置项：

```yaml
ip: 127.0.0.1
port: 8031
model: Qwen2.5-0.5B-Instruct
timeout: 300
```

对话中：

- `/clear` 清空历史消息；
- `/quit` 或 `/exit` 退出；
- 每次请求都发送完整历史消息，保持上下文。
- 如果服务端返回 `exceed_context_size_error`，程序会自动删除最早一轮对话并重试；也可以主动使用 `/clear`。

## musl 静态编译

先安装目标和 musl 工具链（以 Debian/Ubuntu 为例）：

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt install musl-tools
```

然后构建：

```bash
cargo build --release --target x86_64-unknown-linux-musl
```

生成文件：

```text
target/x86_64-unknown-linux-musl/release/agent-chat
```

该程序使用 Rustls，不依赖 OpenSSL；在工具链正确配置时可生成静态 musl 可执行文件。

## GitHub Actions 自动发布

修改 `Cargo.toml` 中的 `[package] version` 并推送到 `main` 后，GitHub Actions 会自动：

1. 安装 musl 工具链；
2. 构建 `x86_64-unknown-linux-musl` 静态可执行文件；
3. 生成 SHA256 校验文件；
4. 创建对应的 `vX.Y.Z` GitHub Release 并上传可执行文件。

发布文件名类似：

```text
agent-chat-v0.2.0-x86_64-unknown-linux-musl
agent-chat-v0.2.0-x86_64-unknown-linux-musl.sha256
```

工作流文件位于 `.github/workflows/release.yml`，也可以通过 GitHub Actions 的 `workflow_dispatch` 手动触发。

工作流也支持直接推送 `vX.Y.Z` 标签，或者在 Actions 页面手动输入已有标签来补传 Release 资产。
