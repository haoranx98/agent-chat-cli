use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
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

pub async fn send_message(
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
