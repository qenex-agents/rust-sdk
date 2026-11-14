//! The seat runtime tools: the execution surface of exactly one seat.
//!
//! Every tool answers for one seat only. The seat identity is attached to the
//! runtime at construction time and travels with every response and every
//! child process, so a caller can always attribute an action (R4).

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ErrorData},
    schemars, tool, tool_router,
};
use serde_json::json;
use tokio::process::Command;
use tokio::time::timeout;

use crate::paths::{Confinement, PathError};
use crate::seat::SeatBinding;

/// Largest file the runtime reads through `seat_read_file`.
const MAX_FILE_BYTES: u64 = 1_048_576;
/// Largest amount of stdout or stderr returned by `seat_exec`.
const MAX_OUTPUT_BYTES: usize = 1_048_576;
/// Default timeout of `seat_exec`.
const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// Upper bound of `seat_exec`.
const MAX_TIMEOUT_SECS: u64 = 600;

/// Programs a seat runtime may start.
const PROGRAM_ALLOWLIST: &[&str] = &[
    "bun",
    "node",
    "deno",
    "npm",
    "npx",
    "pnpm",
    "yarn",
    "python",
    "python3",
    "pip",
    "pip3",
    "cargo",
    "rustc",
    "go",
    "make",
    "cmake",
    "gcc",
    "cc",
    "git",
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "find",
    "mkdir",
    "touch",
    "cp",
    "mv",
    "sed",
    "tar",
    "unzip",
];

/// Git subcommands a seat runtime may run. Everything that writes to a
/// repository (commit, push, reset, checkout, ...) is refused.
const GIT_READONLY_SUBCOMMANDS: &[&str] = &[
    "status",
    "diff",
    "log",
    "show",
    "branch",
    "rev-parse",
    "ls-files",
    "ls-tree",
    "remote",
    "describe",
    "shortlog",
    "blame",
    "cat-file",
];

/// Arguments of `seat_exec`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SeatExecRequest {
    /// Program to run. It must be in the seat runtime allowlist.
    #[schemars(description = "Program to run. Must be in the seat runtime allowlist.")]
    pub program: String,
    /// Arguments for the program, passed as an argv array. No shell is involved.
    #[schemars(description = "Arguments for the program, passed as an argv array. No shell is involved.")]
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory relative to the seat workspace. Defaults to the workspace root.
    #[schemars(description = "Working directory relative to the seat workspace. Defaults to the workspace root.")]
    #[serde(default)]
    pub cwd: Option<String>,
    /// Timeout in seconds, 1 to 600. Defaults to 120.
    #[schemars(description = "Timeout in seconds, 1 to 600. Defaults to 120.")]
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// A single path relative to the seat workspace.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PathRequest {
    /// Path relative to the seat workspace.
    #[schemars(description = "Path relative to the seat workspace.")]
    pub path: String,
}

/// A file to write inside the seat workspace.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteFileRequest {
    /// Path relative to the seat workspace.
    #[schemars(description = "Path relative to the seat workspace.")]
    pub path: String,
    /// Content to write.
    #[schemars(description = "Content to write.")]
    pub content: String,
}

/// The seat runtime: one seat, one workspace, five tools.
#[derive(Debug, Clone)]
pub struct SeatRuntime {
    binding: SeatBinding,
    confinement: Confinement,
}

impl SeatRuntime {
    /// Bind a runtime to one seat.
    pub fn new(binding: SeatBinding) -> Self {
        let confinement = Confinement::new(binding.workspace().to_path_buf());
        Self {
            binding,
            confinement,
        }
    }

    /// The seat this runtime serves.
    pub fn binding(&self) -> &SeatBinding {
        &self.binding
    }
}

#[tool_router(server_handler)]
impl SeatRuntime {
    /// Report the seat, its workspace and the confinement policy.
    #[tool(description = "Report the seat identity, its workspace and the confinement policy of this runtime")]
    fn seat_info(&self) -> Result<CallToolResult, ErrorData> {
        Ok(tool_text(json!({
            "seat": self.binding.seat(),
            "workspace": self.binding.workspace().to_string_lossy(),
            "api": self.binding.api(),
            "confinement": if self.binding.allow_global() { "disabled" } else { "workspace-only" },
            "programs": PROGRAM_ALLOWLIST,
            "git_subcommands": GIT_READONLY_SUBCOMMANDS,
            "max_file_bytes": MAX_FILE_BYTES,
            "max_output_bytes": MAX_OUTPUT_BYTES,
            "default_timeout_secs": DEFAULT_TIMEOUT_SECS,
        })))
    }

