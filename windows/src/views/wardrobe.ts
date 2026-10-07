// Wardrobe — right-click Mochi. Layout follows Coucou 0.1.5's WardrobeView:
// he stays on the left, the pieces sit in a grid on the right.

import { h } from "./dom";
import { State } from "../core/state";
import {
  OUTFIT_IDS, outfitLabel, outfitName, paintSwatch, type OutfitId,
} from "../mochi/outfits";

export interface WardrobeActions {
  dress(id: OutfitId): void;
  previewDress(id: OutfitId | null): void;
}

export function buildWardrobe(actions: WardrobeActions) {
  const name = h("div", { class: "wardrobe-name" });
  const grid = h("div", { class: "wardrobe-grid" });
  const pills = new Map<OutfitId, { btn: HTMLButtonElement; canvas: HTMLCanvasElement }>();

  for (const id of OUTFIT_IDS) {
    const canvas = h("canvas") as HTMLCanvasElement;
    const btn = h("button", {
      class: "wardrobe-pill",
      title: outfitName(id),
      type: "button",
    }, canvas) as HTMLButtonElement;
    if (id === "auto") btn.append(h("span", { class: "wardrobe-auto", text: "AUTO" }));
    btn.addEventListener("pointerenter", () => actions.previewDress(id));
    btn.addEventListener("pointerleave", () => actions.previewDress(null));
    btn.addEventListener("click", () => actions.dress(id));
    grid.append(btn);
    pills.set(id, { btn, canvas });
  }

  const el = h(
    "div",
    { class: "view wardrobe" },
    h(
      "div",
      { class: "card" },
      h(
        "div",
        { class: "wardrobe-copy" },
        h("div", { class: "wardrobe-title", text: "Wardrobe" }),
        name,
      ),
      grid,
    ),
  );

  let painted = false;

  return {
    el,
    sync() {
      const saved = State.settings.mochiOutfit;
      const hover = State.wardrobeHover;
      const shown = (OUTFIT_IDS as readonly string[]).includes(hover ?? saved)
        ? (hover ?? saved) as OutfitId
        : "auto";
      name.textContent = outfitLabel(shown, new Date());
      for (const [id, pill] of pills) {
        pill.btn.classList.toggle("on", id === saved);
        pill.btn.classList.toggle("hot", id === hover);
        if (!painted) paintSwatch(pill.canvas, id);
      }
      painted = true;
    },
  };
}
