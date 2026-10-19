import { createFileRoute } from "@tanstack/react-router";
import { withHermesUser } from "@/lib/hermes-guard";
import { hermesChat } from "@/lib/hermes";

export const Route = createFileRoute("/api/hermes/chat")({
  server: {
    handlers: {
      POST: async ({ request }) =>
        withHermesUser(request, async (userId) => {
          const body = (await request.json().catch(() => ({}))) as {
            messages?: { role: string; content: string }[];
            sessionId?: string;
            projectId?: string;
            lane?: string;
            mode?: string;
          };
          const messages = Array.isArray(body.messages) ? body.messages : [];
          if (!messages.length) {
            return Response.json({ ok: false, error: "messages required" }, { status: 400 });
          }
          return hermesChat({
            messages: messages.slice(-24),
            sessionId: body.sessionId || crypto.randomUUID(),
            projectId: body.projectId || "galion-light",
            userId,
            lane: body.lane,
            mode: body.mode,
          });
        }),
    },
  },
});
