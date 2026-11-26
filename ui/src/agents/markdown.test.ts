import { expect, it } from "vitest";
import { plainChat, renderChat } from "./markdown";

it("shows lists, emphasis, code and headings", () => {
  const html = renderChat("## Plan\n\n- **one** and `two`\n- _three_\n\n```rust\nfn main() {}\n```");
  expect(html).toContain("<h2>Plan</h2>");
  expect(html).toContain("<li><strong>one</strong> and <code>two</code></li>");
  expect(html).toContain("<em>three</em>");
  expect(html).toContain('<pre><code class="language-rust">fn main() {}\n</code></pre>');
});

it("never lets an agent's text add markup, scripts or remote images", () => {
  const html = renderChat(
    [
      "<script>alert(1)</script>",
      '<img src=x onerror="alert(1)">',
      '<iframe src="https://evil.example"></iframe>',
      "[click](javascript:alert(1))",
      "![tracker](https://evil.example/pixel.png)",
      '```"><img src=x onerror=alert(1)>\nx\n```',
    ].join("\n\n"),
  );
  const doc = new DOMParser().parseFromString(html, "text/html");
  expect(doc.querySelectorAll("script, img, iframe").length).toBe(0);
  for (const el of doc.body.querySelectorAll("*")) {
    for (const attr of el.attributes) expect(attr.name).not.toMatch(/^on/);
  }
  expect(doc.querySelector("a")?.getAttribute("href") ?? "").not.toMatch(/javascript/i);
  expect(html).toContain("[tracker]");
  expect(html).toContain("&lt;script&gt;");
});

it("shows where a link goes and keeps task-list boxes read-only", () => {
  const html = renderChat("[docs](https://example.com)\n\n- [x] done");
  const doc = new DOMParser().parseFromString(html, "text/html");
  expect(doc.querySelector("a")?.getAttribute("title")).toBe("https://example.com");
  expect(doc.querySelector("input")?.hasAttribute("disabled")).toBe(true);
});

it("keeps no target for links Ovra would not follow", () => {
  const doc = new DOMParser().parseFromString(renderChat("[m](mailto:a@b.c) [x](xmpp:a) [i](irc://h/c)"), "text/html");
  const links = [...doc.querySelectorAll("a")];
  expect(links.length).toBe(3);
  for (const a of links) expect(a.hasAttribute("href")).toBe(false);
});

it("gives screen readers the words without the Markdown marks", () => {
  const text =
    "# Summary\n\nFixed **two** bugs:\n\n- one\n- two\n\n| Step | Who |\n|---|---|\n| test | me |\n\n```rust\nfn main() {}\n```";
  expect(plainChat(text)).toBe("Summary Fixed two bugs: one two Step Who test me fn main() {}");
});
