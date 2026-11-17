import type { NextConfig } from "next";
import { PHASE_DEVELOPMENT_SERVER } from "next/constants";

/**
 * The panel is a static export of one page, which the Rust binary serves for every path that is a
 * view (`VIEWS`), next to `/api` and `/auth` on one origin (see README.md, "How it is served").
 *
 * `next dev` instead proxies the backend's prefixes to a running one, so the cookies and the CSRF
 * header behave as in production, and answers every other path with the page.
 */
export default function config(phase: string): NextConfig {
  if (phase === PHASE_DEVELOPMENT_SERVER) {
    const backend = process.env.PANEL_DEV_BACKEND;
    if (!backend) throw new Error("PANEL_DEV_BACKEND: the origin `next dev` proxies to, `panel serve`'s");
    return {
      // Next refuses a second `next dev` on one dist dir: a stack on ports of its own (`dev --port`) runs beside the usual one.
      ...(process.env.PANEL_DEV_DIST !== undefined && { distDir: process.env.PANEL_DEV_DIST }),
      // Next writes AGENTS.md and CLAUDE.md into the project on `next dev` otherwise.
      agentRules: false,
      // A trailing-slash redirect would run before the rewrite and bounce /api/v1/me.
      skipTrailingSlashRedirect: true,
      // a study's Pull buys its whole sweep in one request, minutes long; Next's proxy cuts at 30 s
      experimental: { proxyTimeout: 15 * 60_000 },
      async rewrites() {
        // the backend's prefixes; every other path is the page, which parses it
        const afterFiles = [
          ...["/api", "/auth", "/mcp", "/.well-known", "/mfe", "/grafana"].map((p) => ({ source: `${p}/:path*`, destination: `${backend}${p}/:path*` })),
          { source: "/mcp", destination: `${backend}/mcp` },
          { source: "/grafana/", destination: `${backend}/grafana/` }, // `:path*` empty drops the slash: /grafana, unrouted
        ];
        return { beforeFiles: [], afterFiles, fallback: [{ source: "/:path*", destination: "/" }] };
      },
    };
  }
  return {
    output: "export",
    images: { unoptimized: true },
  };
}
