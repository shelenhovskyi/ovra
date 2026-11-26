import "./test-crypto";
import { createHash } from "node:crypto";
import { expect, it } from "vitest";
import {
  base64url,
  challengeText,
  fromBase64url,
  guessName,
  makeKey,
  memoryStore,
  pairingCode,
  publicKey,
  sign,
} from "./identity";
import { pair, pairLink, pairSecret } from "./pair";

it("makes a key whose private half cannot leave the browser and signs the server's text", async () => {
  const key = await makeKey();
  expect(key.privateKey.extractable).toBe(false);
  await expect(crypto.subtle.exportKey("pkcs8", key.privateKey)).rejects.toThrow();
  const raw = fromBase64url(await publicKey(key));
  expect(raw.length).toBe(65);
  expect(raw[0]).toBe(4);

  const text = challengeText("https://192.168.1.20:52044", "abc");
  expect(text).toBe("ovra-companion-v1\nhttps://192.168.1.20:52044\nabc");
  const signature = fromBase64url(await sign(key, text));
  // Raw r||s, as the server reads it (ECDSA_P256_SHA256_FIXED).
  expect(signature.length).toBe(64);
  const verify = (t: string) =>
    crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, key.publicKey, signature, new TextEncoder().encode(t));
  expect(await verify(text)).toBe(true);
  expect(await verify(challengeText("https://evil", "abc"))).toBe(false);
});

it("shows the same four digits as the computer", async () => {
  const digest = createHash("sha256").update("secret\nkey").digest();
  const expected = String(digest.readUInt32BE(0) % 10_000).padStart(4, "0");
  expect(await pairingCode("secret", "key")).toBe(expected);
});

it("round-trips base64url and names the phone", () => {
  const bytes = new Uint8Array([0, 251, 255, 62, 63]);
  expect(base64url(bytes)).toBe("APv_Pj8");
  expect([...fromBase64url("APv_Pj8")]).toEqual([...bytes]);
  expect(guessName("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0)")).toBe("iPhone");
  expect(guessName("Mozilla/5.0 (Linux; Android 15)")).toBe("Android phone");
  expect(guessName("Mozilla/5.0 (X11)")).toBe("Phone");
});

it("reads the pairing secret only from a well-formed hash", () => {
  expect(pairSecret("#pair=AbCdEfGhIjKlMnOpQrStUv_-0123")).toBe("AbCdEfGhIjKlMnOpQrStUv_-0123");
  expect(pairSecret("#pair=short")).toBeNull();
  expect(pairSecret("#/agent/x")).toBeNull();
  expect(pairSecret("#pair=AbCdEfGhIjKlMnOp&x=1")).toBeNull();
});

it("takes a pasted pairing link only for this computer", () => {
  const origin = "https://192.168.1.20:52044";
  expect(pairLink(` ${origin}/#pair=AbCdEfGhIjKlMnOpQrStUv \n`, origin)).toBe("AbCdEfGhIjKlMnOpQrStUv");
  expect(pairLink("https://192.168.1.21:52044/#pair=AbCdEfGhIjKlMnOpQrStUv", origin)).toBeNull();
  expect(pairLink(`${origin}/#/agent/x`, origin)).toBeNull();
  expect(pairLink("AbCdEfGhIjKlMnOpQrStUv", origin)).toBeNull();
});

it("pairs: shows the code, sends the public key and keeps what the computer answers", async () => {
  const sent: { url: string; body: { secret: string; key: string; name: string } }[] = [];
  const post = ((url: string, init: RequestInit) => {
    sent.push({ url, body: JSON.parse(String(init.body)) });
    return Promise.resolve(new Response(JSON.stringify({ device: "d1", name: "Ana's phone" })));
  }) as typeof fetch;
  const steps: string[] = [];
  const store = memoryStore();
  await pair(
    "https://192.168.1.20:52044",
    "the-secret",
    "Ana's phone",
    (step) => {
      steps.push(step.kind);
      if (step.kind === "waiting") expect(step.code).toMatch(/^\d{4}$/);
      if (step.kind === "paired") void store.save(step.identity);
    },
    post,
  );
  expect(steps).toEqual(["waiting", "paired"]);
  expect(sent[0]?.url).toBe("https://192.168.1.20:52044/api/pair");
  expect(sent[0]?.body.secret).toBe("the-secret");
  expect(fromBase64url(sent[0]?.body.key ?? "").length).toBe(65);
  const kept = await store.load();
  expect(kept?.device).toBe("d1");
  expect(kept?.origin).toBe("https://192.168.1.20:52044");
});

it("says why pairing failed", async () => {
  const answer = (status: number, text: string) =>
    (() => Promise.resolve(new Response(text, { status }))) as unknown as typeof fetch;
  const outcome = async (post: typeof fetch) => {
    let last = "";
    await pair(
      "https://h",
      "s",
      "n",
      (step) => {
        if (step.kind === "failed") last = step.message;
      },
      post,
    );
    return last;
  };
  expect(await outcome(answer(403, "the computer did not allow this phone"))).toMatch(/did not allow/);
  expect(await outcome(answer(403, "this pairing code does not work any more"))).toMatch(/new one/);
  expect(await outcome(answer(429, ""))).toMatch(/Too many tries/);
  expect(await outcome((() => Promise.reject(new Error("offline"))) as unknown as typeof fetch)).toMatch(
    /same network/,
  );
});
