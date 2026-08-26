use std::io::{self, Write};
use std::time::Duration;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use crate::api::{self, Message};
use crate::config::AppConfig;

pub async fn run(config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let base = format_base_url(&config.ip, config.port);
    let endpoint = format!("{base}/v1/chat/completions");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout))
        .build()?;
    let mut messages = initial_messages(config);
    let mut editor = DefaultEditor::new()?;

    println!("Agent Chat");
    println!("服务端: {endpoint}");
    println!("模型: {}", config.model);
    println!(
        "输入消息开始对话，上下方向键切换历史输入，输入 /clear 清空上下文，输入 /quit 或 Ctrl-D 退出。\n"
    );

    loop {
        let input = match editor.readline("你> ") {
            Ok(input) => input,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(error) => return Err(error.into()),
        };

        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        editor.add_history_entry(input)?;

        match input {
            "/quit" | "/exit" => break,
            "/clear" => {
                clear_messages(&mut messages);
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
        let mut streamed = false;
        match api::send_message(
            &client,
            &endpoint,
            &config.model,
            &mut messages,
            config.stream,
            |chunk| {
                streamed = true;
                print!("{chunk}");
                let _ = io::stdout().flush();
            },
        )
        .await
        {
            Ok(answer) => {
                if streamed {
                    println!("\n");
                } else {
                    println!("{answer}\n");
                }
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

fn initial_messages(config: &AppConfig) -> Vec<Message> {
    config
        .system
        .as_ref()
        .map(|system| {
            vec![Message {
                role: "system".to_owned(),
                content: system.clone(),
            }]
        })
        .unwrap_or_default()
}

fn clear_messages(messages: &mut Vec<Message>) {
    if messages
        .first()
        .is_some_and(|message| message.role == "system")
    {
        messages.truncate(1);
    } else {
        messages.clear();
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
