import { createFileRoute } from "@tanstack/react-router";
import { closeSync, existsSync, openSync, readFileSync, readSync, statSync } from "node:fs";

const LIVE = [
  "/root/galion-secrets/github.token",
  "/root/galion-secrets/galion-app.token",
  "/root/galion-secrets/hermes.key",
  "/root/galion-secrets/hermes.env",
];
const VOLUME = [
  "/workspace/galion-light/secrets/github.token",
  "/workspace/galion-light/secrets/galion-app.token",
  "/workspace/galion-light/secrets/hermes.key",
  "/workspace/galion-light/secrets/github_ed25519",
  "/workspace/hermes/.env",
];

function head(path: string) {
  const fd = openSync(path, "r");
  try {
    const buf = Buffer.alloc(8);
    const n = readSync(fd, buf, 0, 8, 0);
    return buf.subarray(0, n).toString();
  } finally {
    closeSync(fd);
  }
}

function liveRows(paths: string[]) {
  return paths.map((path) => {
    if (!existsSync(path)) return { path, mode: "missing", ok: false };
    const mode = (statSync(path).mode & 0o777).toString(8).padStart(3, "0");
    return { path, mode, ok: mode === "400" || mode === "600" };
  });
}

function volumeRows(paths: string[]) {
  return paths.map((path) => {
    if (!existsSync(path)) return { path, mode: "missing", ok: false };
    const sealed = head(path).startsWith("SEALED");
    return { path, mode: sealed ? "sealed" : "open", ok: sealed };
  });
}

export const Route = createFileRoute("/api/v1/security")({
  server: {
    handlers: {
      GET: async ({ request }) => {
        const { requireAdmin } = await import("@/lib/admin-device");
        const denied = await requireAdmin(request);
        if (denied) return denied;
        let denylist = true;
        let autoAccept = false;
        try {
          const j = JSON.parse(readFileSync("/workspace/hermes/guard.json", "utf8")) as { on?: boolean; autoAccept?: boolean };
          denylist = j.on !== false;
          autoAccept = j.autoAccept === true;
        } catch {
          denylist = true;
        }
        return Response.json({
          ok: true,
          denylist,
          autoAccept,
          floorAlways: true,
          historyRedaction: true,
          auth: false,
          floor: [
            "gateway stop, kill, or freeze of the watchdog",
            "secret files and the OpenRouter key",
            "editing the guard or its patterns",
            "/etc, docker.sock, libsqlite, reboot",
          ],
          files: liveRows(LIVE),
          volume: volumeRows(VOLUME),
        });
      },
    },
  },
});
