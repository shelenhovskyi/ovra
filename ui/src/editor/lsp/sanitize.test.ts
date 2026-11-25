import { expect, test } from "vitest";
import { sanitizeHtml } from "./sanitize";

test("keeps the markup Markdown produces", () => {
  const html =
    '<h2>fn</h2><p>Takes <em>a</em> and <strong>b</strong>.</p><pre><code class="language-rust"><span class="tok-keyword">x</span></code></pre>' +
    "<ul><li>one</li></ul><table><thead><tr><th>a</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table><hr><br>";
  expect(sanitizeHtml(html)).toBe(html);
});

test("drops scripts, styles and svg with their content", () => {
  expect(sanitizeHtml("<p>a<script>alert(1)</script>b</p><style>p{}</style>")).toBe("<p>ab</p>");
  expect(sanitizeHtml('<svg><script>alert(1)</script><a href="https://x.dev">x</a></svg>ok')).toBe("ok");
  expect(sanitizeHtml('<svg><foreignObject><p onclick="x()">p</p></foreignObject></svg>ok')).toBe("ok");
  expect(sanitizeHtml("<math><mi>x</mi></math><iframe src=x></iframe><template><p>t</p></template>ok")).toBe("ok");
});

test("drops event handlers and every attribute but class and a safe href, which links also show as a title", () => {
  expect(sanitizeHtml('<img src=x onerror="alert(1)"><p onclick="x()" style="color:red" id="i">t</p>')).toBe(
    "<p>t</p>",
  );
  expect(sanitizeHtml('<span class="tok-c" title="t" data-x="1">s</span>')).toBe('<span class="tok-c">s</span>');
  expect(sanitizeHtml('<a href="https://docs.rs/x" target="_blank" onmouseover="x()">d</a>')).toBe(
    '<a href="https://docs.rs/x" title="https://docs.rs/x">d</a> (docs.rs)',
  );
  expect(sanitizeHtml('<a href="https://evil.example/login">Trust project</a>')).toBe(
    '<a href="https://evil.example/login" title="https://evil.example/login">Trust project</a> (evil.example)',
  );
  expect(sanitizeHtml('<a href="https://docs.rs/x">https://docs.rs/x</a>')).toBe(
    '<a href="https://docs.rs/x" title="https://docs.rs/x">https://docs.rs/x</a>',
  );
});

test("links keep only http and https targets", () => {
  const unsafe = [
    "javascript:alert(1)",
    " JaVaScRiPt:alert(1)",
    "java&#x09;script:alert(1)",
    "data:text/html,x",
    "file:///etc/passwd",
    "/relative",
    "ovra:///a.rs",
  ];
  for (const href of unsafe) expect(sanitizeHtml(`<a href="${href}">x</a>`)).toBe("<a>x</a>");
  expect(sanitizeHtml('<a href="http://example.com/a?b=1&amp;c=2">x</a>')).toBe(
    '<a href="http://example.com/a?b=1&amp;c=2" title="http://example.com/a?b=1&amp;c=2">x</a> (example.com)',
  );
});

test("unknown elements keep their text", () => {
  expect(sanitizeHtml("<div><details><summary>s</summary><p>t</p></details></div>")).toBe("s<p>t</p>");
  expect(sanitizeHtml("<form><input value=x><button>b</button></form>")).toBe("b");
});

test("keeps only highlighting classes, so a tooltip cannot borrow the app's styles", () => {
  expect(sanitizeHtml('<span class="tok-keyword \u037c1x modal overlay">s</span>')).toBe(
    '<span class="tok-keyword \u037c1x">s</span>',
  );
  expect(sanitizeHtml('<p class="dialog">x</p>')).toBe("<p>x</p>");
});
