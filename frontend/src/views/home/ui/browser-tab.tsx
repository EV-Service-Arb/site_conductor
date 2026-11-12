"use client";

import { Button } from "@evinvest/uikit";
import { Copy, Keyboard } from "lucide-react";

import { type Gmb, type Listed, categoryOf, googleName, useGmbTags } from "@/entities/gmb";
import { BrowserSession, type TypeIn } from "@/features/browser-session";
import { useT } from "@/shared/i18n";
import { EmptyState } from "@/shared/ui/empty-state";
import { useButtonSize } from "@/shared/ui/touch";

/** The GBP's gmail's browser, started at its point, with what Google's form asks beside it. */
export function BrowserTab({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  if (gmail === null) return <EmptyState title={t("window.browser.needsGmail")} description={t("window.browser.needsGmail.body")} />;
  return <BrowserSession gmail={gmail} gmb={gmb.id} aside={(type) => <FormValues gmb={gmb} type={type} />} />;
}

/** The GBP's values in the order Google's form asks them, each typed into the field focused in the browser. */
function FormValues({ gmb, type }: { gmb: Gmb; type: TypeIn }) {
  const t = useT();
  const { tags } = useGmbTags();
  const placed = tags.trades === null ? null : categoryOf(gmb, tags.trades);
  return (
    <aside className="flex w-full shrink-0 flex-col gap-3 rounded-lg border border-border p-4 xl:w-80">
      <header className="flex flex-col gap-0.5">
        <h3 className="text-sm font-semibold text-ink">{t("window.browser.form")}</h3>
        <p className="text-xs text-ink-soft">{t("window.browser.form.hint")}</p>
      </header>
      <ol className="flex flex-col gap-3">
        <Value label={t("launch.must.name")} value={gmb.name} type={type} />
        <Value label={t("launch.must.category")} value={placed?.status === "set_up" && placed.category.google_primary !== null ? googleName(placed.category.google_primary, placed.category.lang) : null} type={type} />
        <li className="flex flex-col gap-0.5 text-sm">
          <span className="text-xs text-ink-soft">{t("window.browser.visit")}</span>
          {gmb.customers_visit === null ? <span className="text-accent-warn">{t("window.browser.unset")}</span> : <span className="text-ink">{t(gmb.customers_visit ? "launch.visit.yes" : "launch.visit.no")}</span>}
        </li>
        {gmb.areas.length === 0 ? (
          <Value label={t("launch.must.areas")} value={null} type={type} />
        ) : (
          gmb.areas.map((a, i) => <Value key={a} label={`${t("launch.must.areas")} · ${i + 1}/${gmb.areas.length}`} value={a} type={type} />)
        )}
        <Value label={t("launch.must.phone")} value={gmb.phone} type={type} />
        <Value label={t("launch.must.hidden_address")} value={gmb.hidden_address} type={type} />
      </ol>
    </aside>
  );
}

function Value({ label, value, type }: { label: string; value: string | null; type: TypeIn }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <li className="flex flex-col gap-0.5 text-sm">
      <span className="flex items-center gap-1">
        <span className="flex-1 text-xs text-ink-soft">{label}</span>
        {value !== null && (
          <>
            <Button variant="outline" size={button("xs")} disabled={type === null} title={type === null ? t("window.browser.typeOff") : undefined} onClick={() => type?.(value)}>
              <Keyboard aria-hidden />
              {t("window.browser.type")}
            </Button>
            <Button variant="ghost" size={button("xs")} icon aria-label={t("window.browser.copy", { what: label })} title={t("window.browser.copy", { what: label })} onClick={() => void navigator.clipboard.writeText(value)}>
              <Copy aria-hidden />
            </Button>
          </>
        )}
      </span>
      {value === null ? <span className="text-accent-warn">{t("window.browser.unset")}</span> : <span className="break-words text-ink">{value}</span>}
    </li>
  );
}
