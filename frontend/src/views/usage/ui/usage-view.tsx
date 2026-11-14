"use client";

import { Card, CardContent, CardHeader, CardTitle, PageFrame, Skeleton } from "@evinvest/uikit";

import { type Permission, useMe } from "@/entities/session";
import { fetchArchive } from "@/entities/gmb";
import { UsageFigures, fetchUsage } from "@/entities/tokens";
import { ROUTES } from "@/shared/config/routes";
import { type T, useT } from "@/shared/i18n";
import { useResource } from "@/shared/lib/use-resource";

/** Who grants it, by a permission's second segment: the service, or the conductor's own admin. */
const SECTION: Record<string, (t: T) => string> = {
  sites: (t) => t("Sites"),
  admin: (t) => t("Administration"),
  review_archive: (t) => t("Review Archive"),
  playbook: (t) => t("Playbook"),
};

function sectionOf(p: Permission): (t: T) => string {
  const key = SECTION[p.split(":")[1] ?? ""];
  if (key === undefined) throw new Error(`no section for ${p}`);
  return key;
}

/** What the signed-in account may do and has spent on Service-Arb; who they are is the Profile screen. */
export function UsageView() {
  const t = useT();
  return (
    <PageFrame breakpoint="md" title={t("Usage")} description={t("Your access, tokens and spending on Service-Arb.")} width="content">
      <Access />
      <UsageCard />
      <Section title={t("Integrations")}>
        <a className="text-sm text-ink hover:underline" href={ROUTES.alerts}>
          {t("Alerts")}
        </a>
        <p className="text-sm text-ink-soft">{t("Where your alerts go: your Telegram, channels and groups.")}</p>
      </Section>
    </PageFrame>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-xs font-medium uppercase tracking-wide text-ink-soft">{title}</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-2 px-4">{children}</CardContent>
    </Card>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-(--grid-account-row) items-baseline gap-3 text-sm">
      <span className="text-ink-soft">{label}</span>
      <span className="min-w-0 text-ink">{children}</span>
    </div>
  );
}

function Access() {
  const t = useT();
  const me = useMe();
  const sections = new Map<(t: T) => string, Permission[]>();
  for (const p of me.permissions) sections.set(sectionOf(p), [...(sections.get(sectionOf(p)) ?? []), p]);
  return (
    <Section title={t("Access")}>
      {sections.size === 0 && <p className="text-sm text-ink-soft">{t("No Service-Arb permissions yet.")}</p>}
      {[...sections].map(([section, permissions]) => (
        <Row key={section(t)} label={section(t)}>
          <span className="flex flex-wrap gap-x-3 gap-y-1">
            {permissions.map((p) => (
              <code key={p} className="text-xs">
                {p}
              </code>
            ))}
          </span>
        </Row>
      ))}
    </Section>
  );
}

function UsageCard() {
  const t = useT();
  const usage = useResource("account.usage", fetchUsage);
  const archive = useResource("account.archive", fetchArchive);
  return (
    <Section title={t("Usage")}>
      {usage.status === "loading" && <Skeleton className="h-24 w-full" />}
      {usage.status === "error" && <p className="text-sm text-ink-soft">{t("Usage can't be shown right now.")}</p>}
      {usage.status === "ok" && <UsageFigures days={usage.data} />}
      <Row label={t("Places tracked now")}>
        <span className="tabular-nums">{archive.status === "ok" && archive.data !== null ? archive.data.places_tracked : "—"}</span>
      </Row>
    </Section>
  );
}
