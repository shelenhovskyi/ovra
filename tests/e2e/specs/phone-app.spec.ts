import { webcrypto } from "node:crypto";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "../test";

// The phone app (ui/src/mobile) on a phone-sized screen. The shared daemon
// runs with a fake phone listener, so this page plays the listener's part:
// it answers pairing, sends a challenge, checks the phone's signature with
// the key it paired with, then passes frames to the daemon's own socket.
// What a phone may do is checked in Rust (crates/core/src/companion.rs).
test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

function daemon(): URL {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return new URL(url);
}

/** A client of the daemon for this test's own setup. */
async function desktop(): Promise<(method: string, params?: unknown) => Promise<{ data: unknown }>> {
  const url = daemon();
  const socket = new WebSocket(`ws://${url.host}/ws?token=${url.searchParams.get("token") ?? ""}`);
  await new Promise((ok, fail) => {
    socket.addEventListener("open", ok, { once: true });
    socket.addEventListener("error", fail, { once: true });
  });
  let next = 1;
  const waiting = new Map<number, (frame: { result?: { data: unknown }; error?: unknown }) => void>();
  socket.addEventListener("message", (e) => {
    const frame = JSON.parse(String(e.data)) as { id?: number; result?: { data: unknown }; error?: unknown };
    if (frame.id !== undefined) waiting.get(frame.id)?.(frame);
  });
  return (method, params) =>
    new Promise((ok, fail) => {
      const id = next++;
      waiting.set(id, (frame) => (frame.result ? ok(frame.result) : fail(new Error(JSON.stringify(frame.error)))));
      socket.send(JSON.stringify({ id, method, ...(params === undefined ? {} : { params }) }));
    });
}

const b64 = (text: string) => Buffer.from(text.replace(/-/g, "+").replace(/_/g, "/"), "base64");

/** Plays the phone listener for `page`; resolves to the challenges the phone signed. */
async function fakeListener(page: Page): Promise<{ signed: string[] }> {
  const url = daemon();
  const origin = `http://${url.host}`;
  let key: webcrypto.CryptoKey | null = null;
  const signed: string[] = [];
  await page.route("**/api/pair", async (route) => {
    const body = JSON.parse(route.request().postData() ?? "{}") as { key: string; name: string };
    key = await webcrypto.subtle.importKey("raw", b64(body.key), { name: "ECDSA", namedCurve: "P-256" }, false, [
      "verify",
    ]);
    await route.fulfill({ json: { device: "d1", name: body.name } });
  });
  await page.routeWebSocket(/\/ws$/, (ws) => {
    const challenge = `c${signed.length + 1}`;
    let server: WebSocket | null = null;
    ws.send(JSON.stringify({ challenge }));
    ws.onMessage(async (message) => {
      if (server) {
        const frame = JSON.parse(String(message)) as { id: number; method: string };
        // The listener answers `phone.*` itself; forgetting is the one tested here.
        if (frame.method === "phone.forget") ws.send(JSON.stringify({ id: frame.id, result: null }));
        else server.send(String(message));
        return;
      }
      const proof = JSON.parse(String(message)) as { device: string; signature: string };
      const text = new TextEncoder().encode(`ovra-companion-v1\n${origin}\n${challenge}`);
      const ok =
        key !== null &&
        proof.device === "d1" &&
        (await webcrypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, key, b64(proof.signature), text));
      if (!ok) {
        ws.send(JSON.stringify({ error: "not_paired" }));
        ws.close();
        return;
      }
      signed.push(challenge);
      server = new WebSocket(`ws://${url.host}/ws?token=${url.searchParams.get("token") ?? ""}`);
      server.addEventListener("message", (e) => ws.send(String(e.data)));
      server.addEventListener("open", () => ws.send(JSON.stringify({ paired: "Test phone" })));
      server.addEventListener("close", () => ws.close());
    });
    ws.onClose(() => server?.close());
  });
  return { signed };
}

