"use client";

import { Badge, cn } from "@evinvest/uikit";

import { useT } from "@/shared/i18n";

import { useMe } from "../model/context";
import { session } from "../model/i18n";

/** Loud on purpose: a backend that signs anyone in must never pass for the real one. */
export function DevSignInBadge({ className }: { className?: string }) {
  const t = useT();
  const { dev_sign_in } = useMe();
  if (!dev_sign_in) return null;
  return (
    <Badge tone="warn" title={t(session.devSignIn.hint, "This backend signs people in without concierge. It is for local work only.")} className={cn("uppercase tracking-wide", className)}>
      {t(session.devSignIn, "dev sign-in")}
    </Badge>
  );
}
