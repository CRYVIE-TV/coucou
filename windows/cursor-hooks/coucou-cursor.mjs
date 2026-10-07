import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const hookExe = join(process.env.LOCALAPPDATA || "", "Coucou", "bin", "coucou-hook.exe");

function readStdin() {
  try {
    return readFileSync(0, "utf8");
  } catch {
    return "";
  }
}

function asObject(value) {
  if (value && typeof value === "object" && !Array.isArray(value)) return value;
  if (typeof value === "string" && value.trim()) {
    try {
      const parsed = JSON.parse(value);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) return parsed;
    } catch {
      return { command: value.slice(0, 500) };
    }
  }
  return {};
}

function cwdOf(input) {
  if (typeof input.cwd === "string" && input.cwd) return input.cwd;
  const roots = input.workspace_roots;
  if (Array.isArray(roots) && typeof roots[0] === "string") return roots[0];
  return "";
}

function clip(value, max = 180) {
  if (typeof value !== "string") return "";
  const flat = repairPolish(value).replace(/\s+/g, " ").trim();
  return flat.length > max ? flat.slice(0, max) : flat;
}

// Windows-1252 reading of UTF-8: ł (C5 82) becomes Å‚, ć (C4 87) becomes Ä‡.
const CP1252 = new Map([
  [0x20ac, 0x80], [0x201a, 0x82], [0x0192, 0x83], [0x201e, 0x84], [0x2026, 0x85],
  [0x2020, 0x86], [0x2021, 0x87], [0x02c6, 0x88], [0x2030, 0x89], [0x0160, 0x8a],
  [0x2039, 0x8b], [0x0152, 0x8c], [0x017d, 0x8e], [0x2018, 0x91], [0x2019, 0x92],
  [0x201c, 0x93], [0x201d, 0x94], [0x2022, 0x95], [0x2013, 0x96], [0x2014, 0x97],
  [0x02dc, 0x98], [0x2122, 0x99], [0x0161, 0x9a], [0x203a, 0x9b], [0x0153, 0x9c],
  [0x017e, 0x9e], [0x0178, 0x9f],
]);

function repairPolish(text) {
  const bytes = [];
  for (const ch of text) {
    const code = ch.codePointAt(0);
    if (code < 0x80 || (code >= 0xa0 && code <= 0xff)) bytes.push(code);
    else if (CP1252.has(code)) bytes.push(CP1252.get(code));
    else return text;
  }
  const decoded = Buffer.from(bytes).toString("utf8");
  if (decoded.includes("\uFFFD") || decoded === text) return text;
  if (decoded.length < text.length && /[ąćęłńóśźżĄĆĘŁŃÓŚŹŻ]/.test(decoded)) return decoded;
  return text;
}

