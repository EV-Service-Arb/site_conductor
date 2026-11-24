"use client";

import { Empty, EmptyHeader, EmptyTitle } from "@evinvest/uikit";

import { nav, useT } from "@/shared/i18n";

/** Google's mail about the caller's GBPs, as cases. */
export function EmailView() {
  const t = useT();
  return (
    <Empty className="min-h-full">
      <EmptyHeader>
        <EmptyTitle>{t(nav.email, "Email")}</EmptyTitle>
      </EmptyHeader>
    </Empty>
  );
}
