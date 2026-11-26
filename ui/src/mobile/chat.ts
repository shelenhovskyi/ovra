// What an agent says, rendered for the phone. Links are shown as plain text
// with their full address and cannot be tapped: an agent's text could point
// at a look-alike page ("pair again here"), and on a phone the address is
// hard to check before a tap.

import { renderChat } from "../agents/markdown";

/** Renders an agent's Markdown `text` to sanitized HTML without links. */
export function renderPhoneChat(text: string): string {
  const doc = new DOMParser().parseFromString(`<body>${renderChat(text)}</body>`, "text/html");
  for (const link of doc.body.querySelectorAll("a")) {
    const label = link.textContent ?? "";
    const address = link.getAttribute("href") ?? "";
    link.replaceWith(!address || address === label ? label : `${label} (${address})`);
  }
  return doc.body.innerHTML;
}
