import { createFileRoute } from "@tanstack/react-router";
import { checkKey, listKeys, mintKey, revokeKey } from "@/lib/keys";

export const Route = createFileRoute("/api/v1/keys")({
  server: {
    handlers: {
      GET: async ({ request }) => {
        const { requireAdmin } = await import("@/lib/admin-device");
        const denied = await requireAdmin(request);
        if (denied) return denied;
        return Response.json({ ok: true, keys: listKeys() });
      },
      POST: async ({ request }) => {
        const { requireAdmin } = await import("@/lib/admin-device");
        const denied = await requireAdmin(request);
        if (denied) return denied;
        const body = (await request.json().catch(() => ({}))) as {
          action?: string;
          name?: string;
          scopes?: string[];
          id?: string;
          key?: string;
        };
        if (body.action === "revoke") return Response.json(revokeKey(body.id ?? ""));
        if (body.action === "check") return Response.json(checkKey(body.key ?? ""));
        return Response.json(mintKey(body.name ?? "", body.scopes));
      },
    },
  },
});
