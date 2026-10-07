// Chat through the Cursor SDK. The key stays in Credential Manager and is handed
// to a local Node helper over the environment, never to the island.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::claude::{ChatContext, ChatReply};
use crate::secrets;
use crate::settings;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const REPLY_PREFIX: &str = "COUCOU ";
/// Extra High plus tools can run well past a few minutes. Killing the helper
/// early drops the conversation.
const TURN_TIMEOUT: Duration = Duration::from_secs(900);

struct Bridge {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Bridge {
    fn request(&mut self, app: &AppHandle, text: &str) -> Result<String, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = json!({ "op": "send", "id": id, "text": text }).to_string();
        writeln!(self.stdin, "{line}").map_err(|e| format!("Chat helper stopped: {e}"))?;
        self.read_reply(app, id)
    }

    fn read_reply(&mut self, app: &AppHandle, id: u64) -> Result<String, String> {
        let deadline = std::time::Instant::now() + TURN_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err("Grok took too long to answer.".into());
            }
            let line = self
                .lines
                .recv_timeout(remaining)
                .map_err(|_| "Grok took too long to answer.".to_string())?;
            let Some(payload) = line.strip_prefix(REPLY_PREFIX) else {
                continue;
            };
            let value: Value = serde_json::from_str(payload)
                .map_err(|_| "Chat helper sent a broken reply.".to_string())?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            // Live steps and streamed text. The turn is still running.
            if value.get("kind").and_then(Value::as_str).is_some() {
                let _ = app.emit("chat-progress", &value);
                continue;
            }
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                return value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| "No response text.".into());
            }
            let why = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Cursor chat failed.");
            return Err(why.to_string());
        }
    }
}

fn node_program() -> String {
    let Ok(output) = Command::new("where.exe")
        .arg("node")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return "node".into();
    };
    if !output.status.success() {
        return "node".into();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("node")
        .trim()
        .to_string()
}

fn spawn() -> Result<Bridge, String> {
    let key = secrets::get("cursor-api-key")
        .ok_or_else(|| "Cursor API key missing. Open settings.".to_string())?;
    let dir = settings::local_dir().join("cursor-chat");
    let script = dir.join("chat.mjs");
    if !script.is_file() {
        return Err(format!("Chat helper is missing ({}).", script.display()));
    }
    let workspace = settings::local_dir().join("chat-workspace");
    std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;

    let mut child = Command::new(node_program())
        .arg(&script)
        .current_dir(&dir)
        .env("CURSOR_API_KEY", &key)
        .env("COUCOU_CHAT_CWD", &workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Couldn't start the chat helper. Is Node installed? {e}"))?;

    let stdin = child.stdin.take().ok_or("Chat helper has no input.")?;
    let stdout = child.stdout.take().ok_or("Chat helper has no output.")?;
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
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

    Ok(Bridge {
        child,
        stdin,
        lines,
        next_id: 0,
    })
}

static SESSION: Mutex<Option<Bridge>> = Mutex::new(None);

pub fn reset() {
    if let Ok(mut session) = SESSION.lock() {
        *session = None;
    }
    // The helper is gone. Drop its saved thread too, or the next file
    // swallow would resume the previous conversation.
    let dir = settings::local_dir().join("chat-workspace");
    for name in ["agent-id.txt", "transcript.json", "turn.lock", "history.json"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StoredTurn {
    pub role: String,
    pub content: String,
}

fn history_file() -> std::path::PathBuf {
    settings::local_dir().join("chat-workspace").join("history.json")
}

pub fn load_history() -> Vec<StoredTurn> {
    let Ok(text) = std::fs::read_to_string(history_file()) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_history(mut turns: Vec<StoredTurn>) {
    if turns.len() > 40 {
        turns.drain(0..turns.len() - 40);
    }
    for turn in &mut turns {
        if turn.role != "user" {
            turn.role = "assistant".into();
        }
        let count = turn.content.chars().count();
        if count > 8_000 {
            turn.content = turn.content.chars().take(8_000).collect();
        }
    }
    let path = history_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&turns) {
        let _ = std::fs::write(path, text);
    }
}

fn prompt(query: &str, context: Option<ChatContext>) -> String {
    let mut parts = Vec::new();
    if let Some(context) = context {
        match context {
            ChatContext::File { name, path } => {
                parts.push(format!("The user dropped a file named {name} at {path}."));
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let clipped: String = text.chars().take(80_000).collect();
                    parts.push(format!("File contents:\n{clipped}"));
                }
            }
            ChatContext::Window { app_name, title, url } => {
                let mut line = format!("Context — App: {app_name}, Window: {title}");
                if let Some(url) = url {
                    line.push_str(&format!(", URL: {url}"));
                }
                parts.push(line);
            }
        }
    }
    parts.push(query.to_string());
    parts.join("\n\n")
}

pub async fn send(
    app: AppHandle,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    let text = prompt(&query, context);
    tokio::task::spawn_blocking(move || {
        let mut session = SESSION.lock().map_err(|_| "Chat helper is busy.".to_string())?;
        if session.is_none() {
            *session = Some(spawn()?);
        }
        match session.as_mut().unwrap().request(&app, &text) {
            Ok(answer) => Ok(ChatReply { text: answer }),
            Err(err) => {
                *session = None;
                Err(err)
            }
        }
    })
    .await
    .map_err(|_| "Chat helper stopped.".to_string())?
}