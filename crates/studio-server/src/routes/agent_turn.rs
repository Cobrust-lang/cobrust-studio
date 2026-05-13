//! `POST /api/agent-turn` — ADR-0012 bounded agent loop over built-in tools.

use std::convert::Infallible;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures::StreamExt;
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use studio_router::{
    CompletionRequest, DispatchContext, Message, Role, SamplingParams, TokenUsage,
};

use crate::AppState;
use crate::agent_loop::{
    AgentDirective, ToolCall, ToolExecution, TotalTokens, default_tools_allowed, execute_tool,
    normalise_tools_allowed, parse_agent_directive, tool_protocol_message,
};
use crate::error::RouteError;
use crate::routes::dispatch;

const DEFAULT_MAX_ITERATIONS: u8 = 16;
const MAX_ITERATIONS: u8 = 16;

#[derive(Debug, Deserialize)]
pub struct AgentTurnRequest {
    pub model: String,
    #[serde(default)]
    pub system: String,
    pub messages: Vec<AgentTurnMessage>,
    #[serde(default)]
    pub params: SamplingParams,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u8,
    #[serde(default = "default_tools_allowed")]
    pub tools_allowed: Vec<String>,
    #[serde(default)]
    pub task_tag: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AgentTurnMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct IterationPayload {
    n: u8,
    model: String,
    text: String,
    usage: TokenUsage,
    cache_hit: bool,
    stop_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct ToolCallPayload<'a> {
    iteration: u8,
    tool: &'a str,
    input: &'a serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ToolResultPayload<'a> {
    iteration: u8,
    tool: &'a str,
    output: &'a serde_json::Value,
    error: &'a Option<crate::agent_loop::ToolExecutionError>,
    ms: u128,
}

#[derive(Debug, Serialize)]
struct DonePayload {
    final_text: String,
    iterations: u8,
    total_tokens: TotalTokens,
    task_tag: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    error: String,
    code: &'static str,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(agent_turn_sse))
}

pub async fn agent_turn_sse(
    State(state): State<AppState>,
    payload: Result<Json<AgentTurnRequest>, JsonRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    let router = dispatch::resolve_router(&state).await?;
    let req = match payload {
        Ok(Json(req)) => req,
        Err(e) => {
            return Err(RouteError::bad_request(e.body_text(), "invalid_body").into_response());
        }
    };
    let loop_request =
        validate_request(req, state.enable_write_tools).map_err(IntoResponse::into_response)?;
    let project_root = state.project_root().to_path_buf();
    let write_enabled = state.enable_write_tools;

    let stream =
        stream::once(
            async move { run_loop(&project_root, write_enabled, router, loop_request).await },
        )
        .flat_map(|events| stream::iter(events.into_iter().map(Ok::<_, Infallible>)));

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Debug)]
struct LoopRequest {
    model: String,
    params: SamplingParams,
    messages: Vec<Message>,
    max_iterations: u8,
    tools_allowed: std::collections::BTreeSet<String>,
    task_tag: Option<String>,
}

fn validate_request(req: AgentTurnRequest, write_enabled: bool) -> Result<LoopRequest, RouteError> {
    if req.model.trim().is_empty() {
        return Err(RouteError::bad_request(
            "model must be non-empty",
            "invalid_body",
        ));
    }
    if req.messages.is_empty() {
        return Err(RouteError::bad_request(
            "messages must contain at least one entry",
            "invalid_body",
        ));
    }
    if req.max_iterations == 0 || req.max_iterations > MAX_ITERATIONS {
        return Err(RouteError::bad_request(
            format!("max_iterations must be between 1 and {MAX_ITERATIONS}"),
            "invalid_body",
        ));
    }
    let tools_allowed = normalise_tools_allowed(req.tools_allowed, write_enabled)
        .map_err(|e| RouteError::bad_request(e.to_string(), e.code()))?;
    let task_tag = validate_task_tag(req.task_tag)?;
    let mut messages = Vec::new();
    messages.push(Message {
        role: Role::System,
        content: tool_protocol_message(&tools_allowed),
    });
    if !req.system.trim().is_empty() {
        messages.push(Message {
            role: Role::System,
            content: req.system,
        });
    }
    for (i, message) in req.messages.into_iter().enumerate() {
        messages.push(Message {
            role: parse_role(&message.role).ok_or_else(|| {
                RouteError::bad_request(
                    format!("messages[{i}].role must be system|user|assistant"),
                    "invalid_body",
                )
            })?,
            content: message.content,
        });
    }
    Ok(LoopRequest {
        model: req.model,
        params: req.params,
        messages,
        max_iterations: req.max_iterations,
        tools_allowed,
        task_tag,
    })
}

async fn run_loop(
    project_root: &std::path::Path,
    write_enabled: bool,
    router: std::sync::Arc<studio_router::Router>,
    mut req: LoopRequest,
) -> Vec<Event> {
    let mut events = Vec::new();
    let mut totals = TotalTokens::zero();

    for n in 0..req.max_iterations {
        let dispatch_req = CompletionRequest {
            model: req.model.clone(),
            messages: req.messages.clone(),
            params: req.params.clone(),
        };
        let ctx = DispatchContext {
            task_tag: iteration_tag(req.task_tag.as_deref(), n),
            ..DispatchContext::default()
        };
        let response = match router.dispatch_ctx(dispatch_req, ctx).await {
            Ok(response) => response,
            Err(e) => {
                events.push(error_event(&e.to_string(), "router_failed"));
                return events;
            }
        };
        totals.add(response.response.usage);
        let text = response.response.text.clone();
        let directive = parse_agent_directive(&text);
        let stop_reason = match &directive {
            AgentDirective::Final { .. } => "end_turn",
            AgentDirective::ToolUse { .. } => "tool_use",
        };
        events.push(iteration_event(IterationPayload {
            n,
            model: response.response.model.clone(),
            text: text.clone(),
            usage: response.response.usage,
            cache_hit: response.cache_hit,
            stop_reason,
        }));
        req.messages.push(Message {
            role: Role::Assistant,
            content: text,
        });
        match directive {
            AgentDirective::Final { text } => {
                events.push(done_event(DonePayload {
                    final_text: text,
                    iterations: n.saturating_add(1),
                    total_tokens: totals,
                    task_tag: req.task_tag,
                }));
                return events;
            }
            AgentDirective::ToolUse { calls } => {
                let tool_message = execute_calls(
                    project_root,
                    write_enabled,
                    &req.tools_allowed,
                    n,
                    calls,
                    &mut events,
                )
                .await;
                req.messages.push(Message {
                    role: Role::User,
                    content: tool_message,
                });
            }
        }
    }

    events.push(done_event(DonePayload {
        final_text: "max iterations reached".to_string(),
        iterations: req.max_iterations,
        total_tokens: totals,
        task_tag: req.task_tag,
    }));
    events
}

async fn execute_calls(
    project_root: &std::path::Path,
    write_enabled: bool,
    allowed: &std::collections::BTreeSet<String>,
    iteration: u8,
    calls: Vec<ToolCall>,
    events: &mut Vec<Event>,
) -> String {
    let mut results = Vec::with_capacity(calls.len());
    for call in calls {
        events.push(tool_call_event(iteration, &call));
        let execution = execute_tool(project_root, write_enabled, allowed, &call).await;
        events.push(tool_result_event(iteration, &execution));
        results.push(execution);
    }
    serde_json::to_string(&serde_json::json!({ "tool_results": results }))
        .unwrap_or_else(|_| "{\"tool_results\":[]}".to_string())
}

fn parse_role(role: &str) -> Option<Role> {
    match role {
        "system" => Some(Role::System),
        "user" => Some(Role::User),
        "assistant" => Some(Role::Assistant),
        _ => None,
    }
}

fn validate_task_tag(task_tag: Option<String>) -> Result<Option<String>, RouteError> {
    let Some(task_tag) = task_tag else {
        return Ok(None);
    };
    if task_tag.is_empty() {
        return Ok(None);
    }
    if task_tag.len() > 256 {
        return Err(RouteError::bad_request(
            "task_tag must be <= 256 bytes",
            "task_tag_too_long",
        ));
    }
    if task_tag.chars().any(char::is_control) {
        return Err(RouteError::bad_request(
            "task_tag must not contain control characters",
            "task_tag_invalid_chars",
        ));
    }
    Ok(Some(task_tag))
}

fn iteration_tag(parent: Option<&str>, n: u8) -> Option<String> {
    parent.map_or_else(
        || Some(format!("agent-iter-{n}")),
        |tag| Some(format!("{tag}-iter-{n}")),
    )
}

const fn default_max_iterations() -> u8 {
    DEFAULT_MAX_ITERATIONS
}

fn iteration_event(payload: IterationPayload) -> Event {
    json_event("iteration", &payload)
}

fn tool_call_event(iteration: u8, call: &ToolCall) -> Event {
    json_event(
        "tool_call",
        &ToolCallPayload {
            iteration,
            tool: &call.tool,
            input: &call.input,
        },
    )
}

fn tool_result_event(iteration: u8, execution: &ToolExecution) -> Event {
    json_event(
        "tool_result",
        &ToolResultPayload {
            iteration,
            tool: &execution.tool,
            output: &execution.output,
            error: &execution.error,
            ms: execution.ms,
        },
    )
}

fn done_event(payload: DonePayload) -> Event {
    json_event("done", &payload)
}

fn error_event(message: &str, code: &'static str) -> Event {
    json_event(
        "error",
        &ErrorPayload {
            error: message.to_string(),
            code,
        },
    )
}

fn json_event<T: Serialize>(event: &'static str, payload: &T) -> Event {
    let body = serde_json::to_string(payload)
        .unwrap_or_else(|_| r#"{"error":"serialise failed","code":"internal_error"}"#.to_string());
    Event::default().event(event).data(body)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn max_iterations_rejects_zero_and_over_cap() {
        let mut req = sample_request();
        req.max_iterations = 0;
        assert!(validate_request(req, false).is_err());
        let mut req = sample_request();
        req.max_iterations = 17;
        assert!(validate_request(req, false).is_err());
    }

    #[test]
    fn shell_exec_requires_write_policy() {
        let mut req = sample_request();
        req.tools_allowed = vec!["shell.exec".to_string()];
        let err = validate_request(req, false).unwrap_err();
        assert!(err.to_string().contains("requires --enable-write-tools"));
    }

    fn sample_request() -> AgentTurnRequest {
        AgentTurnRequest {
            model: "synthetic-1".to_string(),
            system: String::new(),
            messages: vec![AgentTurnMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            params: SamplingParams::default(),
            max_iterations: 1,
            tools_allowed: default_tools_allowed(),
            task_tag: None,
        }
    }
}
