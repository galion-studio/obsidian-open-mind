import { TARGET } from "./fleet";
import { TUNNEL_CNAME, TUNNEL_ID } from "./probe";

export type DnsRecord = {
  type: "A" | "CNAME";
  name: string;
  host: string;
  proxied: boolean;
};

export type CfZone = {
  zone: string;
  records: DnsRecord[];
};

function cname(zone: string, name: string, host: string): DnsRecord {
  return { type: "CNAME", name, host, proxied: true };
}

export const CF_ZONES: CfZone[] = [
  {
    zone: "galion.studio",
    records: ["@", "www", "api", "docs", "dashboard", "console"].map((name) =>
      cname("galion.studio", name, name === "@" ? "galion.studio" : `${name}.galion.studio`),
    ),
  },
  {
    zone: "galion.app",
    records: ["@", "www", "api", "developer", "status", "console"].map((name) =>
      cname("galion.app", name, name === "@" ? "galion.app" : `${name}.galion.app`),
    ),
  },
  {
    zone: "merlinelement.com",
    records: [
      cname("merlinelement.com", "@", "merlinelement.com"),
      cname("merlinelement.com", "www", "www.merlinelement.com"),
    ],
  },
  {
    zone: "sentialink.com",
    records: [
      cname("sentialink.com", "@", "sentialink.com"),
      cname("sentialink.com", "www", "www.sentialink.com"),
    ],
  },
];

export const CF_STEPS = [
  "Do not change DNS. Records already point at the old tunnels.",
  "Reconnect the existing tunnels from nexuslang-v2 docker-compose.cloudflare.yml (studio + app tokens). Do not create a new tunnel.",
  "Origin listens on the old ingress ports: 3100, 3200, 8080, 8100, 8200.",
  "SSL stays as it was (Flexible or Full). merlinelement.com is not in git — its tunnel lives only in Zero Trust.",
  "Expect Soon-page or console, not 1033.",
] as const;

export function runpodProxyHint() {
  return `https://${TARGET.podId}-${TARGET.originPort}.proxy.runpod.net`;
}

export function tunnelHint() {
  return { id: TUNNEL_ID, cname: TUNNEL_CNAME };
}
