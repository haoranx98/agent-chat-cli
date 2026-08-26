use std::io::{self, Write};
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
    /// 服务端 IP 或主机名，例如 127.0.0.1
    #[arg(short = 'i', long, default_value = "127.0.0.1", global = true)]
    ip: String,

    /// 服务端端口
    #[arg(short, long, default_value_t = 8031, global = true)]
    port: u16,

    /// 请求使用的模型名称
    #[arg(short, long, default_value = "Qwen2.5-0.5B-Instruct", global = true)]
    model: String,

    /// 请求超时时间（秒）
    #[arg(long, default_value_t = 300, global = true)]
    timeout: u64,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 开启交互式对话模式
    Chat,
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

    // 省略子命令时也进入对话模式，方便直接执行 agent-chat。
    match cli.command.as_ref().unwrap_or(&Command::Chat) {
        Command::Chat => {
            if let Err(error) = run_chat(&cli).await {
                eprintln!("错误: {error}");
                std::process::exit(1);
            }
        }
    }
}

async fn run_chat(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let base = format_base_url(&cli.ip, cli.port);
    let endpoint = format!("{base}/v1/chat/completions");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cli.timeout))
        .build()?;
    let mut messages = Vec::new();

    println!("Agent Chat");
    println!("服务端: {endpoint}");
    println!("模型: {}", cli.model);
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
        match send_message(&client, &endpoint, &cli.model, &mut messages).await {
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
}
