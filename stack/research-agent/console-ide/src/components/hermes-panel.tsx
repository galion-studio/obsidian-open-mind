import { Link } from "@tanstack/react-router";
import { Icon } from "@/components/icon";
import { useEffect, useRef, useState, type FormEvent, type PointerEvent } from "react";
import { createPortal } from "react-dom";
import {
  Brain,
  FileText,
  Globe,
  LayoutGrid,
  MessagesSquare,
  Mic,
  Volume2,
  Paperclip,
  Pin,
  Plus,
  Search,
  Send,
  Square,
  Terminal,
  Wrench,
  X,
} from "lucide-react";
import { StatusDot } from "@/components/status-dot";
import { CopyButton } from "@/components/copy-button";
import { cn } from "@/lib/cn";
import { parseLoop } from "@/lib/loop-parse";

type Mode = "plan" | "action";
type ToolState = "queued" | "running" | "done" | "error" | "needs approval";
type Item =
  | { kind: "user"; text: string }
  | { kind: "assistant"; text: string }
  | { kind: "think"; text: string }
  | { kind: "context"; text: string }
  | { kind: "tool"; name: string; status: ToolState; command?: string; output?: string; startedAt?: number }
  | { kind: "status"; text: string };

type Hold = { runId: string; tool: string; payload: string; requestId?: string };
type Skill = { id: string; name: string; group?: string; description: string };
type ToolRow = { id: string; kind: string; status: string };
type Thread = { id: string; title: string; busy?: boolean; at?: number; count?: number; pinned?: boolean; source?: string; tokens?: Tokens };
type Tokens = { input: number; output: number; total: number };
type AgentRow = { id: string; kind: string; task: string; status: string; tokens: number };
type Book = { items: Item[]; runId: string | null; busy: boolean; tokens: Tokens; agents: AgentRow[] };

const EMPTY_TOKENS: Tokens = { input: 0, output: 0, total: 0 };

function readTokens(raw: unknown): Tokens | null {
  if (!raw || typeof raw !== "object") return null;
  const row = raw as Record<string, unknown>;
  const input = Number(row.input ?? row.input_tokens ?? row.prompt_tokens);
  const output = Number(row.output ?? row.output_tokens ?? row.completion_tokens);
  const explicit = Number(row.total ?? row.total_tokens);
  const inn = Number.isFinite(input) ? Math.max(0, input) : 0;
  const out = Number.isFinite(output) ? Math.max(0, output) : 0;
  const total = Number.isFinite(explicit) && explicit > 0 ? explicit : inn + out;
  if (inn <= 0 && out <= 0 && total <= 0) return null;
  return { input: inn, output: out, total };
}

function emptyBook(): Book {
  return { items: [], runId: null, busy: false, tokens: { ...EMPTY_TOKENS }, agents: [] };
}

function clock(sec: number) {
  const s = Math.max(0, Math.floor(sec));
  const m = Math.floor(s / 60);
  const r = s % 60;
  if (m < 1) return `${r}s`;
  return `${m}m ${String(r).padStart(2, "0")}s`;
}

const PROJECT_ID = "galion-light";
function isDangerous(text: string) {
  return /\b(rm\s+-[a-zA-Z]*[rf]|mkfs\b|dd\s+|\bDROP\s+TABLE\b|wipefs|shutdown|reboot|git\s+push\b[^;\n]*--force|curl\s+[^|\n]*\|\s*(ba|z)?sh|chmod\s+777\s+\/|delete_repo)\b/i.test(text);
}
type LoopJob = { id: string; sessionId: string; everySec: number; until: number; stopped: boolean; paused?: boolean; input: string };

function MemoryStrip({
  tokens,
  sessionId,
  loops,
  agents,
  items,
}: {
  tokens: Tokens;
  sessionId: string;
  loops: LoopJob[];
  agents: AgentRow[];
  items: Item[];
}) {
  const tok = tokens.total || tokens.input + tokens.output;
  const live = loops.filter((job) => !job.stopped);
  return (
    <div className="shrink-0 border-b border-line">
      <div className="grid grid-cols-3">
        <p className="truncate px-3 py-1.5 text-[12px] text-muted">
          Tokens <span className="text-fg">{tok}</span>
        </p>
        <p className="truncate px-3 py-1.5 text-[12px] text-muted">
          Session <span className="text-fg">{sessionId ? sessionId.slice(0, 8) : "—"}</span>
        </p>
        <p className="truncate px-3 py-1.5 text-[12px] text-muted">
          Loops <span className="text-fg">{live.length}</span>
          <span> · </span>
          <span className="text-fg">{agents.length}</span>
        </p>
      </div>
    </div>
  );
}

type Alpha = { state: string; run?: string; task?: string; started?: number; ready?: number; finished?: number; stopped?: number; gpu_delta_mib?: number | null; model?: string };
type Scope = { cash: number | null; lane: string; receipt: boolean; when?: string; pulse?: string; warn?: string };
type SettingFile = "control" | "soul" | "skill" | "modes";
type SettingsView = {
  live?: { control?: Record<string, unknown>; scope?: string; fresh?: boolean; when?: string };
  openrouter?: { cash_usd?: number; total_credits?: number; total_usage?: number; key_limit?: number; key_left?: number; key_usage?: number; measured?: boolean } | null;
  gpu?: { name?: string; free_mib?: number; used_mib?: number; util_pct?: number } | null;
  cpu?: { load?: number; processors?: number } | null;
  disk?: { volume_used_pct?: number } | null;
  runpod?: { measured?: boolean; reason?: string } | null;
  revolut?: { measured?: boolean; reason?: string } | null;
  lane?: { action?: string; receipt?: boolean } | null;
  when?: string;
  files?: Partial<Record<SettingFile, string>>;
};
type SavedModel = { id: string; name: string; model: string; note: string };
const MODE_LABEL: Record<Mode, string> = { plan: "Plan", action: "Action" };
const PROVIDER_CHOICES = [
  { id: "laptop", label: "Laptop", model: "huihui_ai/qwen3.5-abliterated:9b" },
  { id: "openrouter", label: "OpenRouter", model: "google/gemini-2.5-flash" },
  { id: "river", label: "River", model: "deepseek-ai/DeepSeek-V4.1-Flash" },
] as const;

const CAT_ORDER = ["text", "image", "video", "audio", "speech", "transcription", "embeddings", "rerank", "decisions", "programming", "roleplay", "marketing", "marketing/seo", "technology", "science", "translation", "legal", "finance", "health", "trivia", "academia"];
const CAT_LABEL: Record<string, string> = {
  all: "All", text: "Text", image: "Image", video: "Video", audio: "Audio", speech: "Speech", transcription: "Transcript", embeddings: "Embeddings", rerank: "Rerank", decisions: "Decisions", programming: "Programming", roleplay: "Roleplay", marketing: "Marketing", "marketing/seo": "SEO", technology: "Technology", science: "Science", translation: "Translation", legal: "Legal", finance: "Finance", health: "Health", trivia: "Trivia", academia: "Academia",
};

function providerOf(model: string) {
  if (model.startsWith("google/")) return "openrouter";
  if (model.startsWith("deepseek-ai/") || model.startsWith("zai-org/") || model.startsWith("river")) return "river";
  return "laptop";
}

function md(text: string) {
  const safe = text.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  return safe
    .replace(/```(\w*)\n?([\s\S]*?)```/g, "<pre data-code=\"1\">\$2</pre>")
    .replace(/`([^`]+)`/g, "<code>\$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>\$1</strong>")
    .replace(/\n/g, "<br/>");
}

const LEAK_USER_LINES = new Set([
  "Do the next step in this same chat.",
  "Do what was asked. Reply with the result only.",
]);

const LEAK_DIVS = "persona-operator|request-inbox|read-long|self-eval|social-reply|stack-map|tulu";

function displayTurn(kind: "user" | "assistant", text: string) {
  if (kind === "user") {
    let out = text.replace(/\s+$/g, "");
    let cut = false;
    for (;;) {
      const lines = out.split("\n");
      const last = (lines[lines.length - 1] ?? "").trim();
      if (!LEAK_USER_LINES.has(last)) break;
      lines.pop();
      out = lines.join("\n").replace(/\s+$/g, "");
      cut = true;
    }
    return cut ? out : text;
  }
  let out = text;
  const open = "<memory-context>";
  const close = "</memory-context>";
  for (;;) {
    const start = out.indexOf(open);
    if (start < 0) break;
    const end = out.indexOf(close, start + open.length);
    if (end < 0) {
      out = out.slice(0, start);
      break;
    }
    out = out.slice(0, start) + out.slice(end + close.length);
  }
  out = out.replace(new RegExp(`<div\\s+class=["'](?:${LEAK_DIVS})["']\\s*>[\\s\\S]*?<\\/div>`, "gi"), "");
  if (out === text) return text;
  return out.replace(/\n{3,}/g, "\n\n").trim();
}

async function readSse(res: Response, onEvent: (ev: Record<string, unknown>) => void) {
  const reader = res.body?.getReader();
  if (!reader) return;
  const decoder = new TextDecoder();
  let buf = "";
  let eventName = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    const parts = buf.split("\n");
    buf = parts.pop() ?? "";
    for (const line of parts) {
      const trimmed = line.trim();
      if (!trimmed) {
        eventName = "";
        continue;
      }
      if (trimmed.startsWith("event:")) {
        eventName = trimmed.slice(6).trim();
        continue;
      }
      if (!trimmed.startsWith("data:")) continue;
      const data = trimmed.slice(5).trim();
      if (!data || data === "[DONE]") continue;
      try {
        const obj = JSON.parse(data) as Record<string, unknown>;
        if (eventName && obj.event == null) obj.event = eventName;
        onEvent(obj);
      } catch {
        onEvent({ event: eventName || "text", delta: data });
      }
    }
  }
}