test("pair a phone, answer an agent's requests, reply and stop it", async ({ page }) => {
  const call = await desktop();
  const dir = mkdtempSync(join(tmpdir(), "ovra-phone-app-"));
  writeFileSync(join(dir, "README.md"), "teh readme\n");
  const { data: project } = (await call("projects.add", { root: dir })) as { data: { id: string; name: string } };
  const title = `Phone check ${Date.now()}`;
  await call("agents.start", { project: project.id, agent: "claude", mode: "direct", prompt: title });

  const listener = await fakeListener(page);
  await page.clock.install();
  await page.goto(`http://${daemon().host}/mobile.html#pair=AbCdEfGhIjKlMnOpQrStUv`);
  await expect(page).toHaveURL(/#\/$/);
  await page.getByRole("textbox", { name: "Name of this phone" }).fill("Test phone");
  await page.getByRole("button", { name: "Pair" }).tap();
  await expect(page.getByRole("heading", { name: "Ovra" })).toBeVisible();
  await expect.poll(() => listener.signed).toEqual(["c1"]);

  const mine = page.getByTestId("needs-you").getByRole("link").filter({ hasText: title });
  await mine.filter({ hasText: "Run ls -la" }).tap();
  const approval = page.getByTestId("approval");
  await expect(approval.getByRole("heading", { name: "Run ls -la" })).toBeVisible();
  await expect(approval.getByRole("button", { name: /Always/ })).toHaveCount(0);
  await page.clock.fastForward(600);
  await approval.getByRole("button", { name: "Allow" }).tap();

  // Allow goes to this agent's next request when it has one, else Home.
  await page.evaluate(() => {
    location.hash = "#/";
  });
  await mine.filter({ hasText: "Edit README.md" }).tap();
  // The buttons arm half a second after the screen appears, not after the tap.
  const withReason = approval.getByRole("button", { name: "Deny with a reason…" });
  await expect(withReason).toBeVisible();
  await page.clock.fastForward(600);
  await withReason.tap();
  await approval.getByRole("textbox", { name: "Tell the agent why (optional)" }).fill("Not now");
  await approval.getByRole("button", { name: "Deny" }).tap();
  await expect(page.getByRole("heading", { name: "Ovra" })).toBeVisible();

  await page.getByTestId("finished").getByRole("link").filter({ hasText: title }).tap();
  await expect(page.getByRole("list", { name: "Conversation" })).toContainText("Run ls -la");
  await page.getByRole("textbox", { name: "Reply" }).fill("Check the status");
  await page.getByRole("button", { name: "Send" }).tap();
  await expect(page.getByRole("link", { name: "Needs you: Run git status" })).toBeVisible();
  await page.getByRole("button", { name: "Stop" }).tap();
  await expect(page.getByRole("link", { name: "Needs you: Run git status" })).toHaveCount(0);

  await page.getByRole("link", { name: "Back" }).tap();
  await page.getByRole("link", { name: "Computer" }).tap();
  await expect(page.getByRole("radiogroup", { name: "Keep awake" })).toBeVisible();
  await page.getByRole("button", { name: "Forget this phone" }).tap();
  await page.getByRole("button", { name: "Forget", exact: true }).tap();
  await expect(page.getByRole("heading", { name: "This phone is no longer paired" })).toBeVisible();
  await call("agents.stop", { session: (await sessionOf(call, project.id, title)) ?? "" });
});

test("a phone the computer does not know is asked to pair", async ({ page }) => {
  await fakeListener(page);
  await page.goto(`http://${daemon().host}/mobile.html`);
  await expect(page.getByRole("heading", { name: "Pair this phone first" })).toBeVisible();
});

async function sessionOf(
  call: Awaited<ReturnType<typeof desktop>>,
  project: string,
  title: string,
): Promise<string | undefined> {
  const { data } = (await call("agents.list", { project })) as { data: { id: string; title: string }[] };
  return data.find((s) => s.title === title)?.id;
}
