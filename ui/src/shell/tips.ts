// Tooltips of the rail and the title bar: a `.tip` button shows its
// `data-tip`. A press hides the pressed button's tooltip until the pointer
// leaves it, so it never covers what opened.

export function hush(event: PointerEvent): void {
  const button = (event.target as Element | null)?.closest<HTMLElement>(".tip");
  if (!button) return;
  button.dataset.hush = "";
  button.addEventListener("pointerleave", () => delete button.dataset.hush, { once: true });
}
