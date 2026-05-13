//! Built-in agent-loop tool execution primitives (ADR-0012).

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use studio_router::TokenUsage;
use tokio::process::Command;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_TREE_ENTRIES: usize = 10_000;
const DEFAULT_SHELL_TIMEOUT_MS: u64 = 30_000;
const MAX_SHELL_TIMEOUT_MS: u64 = 300_000;

const READ_TOOLS: &[&str] = &[
    "fs.read",
    "fs.list",
    "git.status",
    "git.diff",
    "project_tree",
];

const WRITE_TOOLS: &[&str] = &["fs.write", "fs.delete", "shell.exec"];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCall {
    pub tool: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolExecution {
    pub tool: String,
    pub output: Value,
    pub error: Option<ToolExecutionError>,
    pub ms: u128,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolExecutionError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TotalTokens {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl TotalTokens {
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
        }
    }

    pub fn add(&mut self, usage: TokenUsage) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(usage.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(usage.completion_tokens);
    }
}

#[derive(Clone, Debug)]
pub enum AgentDirective {
    Final { text: String },
    ToolUse { calls: Vec<ToolCall> },
}

#[derive(Debug, thiserror::Error)]
pub enum ToolRequestError {
    #[error("unknown tool {0:?}")]
    UnknownTool(String),
    #[error("tool {0:?} requires --enable-write-tools")]
    WriteToolDisabled(String),
}

impl ToolRequestError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownTool(_) => "unknown_tool",
            Self::WriteToolDisabled(_) => "tool_not_allowed",
        }
    }
}

#[must_use]
pub fn default_tools_allowed() -> Vec<String> {
    READ_TOOLS.iter().map(|tool| (*tool).to_string()).collect()
}

pub fn normalise_tools_allowed(
    requested: Vec<String>,
    write_enabled: bool,
) -> Result<BTreeSet<String>, ToolRequestError> {
    let mut allowed = BTreeSet::new();
    for tool in requested {
        let tool = tool.trim().to_string();
        if tool.is_empty() {
            continue;
        }
        if !is_known_tool(&tool) {
            return Err(ToolRequestError::UnknownTool(tool));
        }
        if is_write_tool(&tool) && !write_enabled {
            return Err(ToolRequestError::WriteToolDisabled(tool));
        }
        allowed.insert(tool);
    }
    Ok(allowed)
}

#[must_use]
pub fn tool_protocol_message(allowed: &BTreeSet<String>) -> String {
    let tools = allowed.iter().cloned().collect::<Vec<_>>().join(", ");
    format!(
        "You are running inside Cobrust Studio's agent loop. Available tools: {tools}. \
When you need a tool, reply with ONLY JSON like {{\"tool_calls\":[{{\"tool\":\"fs.list\",\"input\":{{\"dir\":\"/\"}}}}]}}. \
When finished, reply with ONLY JSON like {{\"final_text\":\"your answer\"}}."
    )
}

#[must_use]
pub fn parse_agent_directive(text: &str) -> AgentDirective {
    let Some(value) = parse_json_value(text) else {
        return AgentDirective::Final {
            text: text.to_string(),
        };
    };
    if let Some(final_text) = value.get("final_text").and_then(Value::as_str) {
        return AgentDirective::Final {
            text: final_text.to_string(),
        };
    }
    if let Some(calls) = parse_tool_calls(&value) {
        if !calls.is_empty() {
            return AgentDirective::ToolUse { calls };
        }
    }
    AgentDirective::Final {
        text: text.to_string(),
    }
}

pub async fn execute_tool(
    project_root: &Path,
    write_enabled: bool,
    allowed: &BTreeSet<String>,
    call: &ToolCall,
) -> ToolExecution {
    let start = Instant::now();
    let result = execute_tool_inner(project_root, write_enabled, allowed, call).await;
    let ms = start.elapsed().as_millis();
    match result {
        Ok(output) => ToolExecution {
            tool: call.tool.clone(),
            output,
            error: None,
            ms,
        },
        Err(error) => ToolExecution {
            tool: call.tool.clone(),
            output: Value::Null,
            error: Some(error),
            ms,
        },
    }
}

