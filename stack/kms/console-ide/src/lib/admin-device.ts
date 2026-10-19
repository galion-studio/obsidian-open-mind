import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const FILE = "/root/galion-secrets/admin-device.json";
const COOKIE = "galion_admin";

type Store = { codeHash?: string; tokenHash?: string; tokenHashes?: string[]; deviceHash?: string; fails?: number; lockedUntil?: number };

function sha(value: string) {
  return createHash("sha256").update(value).digest("hex");
}

function readStore(): Store {
  try {
    return JSON.parse(readFileSync(FILE, "utf8")) as Store;
  } catch {
    return {};
  }
}

function writeStore(store: Store) {
  const raw = JSON.stringify(store) + "\n";
  mkdirSync("/root/galion-secrets", { recursive: true });
  writeFileSync(FILE, raw, { mode: 0o600 });
  mkdirSync("/workspace/galion-light/secrets/persist", { recursive: true });
  writeFileSync("/workspace/galion-light/secrets/persist/admin-device.json", raw, { mode: 0o600 });
}

function same(hash: string, raw: string) {
  const next = Buffer.from(sha(raw), "hex");
  const prev = Buffer.from(hash, "hex");
  return next.length === prev.length && timingSafeEqual(next, prev);
}

function hashesOf(store: Store) {
  const list = [...(store.tokenHashes ?? [])];
  if (store.tokenHash && !list.includes(store.tokenHash)) list.push(store.tokenHash);
  return list;
}

function tokenOk(store: Store, token: string) {
  if (!token) return false;
  return hashesOf(store).some((hash) => same(hash, token));
}

function cookieOf(header: string | null) {
  if (!header) return "";
  for (const part of header.split(";")) {
    const [key, ...rest] = part.trim().split("=");
    if (key === COOKIE) return decodeURIComponent(rest.join("="));
  }
  return "";
}

export async function requireAdmin(request: Request): Promise<Response | null> {
  const store = readStore();
  const token = cookieOf(request.headers.get("cookie"));
  if (!tokenOk(store, token)) {
    return Response.json({ error: "phone required" }, { status: 401 });
  }
  return null;
}

export async function adminSession(request: Request) {
  const store = readStore();
  const token = cookieOf(request.headers.get("cookie"));
  const you = tokenOk(store, token);
  return { ok: true, paired: hashesOf(store).length > 0, you };
}

function issue(request: Request, store: Store, deviceHash: string, device?: string) {
  const token = randomBytes(32).toString("base64url");
  const tokenHash = sha(token);
  const tokenHashes = hashesOf(store);
  if (!tokenHashes.includes(tokenHash)) tokenHashes.push(tokenHash);
  while (tokenHashes.length > 4) tokenHashes.shift();
  writeStore({
    codeHash: store.codeHash,
    tokenHash,
    tokenHashes,
    deviceHash: store.deviceHash || deviceHash,
    fails: 0,
  });
  if (store.deviceHash) device = undefined;
  const host = request.headers.get("host") ?? "";
  const secure = host.endsWith("galion.app") || (request.headers.get("x-forwarded-proto") ?? "").includes("https");
  const cookie = `${COOKIE}=${encodeURIComponent(token)}; HttpOnly; Path=/; SameSite=Lax; Max-Age=31536000${secure ? "; Secure" : ""}`;
  return { ok: true as const, cookie, device };
}

export async function pairAdmin(request: Request, code: string, device = "") {
  const store = readStore();
  const presented = device.trim();
  if (presented && store.deviceHash && same(store.deviceHash, presented)) {
    return issue(request, store, store.deviceHash);
  }
  if (store.lockedUntil && Date.now() < store.lockedUntil) {
    return { error: "too many attempts", status: 429 as const };
  }
  const given = code.trim();
  if (!store.codeHash || !given || !same(store.codeHash, given)) {
    const fails = (store.fails ?? 0) + 1;
    const next: Store = { ...store, fails };
    if (fails >= 5) next.lockedUntil = Date.now() + 15 * 60 * 1000;
    writeStore(next);
    return { error: "wrong code", status: 401 as const };
  }
  const secret = randomBytes(32).toString("base64url");
  return issue(request, store, sha(secret), secret);
}

const PIN = "/root/galion-secrets/defender.pin";

export function pinOk(pin: string) {
  try {
    const hash = readFileSync(PIN, "utf8").trim();
    return Boolean(pin.trim()) && same(hash, pin.trim());
  } catch {
    return false;
  }
}
