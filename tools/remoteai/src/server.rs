use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, FromRequest, Request, State, rejection::JsonRejection};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Serialize;
use serde_json::json;

use crate::copilot::{BackendError, BackendReply, ChatBackend, MAX_ORDERED_TOOL_CALLS};
use crate::openai::{
    AssistantFunctionCall, AssistantMessage, AssistantToolCall, ChatCompletionRequest,
    ChatCompletionResponse, Choice, Usage, ValidationError,
};

const MAX_BODY_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct ServerConfig {
    pub bearer_token: String,
    pub advertised_model: String,
}

impl ServerConfig {
    pub fn validate(&self) -> Result<(), String> {
        let token = self.bearer_token.as_bytes();
        if !(24..=2_048).contains(&token.len())
            || token.iter().any(|byte| byte.is_ascii_control())
            || self.bearer_token.contains("REPLACE_")
            || self.bearer_token.contains("ENTER_")
        {
            return Err("bearer_token must be a real 24..=2048 byte secret".to_string());
        }
        if self.advertised_model.trim().is_empty() || self.advertised_model.len() > 128 {
            return Err("model must be non-empty and no longer than 128 bytes".to_string());
        }
        Ok(())
    }
}

#[derive(Clone)]
struct AppState {
    backend: Arc<dyn ChatBackend>,
    advertised_model: String,
    sequence: Arc<AtomicU64>,
}

pub fn app(config: ServerConfig, backend: Arc<dyn ChatBackend>) -> Result<Router, String> {
    config.validate()?;
    let state = AppState {
        backend,
        advertised_model: config.advertised_model,
        sequence: Arc::new(AtomicU64::new(1)),
    };
    Ok(Router::new()
        .route("/healthz", get(health))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status":"ok","service":"remoteAI"}))
}

async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize(&state, &headers)?;
    Ok(Json(json!({
        "object": "list",
        "data": [{
            "id": state.advertised_model,
            "object": "model",
            "created": 0,
            "owned_by": "github-copilot"
        }]
    })))
}

async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    OpenAiJson(request): OpenAiJson<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, ApiError> {
    authorize(&state, &headers)?;
    let chat = request.normalize().map_err(ApiError::validation)?;
    let requested_model = chat.requested_model.clone();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let sequence = state.sequence.fetch_add(1, Ordering::Relaxed);
    let request_id = format!("chatcmpl-remoteai-{created:x}-{sequence:x}");
    let mut trace = CompletionTrace::start(request_id.clone(), requested_model.clone());
    let reply = match state.backend.complete(chat).await {
        Ok(reply) => reply,
        Err(error) => {
            trace.failed();
            return Err(ApiError::backend(error));
        }
    };
    let (message, finish_reason) = match reply {
        BackendReply::Text(content) => (
            AssistantMessage {
                role: "assistant",
                content: Some(content),
                tool_calls: None,
            },
            "stop",
        ),
        BackendReply::Tool(call) => (
            AssistantMessage {
                role: "assistant",
                content: None,
                tool_calls: Some(serialize_tool_calls(vec![call]).inspect_err(|_| {
                    trace.failed();
                })?),
            },
            "tool_calls",
        ),
        BackendReply::Tools(calls) => {
            if calls.is_empty() || calls.len() > MAX_ORDERED_TOOL_CALLS {
                trace.failed();
                return Err(ApiError::internal());
            }
            (
                AssistantMessage {
                    role: "assistant",
                    content: None,
                    tool_calls: Some(serialize_tool_calls(calls).inspect_err(|_| {
                        trace.failed();
                    })?),
                },
                "tool_calls",
            )
        }
    };
    let response = ChatCompletionResponse {
        id: request_id,
        object: "chat.completion",
        created,
        model: if requested_model.is_empty() {
            state.advertised_model
        } else {
            requested_model
        },
        choices: vec![Choice {
            index: 0,
            message,
            finish_reason,
        }],
        usage: Usage::default(),
    };
    trace.completed();
    Ok(Json(response))
}

fn serialize_tool_calls(
    calls: Vec<crate::copilot::CapturedToolCall>,
) -> Result<Vec<AssistantToolCall>, ApiError> {
    calls
        .into_iter()
        .map(|call| {
            Ok(AssistantToolCall {
                id: call.id,
                kind: "function",
                function: AssistantFunctionCall {
                    name: call.name,
                    arguments: serde_json::to_string(&call.arguments)
                        .map_err(|_| ApiError::internal())?,
                },
            })
        })
        .collect()
}

struct CompletionTrace {
    request_id: String,
    model: String,
    finished: bool,
}

impl CompletionTrace {
    fn start(request_id: String, model: String) -> Self {
        tracing::info!(request_id = %request_id, model = %model, "chat completion started");
        Self {
            request_id,
            model,
            finished: false,
        }
    }

    fn completed(&mut self) {
        tracing::info!(
            request_id = %self.request_id,
            model = %self.model,
            "chat completion completed"
        );
        self.finished = true;
    }

    fn failed(&mut self) {
        tracing::warn!(
            request_id = %self.request_id,
            model = %self.model,
            "chat completion failed"
        );
        self.finished = true;
    }
}

impl Drop for CompletionTrace {
    fn drop(&mut self) {
        if !self.finished {
            tracing::warn!(
                request_id = %self.request_id,
                model = %self.model,
                "chat completion cancelled"
            );
        }
    }
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let _ = state;
    let _ = headers;
    Ok(())
}

struct OpenAiJson<T>(T);

impl<S, T> FromRequest<S> for OpenAiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(ApiError::json)
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn json(error: JsonRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_json", error.body_text())
    }

    fn validation(error: ValidationError) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", error.to_string())
    }

    fn backend(error: BackendError) -> Self {
        if matches!(error, BackendError::Busy) {
            return Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                "another remoteAI completion is already in progress",
            );
        }
        Self::new(StatusCode::BAD_GATEWAY, "upstream_error", "Copilot did not complete the request")
    }

    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "remoteAI could not serialize the response",
        )
    }
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    message: String,
    #[serde(rename = "type")]
    kind: &'static str,
    code: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorEnvelope {
            error: ErrorBody {
                message: self.message,
                kind: "remoteai_error",
                code: self.code,
            },
        });
        (self.status, body).into_response()
    }
}
