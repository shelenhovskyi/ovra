import { describe, expect, it } from "vitest";
import { renderMarkdown } from "./markdown";

describe("renderMarkdown", () => {
  it("renders GFM", () => {
    const html = renderMarkdown("# Title\n\n- [x] done\n\n| a |\n| - |\n| 1 |\n\n~~old~~ https://example.com");
    expect(html).toContain('<h1 id="title">Title</h1>');
    expect(html).toContain('<input type="checkbox" disabled="" checked="">');
    expect(html).toContain("<td>1</td>");
    expect(html).toContain("<del>old</del>");
    expect(html).toContain('<a href="https://example.com">https://example.com</a>');
  });

  it("escapes raw HTML instead of rendering it", () => {
    const html = renderMarkdown('<script>alert(1)</script>\n\n<img src=x onerror="alert(1)"> <b>bold</b>');
    expect(html).not.toMatch(/<script|<img|<b>/);
    expect(html).toContain("&lt;script&gt;");
  });

  it("drops unsafe link schemes", () => {
    const html = renderMarkdown("[x](javascript:alert(1)) ![y](data:image/svg+xml,<svg/>)");
    expect(html).not.toContain("javascript:");
    expect(html).not.toContain("data:");
  });

  it("gives headings unique ids and defers image loading", () => {
    const html = renderMarkdown("## Same\n\n## Same\n\n![logo](img/logo.png)");
    expect(html).toContain('<h2 id="same">Same</h2>');
    expect(html).toContain('<h2 id="same-1">Same</h2>');
    expect(html).toContain('<img alt="logo" data-src="img/logo.png">');
  });
});
