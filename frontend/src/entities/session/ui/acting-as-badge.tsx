"use client";

import { Badge, cn } from "@evinvest/uikit";

import { api, ok } from "@/shared/api";
import { useT } from "@/shared/i18n";

import { useMe } from "../model/context";
import { session } from "../model/i18n";

/** Shown while acting in another's account; pressing it returns to one's own. */
export function ActingAsBadge({ className }: { className?: string }) {
  const t = useT();
  const { acting } = useMe();
  if (!acting) return null;
  return (
    <button type="button" onClick={() => void stopActing()} className={cn("inline-flex", className)}>
      <Badge tone="warn" title={t(session.acting.hint, "Signed in as {email}, viewing this account with your permissions. Press to stop.", { email: acting.email })} className="uppercase tracking-wide">
        {t(session.acting, "acting as")}
      </Badge>
    </button>
  );
}

// A reload, not a re-render: the live socket, caches and the map's session are the identity's.
export async function stopActing() {
  await ok(api.DELETE("/me/acting"));
  location.assign(location.origin);
}
