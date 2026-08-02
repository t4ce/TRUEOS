use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use remoteai::{BackendError, BackendReply, CapturedToolCall, ChatBackend, ServerConfig, app};
use serde_json::{Value, json};
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;

const TOKEN: &str = "test-token-with-at-least-24-bytes";

#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for LogCapture {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

struct FakeBackend;

struct BusyBackend;

#[async_trait]
impl ChatBackend for FakeBackend {
    async fn complete(
        &self,
        chat: remoteai::openai::NormalizedChat,
    ) -> Result<BackendReply, BackendError> {
        if chat.tools.is_empty() {
            Ok(BackendReply::Text("Dobby moved, felt joy, and greeted the user.".to_string()))
        } else {
            Ok(BackendReply::Tool(CapturedToolCall {
                id: "call_dobby_1".to_string(),
                name: "move".to_string(),
                arguments: json!({"x": 0.25, "y": 0.75}),
            }))
        }
    }
}

#[async_trait]
impl ChatBackend for BusyBackend {
    async fn complete(
        &self,
        _chat: remoteai::openai::NormalizedChat,
    ) -> Result<BackendReply, BackendError> {
        Err(BackendError::Busy)
    }
}

fn router() -> axum::Router {
    app(
        ServerConfig {
            bearer_token: TOKEN.to_string(),
            advertised_model: "auto".to_string(),
        },
        Arc::new(FakeBackend),
    )
    .unwrap()
}

fn busy_router() -> axum::Router {
    app(
        ServerConfig {
            bearer_token: TOKEN.to_string(),
            advertised_model: "auto".to_string(),
        },
        Arc::new(BusyBackend),
    )
    .unwrap()
}

async fn post(body: Value, token: &str) -> (StatusCode, Value) {
    let response = router()
        .oneshot(
            Request::post("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn dobby_tools() -> Value {
    json!([
        {"type":"function","function":{"name":"text","description":"Speak briefly.","strict":true,"parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"play_emotion","description":"Show emotion.","strict":true,"parameters":{"type":"object","properties":{"idea":{"type":"string"}},"required":["idea"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"move","description":"Move.","strict":true,"parameters":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false}}}
    ])
}

#[tokio::test]
async fn exact_dobby_tool_shape_is_returned() {
    let (status, response) = post(
        json!({
            "model":"auto",
            "messages":[{"role":"system","content":"You are Dobby."},{"role":"user","content":"Move."}],
            "tools":dobby_tools(),
            "tool_choice":"required",
            "parallel_tool_calls":false,
            "max_completion_tokens":256,
            "stream":false
        }),
        TOKEN,
    ).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(response["choices"][0]["message"]["tool_calls"][0]["function"]["name"], "move");
    let arguments = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(arguments).unwrap()["x"], 0.25);
}

#[tokio::test]
async fn summary_is_plain_assistant_content() {
    let (status, response) = post(
        json!({
            "model":"auto",
            "messages":[{"role":"system","content":"You are Dobby."},{"role":"user","content":"Summarize."}],
            "max_completion_tokens":500,
            "stream":false
        }),
        TOKEN,
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap()
            .contains("Dobby moved")
    );
    assert!(
        response["choices"][0]["message"]
            .get("tool_calls")
            .is_none()
    );
}

#[tokio::test]
async fn wrong_bearer_token_is_openai_style_401() {
    let (status, response) =
        post(json!({"model":"auto","messages":[{"role":"user","content":"hi"}]}), "wrong").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(response["error"]["code"], "invalid_api_key");
}

#[tokio::test]
async fn busy_backend_is_openai_style_429() {
    let response = busy_router()
        .oneshot(
            Request::post("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::from(
                    serde_json::to_vec(
                        &json!({"model":"auto","messages":[{"role":"user","content":"hi"}]}),
                    )
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "busy");
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_logs_contain_only_sanitized_request_metadata() {
    const PRIVATE_PROMPT: &str = "prompt-must-never-appear-in-remoteai-logs";

    let capture = LogCapture::default();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_writer(capture.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("install log capture subscriber");

    let (status, _) = post(
        json!({
            "model":"auto",
            "messages":[{"role":"user","content":PRIVATE_PROMPT}],
            "stream":false
        }),
        TOKEN,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let failed = busy_router()
        .oneshot(
            Request::post("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "model":"auto",
                        "messages":[{"role":"user","content":PRIVATE_PROMPT}],
                        "stream":false
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::TOO_MANY_REQUESTS);

    let logs = capture.text();
    assert!(logs.contains("chat completion started"), "logs={logs:?}");
    assert!(logs.contains("chat completion completed"), "logs={logs:?}");
    assert!(logs.contains("chat completion failed"), "logs={logs:?}");
    assert!(logs.contains("request_id=chatcmpl-remoteai-"));
    assert!(logs.contains("model=auto"));
    assert!(!logs.contains(PRIVATE_PROMPT));
    assert!(!logs.contains(TOKEN));
    assert!(!logs.contains("Dobby moved, felt joy"));
}
