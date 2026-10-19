import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, existsSync, mkdirSync, appendFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { probeFleet } from "@/lib/probe";

export const HERMES_BIND = "127.0.0.1:8642";
export const HERMES_URL =
  process.env.HERMES_BASE_URL ?? process.env.HERMES_URL ?? `http://${HERMES_BIND}`;

const MAX_RUNS_PER_USER = 2;
const liveRuns = new Map<string, Set<string>>();

function usableKey(raw: string) {
  const value = raw.trim();
  if (value.length < 16) return "";
  if (/sealed/i.test(value)) return "";
  return value;
}

function hermesKey() {
  const fromEnv = usableKey(process.env.HERMES_API_KEY ?? process.env.API_SERVER_KEY ?? "");
  if (fromEnv) return fromEnv;
  for (const p of ["/workspace/hermes/.env", "/root/galion-secrets/hermes.env", "/root/.hermes/.env"]) {
    try {
      if (!existsSync(p)) continue;
      const text = readFileSync(p, "utf8");
      const match = text.match(/^API_SERVER_KEY=(.+)$/m);
      const got = usableKey(match?.[1] ?? "");
      if (got) return got;
    } catch {
      /* skip */
    }
  }
  return "";
}

const CONTEXT_FILE = "/workspace/hermes/context-cache.json";

type Turn = { role: string; text: string };
type ContextFile = Record<string, { mode?: string; model?: string; turns: Turn[] }>;

function readContextFile(): ContextFile {
  if (!existsSync(CONTEXT_FILE)) return {};
  try {
    return JSON.parse(readFileSync(CONTEXT_FILE, "utf8")) as ContextFile;
  } catch {
    return {};
  }
}

export function contextBrief(sessionId?: string) {
  if (!sessionId) return "";
  const turns = readContextFile()[sessionId]?.turns ?? [];
  if (!turns.length) return "";
  const lines = turns.slice(-6).map((t) => `${t.role}: ${t.text.slice(0, 360)}`);
  return `[context cache]\n${lines.join("\n")}\n[/context cache]`;
}

export function rememberContext(sessionId: string, turns: Turn[], meta?: { mode?: string; model?: string }) {
  if (!sessionId) return { ok: false };
  const file = readContextFile();
  const prev = file[sessionId]?.turns ?? [];
  const next = [...prev, ...turns]
    .map((t) => ({ role: t.role.slice(0, 16), text: t.text.replace(/\s+/g, " ").trim().slice(0, 480) }))
    .filter((t) => t.text)
    .slice(-8);
  file[sessionId] = { mode: meta?.mode, model: meta?.model, turns: next };
  mkdirSync("/workspace/hermes", { recursive: true });
  writeFileSync(CONTEXT_FILE, JSON.stringify(file));
  return { ok: true, n: next.length };
}

export function sessionKeyFor(userId: string, projectId: string) {
  return `console:user:${userId}:project:${projectId}`;
}

export function upstreamHeaders(opts: {
  userId: string;
  projectId: string;
  sessionId?: string;
}) {
  const headers: Record<string, string> = {
    Authorization: `Bearer ${hermesKey()}`,
    "Content-Type": "application/json",
    "X-Hermes-Session-Key": sessionKeyFor(opts.userId, opts.projectId),
  };
  if (opts.sessionId) headers["X-Hermes-Session-Id"] = opts.sessionId;
  return headers;
}

function envVal(name: string) {
  const direct = process.env[name]?.trim();
  if (direct) return direct;
  try {
    if (!existsSync("/workspace/hermes/.env")) return "";
    const m = readFileSync("/workspace/hermes/.env", "utf8").match(
      new RegExp(`^${name}=(.+)$`, "m"),
    );
    return m?.[1]?.trim() ?? "";
  } catch {
    return "";
  }
}

export const LANES = {
  flash: { label: "Flash", model: "deepseek-ai/DeepSeek-V4.1-Flash" },
  agent: { label: "Agent", model: "deepseek-ai/DeepSeek-V4.1-Flash" },
  code: { label: "Code", model: "deepseek-ai/DeepSeek-V4.1-Flash" },
} as const;

export type LaneId = keyof typeof LANES;

