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
  if (gmail === null) return <EmptyState title={t("Needs a gmail")} description={t("A GBP's browser is its managing gmail's: pick one on the Launch tab.")} />;
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
        <h3 className="text-sm font-semibold text-ink">{t("Google's form")}</h3>
        <p className="text-xs text-ink-soft">{t("Click the field in the page, then type its value in.")}</p>
      </header>
      <ol className="flex flex-col gap-3">
        <Value label={t("Name")} value={gmb.name} type={type} />
        <Value label={t("Category")} value={placed?.status === "set_up" && placed.category.google_primary !== null ? googleName(placed.category.google_primary, placed.category.lang) : null} type={type} />
        <li className="flex flex-col gap-0.5 text-sm">
          <span className="text-xs text-ink-soft">{t("Customers visit?")}</span>
          {gmb.customers_visit === null ? <span className="text-accent-warn">{t("not set")}</span> : <span className="text-ink">{(gmb.customers_visit ? t("Customers visit") : t("Customers don't visit"))}</span>}
        </li>
        {gmb.areas.length === 0 ? (
          <Value label={t("Areas")} value={null} type={type} />
        ) : (
          gmb.areas.map((a, i) => <Value key={a} label={`${t("Areas")} · ${i + 1}/${gmb.areas.length}`} value={a} type={type} />)
        )}
        <Value label={t("Phone")} value={gmb.phone} type={type} />
        <Value label={t("Address")} value={gmb.hidden_address} type={type} />
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
            <Button variant="outline" size={button("xs")} disabled={type === null} title={type === null ? t("Open the browser first") : undefined} onClick={() => type?.(value)}>
              <Keyboard aria-hidden />
              {t("Type into the field")}
            </Button>
            <Button variant="ghost" size={button("xs")} icon aria-label={t("Copy {what}", { what: label })} title={t("Copy {what}", { what: label })} onClick={() => void navigator.clipboard.writeText(value)}>
              <Copy aria-hidden />
            </Button>
          </>
        )}
      </span>
      {value === null ? <span className="text-accent-warn">{t("not set")}</span> : <span className="break-words text-ink">{value}</span>}
    </li>
  );
}