    /// Run an allowlisted program inside the seat workspace.
    #[tool(description = "Run an allowlisted program inside the seat workspace and return its exit code, stdout and stderr")]
    async fn seat_exec(
        &self,
        Parameters(request): Parameters<SeatExecRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let program = request.program.clone();
        if !PROGRAM_ALLOWLIST.contains(&program.as_str()) {
            return Ok(tool_error(format!(
                "program {program:?} is not in the seat runtime allowlist"
            )));
        }
        if program == "git" {
            let subcommand = request.args.first().map(String::as_str).unwrap_or("");
            if !GIT_READONLY_SUBCOMMANDS.contains(&subcommand) {
                return Ok(tool_error(format!(
                    "git {subcommand:?} is refused in a seat runtime; only read-only git subcommands are allowed"
                )));
            }
        }

        let cwd = match request.cwd.as_deref() {
            Some(cwd) => match self.confinement.resolve_existing(cwd) {
                Ok(path) => path,
                Err(error) => return Ok(tool_error(format!("{error}: {cwd}"))),
            },
            None => self.confinement.workspace().to_path_buf(),
        };

        let seconds = request
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS);

        let mut command = Command::new(&program);
        command
            .args(&request.args)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("QENEX_SEAT", self.binding.seat())
            .env("QENEX_SEAT_WORKSPACE", self.binding.workspace());

        let output = match timeout(Duration::from_secs(seconds), command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                return Ok(tool_error(format!("failed to run {program:?}: {error}")));
            }
            Err(_) => {
                return Ok(tool_error(format!(
                    "{program:?} timed out after {seconds}s and was killed"
                )));
            }
        };

        Ok(tool_text(json!({
            "seat": self.binding.seat(),
            "program": program,
            "args": request.args,
            "cwd": cwd.to_string_lossy(),
            "exit_code": output.status.code(),
            "stdout": truncate(&String::from_utf8_lossy(&output.stdout)),
            "stderr": truncate(&String::from_utf8_lossy(&output.stderr)),
            "truncated_at_bytes": MAX_OUTPUT_BYTES,
        })))
    }

    /// Read a file from the seat workspace.
    #[tool(description = "Read a file from the seat workspace")]
    fn seat_read_file(
        &self,
        Parameters(request): Parameters<PathRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match self.confinement.resolve_existing(&request.path) {
            Ok(path) => path,
            Err(error) => return Ok(tool_error(format!("{error}: {}", request.path))),
        };
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => return Ok(tool_error(format!("cannot stat {path:?}: {error}"))),
        };
        if metadata.len() > MAX_FILE_BYTES {
            return Ok(tool_error(format!(
                "file is {} bytes, above the {MAX_FILE_BYTES} byte limit",
                metadata.len()
            )));
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(tool_text(json!({
                "seat": self.binding.seat(),
                "path": request.path,
                "bytes": metadata.len(),
                "content": content,
            }))),
            Err(error) => Ok(tool_error(format!("cannot read {path:?}: {error}"))),
        }
    }

    /// Write a file inside the seat workspace.
    #[tool(description = "Write a file inside the seat workspace, creating parent directories when needed")]
    fn seat_write_file(
        &self,
        Parameters(request): Parameters<WriteFileRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if self.binding.allow_global() {
            return Ok(tool_error(
                "seat confinement is disabled for this runtime; writes are refused",
            ));
        }
        let path = match self.confinement.resolve(&request.path) {
            Ok(path) => path,
            Err(error) => return Ok(tool_error(format!("{error}: {}", request.path))),
        };
        if let Some(parent) = path.parent() {
            if let Err(error) = self.confinement.ensure_writable(parent) {
                return Ok(tool_error(format!("{error}: {}", parent.display())));
            }
            if let Err(error) = std::fs::create_dir_all(parent) {
                return Ok(tool_error(format!("cannot create {parent:?}: {error}")));
            }
        }
        match std::fs::write(&path, request.content.as_bytes()) {
            Ok(()) => Ok(tool_text(json!({
                "seat": self.binding.seat(),
                "path": request.path,
                "bytes": request.content.len(),
            }))),
            Err(error) => Ok(tool_error(format!("cannot write {path:?}: {error}"))),
        }
    }

    /// List a directory inside the seat workspace.
    #[tool(description = "List a directory inside the seat workspace")]
    fn seat_list_dir(
        &self,
        Parameters(request): Parameters<PathRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = match self.confinement.resolve_existing(&request.path) {
            Ok(path) => path,
            Err(error) => return Ok(tool_error(format!("{error}: {}", request.path))),
        };
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) => return Ok(tool_error(format!("cannot list {path:?}: {error}"))),
        };
        let mut listing = Vec::new();
        for entry in entries.flatten() {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            listing.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "is_dir": is_dir,
                "bytes": size,
            }));
        }
        listing.sort_by(|left, right| {
            left["name"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["name"].as_str().unwrap_or_default())
        });
        Ok(tool_text(json!({
            "seat": self.binding.seat(),
            "path": request.path,
            "entries": listing,
        })))
    }
}

/// Build a successful tool result from one JSON payload.
fn tool_text(payload: serde_json::Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(payload.to_string())])
}

/// Build a tool-level error the caller can read.
fn tool_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

/// Cut a captured stream down to the output limit.
fn truncate(text: &str) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text.to_string();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated at {MAX_OUTPUT_BYTES} bytes]", &text[..end])
}

/// Path of the seat workspace, for diagnostics.
pub fn workspace_of(binding: &SeatBinding) -> &Path {
    binding.workspace()
}

/// Expose the path error type to callers that map errors themselves.
pub fn describe_path_error(error: PathError) -> String {
    error.to_string()
}
