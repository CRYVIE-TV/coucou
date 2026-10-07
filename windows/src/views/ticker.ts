// Overview task ticker — port of TickerView (V2) from IslandViewContent.swift.
//
// Three rows: completed (A), current → completed (B), incoming (C). Every row
// position is recomputed from a single clock in `tick()`, driven by the island's
// frame loop — no CSS transitions and no timers. Chaining CSS transitions with a
// reset timer let two rows land on the same line when steps arrived in bursts,
// and any step that arrived mid-animation was dropped outright. Steps are now
// queued instead, so a burst scrolls past rather than vanishing.

import { h, svg } from "./dom";
import { ICONS } from "./icons";
import { cubicBezier, clamp } from "../core/anim";
import type { AgentTask } from "../core/state";

const ROW_H = 22;
/** One step transition, milliseconds. */
const DURATION = 380;
/** Beyond this many queued steps we stop trying to show them all. */
const MAX_QUEUE = 4;
const EASE = cubicBezier(0.4, 0, 0.2, 1);

interface Row {
  el: HTMLElement;
  chevron: SVGElement;
  check: SVGElement;
  shimmer: HTMLElement;
  dim: HTMLElement;
  text: string;
}

function makeRow(): Row {
  const chevron = svg(ICONS.chevronRight, 9, { stroke: 2.4 });
  const check = svg(ICONS.check, 8, { stroke: 2.2 });
  check.style.color = "#454850"; // the completed tick is dimmer than the chevron
  check.style.position = "absolute";
  chevron.style.position = "absolute";
  const shimmer = h("span", { class: "tick-text shimmer" });
  const dim = h("span", {
    class: "tick-text",
    style: "position:absolute;left:0;right:0;top:0;color:#6b7079",
  });
  const el = h(
    "div",
    { class: "ticker-row" },
    h("span", { class: "tick-icon", style: "position:relative" }, chevron, check),
    h(
      "span",
      { style: "position:relative;flex:1 1 auto;min-width:0;height:22px" },
      shimmer,
      dim,
    ),
  );
  return { el, chevron, check, shimmer, dim, text: "" };
}

function setText(row: Row, text: string) {
  if (row.text === text) return;
  row.text = text;
  row.shimmer.textContent = text;
  row.dim.textContent = text;
}

/**
 * Places a row. `phase` 0 = current (shimmering, full size), 1 = completed
 * (dim, shifted up-left and scaled down) — same crossfades as the Swift view.
 */
function place(row: Row, phase: number, opacity: number) {
  // No per-row translate or scale. Those shifts let the finished line paint
  // on top of the live one. The track moves; each row stays in its own slot.
  row.el.style.transform = "none";
  row.el.style.opacity = String(opacity);
  // Hard switch: the shimmer and the dim copy are the same words. Crossfading
  // them paints both at once and reads as two texts on top of each other.
  const done = phase >= 0.45;
  row.chevron.style.opacity = done ? "0" : "1";
  row.check.style.opacity = done ? "1" : "0";
  row.shimmer.style.visibility = done ? "hidden" : "visible";
  row.dim.style.visibility = done ? "visible" : "hidden";
}

export class Ticker {
  readonly el: HTMLElement;
  private track: HTMLElement;
  private a = makeRow(); // completed
  private b = makeRow(); // current
  private c = makeRow(); // incoming
  private queue: string[] = [];
  private startMs: number | null = null;
  private displayIndex = -1;

  constructor() {
    this.track = h("div", { class: "ticker-track" }, this.a.el, this.b.el, this.c.el);
    this.el = h("div", { class: "ticker" }, this.track);
    this.rest();
  }

  /**
   * One line on screen: the live step. The finished step sits in the slot
   * above the clip, so it cannot rest on top of the live text.
   */
  private rest() {
    this.track.style.transform = `translateY(${-ROW_H}px)`;
    place(this.a, 1, 1);
    place(this.b, 0, 1);
    place(this.c, 0, 0);
  }

  get animating(): boolean {
    return this.startMs != null || this.queue.length > 0;
  }

  sync(task: AgentTask | null) {
    const steps = task && task.steps.length > 0 ? task.steps : ["…"];
    const idx = task ? Math.min(task.stepIndex, steps.length - 1) : -1;

    // First render: drop straight into place, no animation.
    if (this.displayIndex < 0) {
      this.displayIndex = idx;
      setText(this.a, idx > 0 ? steps[idx - 1] : "…");
      setText(this.b, steps[Math.max(idx, 0)]);
      this.rest();
      return;
    }

    // The session restarted (steps were cleared): re-seed rather than scroll.
    if (idx < this.displayIndex) {
      this.queue = [];
      this.startMs = null;
      this.displayIndex = idx;
      setText(this.a, idx > 0 ? steps[idx - 1] : "…");
      setText(this.b, steps[Math.max(idx, 0)]);
      this.rest();
      return;
    }

    for (let i = this.displayIndex + 1; i <= idx; i++) this.queue.push(steps[i]);
    this.displayIndex = idx;
    if (this.queue.length > MAX_QUEUE) {
      this.queue = this.queue.slice(-MAX_QUEUE);
    }
  }

  /** Called every frame by the island while the overview is on screen. */
  tick(nowMs: number) {
    if (this.startMs == null) {
      if (this.queue.length === 0) return;
      setText(this.c, this.queue[0]);
      place(this.c, 0, 0);
      this.startMs = nowMs;
    }

    const p = clamp((nowMs - this.startMs) / DURATION, 0, 1);
    const e = EASE(p);

    // Slide the live line out and the next line into the same one-line window.
    this.track.style.transform = `translateY(${-(1 + e) * ROW_H}px)`;
    place(this.a, 1, 0);
    place(this.b, e, clamp(1 - p * 1.35, 0, 1));
    place(this.c, 0, e);

    if (p < 1) return;

    // Commit: the current row becomes the completed one, the incoming row the
    // current one. Texts move, elements stay put — no reordering, no overlap.
    setText(this.a, this.b.text);
    setText(this.b, this.c.text);
    this.queue.shift();
    this.startMs = null;
    this.rest();
  }
}