export function readControl(): Record<string, unknown> {
  try {
    const data = JSON.parse(readFileSync("/workspace/hermes/control.json", "utf8"));
    return data && typeof data === "object" ? (data as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

export function modelForLane(lane?: string) {
  const picked = readControl().model;
  if (typeof picked === "string" && picked.includes("/")) return picked;
  if (lane && lane in LANES) return LANES[lane as LaneId].model;
  return LANES.flash.model;
}

export function hermesModelId(lane?: string) {
  return modelForLane(lane);
}

function openrouterKey() {
  return envVal("OPENROUTER_API_KEY");
}

const SKILL_GROUPS: { group: string; roots: string[] }[] = [
  { group: "Local", roots: ["/opt/skills/local", "/workspace/galion-light/skills"] },
  { group: "Addy", roots: ["/opt/skills/addy"] },
  { group: "Ondrej", roots: ["/opt/skills/ondrej/skills", "/opt/skills/ondrej"] },
  { group: "ZeroAI", roots: ["/opt/skills/zeroai"] },
];

function readSkillFile(id: string, md: string, group: string) {
  const text = readFileSync(md, "utf8");
  const name = text.match(/^name:\s*(.+)$/m)?.[1]?.trim() ?? id;
  const description = text.match(/^description:\s*(.+)$/m)?.[1]?.trim() ?? "";
  const body = text.replace(/^---[\s\S]*?---\s*/, "").trim();
  return {
    id,
    name,
    group,
    description: String(description).slice(0, 180),
    body,
  };
}

export function listPrivateSkills() {
  const out: { id: string; name: string; group: string; description: string; body: string }[] = [];
  const seen = new Set<string>();
  for (const { group, roots } of SKILL_GROUPS) {
    for (const root of roots) {
      if (!existsSync(root)) continue;
      try {
        const skillAtRoot = join(root, "SKILL.md");
        if (existsSync(skillAtRoot) && !seen.has(root)) {
          seen.add(root);
          out.push(readSkillFile(root.split("/").pop() ?? "skill", skillAtRoot, group));
        }
        for (const dir of readdirSync(root)) {
          const md = join(root, dir, "SKILL.md");
          if (!existsSync(md) || seen.has(`${group}:${dir}`)) continue;
          seen.add(`${group}:${dir}`);
          out.push(readSkillFile(dir, md, group));
        }
      } catch {
        /* skip */
      }
    }
  }
  return out;
}

export async function hermesHealth() {
  const started = Date.now();
  const skills = listPrivateSkills().map(({ body: _b, ...rest }) => rest);
  try {
    const res = await fetch(`${HERMES_URL}/health`, {
      signal: AbortSignal.timeout(2000),
      headers: hermesKey() ? { Authorization: `Bearer ${hermesKey()}` } : undefined,
    });
    return {
      ok: res.ok,
      status: res.status,
      ms: Date.now() - started,
      bind: HERMES_BIND,
      loopback: true,
      mode: res.ok ? "gateway" : "down",
      skills,
      raw: (await res.text()).slice(0, 400),
    };
  } catch (err) {
    return {
      ok: false,
      status: 0,
      ms: Date.now() - started,
      bind: HERMES_BIND,
      loopback: true,
      mode: "down",
      skills,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

function pickSkill(query: string) {
  const q = query.toLowerCase();
  const skills = listPrivateSkills();
  let best = skills[0];
  let score = -1;
  for (const s of skills) {
    let n = 0;
    if (q.includes(s.id)) n += 5;
    if (q.includes(s.name.toLowerCase())) n += 4;
    if (n > score) {
      score = n;
      best = s;
    }
  }
  return score > 0 ? best : null;
}

async function fallbackText(user: string) {
  if (/\bping\b/.test(user.toLowerCase())) {
    const host = user.match(/([a-z0-9.-]+\.[a-z]{2,})/i)?.[1];
    const data = await probeFleet(host ? [host] : undefined);
    return `Edge ${data.ok}/${data.count} up · ${data.tunnel} × 1033\n${data.items
      .slice(0, 16)
      .map((i) => `${i.code.padEnd(8)} ${i.host}  ${i.ms}ms`)
      .join("\n")}`;
  }
  return "Hermes did not answer this chat. The gateway is the place that replies. Ask again.";
}

function sseFromText(text: string) {
  const encoder = new TextEncoder();
  return new Response(
    new ReadableStream({
      start(controller) {
        const payload = JSON.stringify({
          id: "galion-fallback",
          object: "chat.completion.chunk",
          model: "galion-light",
          choices: [{ index: 0, delta: { content: text }, finish_reason: null }],
        });
        controller.enqueue(encoder.encode(`data: ${payload}\n\n`));
        controller.enqueue(encoder.encode("data: [DONE]\n\n"));
        controller.close();
      },
    }),
    {
      headers: {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      },
    },
  );
}

function pipeSse(res: Response) {
  const encoder = new TextEncoder();
  const reader = res.body?.getReader();
  const stream = new ReadableStream({
    async start(controller) {
      if (!reader) {
        controller.close();
        return;
      }
      let last = Date.now();
      const timer = setInterval(() => {
        if (Date.now() - last < 8000) return;
        try {
          controller.enqueue(encoder.encode(`: ping\n\n`));
          last = Date.now();
        } catch {
          clearInterval(timer);
        }
      }, 4000);
      try {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          last = Date.now();
          controller.enqueue(value);
        }
      } catch {
        /* upstream closed */
      } finally {
        clearInterval(timer);
        try {
          controller.close();
        } catch {
          /* already closed */
        }
      }
    },
    cancel() {
      void reader?.cancel();
    },
  });
  return new Response(stream, {
    status: res.status,
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
}

export async function hermesChat(opts: {
  messages: { role: string; content: string }[];
  sessionId: string;
  projectId: string;
  userId: string;
  lane?: string;
  mode?: string;
}) {
  const blocked = assertSpend();
  if (blocked) return blocked;
  const sent = opts.messages.filter((m) => m && (m.role === "user" || m.role === "assistant") && m.content).slice(-24);
  try {
    const res = await fetch(`${HERMES_URL}/v1/chat/completions`, {
      method: "POST",
      headers: upstreamHeaders({ userId: opts.userId, projectId: opts.projectId, sessionId: opts.sessionId }),
      body: JSON.stringify({
        model: "deepseek-ai/DeepSeek-V4.1-Flash",
        provider: "river",
        stream: true,
        model_options: { reasoning: { enabled: false, effort: "low" } },
        messages: sent,
      }),
      signal: AbortSignal.timeout(30 * 60 * 1000),
    });
    if (res.body && (res.ok || res.headers.get("content-type")?.includes("event-stream"))) {
      return pipeSse(res);
    }
  } catch {
    /* fallback */
  }
  const lastText = [...opts.messages].reverse().find((m) => m.role === "user")?.content ?? "";
  return sseFromText(await fallbackText(lastText));
}

export async function hermesProxy(
  path: string,
  init: RequestInit,
  session: { userId: string; projectId: string; sessionId?: string },
) {
  try {
    const headers = new Headers(upstreamHeaders(session));
    if (init.method === "GET") headers.delete("Content-Type");
    const res = await fetch(`${HERMES_URL}${path}`, {
      ...init,
      headers,
      signal: AbortSignal.timeout(path.endsWith("/events") ? 30 * 60 * 1000 : 120000),
    });
    const ctype = res.headers.get("content-type") ?? "application/json";
    if (ctype.includes("event-stream") && res.body) return pipeSse(res);
    return new Response(await res.text(), {
      status: res.status,
      headers: { "Content-Type": ctype },
    });
  } catch (err) {
    return Response.json(
      { ok: false, error: err instanceof Error ? err.message : "hermes down" },
      { status: 502 },
    );
  }
}

function userLive(userId: string) {
  let set = liveRuns.get(userId);
  if (!set) {
    set = new Set();
    liveRuns.set(userId, set);
  }
  return set;
}

const SPEND_FILE = "/workspace/hermes/spend.json";
const DAILY_RUN_CAP = Number(process.env.HERMES_DAILY_RUN_CAP || 80);

export function spendStatus() {
  const day = new Date().toISOString().slice(0, 10);
  let n = 0;
  try {
    if (existsSync(SPEND_FILE)) {
      const row = JSON.parse(readFileSync(SPEND_FILE, "utf8")) as { day?: string; n?: number };
      if (row.day === day) n = Number(row.n) || 0;
    }
  } catch {
    n = 0;
  }
  return { day, n, cap: DAILY_RUN_CAP };
}

export function assertSpend() {
  const s = spendStatus();
  if (s.n >= s.cap) {
    return Response.json(
      { error: "daily cap", ...s },
      { status: 429 },
    );
  }
  mkdirSync("/workspace/hermes", { recursive: true });
  writeFileSync(SPEND_FILE, JSON.stringify({ day: s.day, n: s.n + 1 }));
  return null;
}

export function releaseRun(userId: string, runId: string) {
  userLive(userId).delete(runId);
}

function ensureWorktree(lane?: string) {
  if (lane !== "code") return "";
  const root = "/workspace/console-ide";
  const base = "/workspace/hermes/worktrees";
  mkdirSync(base, { recursive: true });
  const dest = join(base, `run-${Date.now()}`);
  try {
    if (existsSync(join(root, ".git"))) {
      execFileSync("git", ["-C", root, "worktree", "add", "--detach", dest], {
        timeout: 20000,
        stdio: "ignore",
      });
      return dest;
    }
  } catch {
    /* fall through */
  }
  mkdirSync(dest, { recursive: true });
  return dest;
}


function seedFlashPrompt(sessionId: string) {
  const nl = String.fromCharCode(10);
  const prompt = [
    "Model: deepseek-ai/DeepSeek-V4.1-Flash",
    "Provider: river",
    "Endpoint: https://api.river.ai/v1",
    "Platform: api_server",
    "User home directory: /root",
    "Current working directory: /workspace",
    "",
    "You are the desk. Answer the new line. The board is the past.",
  ].join(nl);
  const code = [
    "import hashlib, sqlite3, sys",
    "sid, prompt = sys.argv[1], sys.argv[2]",
    "c = sqlite3.connect('/workspace/hermes/state.db')",
    "row = c.execute('select system_prompt_hash from sessions where id=?', (sid,)).fetchone()",
    "if row and row[0]:",
    "    raise SystemExit(0)",
    "h = hashlib.sha256(prompt.encode()).hexdigest()",
    "c.execute('INSERT OR IGNORE INTO system_prompts (hash, prompt) VALUES (?, ?)', (h, prompt))",
    "if row:",
    "    c.execute('update sessions set system_prompt_hash=?, model=?, cwd=? where id=? and system_prompt_hash is null', (h, 'deepseek-ai/DeepSeek-V4.1-Flash', '/workspace', sid))",
    "else:",
    "    c.execute(\"insert into sessions (id, source, model, system_prompt_hash, message_count, cwd, started_at, last_activity_at, billing_provider) values (?, 'api_server', 'deepseek-ai/DeepSeek-V4.1-Flash', ?, 0, '/workspace', datetime('now'), datetime('now'), 'river')\", (sid, h))",
    "c.commit()",
  ].join(nl);
  try {
    execFileSync("python3", ["-c", code, sessionId, prompt], { timeout: 4000 });
  } catch {
    return;
  }
}


const FLASH_LANE = "deepseek-ai/DeepSeek-V4.1-Flash";

function survivalCash(): number | null {
  try {
    const row = JSON.parse(readFileSync("/workspace/galion-light/state/survival.json", "utf8")) as {
      openrouter?: { cash_usd?: number };
    };
    const n = Number(row?.openrouter?.cash_usd);
    return Number.isFinite(n) ? n : null;
  } catch {
    return null;
  }
}

function laneModel(requested: string): string {
  return modelForLane();
}

function boardHistory() {
  const live = readLive();
  if (!live.fresh || !live.scope) return [];
  const nl = String.fromCharCode(10);
  return [
    { role: "user", content: "Live measurement only. The conversation stands." + nl + live.scope },
    { role: "assistant", content: "Measurement read. The conversation stands." },
  ];
}

export function readLive() {
  const control = readControl();
  const maxMin = Number(control.live_max_minutes ?? 20);
  let scope = "";
  let when = "";
  try {
    const raw = readFileSync("/workspace/galion-light/state/eval/survival.txt", "utf8").trim();
    const line = raw.split(String.fromCharCode(10))[0] ?? "";
    if (line.startsWith("SCOPE ")) {
      scope = line;
      when = line.match(/when=([0-9T:Z-]+)/)?.[1] ?? "";
    }
  } catch {
    scope = "";
  }
  const stamp = when ? Date.parse(when) : NaN;
  const ageMin = Number.isNaN(stamp) ? null : (Date.now() - stamp) / 60000;
  const fresh = ageMin != null && ageMin >= 0 && ageMin <= (Number.isFinite(maxMin) ? maxMin : 20);
  return { control, scope: fresh ? scope : "", when, ageMin, fresh };
}

const SETTING_FILES = {
  control: { path: "/workspace/hermes/control.json", json: true },
  soul: { path: "/root/.hermes/SOUL.md", json: false },
  skill: { path: "/root/.hermes/skills/galion/local-primary/SKILL.md", json: false },
  modes: { path: "/workspace/hermes/modes.json", json: true },
} as const;

export function readSettings() {
  const live = readLive();
  let snap: Record<string, unknown> = {};
  try {
    snap = JSON.parse(readFileSync("/workspace/galion-light/state/survival.json", "utf8"));
  } catch {
    snap = {};
  }
  const files: Record<string, string> = {};
  for (const [name, row] of Object.entries(SETTING_FILES)) {
    try {
      files[name] = readFileSync(row.path, "utf8");
    } catch {
      files[name] = "";
    }
  }
  return {
    live,
    openrouter: snap.openrouter ?? null,
    gpu: snap.gpu ?? null,
    cpu: snap.cpu ?? null,
    disk: snap.disk ?? null,
    runpod: snap.runpod ?? null,
    revolut: snap.revolut ?? null,
    lane: snap.lane ?? null,
    when: typeof snap.when === "string" ? snap.when : "",
    files,
  };
}

export function writeSetting(file: string, text: string) {
  const row = SETTING_FILES[file as keyof typeof SETTING_FILES];
  if (!row) return { ok: false, error: "unknown file" };
  if (text.length > 200000) return { ok: false, error: "too large" };
  if (row.json) {
    try {
      JSON.parse(text);
    } catch {
      return { ok: false, error: "invalid json" };
    }
  }
  writeFileSync(row.path, text.endsWith("\n") ? text : text + "\n");
  return { ok: true, file };
}


const PROVIDER_MODELS = [
  { provider: "laptop", model: "huihui_ai/qwen3.5-abliterated:9b" },
  { provider: "openrouter", model: "anthropic/claude-opus-5.5" },
  { provider: "openrouter", model: "anthropic/claude-sonnet-5.5" },
  { provider: "river", model: "deepseek-ai/DeepSeek-V4.1-Flash" },
  { provider: "river", model: "zai-org/GLM-5.3-Flash" },
  { provider: "river", model: "river-flash" },
  { provider: "river", model: "river-glm" },
] as const;

function resolveProvider(model?: string, provider?: string) {
  const named = PROVIDER_MODELS.find((row) => row.provider === provider && row.model === model);
  if (named) return named;
  const safe = typeof model === "string" && /^[A-Za-z0-9_.:~+-]{1,120}\/[A-Za-z0-9_.:~+-]{1,160}$/.test(model);
  if (provider === "laptop") return PROVIDER_MODELS[0];
  if ((provider === "openrouter" || provider === "river") && safe) return { provider, model };
  return PROVIDER_MODELS.find((row) => row.model === model) ?? { provider: "openrouter", model: "anthropic/claude-opus-5.5" };
}

function releaseSessionModel(sessionId: string, model: string, provider: string) {
  if (sessionId.split("-").length < 5) return;
  const script = [
    "import sqlite3, sys",
    "sid, model, provider = sys.argv[1], sys.argv[2], sys.argv[3]",
    "con = sqlite3.connect('/opt/galion/hermes/state.db', timeout=3)",
    "row = con.execute('select model from sessions where id=?', (sid,)).fetchone()",
    "stored = row[0] if row else None",
    "if provider == 'laptop' and stored == model:",
    "    raise SystemExit(0)",
    "con.execute('update sessions set model=NULL where id=?', (sid,))",
    "con.commit()",
  ].join("\n")
  try {
    execFileSync("python3", ["-c", script, sessionId, model, provider], { timeout: 4000 });
  } catch {
    /* the standing session model stays; the run still starts */
  }
}

export async function startHermesRun(opts: {
  userId: string;
  projectId: string;
  sessionId?: string;
  input: string;
  lane?: string;
  skillHint?: string;
  model?: string;
  provider?: string;
  mode?: string;
}) {
  const live = userLive(opts.userId);
  if (live.size >= MAX_RUNS_PER_USER) {
    return Response.json(
      { error: "too many concurrent runs", max: MAX_RUNS_PER_USER },
      { status: 429 },
    );
  }
  const blocked = assertSpend();
  if (blocked) return blocked;
  ensureWorktree(opts.lane);
  const input = opts.input;
  const picked = resolveProvider(opts.model, opts.provider);
  if (opts.sessionId) releaseSessionModel(opts.sessionId, picked.model, picked.provider);
  const res = await hermesProxy(
    "/v1/runs",
    {
      method: "POST",
      body: JSON.stringify({
        input,
        instructions:
          "You are Hermes Research, a colleague, not a parent. Answer the machine the user asked about. A question about H200s or a future fleet is not a question about the laptop or this VPS. The laptop is offline. Do not describe it. Do not say the goal is not possible. Give the path that makes it real. Open with that path, in plain sentences, no headers and no lecture. Search first and use only figures a tool returned. Do not assign a task. Do not recite memory or file paths.",
        model: picked.model,
        provider: picked.provider,
        ...(opts.sessionId
          ? { session_id: opts.sessionId }
          : { conversation_history: boardHistory() }),
        model_options: { reasoning: { enabled: true, effort: "high" } },
      }),
    },
    opts,
  );
  const text = await res.text();
  let json: { run_id?: string; runId?: string; status?: string } = {};
  try {
    json = JSON.parse(text) as typeof json;
  } catch {
    return new Response(text, { status: res.status, headers: { "Content-Type": "application/json" } });
  }
  const runId = json.run_id ?? json.runId;
  if (runId) {
    live.add(runId);
    writeActivity({ kind: "run", text: opts.input.slice(0, 160) });
  }
  return Response.json(
    { runId, run_id: runId, status: json.status ?? "started" },
    { status: res.ok || res.status === 202 ? 202 : res.status },
  );
}

export async function resolveApproval(
  runId: string,
  body: { approve?: boolean; choice?: string; request_id?: string; sessionId?: string; projectId?: string },
  userId: string,
) {
  const choice = body.choice ?? (body.approve === false ? "deny" : "once");
  return hermesProxy(
    `/v1/runs/${runId}/approval`,
    { method: "POST", body: JSON.stringify({ choice, request_id: body.request_id }) },
    {
      userId,
      projectId: body.projectId || "galion-light",
      sessionId: body.sessionId,
    },
  );
}

export async function transcribeAudio(blob: Blob) {
  const local = process.env.STT_URL || "http://127.0.0.1:8643";
  const form = new FormData();
  form.append("file", blob, "speech.webm");
  form.append("language", "en");
  const res = await fetch(`${local}/v1/audio/transcriptions`, {
    method: "POST",
    body: form,
    signal: AbortSignal.timeout(60000),
  });
  if (!res.ok) {
    throw new Error(`local stt ${res.status}`);
  }
  const json = (await res.json()) as { text?: string };
  return { text: json.text ?? "", provider: "local" };
}

const ACTIVITY = "/workspace/hermes/activity.jsonl";

export type ActivityItem = {
  at: number;
  kind: string;
  text: string;
};

export function writeActivity(item: { kind: string; text: string; at?: number }) {
  const row: ActivityItem = {
    at: item.at ?? Date.now(),
    kind: item.kind.slice(0, 32),
    text: item.text.slice(0, 2000),
  };
  mkdirSync("/workspace/hermes", { recursive: true });
  appendFileSync(ACTIVITY, JSON.stringify(row) + "\n");
  return row;
}

export type LogLine = { at: string; level: string; text: string };

export function readHermesLog(limit = 40): LogLine[] {
  const path = "/workspace/galion-light/logs/hermes.log";
  if (!existsSync(path)) return [];
  const lines = readFileSync(path, "utf8").split("\n").filter(Boolean);
  const out: LogLine[] = [];
  let wal = 0;
  for (const line of lines.slice(-400)) {
    if (line.includes("WAL-reset corruption")) {
      wal += 1;
      continue;
    }
    const m = line.match(/^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}),\d+ (\w+) ([^:]+): (.*)$/);
    if (!m) continue;
    const text = m[4].replace(/\s+/g, " ").slice(0, 280);
    if (text.startsWith("Shutdown context") || text.includes("platform '")) {
      out.push({ at: m[1].slice(11, 16), level: m[2].toLowerCase(), text: text.slice(0, 180) });
      continue;
    }
    out.push({ at: m[1].slice(11, 16), level: m[2].toLowerCase(), text });
  }
  if (wal) {
    out.unshift({
      at: "boot",
      level: "warning",
      text: `SQLite 3.37.2 WAL bug ×${wal}. Hermes is on journal_mode=DELETE. Not a wipe.`,
    });
  }
  return out.slice(-limit);
}

export function readActivity(hours = 24) {
  const since = Date.now() - hours * 3600 * 1000;
  if (!existsSync(ACTIVITY)) return [];
  const out: ActivityItem[] = [];
  try {
    for (const line of readFileSync(ACTIVITY, "utf8").split("\n")) {
      if (!line.trim()) continue;
      const row = JSON.parse(line) as ActivityItem;
      if (row.at >= since) out.push(row);
    }
  } catch {
    /* skip */
  }
  return out.slice(-200);
}


