import { createFileRoute } from "@tanstack/react-router";
import { withHermesUser } from "@/lib/hermes-guard";
import { startHermesRun } from "@/lib/hermes";

export const Route = createFileRoute("/api/hermes/runs")({
  server: {
    handlers: {
      POST: async ({ request }) =>
        withHermesUser(request, async (userId) => {
          const body = (await request.json().catch(() => ({}))) as {
            sessionId?: string;
            projectId?: string;
            input?: string;
            lane?: string;
            skillHint?: string;
            model?: string;
            provider?: string;
            mode?: string;
            messages?: { role: string; content: string }[];
          };
          const input =
            body.input?.trim() ||
            [...(body.messages ?? [])].reverse().find((m) => m.role === "user")?.content?.trim() ||
            "";
          if (!input) return Response.json({ error: "input required" }, { status: 400 });
          return startHermesRun({
            userId,
            projectId: body.projectId || "galion-light",
            sessionId: body.sessionId,
            input,
            lane: body.lane,
            skillHint: body.skillHint,
            model: body.model,
            provider: body.provider === "laptop" || body.provider === "openrouter" || body.provider === "river" ? body.provider : undefined,
            mode: body.mode,
          });
        }),
    },
  },
});
