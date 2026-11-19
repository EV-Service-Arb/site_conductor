"use client";

import { Button, Input, cn } from "@evinvest/uikit";
import { Plus } from "lucide-react";
import { useState } from "react";

import { api, ok } from "@/shared/api";
import type { Brand, Country } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { logoSrc } from "../api/kit";
import { useBrands, useGmbTags } from "../model/gmails-context";
import { brandKeys } from "../model/i18n";

/** A brand's logo, or its initial while it has none. */
export function BrandMark({ brand, className }: { brand: Brand; className?: string }) {
  if (brand.logo !== null)
    // eslint-disable-next-line @next/next/no-img-element -- the API's own PNG, behind the session
    return <img src={logoSrc(brand.id, brand.logo)} alt="" className={cn("size-6 shrink-0 rounded-md object-contain", className)} />;
  return <span className={cn("flex size-6 shrink-0 items-center justify-center rounded-md bg-accent-info font-semibold text-white", className)}>{brand.name.slice(0, 1).toUpperCase()}</span>;
}

/** The sites the caller's GBPs' pages are on, as sites' places name them: what a brand's site is picked from. */
export function useSites(): string[] {
  const { sites } = useGmbTags();
  return sites.status === "ok" ? [...new Set(Object.values(sites.data).map((p) => p.brand))].sort() : [];
}

/** A new brand under `gmail` in `trade` · `country`. */
export function NewBrand({ gmail, trade, country, onDone }: { gmail: number; trade: number; country: Country; onDone: (added: Brand | null) => void }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const brands = useBrands();
  const sites = useSites();
  const [name, setName] = useState("");
  const [site, setSite] = useState("");
  const [busy, setBusy] = useState(false);
  const list = `sites-new-${gmail}-${trade}-${country}`;
  return (
    <form
      className="flex items-center gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        void ok(api.POST("/brands", { body: { gmail, trade, country, name: name.trim(), site: site.trim() } }))
          .then((added) => {
            brands.reload();
            onDone(added);
          })
          .catch((err: unknown) => notifyFailure(err, t))
          .finally(() => setBusy(false));
      }}
      onKeyDown={(e) => e.key === "Escape" && onDone(null)}
    >
      <Input size={size} autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder={t(brandKeys.new.name, "A brand's name: Aquafix")} aria-label={t(brandKeys.new.name, "A brand's name: Aquafix")} className="min-w-0 flex-1" />
      <Input size={size} list={list} value={site} onChange={(e) => setSite(e.target.value)} placeholder={t(brandKeys.new.site, "its site: aquafix")} aria-label={t(brandKeys.new.site, "its site: aquafix")} className="w-32 font-mono" />
      <datalist id={list}>
        {sites.map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
      <Button type="submit" variant="outline" size={button("sm")} disabled={busy || name.trim() === "" || site.trim() === ""}>
        <Plus aria-hidden />
        {t(brandKeys.new.add, "Add the brand")}
      </Button>
    </form>
  );
}