function ago(at?: number | string) {
  if (at == null || at === "") return "";
  let n = typeof at === "number" ? at : Number(at);
  if (!Number.isFinite(n)) {
    const parsed = Date.parse(String(at));
    if (!Number.isFinite(parsed)) return "";
    n = parsed;
  }
  if (n > 0 && n < 1e12) n *= 1000;
  const delta = (Date.now() - n) / 1000;
  if (!Number.isFinite(delta) || delta < 0) return "";
  if (delta < 60) return "now";
  if (delta < 3600) return `${Math.floor(delta / 60)}m`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h`;
  return `${Math.floor(delta / 86400)}d`;
}
function loadThreads(): Thread[] {
  try {
    return JSON.parse(localStorage.getItem("galion-hermes-threads") ?? "[]") as Thread[];
  } catch {
    return [];
  }
}

function stepIcon(name: string) {
  const n = name.toLowerCase();
  if (n.includes("terminal") || n.includes("shell")) return Terminal;
  if (n.includes("search") || n.includes("grep") || n.includes("find")) return Search;
  if (n.includes("browser") || n.includes("web") || n.includes("http")) return Globe;
  if (n.includes("file") || n.includes("read") || n.includes("write") || n.includes("patch")) return FileText;
  if (n.includes("think")) return Brain;
  return Wrench;
}

function Roadmap({
  steps,
  onOpen,
}: {
  steps: { item: Item; index: number }[];
  onOpen: (index: number) => void;
}) {
  return (
    <ol className="relative ml-2 border-l border-line py-1 pl-4" aria-label="Roadmap">
      {steps.map(({ item, index }, order) => {
        const name = item.kind === "tool" ? item.name : item.kind === "think" ? "thinking" : "note";
        const Icon = stepIcon(name);
        const live = item.kind === "tool" && (item.status === "running" || item.status === "queued");
        const label =
          item.kind === "tool"
            ? (() => {
                const lines = splitCommands(item.command || "");
                if (lines.length > 1) return `${lines.length} commands`;
                return lines[0] || item.name;
              })()
            : item.kind === "think"
              ? "Thinking"
              : item.text;
        return (
          <li key={index} className={cn("relative", order === steps.length - 1 ? "" : "pb-3")}>
            <span
              className={cn(
                "absolute -left-[23px] top-0.5 flex size-5 items-center justify-center rounded-full border bg-bg",
                live ? "border-accent text-accent" : "border-line text-muted",
              )}
            >
              <Icon className="size-3" strokeWidth={1.7} />
            </span>
            {item.kind === "tool" ? (
              <button type="button" className="block w-full text-left" onClick={() => onOpen(index)}>
                <span className="font-mono text-[11px] uppercase tracking-wider text-subtle">{item.name}</span>
                <span className="mt-0.5 block truncate text-[13px] leading-5 text-fg">{label}</span>
                <span className={cn("font-mono text-[11px]", live ? "text-accent" : "text-muted")}>{item.status}</span>
              </button>
            ) : item.kind === "think" ? (
              <details className="research-think rounded-md px-3 py-1.5">
                <summary className="cursor-pointer text-[13px] text-muted">Thinking</summary>
                <pre className="mt-1 whitespace-pre-wrap font-sans text-[14px] leading-relaxed text-fg">
                  {item.text}
                </pre>
              </details>
            ) : (
              <p className="text-[13px] leading-5 text-muted">{item.kind === "status" ? item.text : label}</p>
            )}
          </li>
        );
      })}
    </ol>
  );
}
function orderTurns(items: Item[]): Item[] {
  const out: Item[] = [];
  for (const item of items) {
    if (item.kind !== "think") {
      out.push(item);
      continue;
    }
    const lastUser = (() => {
      for (let i = out.length - 1; i >= 0; i -= 1) if (out[i].kind === "user" || out[i].kind === "context") return i;
      return -1;
    })();
    const lastThink = (() => {
      for (let i = out.length - 1; i >= 0; i -= 1) if (out[i].kind === "think") return i;
      return -1;
    })();
    if (lastThink > lastUser) {
      const prev = out[lastThink];
      if (prev.kind === "think") out[lastThink] = { kind: "think", text: `${prev.text}\n${item.text}` };
      continue;
    }
    out.splice(Math.max(lastUser + 1, 0), 0, item);
  }
  return out;
}

function chatBlocks(items: Item[]) {
  const out: ({ kind: "road"; steps: { item: Item; index: number }[] } | { kind: "one"; item: Item; index: number })[] = [];
  let road: { item: Item; index: number }[] = [];
  const flush = () => {
    if (!road.length) return;
    out.push({ kind: "road", steps: road });
    road = [];
  };
  items.forEach((item, index) => {
    if (item.kind === "tool" || item.kind === "think" || item.kind === "status") road.push({ item, index });
    else {
      flush();
      out.push({ kind: "one", item, index });
    }
  });
  flush();
  return out;
}

function splitCommands(raw: string): string[] {
  const text = raw.trim();
  if (!text) return [];
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
  if (lines.length > 1) return lines;
  const parts = text.split(/\s*(?:&&|;)\s*/).map((part) => part.trim()).filter(Boolean);
  return parts.length ? parts : [text];
}

function ToolRun({ item }: { item: Extract<Item, { kind: "tool" }> }) {
  const scroller = useRef<HTMLOListElement>(null);
  const [now, setNow] = useState(() => Date.now());
  const live = item.status === "running" || item.status === "queued";
  const steps = splitCommands(item.command || "");
  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => setNow(Date.now()), 400);
    return () => window.clearInterval(id);
  }, [live]);
  const follow = useRef(true);
  useEffect(() => {
    const node = scroller.current;
    if (!node || !follow.current) return;
    node.scrollTop = node.scrollHeight;
  }, [item.output, item.command, item.status]);
  const elapsed = item.startedAt ? Math.max(0, Math.floor((now - item.startedAt) / 1000)) : 0;
  return (
    <div className="mt-1 overflow-hidden rounded-[16px] border border-line bg-bg">
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <span className={cn("size-2 shrink-0 rounded-full", live ? "animate-pulse bg-accent" : item.status === "error" ? "bg-offline" : "bg-muted")} />
        <p className="min-w-0 flex-1 truncate font-mono text-[12px] uppercase tracking-wider text-muted">{item.name}</p>
        <p className="shrink-0 font-mono text-[11px] tabular-nums text-subtle">{steps.length > 1 ? `${steps.length}` : ""} {live ? `${elapsed}s` : item.status}</p>
      </div>
      <ol
        ref={scroller}
        onScroll={(e) => {
          const node = e.currentTarget;
          follow.current = node.scrollHeight - node.scrollTop - node.clientHeight < 48;
        }}
        className="max-h-[42dvh] overflow-y-auto overscroll-y-contain"
      >
        {(steps.length ? steps : ["waiting"]).map((step, index) => (
          <li key={`${index}-${step.slice(0, 24)}`} className="flex gap-3 border-b border-line px-3 py-2.5">
            <span className="w-5 shrink-0 pt-0.5 font-mono text-[11px] tabular-nums text-subtle">{index + 1}</span>
            <p className="min-w-0 flex-1 whitespace-pre-wrap break-words font-mono text-[13px] leading-5 text-[#d6d3cc]">{step}</p>
          </li>
        ))}
      </ol>
      {item.output ? (
        <pre className="max-h-[22dvh] overflow-auto whitespace-pre-wrap break-words border-t border-line px-3 py-2 font-mono text-[12px] leading-5 text-muted">{item.output}</pre>
      ) : live ? (
        <p className="border-t border-line px-3 py-2 font-mono text-[12px] text-accent">▍</p>
      ) : null}
    </div>
  );
}

export function HermesPanel() {
  const [health, setHealth] = useState<{ ok?: boolean } | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [runId, setRunId] = useState<string | null>(null);
  const [hold, setHold] = useState<Hold | null>(null);
  const [mode, setMode] = useState<Mode>(() => {
    if (typeof window === "undefined") return "action";
    const v = localStorage.getItem("galion-hermes-mode");
    if (v === "action" || v === "execution") return "action";
    if (v === "plan" || v === "planning" || v === "thinking") return "plan";
    return "action";
  });
  const [modes, setModes] = useState<Record<string, string>>({});
  const [savedModels, setSavedModels] = useState<SavedModel[]>([]);
  const [catalog, setCatalog] = useState<{ id: string; name: string }[]>([]);
  const [modelBox, setModelBox] = useState<"" | "laptop" | "openrouter" | "river">("");
  const [boxRows, setBoxRows] = useState<{ id: string; name: string; modality?: string; uses?: string[] }[]>([]);
  const [boxQ, setBoxQ] = useState("");
  const [boxCat, setBoxCat] = useState("all");
  const [stars, setStars] = useState<{ laptop: string[]; openrouter: string[]; river: string[] }>({ laptop: [], openrouter: [], river: [] });
  const [providerNow, setProviderNow] = useState<"laptop" | "openrouter" | "river">("river");
  const [gateway, setGateway] = useState("");
  const [modelQuery, setModelQuery] = useState("");
  const [modelFor, setModelFor] = useState<Mode>("action");
  const [draftName, setDraftName] = useState("");
  const [draftModel, setDraftModel] = useState("");
  const [draftNote, setDraftNote] = useState<Mode>("plan");
  const [activity, setActivity] = useState<{ at: number; kind: string; text: string }[]>([]);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [settingFile, setSettingFile] = useState<SettingFile>("soul");
  const [settingText, setSettingText] = useState("");
  const [settingMsg, setSettingMsg] = useState("");
  const [toolRows, setToolRows] = useState<ToolRow[]>([]);
  const [drawer, setDrawer] = useState(false);
  const [sheet, setSheet] = useState<
    "plus" | "skills" | "models" | "tools" | "artifact" | "tool" | "activity" | "live" | "slash" | "hub" | "data" | null
  >(null);
  const [slash, setSlash] = useState<{ cmd: string; hint: string }[]>([]);
  const [artifact, setArtifact] = useState("");
  const [toolAt, setToolAt] = useState<number | null>(null);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [enabled, setEnabled] = useState<Record<string, boolean>>({});
  const [pin, setPin] = useState<string | null>(null);
  const [rec, setRec] = useState(false);
  const [voiceOn, setVoiceOn] = useState(true);
  const [speaking, setSpeaking] = useState(false);
  const [speakingAt, setSpeakingAt] = useState<number | null>(null);
  const voiceRef = useRef(true);
  const voiceSrc = useRef<AudioBufferSourceNode | null>(null);
  const speakAt = useRef<number | null>(null);
  const earRef = useRef<AudioContext | null>(null);
  voiceRef.current = voiceOn;
  const [recMs, setRecMs] = useState(0);
  const [guard, setGuard] = useState(true);
  const [theme, setTheme] = useState<"light" | "dark">("dark");
  const [accept, setAccept] = useState(false);
  const acceptRef = useRef(false);
  const runRef = useRef("");
  acceptRef.current = accept;
  const [threads, setThreads] = useState<Thread[]>([]);
  const [dbs, setDbs] = useState<{ file: string; bytes: number; error: string; tables: { name: string; rows: number }[] }[]>([]);
  const [files, setFiles] = useState<{ id: string; name: string; kind: "image" | "pdf" | "text" | "other"; size: number }[]>([]);
  const [unread, setUnread] = useState<Record<string, boolean>>({});
  const [chatQuery, setChatQuery] = useState("");
  const [sessionId, setSessionId] = useState("");
  const [tokens, setTokens] = useState<Tokens>(EMPTY_TOKENS);
  const [agents, setAgents] = useState<AgentRow[]>([]);
  const [loops, setLoops] = useState<LoopJob[]>([]);
  const [alpha, setAlpha] = useState<Alpha | null>(null);
  const [scope, setScope] = useState<Scope | null>(null);
  const book = useRef<Record<string, Book>>({});
  const sessionRef = useRef("");
  const providerModel = useRef("");
  const boxFrame = useRef<HTMLDivElement>(null);
  const providerName = useRef("");
  const skipTap = useRef(false);
  const holdTimer = useRef(0);
  const owner = useRef("");
  const transcriptRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(true);
  const [away, setAway] = useState(false);
  const [workFrom, setWorkFrom] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const lastRef = useRef("");
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const inThink = useRef(false);
  const [clips, setClips] = useState<File[]>([]);
  const recRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const holdRef = useRef(false);
  const recTick = useRef<number>(0);
  const skipTx = useRef(false);
  const draftRef = useRef("");
  const booted = useRef(false);

  useEffect(() => {
    const voice = localStorage.getItem("galion-life-voice");
    if (voice !== null) setVoiceOn(voice === "1");
  }, []);

  function armEar() {
    if (!earRef.current) earRef.current = new AudioContext();
    void earRef.current.resume();
  }

  async function playVoice(raw: string, at?: number) {
    const clean = raw.replace(/```[\s\S]*?```/g, " ").replace(/[#>*_`]/g, " ").replace(/\s+/g, " ").trim();
    const first = clean.split(/(?<=[.!?])\s/).slice(0, 2).join(" ").trim() || clean;
    const spoken = (first.length > 320 ? first.slice(0, 320).replace(/\s+\S*$/, "") : first).trim();
    if (spoken.length < 2) return;
    if (at != null && speakAt.current === at) {
      try { voiceSrc.current?.stop(); } catch { /* already ended */ }
      voiceSrc.current = null;
      speakAt.current = null;
      setSpeaking(false);
      setSpeakingAt(null);
      return;
    }
    try { voiceSrc.current?.stop(); } catch { /* already ended */ }
    setSpeaking(true);
    setSpeakingAt(at ?? null);
    speakAt.current = at ?? null;
    try {
      const res = await fetch("/api/hermes/speak", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: spoken, voice: "samantha" }),
      });
      if (!res.ok) { setSpeaking(false); setSpeakingAt(null); speakAt.current = null; return; }
      const ctx = earRef.current ?? new AudioContext();
      earRef.current = ctx;
      await ctx.resume();
      const audio = await ctx.decodeAudioData(await res.arrayBuffer());
      const src = ctx.createBufferSource();
      src.buffer = audio;
      src.connect(ctx.destination);
      voiceSrc.current = src;
      src.onended = () => {
        voiceSrc.current = null;
        speakAt.current = null;
        setSpeaking(false);
        setSpeakingAt(null);
      };
      src.start();
    } catch {
      setSpeaking(false);
      setSpeakingAt(null);
      speakAt.current = null;
    }
  }

  useEffect(() => {
    let dead = false;
    void (async () => {
      const rows = await refreshHistory();
      if (dead) return;
      const saved = localStorage.getItem("galion-hermes-session");
      const research = rows.filter((t) => t.id !== "galion-one");
      const pick = research.find((t) => t.id === saved)?.id ?? research[0]?.id;
      if (pick) await openHistory(pick);
    })();
    setTheme(localStorage.getItem("galion-theme") === "light" ? "light" : "dark");
    void fetch("/api/hermes/guard")
      .then((r) => r.json())
      .then((j: { on?: boolean; autoAccept?: boolean }) => {
        setGuard(j.on !== false);
        setAccept(j.autoAccept === true);
      })
      .catch(() => setGuard(true));
    return () => {
      dead = true;
    };
  }, []);

  useEffect(() => {
    function ping() {
      void fetch("/api/hermes/health")
        .then((r) => r.json())
        .then(setHealth)
        .catch(() => setHealth({ ok: false }));
    }
    ping();
    const t = setInterval(ping, 12000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    void fetch("/api/hermes/modes")
      .then((r) => r.json())
      .then((j: { modes?: Record<string, string>; saved?: SavedModel[]; stars?: { laptop?: string[]; openrouter?: string[]; river?: string[] }; provider?: string }) => {
        setModes(j.modes ?? {});
        setSavedModels(j.saved ?? []);
        setStars({ laptop: j.stars?.laptop ?? [], openrouter: j.stars?.openrouter ?? [], river: j.stars?.river ?? [] });
        const picked = j.provider === "openrouter" || j.provider === "river" || j.provider === "laptop" ? j.provider : providerOf(j.modes?.action || "");
        providerName.current = picked;
        providerModel.current = j.modes?.[picked === "laptop" ? "action" : "action"] || j.modes?.action || "";
        setProviderNow(picked);
        void fetch("/api/hermes/models")
          .then((r) => r.json())
          .then((c: { models?: { id: string; name: string }[]; gateway?: string }) => {
            setCatalog(c.models ?? []);
            setGateway(c.gateway ?? "");
          })
          .catch(() => undefined);
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    void fetch("/api/hermes/skills")
      .then((r) => r.json())
      .then((j: { items?: Skill[] }) => {
        const items = j.items ?? [];
        setSkills(items);
        setEnabled(Object.fromEntries(items.map((s) => [s.id, true])));
      })
      .catch(() => setSkills([]));
  }, []);

  useEffect(() => {
    if (!busy) {
      setWorkFrom(null);
      return;
    }
    setWorkFrom((t) => t ?? Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [busy]);

  useEffect(() => {
    const node = transcriptRef.current;
    if (!node) return;
    if (!followRef.current) {
      setAway(true);
      return;
    }
    node.scrollTop = node.scrollHeight;
  }, [items, busy]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        setSheet(null);
        setDrawer(false);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    void refreshHistory();
  }, []);

  useEffect(() => {
    if (drawer) void refreshHistory();
  }, [drawer]);

  useEffect(() => {
    const t = window.setInterval(() => {
      void refreshHistory();
    }, 8000);
    return () => window.clearInterval(t);
  }, []);

  function ensure(id: string) {
    if (!book.current[id]) book.current[id] = emptyBook();
    return book.current[id];
  }

  function show(id: string) {
    const row = ensure(id);
    if (sessionRef.current !== id) return;
    setItems(row.items.slice());
    setRunId(row.runId);
    setBusy(row.busy);
    setTokens({ ...row.tokens });
    setAgents(row.agents.slice());
  }

  function applyItems(sid: string, fn: (items: Item[]) => Item[]) {
    const row = ensure(sid);
    row.items = fn(row.items).slice(-500);
    show(sid);
  }

  function openThread(id: string) {
    sessionRef.current = id;
    owner.current = id;
    setSessionId(id);
    localStorage.setItem("galion-hermes-session", id);
    followRef.current = true;
    setAway(false);
    show(id);
    setDrawer(false);
  }

  async function refreshHistory() {
    const remote = await fetch("/api/hermes/history")
      .then((r) => r.json())
      .then((j: { items?: Thread[]; files?: { id: string; name: string; kind: "image" | "pdf" | "text" | "other"; size: number }[] }) => {
        if (j.files) setFiles(j.files);
        return j.items ?? [];
      })
      .catch(() => [] as Thread[]);
    const draft =
      draftRef.current && !remote.some((row) => row.id === draftRef.current)
        ? [{ id: draftRef.current, title: "New chat", at: Date.now() / 1000 }]
        : [];
    const next = [...draft, ...remote].slice(0, 500);
    const cur = sessionRef.current;
    if (cur && !next.some((row) => row.id === cur)) {
      next.unshift({ id: cur, title: "This chat", at: Date.now() / 1000, busy: Boolean(book.current[cur]?.busy) });
    }
    for (const [id, row] of Object.entries(book.current)) {
      if (!row.busy) continue;
      const at = next.findIndex((thread) => thread.id === id);
      if (at < 0) next.unshift({ id, title: "Running", at: Date.now() / 1000, busy: true });
      else next[at] = { ...next[at], busy: true };
    }
    try {
      const live = JSON.parse(localStorage.getItem("galion-hermes-live") ?? "null") as {
        sessionId?: string;
        runId?: string;
        title?: string;
      } | null;
      if (live?.sessionId && live.runId) {
        const held = ensure(live.sessionId);
        if (!held.runId) held.runId = live.runId;
        held.busy = true;
        const at = next.findIndex((thread) => thread.id === live.sessionId);
        if (at < 0) next.unshift({ id: live.sessionId, title: live.title || "Running", at: Date.now() / 1000, busy: true });
        else next[at] = { ...next[at], busy: true };
      }
    } catch {
      /* keep the server list */
    }
    let seen: Record<string, number> = {};
    try {
      seen = JSON.parse(localStorage.getItem("galion-hermes-seen") ?? "{}") as Record<string, number>;
    } catch {
      seen = {};
    }
    const lights: Record<string, boolean> = {};
    for (const row of remote) {
      const count = row.count ?? 0;
      if (!booted.current || seen[row.id] == null || row.id === sessionRef.current) seen[row.id] = count;
      else if (count > seen[row.id]) lights[row.id] = true;
    }
    booted.current = true;
    localStorage.setItem("galion-hermes-seen", JSON.stringify(seen));
    setUnread(lights);
    const active = sessionRef.current;
    const listed = readTokens(remote.find((row) => row.id === active)?.tokens);
    if (listed && active && !book.current[active]?.busy) {
      const row = ensure(active);
      const cur = row.tokens.total || row.tokens.input + row.tokens.output;
      if (listed.total !== cur) {
        row.tokens = listed;
        setTokens(listed);
      }
    }
    setThreads(next);
    return next;
  }

  function markSeen(id: string, count?: number) {
    let seen: Record<string, number> = {};
    try {
      seen = JSON.parse(localStorage.getItem("galion-hermes-seen") ?? "{}") as Record<string, number>;
    } catch {
      seen = {};
    }
    if (count != null) seen[id] = count;
    localStorage.setItem("galion-hermes-seen", JSON.stringify(seen));
    setUnread((cur) => {
      if (!cur[id]) return cur;
      const next = { ...cur };
      delete next[id];
      return next;
    });
  }

  async function togglePin(id: string, pinned: boolean) {
    const nextPinned = !pinned;
    setThreads((list) => list.map((row) => (row.id === id ? { ...row, pinned: nextPinned } : row)).sort((a, b) => Number(b.pinned) - Number(a.pinned) || (b.at ?? 0) - (a.at ?? 0)));
    await fetch("/api/hermes/history", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, pinned: nextPinned }),
    }).catch(() => undefined);
  }

  async function openHistory(id: string) {
    const count = threads.find((t) => t.id === id)?.count;
    markSeen(id, count);
    const known = readTokens(threads.find((t) => t.id === id)?.tokens);
    openThread(id);
    if (known && !ensure(id).busy) {
      ensure(id).tokens = known;
      show(id);
    }
    const loaded = (book.current[id]?.items.length ?? 0) > 0;
    const body = await fetch(`/api/hermes/history?id=${encodeURIComponent(id)}`)
      .then((r) => r.json())
      .then((j: { items?: Item[]; tokens?: Tokens }) => j)
      .catch(() => ({ items: [] as Item[] }));
    const row = ensure(id);
    if (!loaded) {
      row.items = body.items ?? [];
      if (sessionRef.current === id) setItems(row.items);
    }
    const tok = readTokens(body.tokens);
    if (tok && !row.busy) {
      row.tokens = tok;
      if (sessionRef.current === id) setTokens(tok);
    }
  }

  function push(item: Item) {
    applyItems(owner.current || sessionRef.current, (m) => [...m, item]);
  }

  function addThink(chunk: string) {
    if (!chunk) return;
    applyItems(owner.current || sessionRef.current, (m) => {
      const copy = [...m];
      const last = copy[copy.length - 1];
      if (last?.kind === "think") {
        copy[copy.length - 1] = { kind: "think", text: last.text + chunk };
        return copy;
      }
      if (last?.kind === "assistant") {
        const prev = copy[copy.length - 2];
        if (prev?.kind === "think") {
          copy[copy.length - 2] = { kind: "think", text: prev.text + chunk };
          return copy;
        }
        copy.splice(copy.length - 1, 0, { kind: "think", text: chunk });
        return copy;
      }
      return [...copy, { kind: "think", text: chunk }];
    });
  }

  function addDelta(chunk: string) {
    if (!chunk) return;
    const open = "<think>";
    const close = "</think>";
    let rest = chunk;
    while (rest) {
      if (inThink.current) {
        const end = rest.indexOf(close);
        if (end === -1) {
          addThink(rest);
          return;
        }
        addThink(rest.slice(0, end));
        rest = rest.slice(end + close.length);
        inThink.current = false;
        continue;
      }
      const start = rest.indexOf(open);
      if (start === -1) {
        appendAnswer(rest);
        return;
      }
      if (start > 0) appendAnswer(rest.slice(0, start));
      rest = rest.slice(start + open.length);
      inThink.current = true;
    }
  }

  function appendAnswer(chunk: string) {
    if (!chunk) return;
    applyItems(owner.current || sessionRef.current, (m) => {
      const copy = [...m];
      const last = copy[copy.length - 1];
      if (last?.kind === "assistant") {
        copy[copy.length - 1] = { kind: "assistant", text: last.text + chunk };
        return copy;
      }
      return [...copy, { kind: "assistant", text: chunk }];
    });
  }

  function onEvent(ev: Record<string, unknown>) {
    const event = String(ev.event ?? ev.type ?? "");
    const choice = (ev.choices as { delta?: { content?: string; reasoning?: string; reasoning_content?: string } }[] | undefined)?.[0]?.delta;
    const delta = (typeof ev.delta === "string" && ev.delta) || choice?.content || "";
    const reason =
      (typeof ev.reasoning === "string" && ev.reasoning) ||
      choice?.reasoning_content ||
      choice?.reasoning ||
      "";
    const toolName = String(ev.label ?? ev.tool_name ?? ev.tool ?? ev.name ?? "");
    if (toolName === "_thinking" || event === "reasoning.available" || event.startsWith("reasoning")) {
      const bit = String(ev.preview ?? ev.text ?? reason ?? delta ?? "");
      if (bit) addThink(bit);
      return;
    }
    if (reason) addThink(reason);
    if (delta) addDelta(delta);
    if (event === "message.interim" && typeof ev.text === "string" && ev.already_streamed !== true && ev.text !== delta) addDelta(ev.text);
    if (event.startsWith("tool.") || event.includes("tool") || event.startsWith("hermes.tool")) {
      const name = String(ev.label ?? ev.tool_name ?? ev.tool ?? ev.name ?? "tool");
      const done = event.includes("complete") || event.includes("done");
      const status: ToolState = event.includes("fail") || ev.error
        ? "error"
        : done
          ? "done"
          : event.includes("queued")
            ? "queued"
            : "running";
      const preview = String(ev.preview ?? ev.path ?? "").slice(0, 4000);
      const full = String(ev.command ?? "");
      const command = (full || preview).slice(0, 8000);
      applyItems(owner.current || sessionRef.current, (m) => {
        const copy = [...m];
        let at = -1;
        for (let i = copy.length - 1; i >= 0; i -= 1) {
          const row = copy[i];
          if (row?.kind === "tool" && row.name === name && row.status !== "done" && row.status !== "error") {
            at = i;
            break;
          }
        }
        if (at >= 0) {
          const row = copy[at];
          if (row.kind !== "tool") return copy;
          copy[at] = {
            kind: "tool",
            name,
            status,
            command: done ? row.command || command : command || row.command,
            output: done ? preview : row.output,
            startedAt: row.startedAt,
          };
          return copy;
        }
        return [...copy, { kind: "tool", name, status, command: done ? "" : command, output: done ? preview : "", startedAt: Date.now() }];
      });
    }
    if (event === "subagent.start" || event === "subagent.complete") {
      const sid = owner.current || sessionRef.current;
      const row = ensure(sid);
      const id = String(ev.subagent_id ?? ev.child_session_id ?? ev.delegation_id ?? ev.goal ?? "child");
      const next: AgentRow = {
        id,
        kind: String(ev.model ?? "subagent"),
        task: String(ev.goal ?? ev.summary ?? ev.task ?? "").slice(0, 160),
        status: event.endsWith("complete") ? "done" : "running",
        tokens: Number(ev.total_tokens ?? ev.input_tokens ?? 0) || 0,
      };
      const at = row.agents.findIndex((a) => a.id === id);
      if (at >= 0) row.agents[at] = { ...row.agents[at], ...next };
      else row.agents = [...row.agents, next].slice(-8);
      show(sid);
    }
    const usage = readTokens(ev.usage);
    if (usage) {
      const sid = owner.current || sessionRef.current;
      const row = ensure(sid);
      const cur = row.tokens.total || row.tokens.input + row.tokens.output;
      if (usage.total >= cur) row.tokens = usage;
      show(sid);
    }
    if (event === "approval.request") {
      const name = String(ev.tool_name ?? ev.tool ?? ev.command ?? "write");
      const payload = String(ev.command ?? ev.preview ?? ev.reason ?? "").slice(0, 800);
      const next: Hold = {
        runId: String(ev.run_id ?? runRef.current ?? ""),
        tool: name,
        payload,
        requestId: typeof ev.request_id === "string" ? ev.request_id : undefined,
      };
      if (acceptRef.current) {
        void sendApproval(next, true);
        return;
      }
      setHold(next);
      setSheet(null);
      push({ kind: "tool", name, status: "needs approval", command: payload, startedAt: Date.now() });
    }
    if (event.includes("fail") || ev.error) {
      const detail = typeof ev.error === "string" && ev.error ? ev.error : typeof ev.preview === "string" && ev.preview ? ev.preview : event;
      push({ kind: "status", text: detail.slice(0, 240) });
    }
  }

  function titleFrom(text: string) {
    return text.slice(0, 42);
  }

  useEffect(() => {
    if (!modelBox) return;
    const root = document.documentElement;
    const prev = root.style.overflow;
    root.style.overflow = "hidden";
    const place = () => {
      const el = boxFrame.current;
      const vv = window.visualViewport;
      if (!el) return;
      const viewTop = vv?.offsetTop ?? 0;
      const viewLeft = vv?.offsetLeft ?? 0;
      const viewH = vv?.height ?? window.innerHeight;
      const viewW = vv?.width ?? window.innerWidth;
      const width = Math.min(512, Math.max(240, viewW - 16));
      el.style.top = `${viewTop + 8}px`;
      el.style.left = `${viewLeft + Math.max(8, (viewW - width) / 2)}px`;
      el.style.width = `${width}px`;
      el.style.height = `${Math.max(240, viewH - 16)}px`;
    };
    place();
    const again = window.setTimeout(place, 250);
    const vv = window.visualViewport;
    vv?.addEventListener("resize", place);
    vv?.addEventListener("scroll", place);
    window.addEventListener("resize", place);
    return () => {
      window.clearTimeout(again);
      root.style.overflow = prev;
      vv?.removeEventListener("resize", place);
      vv?.removeEventListener("scroll", place);
      window.removeEventListener("resize", place);
    };
  }, [modelBox]);

  async function sendRun(text: string, attached: File[] = [], asMode?: Mode) {
    const typed = text.trim();
    const sid = sessionRef.current || sessionId;
    const useMode = asMode ?? mode;
    const useModel = providerModel.current || modes[useMode];
    const useProvider = providerName.current || providerOf(useModel || "");
    armEar();
    if ((!typed && attached.length === 0) || ensure(sid).busy) return;
    followRef.current = true;
    setAway(false);
    owner.current = sid;
    ensure(sid).busy = true;
    setThreads((list) => list.map((t) => (t.id === sid ? { ...t, busy: true } : t)));
    show(sid);
    const parts: string[] = [];
    if (typed) parts.push(typed);
    for (const file of attached) {
      const form = new FormData();
      form.append("file", file, file.name);
      const res = await fetch("/api/hermes/attach", { method: "POST", body: form });
      const json = (await res.json().catch(() => ({}))) as {
        name?: string;
        path?: string;
        text?: string;
        error?: string;
        kind?: string;
      };
      if (!res.ok) {
        parts.push(`[${file.name}] ${json.error || res.status}`);
        continue;
      }
      parts.push(
        [`Attached ${json.kind || "file"}: ${json.name}`, json.path ? `Path: ${json.path}` : "", json.text || ""]
          .filter(Boolean)
          .join("\n"),
      );
    }
    const q = parts.join("\n\n").trim();
    if (!q) {
      ensure(sid).busy = false;
      setThreads((list) => list.map((t) => (t.id === sid ? { ...t, busy: false } : t)));
      show(sid);
      return;
    }
    if (parseLoop(q)) {
      void fetch("/api/hermes/loops", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: q, sessionId: sid, mode: useMode, model: useModel }),
      })
        .then((r) => r.json())
        .then((j: { jobs?: LoopJob[] }) => {
          if (j.jobs) setLoops(j.jobs);
        })
        .catch(() => undefined);
    }
    lastRef.current = q;
    inThink.current = false;
    const files = attached.map((f) => f.name).join(", ");
    if (files) push({ kind: "context", text: files });
    if (typed.startsWith("/") && attached.length === 0) {
      push({ kind: "user", text: q });
      setInput("");
      const res = await fetch("/api/hermes/tui", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          method: "slash.exec",
          params: { command: q, sessionId: sid, runId: ensure(sid).runId, mode: useMode, skillHint: pin },
        }),
      });
      const json = (await res.json().catch(() => ({}))) as { text?: string; error?: string };
      push({ kind: "status", text: json.text || json.error || `slash ${res.status}` });
      setPin(null);
      setBusy(false);
      ensure(sid).busy = false;
      setThreads((list) => list.map((t) => (t.id === sid ? { ...t, busy: false } : t)));
      return;
    }
    push({ kind: "user", text: attached.length ? `${typed || "file"}\n${attached.map((f) => f.name).join(", ")}` : q });
    setInput("");
    setClips([]);
    ensure(sid).busy = true;
    ensure(sid).agents = [];
    show(sid);
    setHold(null);
    const ask = q;
    setThreads((cur) => {
      const has = cur.some((t) => t.id === sid);
      return has
        ? cur.map((t) => (t.id === sid ? { ...t, busy: true } : t))
        : [{ id: sid, title: titleFrom(q), busy: true, at: Date.now() / 1000 }, ...cur];
    });
    let keepBusy = false;
    try {
      const started = await fetch("/api/hermes/runs", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          sessionId: sid,
          projectId: PROJECT_ID,
          input: ask,
          mode: useMode,
          model: useModel,
          provider: useProvider,
          skillHint: pin,
        }),
      });
      const json = (await started.json().catch(() => ({}))) as {
        runId?: string;
        run_id?: string;
        error?: string;
      };
      const id = json.runId ?? json.run_id;
      if (!id) {
        const chat = await fetch("/api/hermes/chat", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            sessionId: sid,
            projectId: PROJECT_ID,
            mode: useMode,
            model: useModel,
            messages: (() => {
              const history = ensure(sid).items
                .filter((item) => item.kind === "user" || item.kind === "assistant")
                .slice(-16)
                .map((item) => ({ role: item.kind, content: item.text.slice(0, 2000) }));
              const question = pin ? `[use skill: ${pin}]\n${ask}` : ask;
              const last = history[history.length - 1];
              if (
                last?.role === "user" &&
                (last.content === question.slice(0, 2000) || last.content === ask.slice(0, 2000))
              ) {
                return last.content === question
                  ? history
                  : [...history.slice(0, -1), { role: "user", content: question }];
              }
              return [...history, { role: "user", content: question }];
            })(),
          }),
        });
        if (chat.body) {
          await readSse(chat, onEvent);
          return;
        }
        push({ kind: "status", text: json.error || `run ${started.status}` });
        return;
      }
      setRunId(id);
      runRef.current = id;
      ensure(sid).runId = id;
      try {
        localStorage.setItem("galion-hermes-live", JSON.stringify({ sessionId: sid, runId: id, title: titleFrom(q) }));
      } catch {
        /* the stop button still works this tab */
      }
      show(sid);
      const settled = { current: false };
      const ev = await fetch(
        `/api/hermes/runs/${id}/events?sessionId=${encodeURIComponent(sid)}&projectId=${PROJECT_ID}`,
      );
      await readSse(ev, (row) => {
        const event = String(row.event ?? row.type ?? "");
        if (
          event.includes("complete") ||
          event.includes("interrupt") ||
          event.includes("fail") ||
          row.finish_reason
        ) {
          settled.current = true;
        }
        onEvent(row);
      });
      if (!settled.current) {
        owner.current = sid;
        push({ kind: "status", text: "stream dropped — run still going" });
        keepBusy = true;
        show(sid);
        return;
      }
    } catch (err) {
      push({ kind: "status", text: err instanceof Error ? err.message : "run fail" });
    } finally {
      if (!keepBusy) {
        const row = ensure(sid);
        row.busy = false;
        row.items = row.items.map((item) =>
          item.kind === "tool" && (item.status === "running" || item.status === "queued")
            ? { ...item, status: "done" }
            : item,
        );
        setThreads((list) => list.map((t) => (t.id === sid ? { ...t, busy: false } : t)));
      }
      const turns = ensure(sid).items
        .filter((item) => item.kind === "user" || item.kind === "assistant")
        .slice(-2)
        .map((item) => ({ role: item.kind === "user" ? "user" : "hermes", text: "text" in item ? item.text : "" }));
      void fetch("/api/hermes/context", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ sessionId: sid, mode: useMode, model: useModel, turns }),
      }).catch(() => undefined);
      show(sid);
      void fetch(`/api/hermes/history?id=${encodeURIComponent(sid)}`)
        .then((r) => r.json())
        .then((j: { tokens?: Tokens }) => {
          const tok = readTokens(j.tokens);
          if (!tok) return;
          const row = ensure(sid);
          const cur = row.tokens.total || row.tokens.input + row.tokens.output;
          if (tok.total >= cur || !row.busy) row.tokens = tok;
          if (sessionRef.current === sid) setTokens({ ...row.tokens });
        })
        .catch(() => undefined);
      setPin(null);
      fieldRef.current?.focus();
    }
  }

  async function sendApproval(h: Hold, ok: boolean) {
    if (!h.runId) return;
    await fetch(`/api/hermes/runs/${h.runId}/approval`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        choice: ok ? "once" : "deny",
        request_id: h.requestId,
        sessionId,
        projectId: PROJECT_ID,
      }),
    });
    if (ok) navigator.vibrate?.(16);
    else navigator.vibrate?.(8);
    setHold((cur) => (cur && cur.requestId === h.requestId && cur.tool === h.tool ? null : cur));
  }

  async function approve(ok: boolean) {
    if (!hold) return;
    await sendApproval(hold, ok);
    setHold(null);
  }

  async function stop() {
    const sid = sessionRef.current;
    const id = ensure(sid).runId;
    if (!id) return;
    await fetch(`/api/hermes/runs/${id}/stop`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sessionId: sid, projectId: PROJECT_ID }),
    });
    ensure(sid).busy = false;
    ensure(sid).runId = null;
    try { localStorage.removeItem("galion-hermes-live"); } catch { /* ignore */ }
    setThreads((list) => list.map((t) => (t.id === sid ? { ...t, busy: false } : t)));
    owner.current = sid;
    push({ kind: "status", text: "stopped" });
    show(sid);
  }

  async function steer() {
    const q = input.trim();
    if (!runId || !q) return;
    setInput("");
    push({ kind: "user", text: `steer: ${q}` });
    await fetch(`/api/hermes/runs/${runId}/steer`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: q, sessionId, projectId: PROJECT_ID }),
    });
  }

  async function admin(kind: "health" | "ping" | "boot") {
    setSheet(null);
    const path = kind === "boot" ? "/api/v1/markers" : kind === "ping" ? "/api/v1/ping" : "/api/v1/health";
    const res = await fetch(path, kind === "boot" ? {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ zone: "active", kind: "boot", source: "hermes", note: "operator" }),
    } : undefined);
    const json = await res.json().catch(() => ({}));
    push({ kind: "status", text: `${kind} ${res.status} ${JSON.stringify(json).slice(0, 180)}` });
  }

  async function finishRec(blob: Blob) {
    const form = new FormData();
    form.append("file", blob, "speech.webm");
    const res = await fetch("/api/hermes/transcribe", { method: "POST", body: form });
    const json = (await res.json().catch(() => ({}))) as { text?: string; error?: string };
    if (json.text) setInput((v) => (v ? `${v} ${json.text}` : json.text ?? ""));
    else if (json.error) push({ kind: "status", text: json.error });
  }

  async function startRec() {
    if (rec) return;
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const recoder = new MediaRecorder(stream);
    chunksRef.current = [];
    recoder.ondataavailable = (e) => {
      if (e.data.size) chunksRef.current.push(e.data);
    };
    recoder.onstop = () => {
      stream.getTracks().forEach((t) => t.stop());
      const blob = new Blob(chunksRef.current, { type: recoder.mimeType || "audio/webm" });
      if (skipTx.current) {
        skipTx.current = false;
        return;
      }
      void finishRec(blob);
    };
    recRef.current = recoder;
    recoder.start();
    setRec(true);
    setRecMs(0);
    recTick.current = window.setInterval(() => setRecMs((n) => n + 200), 200);
  }

  function stopRec(opts?: { cancel?: boolean }) {
    skipTx.current = Boolean(opts?.cancel);
    recRef.current?.stop();
    recRef.current = null;
    setRec(false);
    window.clearInterval(recTick.current);
  }

  function onMicDown(e: PointerEvent<HTMLButtonElement>) {
    e.preventDefault();
    holdRef.current = false;
    const start = Date.now();
    void startRec().then(() => {
      window.setTimeout(() => {
        if (recRef.current && Date.now() - start > 280) holdRef.current = true;
      }, 300);
    });
  }

  function onMicUp() {
    if (holdRef.current) stopRec();
  }

  function newThread() {
    const id = crypto.randomUUID();
    draftRef.current = id;
    book.current[id] = emptyBook();
    setThreads((list) => [{ id, title: "New chat", at: Date.now() / 1000 }, ...list.filter((row) => row.id !== id)]);
    openThread(id);
    void fetch("/api/hermes/history", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, create: true, title: "New chat" }),
    })
      .then(() => refreshHistory())
      .catch(() => undefined);
  }

  useEffect(() => {
    const t = window.setInterval(() => {
      for (const [id, row] of Object.entries(book.current)) {
        if (!row.busy || !row.runId) continue;
        void fetch(`/api/hermes/runs/${row.runId}?sessionId=${encodeURIComponent(id)}&projectId=${PROJECT_ID}`)
          .then((r) => r.json())
          .then((j: { status?: string; usage?: { input_tokens?: number; output_tokens?: number; total_tokens?: number; prompt_tokens?: number; completion_tokens?: number }; agents?: AgentRow[] }) => {
            const tok = readTokens(j.usage);
            if (tok) {
              const cur = row.tokens.total || row.tokens.input + row.tokens.output;
              if (tok.total >= cur) row.tokens = tok;
            }
            if (Array.isArray(j.agents)) row.agents = j.agents.slice(-8);
            if (j.status && ["completed", "stopped", "failed", "error", "interrupted"].includes(j.status)) {
              row.busy = false;
              row.runId = null;
              try {
                const live = JSON.parse(localStorage.getItem("galion-hermes-live") ?? "null") as { sessionId?: string } | null;
                if (!live?.sessionId || live.sessionId === id) localStorage.removeItem("galion-hermes-live");
              } catch {
                localStorage.removeItem("galion-hermes-live");
              }
              setThreads((list) => list.map((th) => (th.id === id ? { ...th, busy: false } : th)));
            }
            show(id);
          })
          .catch(() => undefined);
      }
    }, 5000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    function load() {
      void fetch("/api/hermes/loops")
        .then((r) => r.json())
        .then((j: { jobs?: LoopJob[]; alpha?: Alpha; scope?: Scope }) => {
          setLoops(j.jobs ?? []);
          if (j.alpha) setAlpha(j.alpha);
          if (j.scope) setScope(j.scope);
        })
        .catch(() => undefined);
    }
    load();
    const t = window.setInterval(load, 2000);
    return () => window.clearInterval(t);
  }, []);

  function startBackground() {
    void fetch("/api/hermes/loops", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action: "alpha", text: "keep: local ports, no model", n: 6, budget: 120 }),
    })
      .then((r) => r.json())
      .then((j: { alpha?: Alpha }) => {
        if (j.alpha) setAlpha(j.alpha);
      })
      .catch(() => undefined);
  }
  function stopBackground() {
    void fetch("/api/hermes/loops", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action: "alpha-stop" }),
    })
      .then((r) => r.json())
      .then((j: { alpha?: Alpha }) => {
        if (j.alpha) setAlpha(j.alpha);
      })
      .catch(() => undefined);
  }
  function loopAct(action: "pause" | "resume" | "stop" | "stop-all" | "pause-all" | "resume-all", job?: LoopJob) {
    void fetch("/api/hermes/loops", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(action.endsWith("-all") ? { action } : { action, id: job?.id }),
    })
      .then((r) => r.json())
      .then((j: { jobs?: LoopJob[]; alpha?: Alpha }) => {
        if (j.jobs) setLoops(j.jobs);
        if (j.alpha) setAlpha(j.alpha);
      })
      .catch(() => undefined);
  }
  const sessionLoops = loops.filter((j) => j.sessionId === sessionId && !j.stopped);
  const shown = threads.filter((t) => t.title.toLowerCase().includes(chatQuery.trim().toLowerCase()));
  const up = Boolean(health?.ok);
  const workSecs = workFrom ? Math.max(0, Math.floor((now - workFrom) / 1000)) : 0;

  function onTranscriptScroll() {
    const node = transcriptRef.current;
    if (!node) return;
    const atEnd = node.scrollHeight - node.scrollTop - node.clientHeight < 72;
    followRef.current = atEnd;
    setAway(!atEnd);
  }

  function jumpLatest() {
    followRef.current = true;
    setAway(false);
    const node = transcriptRef.current;
    if (node) node.scrollTop = node.scrollHeight;
  }
  const modelId = modes[mode] || "";
  const modelName = catalog.find((row) => row.id === modelId)?.name || modelId;
  const modelLabel = (modelName.includes("/") ? modelName.split("/").pop() : modelName) || "No model";
  const canSend = Boolean(input.trim() || clips.length);
  const groups = ["Local", "Addy", "Ondrej", "ZeroAI", "Other"];

  return (
    <div className="relative flex h-full min-h-0 w-full bg-bg">
      <input
        ref={fileRef}
        type="file"
        multiple
        accept=".md,.markdown,.txt,.pdf,.png,.jpg,.jpeg,.webp,.gif,.json,.csv,.docx,.mp4,.mov,.webm,.m4v,text/*,image/*,video/*"
        className="hidden"
        onChange={(e) => {
          const picked = Array.from(e.target.files ?? []);
          if (picked.length) setClips((list) => [...list, ...picked].slice(0, 6));
          e.target.value = "";
          setSheet(null);
        }}
      />
      <aside
        className={cn(
          "z-40 flex w-60 max-w-[88vw] shrink-0 flex-col border-r border-line bg-bg",
          "max-lg:absolute max-lg:inset-0 max-lg:w-full max-lg:max-w-none",
          drawer ? "max-lg:translate-x-0" : "max-lg:-translate-x-full",
        )}
      >
        <div className="flex min-h-11 items-center gap-2 border-b border-line px-2">
          <button type="button" onClick={newThread} className="inline-flex shrink-0 items-center gap-1 text-[13px] text-fg">
            <Icon name="chats" className="size-3.5" />
            New
          </button>
          <input
            value={chatQuery}
            onChange={(e) => setChatQuery(e.target.value)}
            placeholder="Find a chat"
            aria-label="Search chats"
            className="min-h-11 min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none"
          />
          <span className="shrink-0 text-[12px] text-muted">{up ? "up" : "down"}</span>
          <button type="button" className="inline-flex shrink-0 items-center gap-1 text-[13px] text-muted lg:hidden" onClick={() => setDrawer(false)} aria-label="Close">
            <Icon name="close" className="size-3.5" />
            Close
          </button>
        </div>
        <div className="panel-scroll min-h-0 flex-1 overflow-y-auto">
          {shown.length === 0 ? <p className="px-2 py-2 text-[13px] text-muted">No chats.</p> : null}
          {shown.map((t) => {
            const job = loops.find((row) => row.sessionId === t.id && !row.stopped);
            const open = t.id === sessionId;
            return (
              <div key={t.id} className="flex min-h-11 items-center gap-2 border-b border-line px-2">
                <button type="button" className={cn("min-w-0 flex-1 truncate text-left text-[12px]", open ? "text-fg" : "text-muted")} onClick={() => void openHistory(t.id)}>
                  {t.busy ? "● " : ""}
                  {t.title}
                </button>
                {open && (busy || runId) ? (
                  <button type="button" className="shrink-0 min-h-11 px-1 text-[11px] text-fg" onClick={() => void stop()}>
                    Stop
                  </button>
                ) : null}
                {open ? (
                  <button type="button" className="shrink-0 min-h-11 px-1 text-[11px] text-muted" onClick={() => void togglePin(t.id, Boolean(t.pinned))}>
                    {t.pinned ? "Unpin" : "Pin"}
                  </button>
                ) : null}
                {job ? (
                  <button type="button" className="shrink-0 min-h-11 px-1 text-[11px] text-fg" onClick={() => loopAct(job.paused ? "resume" : "pause", job)}>
                    {job.paused ? "Go" : "Pause"}
                  </button>
                ) : null}
                {job ? (
                  <button type="button" className="shrink-0 min-h-11 px-1 text-[11px] text-fg" onClick={() => loopAct("stop", job)}>
                    End
                  </button>
                ) : null}
                {!open && !job ? <span className="shrink-0 text-[11px] text-subtle">{ago(t.at)}</span> : null}
              </div>
            );
          })}
          {loops.filter((job) => job.stopped || !shown.some((t) => t.id === job.sessionId)).map((job) => (
            <div key={job.id} className="flex min-h-11 items-center justify-between gap-2 border-b border-line px-2">
              <p className="min-w-0 truncate text-[12px] text-muted">
                {job.stopped ? "stopped" : job.paused ? "paused" : "loop"} · {Math.max(1, Math.round(job.everySec / 60))}m · {job.input}
              </p>
              <span className="flex shrink-0 gap-3">
                <button type="button" className="text-[12px] text-fg" onClick={() => loopAct(job.stopped || job.paused ? "resume" : "pause", job)}>
                  {job.stopped || job.paused ? "Resume" : "Pause"}
                </button>
                {job.stopped ? null : (
                  <button type="button" className="text-[12px] text-fg" onClick={() => loopAct("stop", job)}>
                    Stop
                  </button>
                )}
              </span>
            </div>
          ))}
          {loops.some((job) => !job.stopped) ? (
            <button type="button" className="inline-flex min-h-11 items-center gap-1 px-2 text-left text-[12px] text-fg" onClick={() => loopAct("stop-all")}>
              <Icon name="stop" className="size-3.5" />
              Stop all loops
            </button>
          ) : null}
          <div className="flex min-h-11 items-center justify-between gap-2 border-y border-line px-2">
            <span className="truncate text-[12px] text-fg">Background {alpha?.state || "idle"}</span>
            <span className="flex gap-3">
              <button type="button" className="text-[12px] text-fg disabled:text-subtle" disabled={alpha?.state === "running" || alpha?.state === "starting"} onClick={startBackground}>
                Start
              </button>
              <button type="button" className="text-[12px] text-fg" onClick={stopBackground}>
                Stop
              </button>
            </span>
          </div>
          <button type="button" className="block min-h-11 w-full border-b border-line px-2 text-left text-[12px] text-fg" onClick={() => { setSheet("data"); void fetch("/api/hermes/history?view=databases").then((r) => r.json()).then((j: { items?: { file: string; bytes: number; error: string; tables: { name: string; rows: number }[] }[] }) => setDbs(j.items ?? [])).catch(() => setDbs([])); }}>
            <span className="inline-flex items-center gap-2"><Icon name="database" className="size-3.5" />Databases</span>
          </button>
          <button type="button" className="block min-h-11 w-full border-b border-line px-2 text-left text-[12px] text-fg" onClick={() => { setDrawer(false); setSheet("live"); setSettingMsg(""); void fetch("/api/hermes/settings").then((r) => r.json()).then((j: SettingsView) => { setSettings(j); setSettingText(j.files?.[settingFile] ?? j.files?.soul ?? ""); }).catch(() => setSettings(null)); }}>
            <span className="inline-flex items-center gap-2"><Icon name="settings" className="size-3.5" />Settings</span>
          </button>
          <button type="button" className="block min-h-11 w-full border-b border-line px-2 text-left text-[12px] text-fg" onClick={() => { setSheet("skills"); setDrawer(false); }}>
            <span className="inline-flex items-center gap-2"><Icon name="skill" className="size-3.5" />Skills · {skills.length}</span>
          </button>
          <button type="button" className="block min-h-11 w-full border-b border-line px-2 text-left text-[12px] text-fg" onClick={() => { setDrawer(false); setSheet("tools"); void fetch("/api/hermes/tools").then((r) => r.json()).then((j: { items?: ToolRow[] }) => setToolRows(j.items ?? [])).catch(() => setToolRows([])); }}>
            <span className="inline-flex items-center gap-2"><Icon name="tools" className="size-3.5" />Tools</span>
          </button>
          <button type="button" className="block min-h-11 w-full border-b border-line px-2 text-left text-[12px] text-fg" onClick={() => { setDrawer(false); setSheet("activity"); void fetch("/api/hermes/activity").then((r) => r.json()).then((j: { items?: { at: number; kind: string; text: string }[] }) => setActivity(j.items ?? [])).catch(() => setActivity([])); }}>
            <span className="inline-flex items-center gap-2"><Icon name="activity" className="size-3.5" />Activity</span>
          </button>
          <Link to="/" onClick={() => setDrawer(false)} className="flex min-h-11 items-center border-b border-line px-2 text-[12px] text-fg">
            <span className="inline-flex items-center gap-2"><Icon name="desk" className="size-3.5" />Desk</span>
          </Link>
          <button
            type="button"
            className="flex min-h-11 w-full items-center justify-between border-b border-line px-2 text-[12px] text-fg"
            onClick={() => {
              const next = theme === "light" ? "dark" : "light";
              const light: Record<string, string> = {
                "--color-bg": "#f6f6f4",
                "--color-surface": "#ffffff",
                "--color-elevated": "#efefec",
                "--color-fg": "#1a1a1a",
                "--color-muted": "#5e5e5e",
                "--color-subtle": "#8a8a86",
                "--color-line": "#e2e2de",
                "--color-winter": "#f6f6f4",
              };
              const root = document.documentElement;
              root.dataset.theme = next;
              root.style.colorScheme = next;
              for (const key of Object.keys(light)) {
                if (next === "light") root.style.setProperty(key, light[key]);
                else root.style.removeProperty(key);
              }
              try {
                localStorage.setItem("galion-theme", next);
              } catch {
                /* ignore */
              }
              setTheme(next);
            }}
          >
            <span className="inline-flex items-center gap-2"><Icon name={theme === "light" ? "moon" : "sun"} className="size-3.5" />Theme</span>
            <span className="text-muted">{theme}</span>
          </button>
          <button type="button" className="flex min-h-11 w-full items-center justify-between border-b border-line px-2 text-[12px] text-fg" onClick={() => { const next = !guard; setGuard(next); void fetch("/api/hermes/guard", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ on: next }) }); }}>
            <span>Denylist</span>
            <span className="text-muted">{guard ? "on" : "off"}</span>
          </button>
          <button type="button" className="flex min-h-11 w-full items-center justify-between border-b border-line px-2 text-[12px] text-fg" onClick={() => { const next = !accept; setAccept(next); void fetch("/api/hermes/guard", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ autoAccept: next }) }); }}>
            <span>Auto accept</span>
            <span className="text-muted">{accept ? "on" : "off"}</span>
          </button>
          <div className="flex flex-wrap gap-x-3 px-2">
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => void refreshHistory()}>
              Refresh
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg disabled:text-subtle" disabled={!sessionId} onClick={() => void navigator.clipboard.writeText(sessionId)}>
              Copy id
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => fileRef.current?.click()}>
              File
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                void fetch("/api/hermes/tui")
                  .then((r) => r.json())
                  .then((j: { slash?: { cmd: string; hint: string }[] }) => setSlash(j.slash ?? []))
                  .catch(() => setSlash([]));
                setSheet("slash");
                setDrawer(false);
              }}
            >
              Slash
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => void admin("health")}>
              Probe
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => void admin("ping")}>
              Ping
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => void admin("boot")}>
              Boot
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                setModelFor(mode);
                setSheet("models");
                setDrawer(false);
              }}
            >
              Models
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                const next = mode === "plan" ? "action" : "plan";
                setMode(next);
                localStorage.setItem("galion-hermes-mode", next);
              }}
            >
              {MODE_LABEL[mode]}
            </button>
            <button type="button" className="inline-flex min-h-11 items-center gap-1 text-[12px] text-fg" onClick={() => setVoiceOn((on) => !on)}>
              <Icon name="voice" className="size-3.5" />
              Voice {voiceOn ? "on" : "off"}
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => void sendRun("Drain the request inbox.")}>
              Inbox
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg disabled:text-subtle" disabled={!lastRef.current} onClick={() => void sendRun(lastRef.current)}>
              Retry
            </button>
            <Link to="/context" onClick={() => setDrawer(false)} className="inline-flex min-h-11 items-center gap-1 text-[12px] text-fg">
              <Icon name="context" className="size-3.5" />
              Context
            </Link>
            <Link to="/cache" onClick={() => setDrawer(false)} className="inline-flex min-h-11 items-center gap-1 text-[12px] text-fg">
              <Icon name="cache" className="size-3.5" />
              Cache
            </Link>
            <button type="button" className="inline-flex min-h-11 items-center gap-1 text-[12px] text-fg" onClick={() => loopAct("pause-all")}>
              <Icon name="loop" className="size-3.5" />
              Pause all
            </button>
            <button type="button" className="inline-flex min-h-11 items-center gap-1 text-[12px] text-fg" onClick={() => loopAct("resume-all")}>
              <Icon name="loop" className="size-3.5" />
              Resume all
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg disabled:text-subtle" disabled={!hold} onClick={() => void approve(true)}>
              Allow
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg disabled:text-subtle" disabled={!hold} onClick={() => void approve(false)}>
              Deny
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg disabled:text-subtle" disabled={!runId} onClick={() => void steer()}>
              Steer
            </button>
            <button type="button" className="inline-flex min-h-11 items-center gap-1 text-[12px] text-fg" onClick={() => void startRec()}>
              <Icon name="voice" className="size-3.5" />
              Mic
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                try {
                  voiceSrc.current?.stop();
                } catch {
                  /* already ended */
                }
                setSpeaking(false);
              }}
            >
              Quiet
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                const last = [...items].reverse().find((item) => item.kind === "assistant");
                if (last?.kind === "assistant") void navigator.clipboard.writeText(displayTurn("assistant", last.text));
              }}
            >
              Copy reply
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => setInput("")}>
              Clear
            </button>
            <button type="button" className="min-h-11 text-[12px] text-fg" onClick={() => fieldRef.current?.focus()}>
              Focus
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                void fetch("/api/hermes/gpu")
                  .then((r) => r.json())
                  .then((j: { raw?: string }) => push({ kind: "status", text: j.raw || "gpu unread" }))
                  .catch(() => push({ kind: "status", text: "gpu unread" }));
              }}
            >
              GPU
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                void fetch("/api/hermes/spend")
                  .then((r) => r.json())
                  .then((j) => push({ kind: "status", text: `spend ${JSON.stringify(j).slice(0, 160)}` }))
                  .catch(() => push({ kind: "status", text: "spend unread" }));
              }}
            >
              Spend
            </button>
            <button
              type="button"
              className="min-h-11 text-[12px] text-fg"
              onClick={() => {
                void fetch("/api/hermes/inbox")
                  .then((r) => r.json())
                  .then((j: { items?: { status?: string }[] }) => {
                    const rows = j.items ?? [];
                    const open = rows.filter((row) => row.status === "open").length;
                    push({ kind: "status", text: `queue ${open} open · ${rows.length} kept` });
                  })
                  .catch(() => push({ kind: "status", text: "queue unread" }));
              }}
            >
              Queue
            </button>
          </div>
        </div>
      </aside>

      {drawer ? (
        <button type="button" className="absolute inset-0 z-20" aria-label="Close drawer" onClick={() => setDrawer(false)} />
      ) : null}

      <section
        className="life-stage relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
        data-phase={rec ? "listen" : speaking ? "speak" : busy ? "hold" : "still"}
      >
        <header className="flex shrink-0 items-baseline justify-between gap-3 border-b border-line px-3 py-2">
          <div className="min-w-0">
            <p className="truncate text-[12px] text-muted">
              {up ? "Line up" : "Line down"} · {MODE_LABEL[mode]} · {modelLabel} · {busy ? `Working ${clock(workSecs)}` : "idle"}
            </p>
          </div>
          <button type="button" className="inline-flex min-h-11 shrink-0 items-center gap-1 text-[12px] text-fg" onClick={() => setDrawer(true)}>
            <Icon name="chats" className="size-3.5" />
            Chats
          </button>
        </header>
        <MemoryStrip tokens={tokens} sessionId={sessionId} loops={loops} agents={agents} items={items} />

        <div className="relative min-h-0 flex-1">
        <div
          ref={transcriptRef}
          onScroll={onTranscriptScroll}
          className={items.length === 0 ? "flex h-full min-h-0 flex-col justify-center overflow-hidden px-5" : "panel-scroll h-full min-h-0 overflow-y-auto px-3 py-4"}
        >
          {items.length === 0 ? (
            <div className="research-empty max-w-md">
              <p className="inline-flex items-center gap-2 font-mono text-[11px] uppercase tracking-wider text-subtle">
                <Icon name="research" className="size-3.5" />
                By Galion
              </p>
              <h1 className="mt-2 text-2xl font-medium tracking-tight">Hermes Research</h1>
              <p className="mt-3 max-w-sm text-sm leading-relaxed text-muted">Agent. Launched on the Obsidian Open Mind stack.</p>
            </div>
          ) : (
            <ol className="mx-auto flex max-w-lg flex-col gap-3">
              {chatBlocks(orderTurns(items)).map((block) => {
                if (block.kind === "road") {
                  return (
                    <li key={`road-${block.steps[0].index}`}>
                      <Roadmap
                        steps={block.steps}
                        onOpen={(index) => {
                          setToolAt(index);
                          setSheet("tool");
                        }}
                      />
                    </li>
                  );
                }
                const item = block.item;
                const i = block.index;
                if (item.kind === "context") {
                  return (
                    <li key={i} className="rounded-[12px] border border-line bg-surface px-3 py-2 text-[13px] leading-snug text-muted">
                      {item.text}
                    </li>
                  );
                }
                if (item.kind === "think") {
                  const latest = items.slice(i + 1).every((n) => n.kind !== "think");
                  return (
                    <li key={i}>
                      <details className="research-think rounded-md px-3 py-1.5">
                        <summary className="cursor-pointer text-[13px] text-muted">
                          {latest && busy ? `Thinking · Working ${clock(workSecs)}` : "Thinking"}
                        </summary>
                        <pre className="mt-2 whitespace-pre-wrap font-sans text-[14px] leading-relaxed text-fg">
                          {item.text}
                        </pre>
                      </details>
                    </li>
                  );
                }
                if (item.kind === "user") {
                  const text = displayTurn("user", item.text);
                  if (!text) return null;
                  return (
                    <li key={i} className="ml-10 rounded-[12px] bg-surface px-3 py-2 text-[15px] leading-[1.45] text-fg">
                      {text}
                    </li>
                  );
                }
                if (item.kind === "assistant") {
                  const text = displayTurn("assistant", item.text);
                  if (!text) return null;
                  const long = text.length > 900 || text.includes("```");
                  return (
                    <li key={i} className="mr-6">
                      <p className="mb-1 font-mono text-[11px] uppercase tracking-wider text-subtle">Agent</p>
                      <div
                        className="text-[15px] leading-[1.45] [&_pre]:mt-2 [&_pre]:overflow-x-auto [&_pre]:rounded-[12px] [&_pre]:border [&_pre]:border-line [&_pre]:bg-surface [&_pre]:p-3 [&_pre]:font-mono [&_pre]:text-xs"
                        dangerouslySetInnerHTML={{ __html: md(text) }}
                      />
                      {busy && i === items.length - 1 ? (
                        <span className="mt-1 inline-block h-4 w-0.5 animate-pulse bg-fg" />
                      ) : null}
                      <div className="mt-1 flex items-center gap-1">
                        <CopyButton value={text} label="reply" compact />
                        <button
                          type="button"
                          className={cn("inline-flex h-8 items-center gap-1 rounded-full px-2 font-mono text-[11px] uppercase", speakingAt === i ? "text-accent" : "text-muted")}
                          aria-label={speakingAt === i ? "Stop speaking" : "Speak this reply"}
                          onClick={() => void playVoice(text, i)}
                        >
                          <Volume2 className="size-3.5" strokeWidth={1.6} />
                          {speakingAt === i ? "Stop" : "Speak"}
                        </button>
                        {long ? (
                          <button
                            type="button"
                            className="inline-flex h-8 items-center rounded-full px-2 font-mono text-[11px] uppercase text-muted"
                            onClick={() => {
                              setArtifact(text);
                              setSheet("artifact");
                            }}
                          >
                            Artifact
                          </button>
                        ) : null}
                      </div>
                    </li>
                  );
                }
                if (item.kind === "tool") {
                  return (
                    <li key={i}>
                      <button
                        type="button"
                        className="flex w-full items-center gap-2 rounded-[12px] border border-line bg-surface px-3 py-2 text-left"
                        onClick={() => {
                          setToolAt(i);
                          setSheet("tool");
                        }}
                      >
                        <Terminal className="size-3.5 shrink-0 text-muted" strokeWidth={1.6} />
                        <span className="shrink-0 text-[12px] text-muted">Run</span>
                        <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-fg">
                          {(() => {
                            const steps = splitCommands(item.command || "");
                            if (steps.length > 1) return `${steps.length} commands`;
                            return steps[0] || item.name;
                          })()}
                        </span>
                        <span className={cn("shrink-0 font-mono text-[11px]", item.status === "running" || item.status === "queued" ? "text-accent" : "text-muted")}>
                          {item.status === "running" || item.status === "queued" ? `Working ${clock(item.startedAt ? Math.max(0, Math.floor((now - item.startedAt) / 1000)) : workSecs)}` : item.status}
                        </span>
                      </button>
                    </li>
                  );
                }
                return (
                  <li key={i} className="font-mono text-[11px] text-subtle">
                    {item.text}
                  </li>
                );
              })}
            </ol>
          )}
        </div>
        {away ? (
          <button
            type="button"
            className="absolute bottom-3 left-1/2 z-20 -translate-x-1/2 rounded-full border border-line bg-bg px-3 py-1.5 text-[12px] text-fg"
            onClick={jumpLatest}
          >
            Latest
          </button>
        ) : null}
        </div>

        {hold ? (
          <div
            className="relative z-40 shrink-0 border-t border-accent bg-surface px-3 py-3 shadow-[0_-8px_24px_rgb(0_0_0/0.45)]"
            role="dialog"
            aria-label="Approve"
          >
            <p className="font-mono text-[11px] uppercase tracking-wider text-accent">
              {isDangerous(`${hold.tool}\n${hold.payload}`) ? "Stays until you choose" : "Approval"}
            </p>
            <p className="mt-1 text-[15px] font-medium">{hold.tool}</p>
            <p className="mt-1 max-h-24 overflow-y-auto whitespace-pre-wrap break-words font-mono text-xs text-muted">{hold.payload || "No detail"}</p>
            <div className="mt-3 grid grid-cols-2 gap-2">
              <button type="button" className="h-12 rounded-[12px] bg-accent text-sm font-medium text-accent-fg" onClick={() => void approve(true)}>
                Allow
              </button>
              <button type="button" className="h-12 rounded-[12px] border border-offline/50 text-sm text-offline" onClick={() => void approve(false)}>
                Deny
              </button>
            </div>
          </div>
        ) : null}

        {sessionLoops.length > 0 ? (
          <div className="border-t border-line bg-surface px-3 py-2">
            {sessionLoops.map((job) => (
              <div key={job.id} className="mx-auto flex max-w-lg items-center justify-between gap-3 py-1">
                <p className="min-w-0 truncate text-[13px] text-muted">
                  {job.paused ? "Paused" : "Loop"} · every {Math.max(1, Math.round(job.everySec / 60))}m
                </p>
                <span className="flex shrink-0 gap-3">
                  <button type="button" className="min-h-11 text-sm text-fg" onClick={() => loopAct(job.paused ? "resume" : "pause", job)}>
                    {job.paused ? "Resume" : "Pause"}
                  </button>
                  <button type="button" className="min-h-11 text-sm text-fg" onClick={() => loopAct("stop", job)}>
                    Stop
                  </button>
                </span>
              </div>
            ))}
          </div>
        ) : null}

        {busy || runId || agents.length > 0 ? (
          <div className="border-t border-line bg-surface px-3 py-2">
            <div className="mx-auto flex max-w-lg flex-wrap items-center gap-2">
              <span className="font-mono text-[11px] uppercase tracking-wider text-subtle">
                {busy ? "running" : "idle"} · {tokens.total || tokens.input + tokens.output} tok
              </span>
              {busy || runId ? (
                <button type="button" onClick={() => setDrawer(true)} className="max-w-full truncate rounded-full border border-line px-2 py-1 text-[12px] text-fg">
                  Hermes · {threads.find((t) => t.id === sessionId)?.title || "this chat"}
                </button>
              ) : null}
              {busy || runId ? (
                <button type="button" onClick={() => void stop()} className="min-h-11 rounded-full border border-offline/50 px-3 text-sm text-offline">
                  Stop
                </button>
              ) : null}
              {agents.map((a) => (
                <span key={a.id} className="inline-flex max-w-full items-center gap-1 rounded-full border border-line px-2 py-1 text-[12px] text-muted">
                  <button type="button" className="truncate text-left" onClick={() => setDrawer(true)}>
                    {a.kind} · {a.status}{a.task ? ` · ${a.task}` : ""}
                  </button>
                  {a.status === "running" ? (
                    <button type="button" className="text-offline" onClick={() => void stop()}>
                      Stop
                    </button>
                  ) : null}
                </span>
              ))}
            </div>
          </div>
        ) : null}

        <div className={cn("research-composer obsidian-foot relative z-10 shrink-0 border-t border-line bg-bg px-2 pb-2 pt-1", drawer && "hidden")}>
          {clips.length > 0 ? (
            <ul className="mx-auto mb-2 flex max-w-lg flex-wrap gap-2" aria-label="Files to send">
              {clips.map((file, i) => (
                <li key={`${file.name}-${file.size}-${i}`} className="flex max-w-full items-center gap-2 rounded-full border border-line bg-surface px-3 py-1.5 text-sm">
                  <span className="truncate">{file.name}</span>
                  <span className="shrink-0 font-mono text-[11px] text-subtle">{Math.max(1, Math.round(file.size / 1024))}kb</span>
                  <button
                    type="button"
                    className="inline-flex size-8 items-center justify-center"
                    aria-label={`Remove ${file.name}`}
                    onClick={() => setClips((list) => list.filter((_, n) => n !== i))}
                  >
                    <X className="size-3.5" />
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          <div className="mx-auto mb-1 flex max-w-lg gap-3 px-1">
            {(Object.keys(MODE_LABEL) as Mode[]).map((id) => (
              <button
                key={id}
                type="button"
                aria-pressed={mode === id}
                onClick={() => { setMode(id); localStorage.setItem("galion-hermes-mode", id); }}
                className={mode === id ? "text-sm text-fg" : "text-sm text-muted"}
              >
                {MODE_LABEL[id]}
              </button>
            ))}
          </div>
          {modelBox && typeof document !== "undefined" ? createPortal(
            <div className="fixed inset-0 z-[80] bg-black/40" onPointerDown={() => setModelBox("")}>
            <div
              ref={boxFrame}
              className="fixed z-[81] flex flex-col rounded-[16px] border border-line bg-surface p-3"
              onPointerDown={(e) => e.stopPropagation()}
            >
              <div className="mb-2 flex items-center gap-2">
              <input
                autoFocus
                value={boxQ}
                onChange={(e) => setBoxQ(e.target.value)}
                placeholder="Search models"
                aria-label="Search models"
                className="h-11 min-w-0 flex-1 rounded-[10px] border border-line bg-bg px-3 text-sm"
              />
              <button type="button" className="inline-flex size-11 shrink-0 items-center justify-center" aria-label="Close" onClick={() => setModelBox("")}>
                <X className="size-5" />
              </button>
              </div>
              {(() => {
                const present = new Set<string>();
                for (const row of boxRows) {
                  if (row.modality) present.add(row.modality);
                  for (const use of row.uses ?? []) present.add(use);
                }
                const chips = ["all", ...CAT_ORDER.filter((id) => present.has(id))];
                if (chips.length < 3) return null;
                const count = (id: string) => id === "all" ? boxRows.length : boxRows.filter((row) => row.modality === id || (row.uses ?? []).includes(id)).length;
                return (
                  <div className="mb-2 flex shrink-0 gap-1.5 overflow-x-auto pb-1">
                    {chips.map((id) => (
                      <button
                        key={id}
                        type="button"
                        aria-pressed={(boxCat || "all") === id}
                        onClick={() => setBoxCat(id)}
                        className={`shrink-0 rounded-full border px-3 py-1 text-sm ${(boxCat || "all") === id ? "border-accent text-fg" : "border-line text-muted"}`}
                      >
                        {CAT_LABEL[id] || id} {count(id)}
                      </button>
                    ))}
                  </div>
                );
              })()}
              <ul className="min-h-0 flex-1 space-y-0.5 overflow-y-auto">
                {(() => {
                  const current = providerNow === modelBox ? providerModel.current || modes[mode] || "" : "";
                  const starred = new Set(stars[modelBox] ?? []);
                  const q = boxQ.trim().toLowerCase();
                  const cat = boxCat || "all";
                  const pool = boxRows.filter((row) => cat === "all" || row.modality === cat || (row.uses ?? []).includes(cat));
                  const head = pool.find((row) => row.id === current);
                  const rest = pool
                    .filter((row) => row.id !== current && (!q || `${row.name} ${row.id}`.toLowerCase().includes(q)))
                    .sort((a, b) => Number(starred.has(b.id)) - Number(starred.has(a.id)) || a.name.localeCompare(b.name));
                  const shown = [...(head ? [head] : []), ...rest];
                  if (!shown.length) return <li className="px-2 py-2 text-sm text-muted">{boxRows.length ? "No model" : "Loading"}</li>;
                  return shown.map((row) => {
                    const on = row.id === current;
                    const star = starred.has(row.id);
                    return (
                      <li key={row.id}>
                        <div className={`flex min-h-11 items-center gap-2 rounded-[10px] px-1 ${on ? "bg-elevated" : ""}`}>
                          <button
                            type="button"
                            className="inline-flex size-9 shrink-0 items-center justify-center text-base"
                            aria-label={star ? "Unstar" : "Star"}
                            aria-pressed={star}
                            onClick={() => {
                              const next = star ? (stars[modelBox] ?? []).filter((id) => id !== row.id) : [row.id, ...(stars[modelBox] ?? [])];
                              setStars((cur) => ({ ...cur, [modelBox]: next.slice(0, 24) }));
                              void fetch("/api/hermes/modes", {
                                method: "POST",
                                headers: { "Content-Type": "application/json" },
                                body: JSON.stringify({ action: "star", provider: modelBox, model: row.id }),
                              });
                            }}
                          >
                            {star ? "★" : "☆"}
                          </button>
                          <button
                            type="button"
                            className="flex min-w-0 flex-1 items-center justify-between gap-2 py-1 text-left"
                            onClick={() => {
                              providerModel.current = row.id;
                              providerName.current = modelBox;
                              setProviderNow(modelBox);
                              setModes((cur) => ({ ...cur, plan: row.id, action: row.id }));
                              setModelBox("");
                              for (const name of ["plan", "action"] as const) {
                                void fetch("/api/hermes/modes", {
                                  method: "POST",
                                  headers: { "Content-Type": "application/json" },
                                  body: JSON.stringify({ action: "assign", mode: name, model: row.id, provider: modelBox }),
                                });
                              }
                            }}
                          >
                            <span className="min-w-0">
                              <span className="block truncate text-sm">{row.name}</span>
                              <span className="block truncate font-mono text-[11px] text-subtle">{row.id}</span>
                            </span>
                            {on ? <span aria-label="Running">✓</span> : null}
                          </button>
                        </div>
                      </li>
                    );
                  });
                })()}
              </ul>
            </div>
            </div>,
            document.body,
          ) : null}
          <div className="mx-auto mb-1 flex max-w-lg gap-3 px-1" role="group" aria-label="Provider">
            {PROVIDER_CHOICES.map((row) => {
              const on = providerNow === row.id;
              return (
                <button
                  key={row.id}
                  type="button"
                  aria-pressed={on}
                  onContextMenu={(e) => e.preventDefault()}
                  onPointerDown={() => {
                    skipTap.current = false;
                    window.clearTimeout(holdTimer.current);
                    holdTimer.current = window.setTimeout(() => {
                      skipTap.current = true;
                      setBoxQ("");
                      setBoxCat("all");
                      setBoxRows([]);
                      setModelBox(row.id);
                      void fetch(`/api/hermes/models?provider=${row.id}`)
                        .then((r) => r.json())
                        .then((j: { models?: { id: string; name: string; modality?: string; uses?: string[] }[] }) => setBoxRows(j.models ?? []))
                        .catch(() => setBoxRows([]));
                    }, 420);
                  }}
                  onPointerUp={() => window.clearTimeout(holdTimer.current)}
                  onPointerCancel={() => window.clearTimeout(holdTimer.current)}
                  onClick={() => {
                    if (skipTap.current) {
                      skipTap.current = false;
                      return;
                    }
                    providerModel.current = row.model;
                    providerName.current = row.id;
                    setProviderNow(row.id);
                    setModelBox("");
                    setModes((cur) => ({ ...cur, plan: row.model, action: row.model }));
                    for (const name of ["plan", "action"] as const) {
                      void fetch("/api/hermes/modes", {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ action: "assign", mode: name, model: row.model, provider: row.id }),
                      });
                    }
                  }}
                  className={on ? "text-sm text-fg" : "text-sm text-muted"}
                >
                  {row.label}
                </button>
              );
            })}
          </div>
          <form
            className="mx-auto flex max-w-lg items-end gap-1"
            onSubmit={(e: FormEvent) => {
              e.preventDefault();
              if (busy) void steer();
              else void sendRun(input, clips);
            }}
          >
            <button
              type="button"
              className="relative inline-flex size-11 shrink-0 items-center justify-center rounded-[12px]"
              aria-label="Chats"
              onClick={() => setDrawer(true)}
            >
              <Icon name="chats" className="size-5" />
              {Object.values(unread).some(Boolean) ? <span className="absolute right-2 top-2 size-1.5 rounded-full bg-accent" /> : null}
            </button>
            <button
              type="button"
              className="inline-flex size-11 shrink-0 items-center justify-center rounded-[12px]"
              aria-label="Attach"
              onClick={() => setSheet("plus")}
            >
              <Plus className="size-5" />
            </button>
            <textarea
              ref={fieldRef}
              rows={1}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={busy ? "Sending…" : rec ? "Listening…" : "Ask"}
              aria-label="Ask Hermes Research"
              className="relative z-30 max-h-24 min-h-11 flex-1 resize-none bg-transparent py-2.5 text-[16px] leading-[1.45] text-fg outline-none"
            />
            {rec ? (
              <span className="mb-3 flex items-end gap-0.5" aria-hidden>
                {Array.from({ length: 7 }).map((_, i) => (
                  <i key={i} className="wave-bar" style={{ animationDelay: `${i * 70}ms` }} />
                ))}
              </span>
            ) : null}
            {rec ? (
              <span className="mb-4 font-mono text-[11px] text-offline">{Math.floor(recMs / 1000)}s</span>
            ) : null}
            {rec ? (
              <button type="button" className="mb-3 text-sm text-muted" onClick={() => stopRec({ cancel: true })}>
                Cancel
              </button>
            ) : null}
            <button
              type="button"
              className={cn("inline-flex size-11 shrink-0 items-center justify-center rounded-[12px]", rec && "text-offline")}
              aria-label={rec ? "Stop recording" : "Voice"}
              onPointerDown={onMicDown}
              onPointerUp={onMicUp}
              onClick={() => {
                if (rec && !holdRef.current) stopRec();
              }}
            >
              {rec ? <Icon name="stop" className="size-5" /> : <Icon name="voice" className="size-5" />}
            </button>
            {canSend ? (
              <button
                type="button"
                className="relative z-30 inline-flex size-11 shrink-0 items-center justify-center rounded-[12px] bg-accent text-accent-fg"
                aria-label="Send"
                onClick={() => {
                  if (busy) return;
                  void sendRun(input, clips);
                }}
              >
                <Icon name="send" className="size-5" />
              </button>
            ) : null}
          </form>
        </div>
      </section>

      {sheet ? (
        <div className="fixed inset-0 z-50 flex items-end justify-center bg-black/40 sm:items-center" onClick={() => setSheet(null)}>
          <div
            className="max-h-[80dvh] w-full min-w-0 max-w-full overflow-x-hidden overflow-y-auto border border-line bg-surface p-4 pb-[calc(5.5rem+env(safe-area-inset-bottom))] sm:max-w-lg sm:rounded-[16px] sm:pb-4"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
          >
            <div className="mb-2 flex items-center justify-between">
              <p className="flex items-center gap-2 text-sm font-medium capitalize">
                <Icon
                  name={
                    sheet === "plus" ? "file"
                    : sheet === "models" ? "model"
                    : sheet === "skills" ? "skill"
                    : sheet === "tools" || sheet === "tool" ? "tools"
                    : sheet === "live" ? "settings"
                    : sheet === "data" ? "database"
                    : sheet === "activity" ? "activity"
                    : sheet === "slash" ? "search"
                    : "research"
                  }
                  className="size-4"
                />
                {sheet === "plus" ? "Add" : sheet === "tool" ? "Run" : sheet}
              </p>
              <button type="button" className="inline-flex size-11 items-center justify-center" aria-label="Close" onClick={() => setSheet(null)}>
                <X className="size-5" />
              </button>
            </div>
            {sheet === "hub" ? (
              <div className="mt-3 space-y-4">
                <div className="grid grid-cols-2 gap-2">
                  {(Object.keys(MODE_LABEL) as Mode[]).map((id) => (
                    <button
                      key={id}
                      type="button"
                      aria-pressed={mode === id}
                      onClick={() => {
                        setMode(id);
                        localStorage.setItem("galion-hermes-mode", id);
                      }}
                      className={cn(
                        "min-h-14 rounded-[12px] border text-left px-3",
                        mode === id ? "border-accent text-accent" : "border-line text-muted",
                      )}
                    >
                      <span className="block font-mono text-[11px] uppercase tracking-wider">{MODE_LABEL[id]}</span>
                    </button>
                  ))}
                </div>
                <button
                  type="button"
                  className="flex min-h-12 w-full items-center justify-between rounded-[12px] border border-line px-3 text-left"
                  onClick={() => {
                    setModelFor(mode);
                    setSheet("models");
                  }}
                >
                  <span className="font-mono text-[11px] uppercase tracking-wider text-subtle">Model</span>
                  <span className="truncate text-sm">{(() => {
                    const id = modes[mode] || "";
                    const named = catalog.find((row) => row.id === id)?.name;
                    const pretty = named?.includes(":") ? named.split(":").pop()?.trim() : named;
                    return pretty || id.split("/").pop() || "Pick";
                  })()}</span>
                </button>
                <button
                  type="button"
                  className="flex min-h-12 w-full items-center justify-between rounded-[12px] border border-line px-3 text-left"
                  onClick={() => setSheet("skills")}
                >
                  <span className="font-mono text-[11px] uppercase tracking-wider text-subtle">Skills</span>
                  <span className="text-sm">{skills.filter((s) => enabled[s.id]).length}</span>
                </button>
                {pin ? <p className="font-mono text-[11px] uppercase text-muted">{pin}</p> : null}
              </div>
            ) : null}
            {sheet === "plus" ? (
              <ul className="mt-3 space-y-1">
                <li>
                  <button
                    type="button"
                    className="flex min-h-11 w-full items-center gap-2 text-sm"
                    onClick={() => fileRef.current?.click()}
                  >
                    <Icon name="file" className="size-4" /> Photo or file
                  </button>
                </li>
                <li>
                  <button
                    type="button"
                    className="flex min-h-11 w-full items-center text-sm"
                    onClick={() => {
                      void fetch("/api/hermes/tui")
                        .then((r) => r.json())
                        .then((j: { slash?: { cmd: string; hint: string }[] }) => setSlash(j.slash ?? []))
                        .catch(() => setSlash([]));
                      setSheet("slash");
                    }}
                  >
                    Slash
                  </button>
                </li>
                <li>
                  <button type="button" className="flex min-h-11 w-full items-center text-sm" onClick={() => void admin("health")}>
                    Probe
                  </button>
                </li>
                <li>
                  <button type="button" className="flex min-h-11 w-full items-center text-sm" onClick={() => void admin("boot")}>
                    Boot
                  </button>
                </li>
                <li>
                  <button type="button" className="flex min-h-11 w-full items-center text-sm" onClick={() => { setSheet(null); void sendRun("Drain the request inbox."); }}>
                    Inbox
                  </button>
                </li>
                {lastRef.current ? (
                  <li>
                    <button type="button" className="flex min-h-11 w-full items-center text-sm" onClick={() => { setSheet(null); void sendRun(lastRef.current); }}>
                      Retry
                    </button>
                  </li>
                ) : null}
              </ul>
            ) : null}
            {sheet === "models" ? (
              <div className="mt-3">
                <div className="grid grid-cols-2 gap-2">
                  {(Object.keys(MODE_LABEL) as Mode[]).map((id) => {
                    const named = catalog.find((row) => row.id === modes[id])?.name;
                    const pretty = named?.includes(":") ? named.split(":").pop()?.trim() : named;
                    return (
                      <button
                        key={id}
                        type="button"
                        onClick={() => setModelFor(id)}
                        className={`min-h-14 rounded-[12px] border px-3 text-left ${modelFor === id ? "border-accent" : "border-line"}`}
                      >
                        <span className="block font-mono text-[11px] uppercase tracking-wider text-subtle">{MODE_LABEL[id]}</span>
                        <span className="mt-1 block truncate text-sm">{pretty || modes[id]?.split("/").pop() || "Pick"}</span>
                      </button>
                    );
                  })}
                </div>
                <p className="mt-3 text-sm text-muted">Tap a model. It is saved for {MODE_LABEL[modelFor]}.</p>
                <input
                  value={modelQuery}
                  onChange={(e) => setModelQuery(e.target.value)}
                  placeholder="Laptop model"
                  aria-label="Search models"
                  className="mt-3 h-11 w-full rounded-[12px] border border-line bg-bg px-3 text-sm"
                />
                <ul className="mt-3 space-y-1">
                  {(modelQuery.trim()
                    ? catalog.filter((row) => `${row.name} ${row.id}`.toLowerCase().includes(modelQuery.trim().toLowerCase())).slice(0, 40)
                    : [
                        ...catalog,
                      ].filter((row, i, all) => all.findIndex((n) => n.id === row.id) === i).slice(0, 24)
                  ).map((row) => {
                    const tags = (Object.keys(MODE_LABEL) as Mode[]).filter((id) => modes[id] === row.id);
                    const label = row.name.includes(":") ? row.name.split(":").pop()?.trim() : row.name;
                    return (
                      <li key={row.id}>
                        <button
                          type="button"
                          className={`flex min-h-11 w-full items-center justify-between gap-3 rounded-[12px] px-2 text-left ${modes[modelFor] === row.id ? "bg-elevated" : ""}`}
                          onClick={() => {
                            setModes((m) => ({ ...m, [modelFor]: row.id }));
                            void fetch("/api/hermes/modes", {
                              method: "POST",
                              headers: { "Content-Type": "application/json" },
                              body: JSON.stringify({ action: "assign", mode: modelFor, model: row.id }),
                            });
                          }}
                        >
                          <span className="min-w-0">
                            <span className="block truncate text-sm">{label}</span>
                            <span className="block truncate font-mono text-[11px] text-subtle">{row.id}</span>
                          </span>
                          {tags.length ? <span className="shrink-0 font-mono text-[11px] text-accent">{tags.map((id) => MODE_LABEL[id]).join(" · ")}</span> : null}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </div>
            ) : null}
            {sheet === "skills" ? (
              <div className="mt-3 space-y-4">
                {groups.map((g) => {
                  const rows = skills.filter((s) => (s.group ?? "Other") === g);
                  if (!rows.length) return null;
                  return (
                    <div key={g}>
                      <p className="font-mono text-[11px] uppercase text-subtle">{g}</p>
                      <ul>
                        {rows.map((s) => (
                          <li key={s.id} className="flex min-h-11 items-center justify-between gap-3 py-2">
                            <button
                              type="button"
                              className="min-w-0 flex-1 text-left"
                              onClick={() => {
                                setPin(s.id);
                                setSheet(null);
                              }}
                            >
                              <p className="text-sm">{s.name}</p>
                              <p className="truncate text-xs text-muted">{s.description}</p>
                            </button>
                            <button
                              type="button"
                              className={cn("h-8 rounded-full px-3 font-mono text-[11px]", enabled[s.id] ? "bg-elevated" : "text-muted")}
                              onClick={() => setEnabled((e) => ({ ...e, [s.id]: !e[s.id] }))}
                            >
                              {enabled[s.id] ? "on" : "off"}
                            </button>
                          </li>
                        ))}
                      </ul>
                    </div>
                  );
                })}
              </div>
            ) : null}
            {sheet === "tools" ? (
              <ul className="mt-3">
                {(toolRows.length ? toolRows : [{ id: "read", kind: "builtin", status: "held" }]).map((t) => (
                  <li key={`${t.kind}-${t.id}`} className="flex min-h-11 items-center justify-between text-sm">
                    <span>
                      {t.id}
                      <span className="ml-2 font-mono text-[11px] text-subtle">{t.kind}</span>
                    </span>
                    <StatusDot state={t.status === "up" ? "online" : "held"} />
                  </li>
                ))}
              </ul>
            ) : null}
            {sheet === "artifact" ? (
              <div className="mt-3">
                <CopyButton value={artifact} label="artifact" />
                <pre className="mt-3 max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-xs">{artifact}</pre>
              </div>
            ) : null}
            {sheet === "tool" && items[toolAt ?? -1]?.kind === "tool" ? (
              <ToolRun item={items[toolAt ?? -1] as Extract<Item, { kind: "tool" }>} />
            ) : null}
            {sheet === "slash" ? (
              <ul className="mt-3">
                {(slash.length ? slash : [{ cmd: "/health", hint: "gateway" }]).map((s) => (
                  <li key={s.cmd}>
                    <button
                      type="button"
                      className="flex min-h-11 w-full items-center justify-between text-left text-sm"
                      onClick={() => {
                        setInput(s.cmd + " ");
                        setSheet(null);
                        fieldRef.current?.focus();
                      }}
                    >
                      <span className="font-mono">{s.cmd}</span>
                      <span className="text-xs text-muted">{s.hint}</span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}
            {sheet === "live" ? (
              <div className="mt-3">
                <p className="font-mono text-[11px] text-subtle">{settings?.when || "Reading the pod."}</p>
                <div className="mt-3 flex flex-wrap gap-2">
                  {(["soul", "control", "skill", "modes"] as SettingFile[]).map((name) => (
                    <button
                      key={name}
                      type="button"
                      aria-pressed={settingFile === name}
                      className={cn(
                        "min-h-11 rounded-[12px] border px-3 font-mono text-[11px] uppercase",
                        settingFile === name ? "border-accent text-fg" : "border-line text-muted",
                      )}
                      onClick={() => {
                        setSettingFile(name);
                        setSettingText(settings?.files?.[name] ?? "");
                        setSettingMsg("");
                      }}
                    >
                      {name}
                    </button>
                  ))}
                </div>
                <textarea
                  value={settingText}
                  onChange={(e) => setSettingText(e.target.value)}
                  aria-label="Hermes file"
                  className="mt-3 h-40 w-full resize-y rounded-[12px] border border-line bg-bg p-3 font-mono text-xs"
                />
                <button
                  type="button"
                  className="mt-3 min-h-11 rounded-[12px] bg-accent px-4 text-sm text-accent-fg"
                  onClick={() => {
                    setSettingMsg("Saving…");
                    void fetch("/api/hermes/settings", {
                      method: "POST",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify({ file: settingFile, text: settingText }),
                    })
                      .then((r) => r.json())
                      .then((j: { ok?: boolean; error?: string }) => {
                        setSettingMsg(j.ok ? "Saved." : (j.error || "Not saved."));
                        if (j.ok) {
                          setSettings((cur) => cur ? { ...cur, files: { ...cur.files, [settingFile]: settingText } } : cur);
                        }
                      })
                      .catch(() => setSettingMsg("Not saved."));
                  }}
                >
                  Save
                </button>
                {settingMsg ? <p className="mt-2 text-sm text-muted">{settingMsg}</p> : null}
                <dl className="mt-4 grid grid-cols-2 gap-x-4">
                  {[
                    ["cash", settings?.openrouter?.cash_usd != null ? `$${settings.openrouter.cash_usd}` : "—"],
                    ["credits", String(settings?.openrouter?.total_credits ?? "—")],
                    ["used", settings?.openrouter?.total_usage != null ? `$${Number(settings.openrouter.total_usage).toFixed(2)}` : "—"],
                    ["key left", settings?.openrouter?.key_left != null ? `$${Number(settings.openrouter.key_left).toFixed(2)}` : "—"],
                    ["key cap", settings?.openrouter?.key_limit != null ? `$${settings.openrouter.key_limit}` : "—"],
                    ["model", String(settings?.lane?.action ?? settings?.live?.control?.model ?? "—")],
                    ["gpu free", settings?.gpu?.free_mib != null ? `${settings.gpu.free_mib} MiB` : "—"],
                    ["cpu", settings?.cpu?.load != null ? String(settings.cpu.load) : "—"],
                    ["disk", settings?.disk?.volume_used_pct != null ? `${settings.disk.volume_used_pct}%` : "—"],
                    ["runpod", settings?.runpod?.measured ? "measured" : (settings?.runpod?.reason ?? "unmeasured")],
                    ["revolut", settings?.revolut?.measured ? "measured" : (settings?.revolut?.reason ?? "unwired")],
                  ].map(([k, v]) => (
                    <div key={k} className="flex min-h-9 items-start justify-between gap-2 border-b border-line py-1.5 text-sm">
                      <dt className="font-mono text-[11px] uppercase text-subtle">{k}</dt>
                      <dd className="min-w-0 break-words text-right">{v}</dd>
                    </div>
                  ))}
                </dl>
              </div>
            ) : null}
            {sheet === "data" ? (
              <ul className="mt-3 space-y-3">
                {dbs.length === 0 ? <li className="text-sm text-muted">No databases on this desk.</li> : dbs.map((db) => (
                  <li key={db.file} className="rounded-[12px] border border-line px-3 py-2">
                    <p className="break-all text-sm">{db.file.split("/").slice(-2).join("/")}</p>
                    <p className="mt-1 text-xs text-muted">{db.error || `${db.bytes} bytes`}</p>
                    <ul className="mt-2">
                      {db.tables.map((table) => (
                        <li key={table.name} className="flex justify-between gap-3 text-xs text-muted">
                          <span>{table.name}</span>
                          <span>{table.rows}</span>
                        </li>
                      ))}
                    </ul>
                  </li>
                ))}
              </ul>
            ) : null}
            {sheet === "activity" ? (
              <ul className="mt-3 space-y-2">
                {activity.length === 0 ? (
                  <li className="text-sm text-muted">No activity in the last 24h.</li>
                ) : (
                  [...activity].reverse().map((a, i) => (
                    <li key={`${a.at}-${i}`} className="rounded-[12px] border border-line px-3 py-2">
                      <p className="font-mono text-[11px] uppercase text-subtle">
                        {a.kind} · {new Date(a.at).toISOString().slice(11, 16)}
                      </p>
                      <p className="mt-1 whitespace-pre-wrap break-words text-sm">{a.text}</p>
                    </li>
                  ))
                )}
              </ul>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
