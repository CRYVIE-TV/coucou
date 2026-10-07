// Coucou chat relay. One JSON request per stdin line, one "COUCOU " reply per stdout line.
// Progress lines use kind "progress" or "delta" and do not finish the turn.

import { Agent } from "@cursor/sdk";
import readline from "node:readline";
import fs from "node:fs";
import path from "node:path";
import { homedir } from "node:os";

// Grok 4.7 has no separate Max switch. Extra High (xhigh) is the top effort.
// Context stays at 256k, as requested. Fast is on.
const MODEL = {
  id: "grok-4.7",
  params: [
    { id: "context", value: "256k" },
    { id: "reasoning_effort", value: "xhigh" },
    { id: "fast", value: "true" },
  ],
};

const SYSTEM = [
  "You are Mochi, a personal assistant living at the top of the user's screen.",
  "Answer in the user's language.",
  "Write for a narrow panel: short paragraphs separated by a blank line.",
  "Lead with the answer. Use '- ' for lists. You may mark a few key words with **double asterisks**.",
  "No headings and no tables.",
  "This is one continuous conversation. Stay on the user's topic.",
  "Do not explore the disk, list folders, or start a project unless the user asked you to look something up or change a file.",
  "If a tool result is unrelated to the question, ignore it and answer the question.",
  "You can search the web, read and edit files, and run commands when the user needs that.",
  `The user's files live under ${homedir()}. Open a path only when they asked for it.`,
].join(" ");

// Scratch folder, not the whole profile. Indexing the home directory makes the
// agent wander into unrelated projects and forget the conversation.
const cwd = process.env.COUCOU_CHAT_CWD || homedir();
fs.mkdirSync(cwd, { recursive: true });

const idFile = path.join(cwd, "agent-id.txt");
const transcriptFile = path.join(cwd, "transcript.json");
const lockFile = path.join(cwd, "turn.lock");

let agent = null;
let preface = "";

function readId() {
  try {
    const id = fs.readFileSync(idFile, "utf8").trim();
    return id || "";
  } catch {
    return "";
  }
}

