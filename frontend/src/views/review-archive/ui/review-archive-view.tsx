"use client";

import { useSyncExternalStore } from "react";
import { PANEL } from "@config";

import { SIGN_IN_PATH, csrfCookieName } from "@/shared/api";
import type { PATHS } from "@/shared/api/paths";
import { useT, reviewArchive } from "@/shared/i18n";
import { RemoteElement } from "@/shared/mfe";

const never = () => () => {};
const GRAFANA: (typeof PATHS)[number] = "/grafana";

/** review_archive's dashboard, its API and bundle forwarded by the panel (docs/ARCHITECTURE.md, Forward). */
export function ReviewArchiveView() {
  const t = useT();
  const csrf = useSyncExternalStore(never, csrfCookieName, () => null);
  if (csrf === null) return null;
  return (
    <RemoteElement
      className="h-full"
      tag="mfe-review-archive-dashboard"
      scriptUrl="/mfe/review_archive/mfe-review-archive-dashboard.js"
      attributes={{ api: "/api/review_archive", "sign-in": SIGN_IN_PATH, "csrf-cookie": csrf, base: "/review_archive", grafana: `${GRAFANA}/`, ui: PANEL.reviewArchiveUi }}
      fallback={<p className="p-6 text-muted-foreground">{t(reviewArchive.loading, "Loading the review archive…")}</p>}
    />
  );
}
