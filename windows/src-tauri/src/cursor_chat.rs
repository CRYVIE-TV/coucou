// The Cursor chat provider: a Cursor agent (Grok 4.7 by default) driven over
// stdin/stdout by a small Node helper, cursor-chat/chat.mjs, which uses the
// Cursor SDK. Unlike the completion providers it can search the web, read and
// edit files and run commands, so one turn may take minutes.
//
// The key stays in the credential store and reaches the helper through its
// environment, never the island. The helper keeps the agent's thread between
// turns and launches; "New chat" drops it.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::chat::{self, Chat, ChatContext, ChatReply, ModelInfo};
use crate::i18n::{t, tf};
use crate::island::WINDOW_LABEL;
use crate::{secrets, settings};

pub const ID: &str = "cursor";
pub const KEY: &str = "cursor-api-key";
pub const DEFAULT_MODEL: &str = "grok-4.7";

const REPLY_PREFIX: &str = "COUCOU ";
/// Extra High plus tools can run well past a few minutes. Killing the helper
/// early drops the conversation.
const TURN_TIMEOUT: Duration = Duration::from_secs(900);

/// The models the picker offers. The helper tunes Grok (256K, Extra High, Fast);
/// any other id is handed to the SDK as it is.
pub fn models() -> Vec<ModelInfo> {
    vec![ModelInfo { id: DEFAULT_MODEL.into(), label: "Grok 4.7 · 256K · Extra High · Fast".into() }]
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Helper {
    fn request(&mut self, app: &AppHandle, model: &str, text: &str) -> Result<String, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = json!({ "op": "send", "id": id, "model": model, "text": text }).to_string();
        writeln!(self.stdin, "{line}").map_err(|e| tf("Chat helper stopped: {error}", &[("error", &e.to_string())]))?;
        self.read_reply(app, id)
    }

    fn read_reply(&mut self, app: &AppHandle, id: u64) -> Result<String, String> {
        let deadline = std::time::Instant::now() + TURN_TIMEOUT;
        let mut visible = String::new();
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(t("The Cursor agent took too long to answer."));
            }
            let line = self
                .lines
                .recv_timeout(remaining)
                .map_err(|_| t("The Cursor agent took too long to answer."))?;
            let Some(payload) = line.strip_prefix(REPLY_PREFIX) else { continue };
            let Ok(value) = serde_json::from_str::<Value>(payload) else {
                return Err(t("Chat helper sent a broken reply."));
            };
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            // Streamed text and tool steps: the turn is still running. The island
            // shows what is visible so far, like a local model's stream.
            match value.get("kind").and_then(Value::as_str) {
                Some("delta") => {
                    if let Some(text) = value.get("text").and_then(Value::as_str) {
                        visible.push_str(text);
                        let _ = app.emit_to(WINDOW_LABEL, "chat-delta", &visible);
                    }
                    continue;
                }
                Some(_) => {
                    if visible.is_empty() {
                        if let Some(step) = value.get("step").and_then(Value::as_str) {
                            let _ = app.emit_to(WINDOW_LABEL, "chat-delta", format!("*{step}*"));
                        }
                    }
                    continue;
                }
                None => {}
            }
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                return value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| t("No response text."));
            }
            let why = value.get("error").and_then(Value::as_str).unwrap_or("Cursor chat failed.");
            return Err(why.to_string());
        }
    }
}

fn node_program() -> String {
    #[cfg(windows)]
    let found = {
        let mut cmd = Command::new("where.exe");
        cmd.arg("node");
        crate::platform::no_console(&mut cmd).output().ok()
    };
    #[cfg(not(windows))]
    let found = Command::new("which").arg("node").output().ok();
    found
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "node".into())
}