function loadTranscript() {
  try {
    const parsed = JSON.parse(fs.readFileSync(transcriptFile, "utf8"));
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function keep(value, max) {
  const text = String(value ?? "").replace(/\r\n/g, "\n").trim();
  if (text.length <= max) return text;
  return `${text.slice(0, max - 1)}…`;
}

function remember(userText, answer) {
  const turns = loadTranscript();
  turns.push({
    user: keep(userText, 2000),
    assistant: keep(answer, 2000),
  });
  const kept = turns.slice(-12);
  fs.writeFileSync(transcriptFile, JSON.stringify(kept));
}

function memoryBlock() {
  const turns = loadTranscript().slice(-8);
  if (!turns.length) return "";
  const lines = ["Earlier in this same conversation:"];
  for (const turn of turns) {
    lines.push(`User: ${turn.user}`);
    lines.push(`Mochi: ${turn.assistant}`);
  }
  lines.push("Continue from there. Do not start over.");
  return `${lines.join("\n")}\n\n`;
}

function reply(payload) {
  process.stdout.write(`COUCOU ${JSON.stringify(payload)}\n`);
}

function clip(value, max = 88) {
  const text = String(value ?? "").replace(/\s+/g, " ").trim();
  if (!text) return "";
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

function baseName(value) {
  const text = String(value ?? "");
  const parts = text.split(/[\\/]/);
  return parts[parts.length - 1] || text;
}

function describe(toolCall) {
  const type = toolCall?.type || "";
  const args = toolCall?.args || {};
  switch (type) {
    case "semSearch":
      return `Przeszukuję: ${clip(args.query)}`;
    case "grep":
      return `Szukam „${clip(args.pattern, 60)}”`;
    case "glob":
      return `Szukam plików ${clip(args.globPattern, 60)}`;
    case "read":
      return `Czytam ${baseName(args.path)}`;
    case "edit":
      return `Edytuję ${baseName(args.path)}`;
    case "write":
      return `Zapisuję ${baseName(args.path)}`;
    case "delete":
      return `Usuwam ${baseName(args.path)}`;
    case "ls":
      return `Przeglądam ${baseName(args.path)}`;
    case "shell":
      return `Uruchamiam ${clip(args.command, 72)}`;
    case "mcp":
      return `Używam ${clip(args.toolName || args.providerIdentifier || "narzędzia", 40)}`;
    case "readLints":
      return "Sprawdzam błędy";
    case "task":
      return "Pracuję nad zadaniem";
    case "generateImage":
      return "Tworzę obraz";
    default: {
      const hint = args.query || args.pattern || args.command || args.path || args.toolName;
      return hint ? `${type || "Pracuję"}: ${clip(hint, 70)}` : (type ? `Używam ${type}` : "Pracuję");
    }
  }
}

function agentOptions(key) {
  return {
    apiKey: key,
    model: MODEL,
    name: "Coucou",
    local: {
      cwd,
      // "user" loads %USERPROFILE%\.cursor\mcp.json (mouse, browser, Windows).
      settingSources: ["user"],
    },
  };
}

async function ensure() {
  if (agent) return agent;
  const key = process.env.CURSOR_API_KEY;
  if (!key) throw new Error("Cursor API key missing.");

  const saved = readId();
  const wedged = fs.existsSync(lockFile);
  if (saved) {
    try {
      agent = await Agent.resume(saved, agentOptions(key));
      agent.coucouForce = wedged;
      preface = "";
      return agent;
    } catch {
      agent = null;
    }
  }

  preface = `${memoryBlock()}${SYSTEM}\n\n`;
  agent = await Agent.create(agentOptions(key));
  agent.coucouForce = wedged;
  if (agent.agentId) fs.writeFileSync(idFile, agent.agentId);
  return agent;
}

async function closeAgent() {
  const current = agent;
  agent = null;
  preface = "";
  if (current) await current.close();
}

async function handle(message) {
  if (message.op === "reset") {
    await closeAgent();
    for (const file of [idFile, transcriptFile, lockFile]) fs.rmSync(file, { force: true });
    reply({ id: message.id, ok: true });
    return;
  }
  if (message.op !== "send") {
    reply({ id: message.id, ok: false, error: "Unknown request." });
    return;
  }
  const current = await ensure();
  const text = `${preface}${message.text ?? ""}`;
  preface = "";
  const force = Boolean(current.coucouForce);
  current.coucouForce = false;
  fs.writeFileSync(lockFile, String(Date.now()));

  let pending = "";
  let thinkingNoted = false;
  let flushTimer = null;
  const flush = () => {
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    if (!pending) return;
    const chunk = pending;
    pending = "";
    reply({ id: message.id, kind: "delta", text: chunk });
  };
  const queueDelta = (chunk) => {
    pending += chunk;
    if (!flushTimer) flushTimer = setTimeout(flush, 70);
  };

  const run = await current.send(text, {
    local: force ? { force: true } : undefined,
    onDelta({ update }) {
      const kind = update?.type;
      if (kind === "tool-call-started") {
        flush();
        reply({ id: message.id, kind: "progress", step: describe(update.toolCall) });
      } else if (kind === "thinking-delta" && !thinkingNoted) {
        thinkingNoted = true;
        reply({ id: message.id, kind: "progress", step: "Myślę…" });
      } else if (kind === "text-delta" && update.text) {
        queueDelta(update.text);
      }
    },
  });
  const result = await run.wait();
  flush();
  if (result.status === "error") {
    throw new Error(result.error?.message || "The agent run failed.");
  }
  const answer = (result.result || "").trim();
  if (!answer) throw new Error("No response text.");
  remember(message.text ?? "", answer);
  fs.rmSync(lockFile, { force: true });
  reply({ id: message.id, ok: true, text: answer });
}

const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  const trimmed = line.trim();
  if (!trimmed) continue;
  let message;
  try {
    message = JSON.parse(trimmed);
  } catch {
    reply({ ok: false, error: "Bad request." });
    continue;
  }
  try {
    await handle(message);
  } catch (err) {
    reply({
      id: message.id,
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    });
    // Drop the live handle but keep the saved agent id. The turn lock stays,
    // so the next message resumes the same conversation and expires a wedged run.
    try {
      await closeAgent();
    } catch {
      agent = null;
    }
  }
}