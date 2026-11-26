// jsdom's `crypto` has no `subtle`; the phone's tests use Node's WebCrypto.
import { webcrypto } from "node:crypto";
import { vi } from "vitest";

vi.stubGlobal("crypto", webcrypto);
