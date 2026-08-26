use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Parser)]
#[command(
    name = "agent-chat",
    version,
    about = "与 OpenAI 兼容的 Agent 服务进行命令行对话"
)]
struct Cli {
    /// YAML 配置文件路径
    #[arg(short = 'c', long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// 服务端 IP 或主机名，例如 127.0.0.1
    #[arg(short = 'i', long, global = true)]
    ip: Option<String>,

    /// 服务端端口
    #[arg(short, long, global = true)]
    port: Option<u16>,

    /// 请求使用的模型名称
    #[arg(short, long, global = true)]
    model: Option<String>,

    /// 请求超时时间（秒）
    #[arg(long, global = true)]
    timeout: Option<u64>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 开启交互式对话模式
    Chat,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    ip: Option<String>,
    port: Option<u16>,
    model: Option<String>,
    timeout: Option<u64>,
}

#[derive(Debug)]
struct AppConfig {
    ip: String,
    port: u16,
    model: String,
    timeout: u64,
}

impl AppConfig {
    fn from_cli(cli: &Cli) -> Result<Self, Box<dyn std::error::Error>> {
        let file_config = cli
            .config
            .as_deref()
            .map(load_file_config)
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            ip: cli
                .ip
                .clone()
                .or(file_config.ip)
                .unwrap_or_else(|| "127.0.0.1".to_owned()),
            port: cli.port.or(file_config.port).unwrap_or(8031),
            model: cli
                .model
                .clone()
                .or(file_config.model)
                .unwrap_or_else(|| "Qwen2.5-0.5B-Instruct".to_owned()),
            timeout: cli.timeout.or(file_config.timeout).unwrap_or(300),
        })
    }
}

fn load_file_config(path: &Path) -> Result<FileConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("无法读取配置文件 {}: {error}", path.display()))?;
    serde_yaml::from_str(&content)
        .map_err(|error| format!("无法解析 YAML 配置文件 {}: {error}", path.display()).into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = match AppConfig::from_cli(&cli) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("配置错误: {error}");
            std::process::exit(1);
        }
    };

    // 省略子命令时也进入对话模式，方便直接执行 agent-chat。
    match cli.command.as_ref().unwrap_or(&Command::Chat) {
        Command::Chat => {
            if let Err(error) = run_chat(&config).await {
                eprintln!("错误: {error}");
                std::process::exit(1);
            }
        }
    }
}

async fn run_chat(config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let base = format_base_url(&config.ip, config.port);
    let endpoint = format!("{base}/v1/chat/completions");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout))
        .build()?;
    let mut messages = Vec::new();

    println!("Agent Chat");
    println!("服务端: {endpoint}");
    println!("模型: {}", config.model);
    println!("输入消息开始对话，输入 /clear 清空上下文，输入 /quit 或 Ctrl-D 退出。\n");

    loop {
        print!("你> ");
        io::stdout().flush()?;

        let mut input = String::new();
        let read = io::stdin().read_line(&mut input)?;
        if read == 0 {
            println!();
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        match input {
            "/quit" | "/exit" => break,
            "/clear" => {
                messages.clear();
                println!("[已清空对话上下文]");
                continue;
            }
            _ => {}
        }

        messages.push(Message {
            role: "user".to_owned(),
            content: input.to_owned(),
        });

        print!("助手> ");
        io::stdout().flush()?;
        match send_message(&client, &endpoint, &config.model, &mut messages).await {
            Ok(answer) => {
                println!("{answer}\n");
                messages.push(Message {
                    role: "assistant".to_owned(),
                    content: answer,
                });
            }
            Err(error) => {
                println!();
                eprintln!("请求失败: {error}");
                // 服务端失败时移除刚刚追加的 user 消息，避免下一轮带着未成功消息继续。
                messages.pop();
            }
        }
    }

    println!("再见！");
    Ok(())
}

async fn send_message(
    client: &reqwest::Client,
    endpoint: &str,
    model: &str,
    messages: &mut Vec<Message>,
) -> Result<String, Box<dyn std::error::Error>> {
    loop {
        let request = ChatRequest {
            model,
            messages,
            stream: false,
        };
        let response = client.post(endpoint).json(&request).send().await?;
        let status = response.status();
        let body = response.text().await?;

        if status != StatusCode::OK {
            if is_context_size_error(&body) && drop_oldest_turn(messages) {
                eprintln!("\n[上下文超过服务端限制，已移除最早一轮对话并重试]");
                continue;
            }
            return Err(format!("HTTP {status}: {}", compact_body(&body)).into());
        }

        let parsed: ChatResponse = serde_json::from_str(&body).map_err(|error| {
            format!(
                "无法解析服务端 JSON: {error}; 响应: {}",
                compact_body(&body)
            )
        })?;
        return parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .ok_or_else(|| "服务端响应中没有 choices[0].message.content".into());
    }
}

fn is_context_size_error(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| json.get("error").cloned())
        .and_then(|error| error.get("code").and_then(Value::as_str).map(str::to_owned))
        .is_some_and(|code| code == "exceed_context_size_error")
}

fn drop_oldest_turn(messages: &mut Vec<Message>) -> bool {
    // 当前最后一条是刚追加的 user 消息，因此只删除最早的 user/assistant 对。
    if messages.len() >= 3 {
        messages.drain(0..2);
        true
    } else {
        false
    }
}

fn format_base_url(ip: &str, port: u16) -> String {
    // 支持裸 IPv6 地址，例如 ::1。
    let host = if ip.contains(':') && !ip.starts_with('[') {
        format!("[{ip}]")
    } else {
        ip.to_owned()
    };
    format!("http://{host}:{port}")
}

fn compact_body(body: &str) -> String {
    const MAX_LEN: usize = 500;
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_LEN {
        compact
    } else {
        format!("{}...", compact.chars().take(MAX_LEN).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_context_size_error() {
        let body = r#"{"error":{"code":"exceed_context_size_error"}}"#;
        assert!(is_context_size_error(body));
    }

    #[test]
    fn drops_oldest_user_assistant_turn() {
        let mut messages = vec![
            Message {
                role: "user".into(),
                content: "one".into(),
            },
            Message {
                role: "assistant".into(),
                content: "ONE".into(),
            },
            Message {
                role: "user".into(),
                content: "two".into(),
            },
        ];

        assert!(drop_oldest_turn(&mut messages));
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "two");
    }

    #[test]
    fn command_line_values_override_yaml_and_defaults_fill_missing_values() {
        let path = std::env::temp_dir().join(format!(
            "agent-chat-config-test-{}.yaml",
            std::process::id()
        ));
        std::fs::write(&path, "ip: 192.168.1.10\nport: 9000\nmodel: test-model\n").unwrap();

        let cli = Cli {
            config: Some(path.clone()),
            ip: Some("10.0.0.2".into()),
            port: None,
            model: None,
            timeout: Some(10),
            command: None,
        };
        let config = AppConfig::from_cli(&cli).unwrap();

        assert_eq!(config.ip, "10.0.0.2");
        assert_eq!(config.port, 9000);
        assert_eq!(config.model, "test-model");
        assert_eq!(config.timeout, 10);
        std::fs::remove_file(path).unwrap();
    }
}
