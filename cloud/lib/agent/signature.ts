/**
 * The signature the endpoint gives every assistant message it answers:
 * HMAC-SHA256, under a key derived from SESSION_SECRET for this use only,
 * over the session id and the message's content as validate.ts rebuilds
 * it, serialised canonically (keys sorted). A turn is forwarded only if
 * each of its assistant messages carries a valid one for its session, so
 * the client cannot put words in the model's mouth, and a conversation is
 * good for the session it was had in only. See docs/assistant.md.
 */
import type Anthropic from "@anthropic-ai/sdk";
import { Invalid, rebuildAssistant, type ValidTurn } from "./validate.ts";

const encoder = new TextEncoder();

/** JSON with every object's keys sorted, so that equal values serialise equally. */
export function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, v]) => v !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonical(v)}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

const keys = new Map<string, Promise<CryptoKey>>();

/** The message key: HMAC(SESSION_SECRET, a label of this use), so no other HMAC under the secret is one of these. */
function messageKey(secret: string): Promise<CryptoKey> {
  let key = keys.get(secret);
  if (!key) {
    key = (async () => {
      const root = await crypto.subtle.importKey("raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
      const derived = await crypto.subtle.sign("HMAC", root, encoder.encode("workpiece assistant message signature v1"));
      return crypto.subtle.importKey("raw", derived, { name: "HMAC", hash: "SHA-256" }, false, ["sign", "verify"]);
    })();
    keys.set(secret, key);
  }
  return key;
}

const payload = (sid: string, content: Anthropic.ContentBlockParam[]) => encoder.encode(`${sid}\n${canonical(content)}`);

function base64url(bytes: ArrayBuffer): string {
  let binary = "";
  for (const byte of new Uint8Array(bytes)) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64url(text: string): Uint8Array<ArrayBuffer> | null {
  try {
    const binary = atob(text.replace(/-/g, "+").replace(/_/g, "/"));
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return bytes;
  } catch {
    return null;
  }
}

/** The signature of an assistant message `content` (as validate.ts's rebuildAssistant gives it) in the session `sid`. */
export async function signMessage(secret: string, sid: string, content: Anthropic.ContentBlockParam[]): Promise<string> {
  return base64url(await crypto.subtle.sign("HMAC", await messageKey(secret), payload(sid, content)));
}

/** Whether every assistant message of `turn` carries a valid signature for the session `sid`. */
export async function verifyTurn(secret: string, sid: string, turn: ValidTurn): Promise<boolean> {
  const key = await messageKey(secret);
  for (const { content, signature } of turn.signed) {
    const bytes = fromBase64url(signature);
    if (!bytes || !(await crypto.subtle.verify("HMAC", key, bytes, payload(sid, content)))) return false;
  }
  return true;
}

/**
 * The assistant message the endpoint answers for the model's `content`,
 * as the conversation keeps it, and its signature: a tool use is kept only
 * when the model stopped to have its tools run (one cut off by
 * `max_tokens` is never answered), empty text blocks are left out, and an
 * empty answer is "…". The signature is null if the content is not one a
 * turn may carry back (a block of a kind validate.ts refuses).
 */
export async function signedReply(
  secret: string,
  sid: string,
  content: readonly { type: string; text?: unknown }[],
  stopReason: string | null,
  toolNames: ReadonlySet<string>,
): Promise<{ content: unknown[]; signature: string | null }> {
  let kept: unknown[] = content.filter((b) => (b.type === "tool_use" ? stopReason === "tool_use" : !(b.type === "text" && b.text === "")));
  if (kept.length === 0) kept = [{ type: "text", text: "…" }];
  try {
    return { content: kept, signature: await signMessage(secret, sid, rebuildAssistant(kept, toolNames)) };
  } catch (e) {
    if (e instanceof Invalid) return { content: kept, signature: null };
    throw e;
  }
}
