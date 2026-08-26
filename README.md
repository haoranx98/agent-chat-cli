# agent-chat

一个使用 Rust 编写的 OpenAI 兼容接口 CLI 测试助手，用于检查 NPU 板卡上的推理服务是否正常产生输出。默认请求：

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

配置模板见 [`config.example.yaml`](config.example.yaml)。如果不指定 `--config`，程序会使用可执行文件同级目录下的 `config.yaml`。

配置文件行为如下：

- 没有配置文件、也没有命令行参数：使用内置默认值，并自动生成 `config.yaml`；
- 没有配置文件、指定了命令行参数：使用命令行参数和默认值生成 `config.yaml`，然后运行；
- 已有配置文件、没有指定命令行参数：读取配置文件运行，缺失字段使用默认值；
- 已有配置文件、指定了命令行参数：命令行参数覆盖对应配置，并回写配置文件后运行。

配置文件路径和命令行参数示例：

支持的 YAML 配置项：

```yaml
ip: 127.0.0.1
port: 8031
model: Qwen2.5-0.5B-Instruct
# system: 你是一个 NPU 推理测试助手，请客观报告输出结果。
timeout: 300
stream: false
```

其中 `system` 为可选的系统提示词，`stream` 控制是否使用 OpenAI 兼容的 SSE 流式输出。

对话中：

- 上方向键/下方向键切换当前运行期间的历史输入；
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

Release 构建已启用体积优化：使用 size 优先优化、LTO、单 codegen unit、`panic = "abort"`，并通过 `strip = "symbols"` 移除调试符号表。当前构建产物适合发布和部署，不包含调试信息；调试时请使用默认的 debug 构建。

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

工作流文件位于 `.github/workflows/release.yml`，也可以通过 GitHub Actions 的 `workflow_dispatch` 手动触发。`.github/workflows/ci.yml` 会独立执行格式检查、Clippy 和测试。

Release 工作流只在 `main` 推送时根据版本号变化自动运行，也可以在 Actions 页面手动输入已有标签来补传 Release 资产；同一版本的构建通过并发控制串行执行。