fn spawn() -> Result<Helper, String> {
    let key = secrets::get(KEY).ok_or_else(|| tf("{name} API key missing. Add it in Settings.", &[("name", "Cursor")]))?;
    let dir = settings::local_dir().join("cursor-chat");
    let script = dir.join("chat.mjs");
    if !script.is_file() {
        return Err(tf("Chat helper is missing ({path}).", &[("path", &script.display().to_string())]));
    }
    let workspace = settings::local_dir().join("chat-workspace");
    std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(node_program());
    cmd.arg(&script)
        .current_dir(&dir)
        .env("CURSOR_API_KEY", &key)
        .env("COUCOU_CHAT_CWD", &workspace)
        .env("COUCOU_LANG", crate::i18n::current())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    crate::platform::no_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| tf("Couldn't start the chat helper. Is Node installed? {error}", &[("error", &e.to_string())]))?;

    let stdin = child.stdin.take().ok_or_else(|| tf("Chat helper stopped: {error}", &[("error", "no stdin")]))?;
    let stdout = child.stdout.take().ok_or_else(|| tf("Chat helper stopped: {error}", &[("error", "no stdout")]))?;
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok(Helper { child, stdin, lines, next_id: 0 })
}

static SESSION: Mutex<Option<Helper>> = Mutex::new(None);

/// "New chat": the helper goes, and so does its saved thread, or the next
/// question would resume the previous conversation.
pub fn reset() {
    if let Ok(mut session) = SESSION.lock() {
        *session = None;
    }
    let dir = settings::local_dir().join("chat-workspace");
    for name in ["agent-id.txt", "transcript.json", "turn.lock", "history.json"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// What the agent is told. A dropped file is read here (it is one of Coucou's
/// own copies, chat.rs checked) so the agent needs no tool call for it.
fn prompt(query: &str, context: Option<&ChatContext>, carried: &[Value]) -> String {
    let mut parts = Vec::new();
    if !carried.is_empty() {
        // Turns another provider answered before the switch to Cursor.
        let mut lines = vec!["Earlier in this same conversation:".to_string()];
        for turn in carried {
            let who = if turn["role"] == "user" { "User" } else { "Mochi" };
            let text = turn["content"].as_str().unwrap_or("");
            let clipped: String = text.chars().take(2000).collect();
            lines.push(format!("{who}: {clipped}"));
        }
        lines.push("Continue from there. Do not start over.".into());
        parts.push(lines.join("\n"));
    }
    if let Some(context) = context {
        match context {
            ChatContext::File { name, path } => {
                parts.push(format!("The user dropped a file named {name} at {path}."));
                if let Ok(text) = std::fs::read_to_string(path) {
                    let clipped: String = text.chars().take(80_000).collect();
                    parts.push(format!("File contents:\n{clipped}"));
                }
            }
            ChatContext::Window { app_name, title, url } => {
                parts.push(chat::window_line(app_name, title, url.as_deref()));
            }
        }
    }
    parts.push(query.to_string());
    parts.join("\n\n")
}

/// One chat turn with the Cursor agent.
pub async fn send(
    app: &AppHandle,
    chat: &Chat,
    model: &str,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    let turn = chat.begin(ID);
    // The agent remembers its own thread: only turns it never saw are carried.
    let carried = if turn.first || turn.history.is_empty() { Vec::new() } else { turn.history.clone() };
    let plain = chat::plain_question(turn.first, context.as_ref(), &query);
    let text = prompt(&query, context.as_ref().filter(|_| turn.first), &carried);
    let app = app.clone();
    let model = model.to_string();
    let answer = tokio::task::spawn_blocking(move || {
        let mut session = SESSION.lock().map_err(|_| t("Chat helper is busy."))?;
        if session.is_none() {
            *session = Some(spawn()?);
        }
        match session.as_mut().unwrap().request(&app, &model, &text) {
            Ok(answer) => Ok(answer),
            Err(err) => {
                *session = None;
                Err(err)
            }
        }
    })
    .await
    .map_err(|_| tf("Chat helper stopped: {error}", &[("error", "aborted")]))??;
    chat.commit(
        &turn,
        json!({ "role": "user", "content": plain }),
        json!({ "role": "assistant", "content": answer }),
        &plain,
        &answer,
    );
    Ok(ChatReply { text: answer })
}