function toIsland(input) {
  const event = input.hook_event_name || "";
  const base = {
    cwd: cwdOf(input),
    session_id: input.conversation_id || input.session_id || "",
  };
  switch (event) {
    case "sessionStart":
      return { ...base, hook_event_name: "SessionStart" };
    case "sessionEnd":
      return { ...base, hook_event_name: "SessionEnd", message: input.reason || "" };
    case "beforeSubmitPrompt":
      return { ...base, hook_event_name: "UserPromptSubmit", prompt: clip(input.prompt, 500) };
    case "preToolUse":
      return {
        ...base,
        hook_event_name: "PreToolUse",
        tool_name: input.tool_name || "Tool",
        tool_input: asObject(input.tool_input),
      };
    case "postToolUse":
      return {
        ...base,
        hook_event_name: "PostToolUse",
        tool_name: input.tool_name || "Tool",
        tool_input: asObject(input.tool_input),
      };
    case "postToolUseFailure":
      return {
        ...base,
        hook_event_name: "PostToolUseFailure",
        tool_name: input.tool_name || "Tool",
        tool_input: asObject(input.tool_input),
        message: clip(input.error_message),
      };
    case "beforeShellExecution":
      return {
        ...base,
        hook_event_name: "PreToolUse",
        tool_name: "Shell",
        tool_input: { command: clip(input.command, 500) },
      };
    case "afterShellExecution":
      return {
        ...base,
        hook_event_name: "PostToolUse",
        tool_name: "Shell",
        tool_input: { command: clip(input.command, 200) },
      };
    case "beforeMCPExecution":
      return {
        ...base,
        hook_event_name: "PreToolUse",
        tool_name: input.tool_name || "MCP",
        tool_input: {
          command: clip(`${input.mcp_server_name || "mcp"} ${input.tool_name || ""}`.trim(), 200),
        },
      };
    case "afterMCPExecution":
      return {
        ...base,
        hook_event_name: "PostToolUse",
        tool_name: input.tool_name || "MCP",
        tool_input: asObject(input.tool_input),
      };
    case "beforeReadFile":
      return {
        ...base,
        hook_event_name: "PreToolUse",
        tool_name: "Read",
        tool_input: { path: input.file_path || "" },
      };
    case "afterFileEdit":
      return {
        ...base,
        hook_event_name: "PostToolUse",
        tool_name: "Edit",
        tool_input: { file_path: input.file_path || "" },
      };
    case "subagentStart":
      return {
        ...base,
        hook_event_name: "SubagentStart",
        tool_name: input.subagent_type || "Task",
        tool_input: { prompt: clip(input.task, 200) },
      };
    case "subagentStop":
      return {
        ...base,
        hook_event_name: input.status === "error" ? "StopFailure" : "SubagentStop",
        message: clip(input.summary || input.task),
      };
    case "stop":
      return {
        ...base,
        hook_event_name: input.status === "error" ? "StopFailure" : "Stop",
        message: input.status || "",
      };
    case "afterAgentResponse":
      return { ...base, hook_event_name: "Notification", message: clip(input.text) };
    case "afterAgentThought":
      return {
        ...base,
        hook_event_name: "PreToolUse",
        tool_name: "Thought",
        tool_input: { query: clip(input.text, 80) },
      };
    case "preCompact":
      return { ...base, hook_event_name: "Notification", message: "Compacting context" };
    default:
      return null;
  }
}

function kindFor(event) {
  if (
    event === "beforeShellExecution" ||
    event === "beforeMCPExecution" ||
    event === "preToolUse" ||
    event === "beforeReadFile" ||
    event === "subagentStart"
  ) return "allow";
  if (event === "beforeSubmitPrompt") return "continue";
  return "none";
}

function decisionFrom(stdout) {
  const text = (stdout || "").trim();
  if (!text) return "";
  try {
    return JSON.parse(text)?.hookSpecificOutput?.decision?.behavior || "";
  } catch {
    return "";
  }
}

const raw = readStdin().replace(/^\uFEFF/, "").trim();
let input = {};
try {
  input = raw ? JSON.parse(raw) : {};
} catch {
  input = {};
}

function repairValue(value) {
  if (typeof value === "string") return repairPolish(value);
  if (Array.isArray(value)) return value.map(repairValue);
  if (value && typeof value === "object") {
    const out = {};
    for (const [key, item] of Object.entries(value)) out[key] = repairValue(item);
    return out;
  }
  return value;
}

const event = input.hook_event_name || "";
const kind = kindFor(event);
const island = repairValue(toIsland(input));
let stdout = "";
if (island) {
  const run = spawnSync(hookExe, [island.hook_event_name], {
    input: JSON.stringify(island),
    encoding: "utf8",
    timeout: kind === "gate" ? 115000 : 2500,
    windowsHide: true,
  });
  stdout = run.stdout || "";
}

const behavior = decisionFrom(stdout);
if (kind === "gate") {
  if (behavior === "deny") {
    process.stdout.write(JSON.stringify({
      permission: "deny",
      user_message: "Denied from Coucou",
      agent_message: "Denied from Coucou",
    }));
  } else if (behavior === "allow") {
    process.stdout.write(JSON.stringify({ permission: "allow" }));
  } else {
    process.stdout.write(JSON.stringify({ permission: "ask" }));
  }
} else if (kind === "allow") {
  process.stdout.write(JSON.stringify({ permission: "allow" }));
} else if (kind === "continue") {
  process.stdout.write(JSON.stringify({ continue: true }));
}
process.exit(0);