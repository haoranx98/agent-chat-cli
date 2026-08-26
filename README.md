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
