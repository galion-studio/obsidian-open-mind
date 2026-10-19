import { execFileSync } from "node:child_process";
import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { chmodSync, existsSync, mkdirSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";

const DIR = "/root/galion-keys";
const DB_PATH = `${DIR}/keys.db`;
const SEALED = "/workspace/galion-light/keys/keys.db.sealed";
const VAULT = "/root/galion-secrets/vault.key";
export const KEY_SCOPES = ["chat", "tools", "files", "admin"] as const;

type Scope = (typeof KEY_SCOPES)[number];
type Row = { id: string; name: string; prefix: string; scopes: string; created_at: string; revoked_at: string | null };

function sha(value: string) {
  return createHash("sha256").update(value).digest("hex");
}

function same(hash: string, raw: string) {
  const next = Buffer.from(sha(raw), "hex");
  const prev = Buffer.from(hash, "hex");
  return next.length === prev.length && timingSafeEqual(next, prev);
}

function restore() {
  if (existsSync(DB_PATH) || !existsSync(SEALED) || !existsSync(VAULT)) return;
  mkdirSync(DIR, { recursive: true });
  execFileSync("openssl", ["enc", "-d", "-aes-256-cbc", "-pbkdf2", "-salt", "-pass", `file:${VAULT}`, "-in", SEALED, "-out", DB_PATH], { timeout: 8000 });
}

function seal() {
  if (!existsSync(VAULT) || !existsSync(DB_PATH)) return;
  mkdirSync("/workspace/galion-light/keys", { recursive: true });
  const tmp = `${SEALED}.tmp`;
  execFileSync("openssl", ["enc", "-aes-256-cbc", "-pbkdf2", "-salt", "-pass", `file:${VAULT}`, "-in", DB_PATH, "-out", tmp], { timeout: 8000 });
  execFileSync("mv", ["-f", tmp, SEALED]);
}

function openDb() {
  mkdirSync(DIR, { recursive: true });
  try { chmodSync(DIR, 0o700); } catch { /* overlay accepts this */ }
  restore();
  const db = new DatabaseSync(DB_PATH);
  try { chmodSync(DB_PATH, 0o600); } catch { /* live disk */ }
  db.exec(`create table if not exists api_keys (
    id text primary key,
    name text not null,
    prefix text not null,
    hash text not null,
    scopes text not null,
    created_at text not null,
    revoked_at text
  )`);
  return db;
}

function scopesOf(raw: unknown) {
  const list = Array.isArray(raw) ? raw.map(String) : ["chat"];
  const picked = KEY_SCOPES.filter((scope) => list.includes(scope));
  return (picked.length ? picked : ["chat"]) as Scope[];
}

export function listKeys() {
  const db = openDb();
  const rows = db.prepare("select id, name, prefix, scopes, created_at, revoked_at from api_keys order by created_at desc").all() as Row[];
  db.close();
  return rows.map((row) => ({ ...row, scopes: row.scopes.split(",").filter(Boolean) }));
}

export function mintKey(name: string, rawScopes: unknown) {
  const scopes = scopesOf(rawScopes);
  const secret = `gal_sk_${randomBytes(32).toString("base64url")}`;
  const id = randomBytes(4).toString("hex");
  const db = openDb();
  db.prepare("insert into api_keys (id, name, prefix, hash, scopes, created_at) values (?, ?, ?, ?, ?, ?)").run(
    id,
    name.trim().slice(0, 64) || "key",
    secret.slice(0, 12),
    sha(secret),
    scopes.join(","),
    new Date().toISOString(),
  );
  db.close();
  seal();
  return { id, name: name.trim().slice(0, 64) || "key", prefix: secret.slice(0, 12), scopes, key: secret };
}

export function revokeKey(id: string) {
  const db = openDb();
  const row = db.prepare("update api_keys set revoked_at = ? where id = ? and revoked_at is null").run(new Date().toISOString(), id);
  db.close();
  seal();
  return { ok: row.changes > 0 };
}

export function checkKey(secret: string) {
  const db = openDb();
  const rows = db.prepare("select hash, scopes, revoked_at from api_keys").all() as { hash: string; scopes: string; revoked_at: string | null }[];
  db.close();
  let match: { scopes: string } | null = null;
  let dummy = false;
  for (const row of rows) {
    const hit = same(row.hash, secret);
    if (hit && !row.revoked_at && !match) match = row;
    dummy = dummy || hit;
  }
  if (!rows.length) dummy = same(sha("gal_sk_none"), secret);
  void dummy;
  return match ? { ok: true, scopes: match.scopes.split(",") } : { ok: false, scopes: [] as string[] };
}
