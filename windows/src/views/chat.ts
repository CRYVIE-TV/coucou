// Chat view — DOM port of PromptView / ChatBubble / TypingDotsView from
// IslandViewContent.swift.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { Bridge, onEvent, type ChatContext } from "../core/bridge";
import { Sound } from "../core/sound";
import { State, type ChatMessage } from "../core/state";
import type { ViewHost } from "./views";

let nextId = 1;

function inline(text: string): Node[] {
  const nodes: Node[] = [];
  const pattern = /\*\*(.+?)\*\*/g;
  let last = 0;
  for (let match = pattern.exec(text); match; match = pattern.exec(text)) {
    if (match.index > last) nodes.push(document.createTextNode(text.slice(last, match.index)));
    nodes.push(h("strong", { text: match[1] }));
    last = match.index + match[0].length;
  }
  if (last < text.length) nodes.push(document.createTextNode(text.slice(last)));
  if (nodes.length === 0) nodes.push(document.createTextNode(""));
  return nodes;
}

function appendProse(box: HTMLElement, source: string) {
  const blocks = source.split(/\n{2,}/);
  for (const block of blocks) {
    const lines = block
      .split("\n")
      .map((line) => line.replace(/^#{1,6}\s+/, ""))
      .filter((line) => line.trim().length > 0);
    if (lines.length === 0) continue;
    const bullets = lines.every((line) => /^[-•]\s+/.test(line.trim()));
    const numbered = lines.every((line) => /^\d+[.)]\s+/.test(line.trim()));
    if (bullets || numbered) {
      const list = h(numbered ? "ol" : "ul", { class: "reply-list" });
      for (const line of lines) {
        list.append(h("li", {}, ...inline(line.trim().replace(/^(?:[-•]|\d+[.)])\s+/, ""))));
      }
      box.append(list);
    } else {
      const paragraph = h("p");
      lines.forEach((line, index) => {
        if (index > 0) paragraph.append(h("br"));
        paragraph.append(...inline(line));
      });
      box.append(paragraph);
    }
  }
}

function renderReply(text: string): HTMLElement {
  const box = h("div", { class: "reply" });
  const chunks = text.replace(/\r\n/g, "\n").trim().split("```");
  chunks.forEach((chunk, index) => {
    if (index % 2 === 1) {
      const body = chunk.replace(/^\w*\n/, "").replace(/\n$/, "");
      if (body) box.append(h("pre", { class: "reply-code", text: body }));
      return;
    }
    appendProse(box, chunk);
  });
  if (!box.childNodes.length) box.append(h("p", { text: text }));
  return box;
}

function bubble(message: ChatMessage): HTMLElement {
  if (message.role === "user") {
    return h(
      "div",
      { class: "chat-row user" },
      h("div", { class: "bubble", text: message.content }),
    );
  }
  return h("div", { class: "chat-row" }, renderReply(message.content));
}

function typingDots(): HTMLElement {
  return h(
    "div",
    { class: "chat-row" },
    h("div", { class: "typing" }, h("i"), h("i"), h("i")),
  );
}

function liveTurn(): HTMLElement {
  const box = h("div", { class: "chat-live" });
  if (State.chatSteps.length) {
    const steps = h("div", { class: "chat-steps" });
    for (const step of State.chatSteps) {
      steps.append(h("div", { class: "chat-step" }, h("i"), h("span", { text: step })));
    }
    box.append(steps);
  }
  if (State.chatDraft) box.append(renderReply(State.chatDraft));
  else if (State.chatSteps.length === 0) box.append(typingDots());
  return box;
}

/** The coloured chip showing what the question is about (a dropped file). */
function contextChip(label: string): HTMLElement {
  const chip = h("div", { class: "chip" }, h("i", { class: "chip-dot" }), h("span", { text: label }));
  requestAnimationFrame(() => chip.classList.add("settled"));
  return chip;
}

export function buildPrompt(onHeightChange: () => void): ViewHost {
  const chipRow = h("div", { class: "chip-row" });
  const log = h("div", { class: "chat-log" });
  const input = h("input", {
    type: "text",
    class: "chat-input",
    placeholder: "Ask me anything…",
    spellcheck: "false",
  }) as HTMLInputElement;
  const send = h("button", { class: "send-btn", title: "Send" }, svg(ICONS.arrowUp, 11));
  const bar = h("div", { class: "chat-bar" }, input, send);

  const el = h(
    "div",
    { class: "view" },
    h("div", { class: "card wash chat-card" }, h("div", { class: "chat-body" }, chipRow, log, bar)),
  );
  (el.querySelector(".card") as HTMLElement).style.setProperty("--wash", "rgba(99,102,241,0.5)");

  let sending = false;
  let rendered = "";

  async function submit() {
    const query = input.value.trim();
    if (!query || sending) return;
    input.value = "";
    sending = true;
    State.chatSteps = [];
    State.chatDraft = "";
    Sound.play("send");

    State.chatHistory.push({ id: nextId++, role: "user", content: query });
    State.stateOverride = "thinking";
    State.notify();
    onHeightChange();

    const file = State.droppedFile;
    const context: ChatContext | null =
      State.chatHistory.length === 1 && file ? { kind: "file", name: file.name, path: file.path } : null;

    try {
      const reply = await Bridge.chatSend(query, context);
      State.chatHistory.push({ id: nextId++, role: "assistant", content: reply.text });
      State.stateOverride = null;
      Sound.play("finish");
    } catch (err) {
      const message = String(err).replace(/^Error:\s*/, "");
      const partial = State.chatDraft.trim();
      State.chatHistory.push({
        id: nextId++,
        role: "assistant",
        content: partial ? `${partial}\n\n${message}` : message,
      });
      State.stateOverride = null;
      Sound.play("error");
    } finally {
      sending = false;
      State.chatSteps = [];
      State.chatDraft = "";
      State.notify();
      onHeightChange();
      input.focus();
    }
  }

  void onEvent<{ kind?: string; step?: string; text?: string }>("chat-progress", (event) => {
    if (!sending) return;
    if (event.kind === "progress" && event.step) {
      if (State.chatSteps.at(-1) !== event.step) State.chatSteps.push(event.step);
      if (State.chatSteps.length > 10) State.chatSteps.shift();
    } else if (event.kind === "delta" && event.text) {
      State.chatDraft += event.text;
    }
    State.notify();
  });
  send.addEventListener("click", () => void submit());
  input.addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key === "Enter") {
      e.preventDefault();
      void submit();
    }
    e.stopPropagation(); // Escape closes the island, not the chat
  });

  return {
    el,
    sync() {
      const file = State.droppedFile;
      const wantChip = file?.name ?? "";
      if (chipRow.dataset.label !== wantChip) {
        chipRow.dataset.label = wantChip;
        clear(chipRow);
        if (wantChip) chipRow.append(contextChip(wantChip));
      }

      const thinking = State.stateOverride === "thinking";
      const signature = [
        State.chatHistory.map((m) => `${m.id}:${m.content}`).join("\n"),
        thinking ? "1" : "0",
        State.chatSteps.join("\n"),
        State.chatDraft,
      ].join("~");
      if (signature !== rendered) {
        rendered = signature;
        const follow = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
        const kept = log.scrollTop;
        clear(log);
        for (const m of State.chatHistory) log.append(bubble(m));
        if (thinking) log.append(liveTurn());
        log.scrollTop = follow ? log.scrollHeight : kept;
      }

      input.placeholder = State.chatHistory.length === 0 ? "Ask me anything…" : "Continue…";
      input.disabled = sending;
    },
    focus() {
      input.focus();
      input.select();
    },
  };
}