async fn execute_tool_inner(
    project_root: &Path,
    write_enabled: bool,
    allowed: &BTreeSet<String>,
    call: &ToolCall,
) -> Result<Value, ToolExecutionError> {
    if !is_known_tool(&call.tool) {
        return Err(tool_error("unknown_tool", "unknown tool"));
    }
    if !allowed.contains(&call.tool) {
        return Err(tool_error(
            "tool_not_allowed",
            "tool not allowed for this turn",
        ));
    }
    if is_write_tool(&call.tool) && !write_enabled {
        return Err(tool_error(
            "tool_not_allowed",
            "write/exec tools require --enable-write-tools",
        ));
    }

    match call.tool.as_str() {
        "fs.read" => fs_read(project_root, &call.input).await,
        "fs.list" => fs_list(project_root, &call.input).await,
        "git.status" => git_status(project_root).await,
        "git.diff" => git_diff(project_root, &call.input).await,
        "project_tree" => project_tree(project_root, &call.input).await,
        "fs.write" => fs_write(project_root, &call.input).await,
        "fs.delete" => fs_delete(project_root, &call.input).await,
        "shell.exec" => shell_exec(project_root, &call.input).await,
        _ => Err(tool_error("unknown_tool", "unknown tool")),
    }
}

async fn fs_read(project_root: &Path, input: &Value) -> Result<Value, ToolExecutionError> {
    let path = required_string(input, "path")?;
    let resolved = resolve_existing(project_root, &path).await?;
    let meta = tokio::fs::metadata(&resolved)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    if !meta.is_file() {
        return Err(tool_error("not_file", "path is not a file"));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(tool_error("file_too_large", "file exceeds 1 MiB cap"));
    }
    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    let content =
        String::from_utf8(bytes).map_err(|_| tool_error("non_utf8", "file is not valid UTF-8"))?;
    Ok(json!({ "path": display_relative(project_root, &resolved).await, "content": content }))
}

async fn fs_list(project_root: &Path, input: &Value) -> Result<Value, ToolExecutionError> {
    let dir = optional_string(input, "dir").unwrap_or_else(|| "/".to_string());
    let resolved = resolve_existing(project_root, &dir).await?;
    let mut reader = tokio::fs::read_dir(&resolved)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    let mut entries = Vec::new();
    while let Some(entry) = reader
        .next_entry()
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|e| tool_error("fs_error", e.to_string()))?;
        let kind = if file_type.is_dir() {
            "dir"
        } else if file_type.is_file() {
            "file"
        } else if file_type.is_symlink() {
            "symlink"
        } else {
            "other"
        };
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "kind": kind,
        }));
    }
    entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(json!({ "dir": display_relative(project_root, &resolved).await, "entries": entries }))
}

async fn git_status(project_root: &Path) -> Result<Value, ToolExecutionError> {
    run_command(
        "git",
        &["status", "--porcelain=v1", "--branch"],
        project_root,
        DEFAULT_SHELL_TIMEOUT_MS,
    )
    .await
}

async fn git_diff(project_root: &Path, input: &Value) -> Result<Value, ToolExecutionError> {
    let mut args = vec!["diff".to_string(), "--".to_string()];
    if let Some(paths) = input.get("paths").and_then(Value::as_array) {
        for path in paths {
            let Some(path) = path.as_str() else {
                return Err(tool_error("invalid_input", "paths must be strings"));
            };
            let rel = checked_relative_path(path)?;
            args.push(rel.to_string_lossy().into_owned());
        }
    }
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_command("git", &refs, project_root, DEFAULT_SHELL_TIMEOUT_MS).await
}

