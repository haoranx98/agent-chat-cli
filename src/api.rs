use futures_util::StreamExt;
use reqwest::{Response, StatusCode};
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

#[derive(Debug, Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

pub async fn send_message(
    client: &reqwest::Client,
    endpoint: &str,
    model: &str,
    messages: &mut Vec<Message>,
    stream: bool,
    mut on_chunk: impl FnMut(&str),
) -> Result<String, Box<dyn std::error::Error>> {
    loop {
        let request = ChatRequest {
            model,
            messages,
            stream,
        };
        let response = client.post(endpoint).json(&request).send().await?;

        if response.status() != StatusCode::OK {
            let status = response.status();
            let body = response.text().await?;
            if is_context_size_error(&body) && drop_oldest_turn(messages) {
                eprintln!("\n[上下文超过服务端限制，已移除最早一轮对话并重试]");
                continue;
            }
            return Err(format!("HTTP {status}: {}", compact_body(&body)).into());
        }

        if stream {
            return read_stream(response, &mut on_chunk).await;
        }

        let body = response.text().await?;
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

async fn read_stream(
    response: Response,
    on_chunk: &mut impl FnMut(&str),
) -> Result<String, Box<dyn std::error::Error>> {
    let mut bytes = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut answer = String::new();

    while let Some(chunk) = bytes.next().await {
        buffer.extend_from_slice(&chunk?);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = buffer.drain(..=newline).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8(line)?;
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    continue;
                }
                let event: StreamResponse = serde_json::from_str(data)
                    .map_err(|error| format!("无法解析流式响应 JSON: {error}; 数据: {data}"))?;
                if let Some(content) = event
                    .choices
                    .into_iter()
                    .next()
                    .and_then(|choice| choice.delta.content)
                {
                    on_chunk(&content);
                    answer.push_str(&content);
                }
            }
        }
    }

    if !buffer.iter().all(|byte| byte.is_ascii_whitespace()) {
        let remaining = String::from_utf8_lossy(&buffer);
        return Err(format!("流式响应缺少换行结束符: {}", compact_body(&remaining)).into());
    }
    Ok(answer)
}

fn is_context_size_error(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| json.get("error").cloned())
        .and_then(|error| error.get("code").and_then(Value::as_str).map(str::to_owned))
        .is_some_and(|code| code == "exceed_context_size_error")
}

fn drop_oldest_turn(messages: &mut Vec<Message>) -> bool {
    // 保留 system 消息；当前最后一条是刚追加的 user 消息。
    let start = usize::from(
        messages
            .first()
            .is_some_and(|message| message.role == "system"),
    );
    if messages.len() >= start + 3 {
        messages.drain(start..start + 2);
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
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
    fn preserves_system_message_when_dropping_oldest_turn() {
        let mut messages = vec![
            Message {
                role: "system".into(),
                content: "system".into(),
            },
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
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].content, "two");
    }

    #[tokio::test]
    async fn sends_and_parses_non_stream_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_json(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "system", "content": "system"}, {"role": "user", "content": "测试"}],
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "板卡有输出"}}]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let mut messages = vec![
            Message {
                role: "system".into(),
                content: "system".into(),
            },
            Message {
                role: "user".into(),
                content: "测试".into(),
            },
        ];
        let answer = send_message(
            &client,
            &format!("{}/v1/chat/completions", server.uri()),
            "test-model",
            &mut messages,
            false,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(answer, "板卡有输出");
    }

    #[tokio::test]
    async fn sends_and_collects_stream_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_json(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "测试"}],
                "stream": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"choices\":[{\"delta\":{\"content\":\"板卡\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"输出\"}}]}\n\ndata: [DONE]\n\n",
            ))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let mut messages = vec![Message {
            role: "user".into(),
            content: "测试".into(),
        }];
        let mut chunks = Vec::new();
        let answer = send_message(
            &client,
            &format!("{}/v1/chat/completions", server.uri()),
            "test-model",
            &mut messages,
            true,
            |chunk| chunks.push(chunk.to_owned()),
        )
        .await
        .unwrap();
        assert_eq!(answer, "板卡输出");
        assert_eq!(chunks, ["板卡", "输出"]);
    }
}
