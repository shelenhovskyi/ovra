import { expect, it } from "vitest";
import { renderPhoneChat } from "./chat";

it("shows an agent's links as text with their full address, never as links", () => {
  const html = renderPhoneChat(
    "[Pair again](https://evil.example/pair) or [the guide](docs/a.md), see https://example.com/x and **bold**",
  );
  const doc = new DOMParser().parseFromString(html, "text/html");
  expect(doc.querySelectorAll("a").length).toBe(0);
  expect(doc.body.textContent).toBe(
    "Pair again (https://evil.example/pair) or the guide (docs/a.md), see https://example.com/x and bold",
  );
  expect(html).toContain("<strong>bold</strong>");
});

it("keeps the desktop's sanitizing", () => {
  const html = renderPhoneChat("<script>alert(1)</script>\n\n[x](javascript:alert(1))");
  expect(html).not.toContain("<script>");
  expect(html).toContain("<p>x</p>");
});