async fn project_tree(project_root: &Path, input: &Value) -> Result<Value, ToolExecutionError> {
    let max_depth = input
        .get("max_depth")
        .and_then(Value::as_u64)
        .unwrap_or(3)
        .min(8) as usize;
    let root = tokio::fs::canonicalize(project_root)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    let mut out = Vec::new();
    let mut stack = vec![(root.clone(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if out.len() >= MAX_TREE_ENTRIES {
            break;
        }
        let mut reader = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| tool_error("fs_error", e.to_string()))?;
        let mut children = Vec::new();
        while let Some(entry) = reader
            .next_entry()
            .await
            .map_err(|e| tool_error("fs_error", e.to_string()))?
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if should_skip_tree_entry(&name) {
                continue;
            }
            children.push(entry.path());
        }
        children.sort();
        for path in children.into_iter().rev() {
            if out.len() >= MAX_TREE_ENTRIES {
                break;
            }
            let meta = tokio::fs::metadata(&path)
                .await
                .map_err(|e| tool_error("fs_error", e.to_string()))?;
            let rel = path
                .strip_prefix(&root)
                .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
            out.push(if meta.is_dir() {
                format!("{rel}/")
            } else {
                rel
            });
            if meta.is_dir() && depth < max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
    Ok(json!({ "entries": out, "truncated": out.len() >= MAX_TREE_ENTRIES }))
}

async fn fs_write(project_root: &Path, input: &Value) -> Result<Value, ToolExecutionError> {
    let path = required_string(input, "path")?;
    let content = required_string(input, "content")?;
    if content.len() > usize::try_from(MAX_FILE_BYTES).unwrap_or(usize::MAX) {
        return Err(tool_error("file_too_large", "content exceeds 1 MiB cap"));
    }
    let resolved = resolve_write_path(project_root, &path).await?;
    tokio::fs::write(&resolved, content.as_bytes())
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    Ok(json!({ "path": display_relative(project_root, &resolved).await, "bytes": content.len() }))
}

async fn fs_delete(project_root: &Path, input: &Value) -> Result<Value, ToolExecutionError> {
    let path = required_string(input, "path")?;
    let resolved = resolve_existing(project_root, &path).await?;
    let meta = tokio::fs::metadata(&resolved)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    if !meta.is_file() {
        return Err(tool_error("not_file", "fs.delete only deletes files"));
    }
    if git_tracks_path(project_root, &resolved).await? {
        return Err(tool_error(
            "tracked_file_refused",
            "refusing to delete a git-tracked file",
        ));
    }
    tokio::fs::remove_file(&resolved)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    Ok(json!({ "path": display_relative(project_root, &resolved).await, "deleted": true }))
}

async fn shell_exec(project_root: &Path, input: &Value) -> Result<Value, ToolExecutionError> {
    let cmd = required_string(input, "cmd")?;
    if cmd.trim().is_empty() {
        return Err(tool_error("invalid_input", "cmd must be non-empty"));
    }
    if cmd.split_whitespace().any(|word| word == "sudo") {
        return Err(tool_error("sudo_refused", "sudo is not allowed"));
    }
    let timeout_ms = input
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_SHELL_TIMEOUT_MS)
        .min(MAX_SHELL_TIMEOUT_MS);
    let cwd = if let Some(cwd) = optional_string(input, "cwd") {
        let resolved = resolve_existing(project_root, &cwd).await?;
        let meta = tokio::fs::metadata(&resolved)
            .await
            .map_err(|e| tool_error("fs_error", e.to_string()))?;
        if !meta.is_dir() {
            return Err(tool_error("not_dir", "cwd is not a directory"));
        }
        resolved
    } else {
        tokio::fs::canonicalize(project_root)
            .await
            .map_err(|e| tool_error("fs_error", e.to_string()))?
    };
    run_shell(&cmd, &cwd, timeout_ms).await
}

async fn run_shell(cmd: &str, cwd: &Path, timeout_ms: u64) -> Result<Value, ToolExecutionError> {
    let output = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| tool_error("timeout", "command timed out"))?
    .map_err(|e| tool_error("process_error", e.to_string()))?;
    Ok(json!({
        "exit_code": output.status.code(),
        "stdout": truncate_output(&String::from_utf8_lossy(&output.stdout)),
        "stderr": truncate_output(&String::from_utf8_lossy(&output.stderr)),
    }))
}

async fn run_command(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout_ms: u64,
) -> Result<Value, ToolExecutionError> {
    let output = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        Command::new(program).args(args).current_dir(cwd).output(),
    )
    .await
    .map_err(|_| tool_error("timeout", "command timed out"))?
    .map_err(|e| tool_error("process_error", e.to_string()))?;
    Ok(json!({
        "exit_code": output.status.code(),
        "stdout": truncate_output(&String::from_utf8_lossy(&output.stdout)),
        "stderr": truncate_output(&String::from_utf8_lossy(&output.stderr)),
    }))
}

async fn git_tracks_path(project_root: &Path, path: &Path) -> Result<bool, ToolExecutionError> {
    let root = tokio::fs::canonicalize(project_root)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    let rel = path
        .strip_prefix(&root)
        .map_err(|_| tool_error("path_escape", "path is outside project root"))?;
    let output = Command::new("git")
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(rel)
        .current_dir(&root)
        .output()
        .await
        .map_err(|e| tool_error("process_error", e.to_string()))?;
    Ok(output.status.success())
}

async fn resolve_existing(project_root: &Path, input: &str) -> Result<PathBuf, ToolExecutionError> {
    let root = tokio::fs::canonicalize(project_root)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    let rel = checked_relative_path(input)?;
    let candidate = root.join(rel);
    let resolved = tokio::fs::canonicalize(&candidate)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    if !resolved.starts_with(&root) {
        return Err(tool_error("path_escape", "path is outside project root"));
    }
    Ok(resolved)
}

