// Pairing (ADR 0014): the QR code opens `https://<computer>/#pair=<secret>`.
// The phone makes its key, sends the public half with the secret, and waits
// while the person compares the code and clicks Allow on the computer.

import { type Identity, makeKey, pairingCode, publicKey } from "./identity";

/** The secret in `hash` (`#pair=...`), or null. */
export function pairSecret(hash: string): string | null {
  const match = /^#pair=([A-Za-z0-9_-]{16,128})$/.exec(hash);
  return match?.[1] ?? null;
}

/** `waiting`: the key is made and `code` is what the computer should show. */
export type PairStep =
  | { kind: "waiting"; code: string }
  | { kind: "paired"; identity: Identity }
  | { kind: "failed"; message: string };

/**
 * Pairs with the computer at `origin`. `post` is `fetch` in the app. Calls
 * `onStep` with the code first, then the outcome.
 */
export async function pair(
  origin: string,
  secret: string,
  name: string,
  onStep: (step: PairStep) => void,
  post: typeof fetch = fetch,
): Promise<void> {
  let key: CryptoKeyPair;
  let raw: string;
  try {
    key = await makeKey();
    raw = await publicKey(key);
  } catch {
    onStep({ kind: "failed", message: "This browser cannot make a key. Open the page in Safari or Chrome." });
    return;
  }
  onStep({ kind: "waiting", code: await pairingCode(secret, raw) });
  let response: Response;
  try {
    response = await post(`${origin}/api/pair`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ secret, key: raw, name }),
    });
  } catch {
    onStep({ kind: "failed", message: "The computer cannot be reached. Check that the phone is on the same network." });
    return;
  }
  if (!response.ok) {
    const text = response.status === 403 ? await response.text().catch(() => "") : "";
    onStep({ kind: "failed", message: failure(response.status, text) });
    return;
  }
  const body = (await response.json()) as { device: string; name: string };
  onStep({ kind: "paired", identity: { device: body.device, name: body.name, origin, key } });
}

function failure(status: number, text: string): string {
  if (status === 429) return "Too many tries. Wait a minute, then scan a new code.";
  if (status === 403 && text.includes("did not allow")) return "The computer did not allow this phone.";
  if (status === 403) return "This code does not work any more. Show a new one on the computer and scan it again.";
  return `Pairing failed (${status}).`;
}
