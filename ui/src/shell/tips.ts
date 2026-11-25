// Tooltips of the rail and the title bar: a `.tip` button shows its
// `data-tip` after 600 ms. A press hides the pressed button's tooltip until
// the pointer leaves it, so it never covers what opened. Once one tooltip
// showed, the group is warm: the next ones show at once while the pointer
// moves along, until it has been away from the group for a second.

export const TIP_DELAY_MS = 600;
const COOL_MS = 1000;

export function hush(event: PointerEvent): void {
  const button = (event.target as Element | null)?.closest<HTMLElement>(".tip");
  if (!button) return;
  button.dataset.hush = "";
  button.addEventListener("pointerleave", () => delete button.dataset.hush, { once: true });
}

/** An attachment for a group of `.tip` buttons: sets `data-warm` on it while it is warm. */
export function warmTips(group: HTMLElement): () => void {
  let warming: ReturnType<typeof setTimeout> | undefined;
  let cooling: ReturnType<typeof setTimeout> | undefined;
  const over = (event: PointerEvent) => {
    clearTimeout(cooling);
    if ("warm" in group.dataset || warming || !(event.target as Element | null)?.closest(".tip")) return;
    warming = setTimeout(() => {
      group.dataset.warm = "";
    }, TIP_DELAY_MS);
  };
  const leave = () => {
    clearTimeout(warming);
    warming = undefined;
    cooling = setTimeout(() => delete group.dataset.warm, COOL_MS);
  };
  group.addEventListener("pointerover", over);
  group.addEventListener("pointerleave", leave);
  return () => {
    clearTimeout(warming);
    clearTimeout(cooling);
    group.removeEventListener("pointerover", over);
    group.removeEventListener("pointerleave", leave);
  };
}