async fn resolve_write_path(
    project_root: &Path,
    input: &str,
) -> Result<PathBuf, ToolExecutionError> {
    let root = tokio::fs::canonicalize(project_root)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    let rel = checked_relative_path(input)?;
    let candidate = root.join(rel);
    let Some(parent) = candidate.parent() else {
        return Err(tool_error("invalid_path", "path has no parent"));
    };
    let parent = tokio::fs::canonicalize(parent)
        .await
        .map_err(|e| tool_error("fs_error", e.to_string()))?;
    if !parent.starts_with(&root) {
        return Err(tool_error("path_escape", "path is outside project root"));
    }
    let Some(file_name) = candidate.file_name() else {
        return Err(tool_error("invalid_path", "path must include a file name"));
    };
    Ok(parent.join(file_name))
}

fn checked_relative_path(input: &str) -> Result<PathBuf, ToolExecutionError> {
    let trimmed = input.trim();
    let rel = if trimmed.is_empty() || trimmed == "/" {
        PathBuf::from(".")
    } else if let Some(stripped) = trimmed.strip_prefix('/') {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(trimmed)
    };
    for component in rel.components() {
        match component {
            Component::CurDir | Component::Normal(_) => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(tool_error("path_escape", "path traversal is not allowed"));
            }
        }
    }
    Ok(rel)
}

async fn display_relative(project_root: &Path, path: &Path) -> String {
    let root = match tokio::fs::canonicalize(project_root).await {
        Ok(root) => root,
        Err(_) => return path.display().to_string(),
    };
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |p| format!("/{}", p.display()),
    )
}

fn parse_json_value(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(str::trim);
    if let Some(unfenced) = unfenced {
        if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
            return Some(value);
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str::<Value>(&trimmed[start..=end]).ok()
}

fn parse_tool_calls(value: &Value) -> Option<Vec<ToolCall>> {
    if let Some(items) = value.get("tool_calls").and_then(Value::as_array) {
        let calls = items
            .iter()
            .filter_map(|item| serde_json::from_value::<ToolCall>(item.clone()).ok())
            .collect::<Vec<_>>();
        return Some(calls);
    }
    if let Some(item) = value.get("tool_call") {
        return serde_json::from_value::<ToolCall>(item.clone())
            .ok()
            .map(|call| vec![call]);
    }
    None
}

fn is_known_tool(tool: &str) -> bool {
    READ_TOOLS.contains(&tool) || WRITE_TOOLS.contains(&tool)
}

fn is_write_tool(tool: &str) -> bool {
    WRITE_TOOLS.contains(&tool)
}

fn required_string(input: &Value, key: &'static str) -> Result<String, ToolExecutionError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| tool_error("invalid_input", format!("{key} must be a string")))
}

fn optional_string(input: &Value, key: &'static str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_string)
}

fn tool_error(code: &'static str, message: impl Into<String>) -> ToolExecutionError {
    ToolExecutionError {
        code,
        message: message.into(),
    }
}

fn truncate_output(text: &str) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text.to_string();
    }
    format!("{}\n[truncated]", &text[..MAX_OUTPUT_BYTES])
}

fn should_skip_tree_entry(name: &str) -> bool {
    matches!(name, ".git" | "target" | "node_modules" | ".svelte-kit")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_dir_paths() {
        let err = checked_relative_path("../Cargo.toml").unwrap_err();
        assert_eq!(err.code, "path_escape");
    }

    #[test]
    fn slash_means_project_root() {
        assert_eq!(checked_relative_path("/").unwrap(), PathBuf::from("."));
    }

    #[test]
    fn parses_final_text_json() {
        let directive = parse_agent_directive(r#"{"final_text":"done"}"#);
        match directive {
            AgentDirective::Final { text } => assert_eq!(text, "done"),
            AgentDirective::ToolUse { .. } => panic!("expected final"),
        }
    }

    #[test]
    fn parses_tool_calls_json() {
        let directive =
            parse_agent_directive(r#"{"tool_calls":[{"tool":"fs.list","input":{"dir":"/"}}]}"#);
        match directive {
            AgentDirective::ToolUse { calls } => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].tool, "fs.list");
            }
            AgentDirective::Final { .. } => panic!("expected tool call"),
        }
    }

    #[test]
    fn write_tool_requires_policy() {
        let err = normalise_tools_allowed(vec!["shell.exec".into()], false).unwrap_err();
        assert_eq!(err.code(), "tool_not_allowed");
    }
}
