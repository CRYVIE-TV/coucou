// « OK Coucou »'s brain: Grok 4.7 through the Cursor key, with Coucou's tools —
// pills, music, the user's own data, opening an app, preparing an email (the
// mail app opens with it filled in; the user sends it) and web search. The
// conversation lives in memory only and is forgotten 90 s after the last turn.
// Port of ClaudeVoiceBrain.swift.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::AppHandle;

const FORGET_AFTER: Duration = Duration::from_secs(90);

pub struct Brain {
    turns: Vec<(String, String)>,
    last: Instant,
}

impl Default for Brain {
    fn default() -> Self {
        Self { turns: Vec::new(), last: Instant::now() }
    }
}

pub struct Reply {
    pub text: String,
    pub acted: bool,
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "pills",
            "description": "Add, remove, set as main or list the pills shown in the island at the top of the screen. At most 4 pills besides the main one: remove one first if needed.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["add", "remove", "set_main", "list"] },
                    "names": { "type": "array", "items": { "type": "string" }, "description": "Pill names as listed (e.g. Stripe, GitHub, Cursor). Empty for list." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "music",
            "description": "Control Spotify: play or pause, next, previous, set the volume, or search a song, an artist or a playlist in Spotify.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["play_pause", "next", "previous", "set_volume", "search"] },
                    "query": { "type": "string", "description": "What to search, for search." },
                    "volume": { "type": "integer", "description": "0-100 for set_volume." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "service_info",
            "description": "Read the user's real data that Coucou already has, as JSON: the active pills, the agent sessions (Claude Code, Codex…) and what they are doing, Claude or Codex plan usage, Spotify now playing, and the services' latest data (Stripe, GitHub, Vercel, Resend, n8n, Notion, Cal.com).",
            "input_schema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "open_app",
            "description": "Open an app on this computer by name (Firefox, VS Code, Notion, Spotify…).",
            "input_schema": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }
        }),
        json!({
            "name": "prepare_email",
            "description": "Open the user's mail app with an email filled in, for them to check and send (it is never sent automatically). The recipient must be an email address.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "recipient": { "type": "string", "description": "An email address." },
                    "subject": { "type": "string" },
                    "body": { "type": "string", "description": "The full email text, written for the user." }
                },
                "required": ["recipient", "subject", "body"]
            }
        }),
    ]
}

fn system_prompt(pills: &str) -> String {
    let t = crate::platform::local_time();
    let os = if cfg!(windows) { "Windows" } else { "Linux" };
    format!(
        "You are Coucou, a voice assistant living at the top of the screen of the user's {os} computer, a bit like Jarvis: \
quick, warm, a little playful. It is {:04}-{:02}-{:02} {:02}:{:02}.\n\
Everything you write is read aloud: answer in the language the user spoke, in one or two short sentences, \
no markdown, no lists, no emojis, no links.\n\
Act with the tools instead of describing: pills, music, service_info for the user's own data, open_app, \
prepare_email, and your web search for anything current (weather, news, scores, prices, facts you are not sure of). \
Never invent the user's data.\n\
Email: gather the recipient's address, the subject and what to say (write the text yourself from what the user \
wants to say, in the language they used), asking for what is missing one short question at a time, then call \
prepare_email. The email is never sent by you: the user checks it in their mail app and sends it.\n\
When you need an answer from the user, end with one short question. Otherwise never end with a question.\n\
Pills: {pills}",
        t.year, t.month, t.day, t.hour, t.minute
    )
}

impl Brain {
    pub fn respond(&mut self, app: &AppHandle, transcript: &str) -> Option<Reply> {
        if self.last.elapsed() > FORGET_AFTER {
            self.turns.clear();
        }
        self.last = Instant::now();
        let pills = super::ask_island(app, "pills", json!({ "action": "list" })).unwrap_or_default();
        let system = system_prompt(&pills);
        let mut user = String::new();
        for (said, answer) in &self.turns {
            user.push_str(&format!("User: {said}\nCoucou: {answer}\n"));
        }
        user.push_str(&format!("User: {transcript}"));
        let (text, acted) = match crate::cursor_chat::voice_reply(app, &system, &user, Duration::from_secs(90), &tools()) {
            Ok(reply) => reply,
            Err(err) => {
                crate::log::line(format!("voice: Grok turn failed: {err}"));
                return None;
            }
        };
        let spoken = spoken_text(&text);
        if !spoken.is_empty() {
            self.turns.push((transcript.to_string(), spoken.clone()));
            if self.turns.len() > 8 {
                self.turns.drain(..self.turns.len() - 8);
            }
        }
        Some(Reply { text: spoken, acted })
    }
}

/// What Grok wrote, ready to be said: citations and markdown left out.
pub fn spoken_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skip = 0;
    for c in text.chars() {
        match c {
            '[' => skip += 1,
            ']' if skip > 0 => skip -= 1,
            '*' | '#' | '`' | '_' if skip == 0 => {}
            _ if skip == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_and_citations_are_not_read_aloud() {
        assert_eq!(spoken_text("**Done** — Stripe [1] is in the island."), "Done — Stripe is in the island.");
        assert_eq!(spoken_text("  Il fait  20 °C à Bordeaux. "), "Il fait 20 °C à Bordeaux.");
    }

    #[test]
    fn every_tool_has_a_schema() {
        for tool in tools() {
            assert!(tool["name"].is_string());
            assert!(tool.get("input_schema").is_some());
        }
    }
}
