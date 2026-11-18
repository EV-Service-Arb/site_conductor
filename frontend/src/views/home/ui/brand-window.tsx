"use client";

import { Button, ConfirmDialog, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger, Input, cn, toast } from "@evinvest/uikit";
import { Ellipsis, ImagePlus, ListFilter, MapPin, Plus, Tag, Trash2, Upload as UploadIcon, X } from "lucide-react";
import { useState } from "react";

import { brandKeys, chooseCategoryScope, chooseGmailScope, kit, logoSrc, move, open, openCategories, openTrade, photoMax, photoSrc, scopeKeys, trades as tradesKeys, uploadLogo, uploadPhoto, useBrands, useGmails, useGmbTags, windowKeys } from "@/entities/gmb";
import { Refused, api, ok } from "@/shared/api";
import type { Brand, Gmail, Gmb, KitPhoto, Trade } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { useResource } from "@evinvest/api/react";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
const HINT = "font-normal tracking-normal normal-case";

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

/** A brand's page (09h), under its category's tab: what every GBP of it shares, its photos, its GBPs. */
export function BrandWindow({ trade, brand, gmbs, gmails }: { trade: Trade; brand: Brand; gmbs: readonly Gmb[]; gmails: readonly Gmail[] }) {
  const t = useT();
  const button = useButtonSize();
  const gmail = gmails.find((g) => g.id === brand.gmail_id);
  const category = `${trade.name} · ${brand.country}`;
  const held = gmbs.filter((b) => b.brand_id === brand.id);
  const brandless = gmbs.filter((b) => b.brand_id === null && b.gmail_id === brand.gmail_id && b.trade_id === brand.trade_id && b.country?.code === brand.country);
  return (
    <section aria-label={brand.name} className="absolute inset-y-0 right-0 left-0 flex flex-col overflow-y-auto border-l border-border bg-background md:left-36">
      <header className="flex items-start gap-3 px-6 pt-4">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 className="flex min-w-0 items-center gap-2 text-lg font-semibold text-ink">
            <Tag aria-hidden className="size-4 shrink-0 text-ink-soft" />
            <button type="button" onClick={() => openCategories(true)} className="shrink-0 rounded font-medium text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
              {t(scopeKeys.categories, "Categories")}
            </button>
            <span aria-hidden className="text-ink-soft/60">/</span>
            <button type="button" onClick={() => openTrade(trade.id, brand.country)} className="shrink-0 rounded font-medium text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
              {category}
            </button>
            <span aria-hidden className="text-ink-soft/60">/</span>
            <span className="truncate">{brand.name}</span>
          </h2>
          <p className="text-sm text-ink-soft tabular-nums">{t(brandKeys.counts, "{gmail} · {gbps} GBPs", { gmail: gmail?.gmail ?? "", gbps: held.length })}</p>
        </div>
        <Button
          variant="outline"
          size={button("sm")}
          onClick={() => {
            chooseCategoryScope([{ trade: trade.id, country: brand.country }]);
            chooseGmailScope([brand.gmail_id]);
          }}
        >
          <ListFilter aria-hidden />
          {t(tradesKeys.window.scope, "Scope the outline to it")}
        </Button>
        <More brand={brand} />
        <Button variant="ghost" size={button("sm")} icon aria-label={t(windowKeys.close, "Close the window")} title={t(windowKeys.close, "Close the window")} onClick={() => openTrade(null)}>
          <X aria-hidden />
        </Button>
      </header>
      <div className="flex max-w-3xl flex-col gap-6 p-6">
        <Identity brand={brand} example={held[0]?.name} />
        <Photos brand={brand} category={category} />
        <section className={SECTION}>
          <h3 className={SECTION_HEAD}>
            {t(brandKeys.gbps, "GBPs")} <span className="font-normal tabular-nums">· {held.length}</span>
            <span className="flex-1" />
            <span className={HINT}>{t(brandKeys.gbps.whose, "{gmail}'s {category} GBPs", { gmail: gmail?.gmail ?? "", category })}</span>
          </h3>
          {held.length === 0 ? (
            <p className="text-sm text-ink-soft">{t(brandKeys.gbps.none, "No GBP holds it yet.")}</p>
          ) : (
            <span className="flex flex-wrap gap-1.5">
              {held.map((b) => (
                <button key={b.id} type="button" onClick={() => open(b)} className="flex items-center gap-1.5 rounded-md border border-border px-2 py-1 text-sm text-ink outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring">
                  <MapPin aria-hidden className="size-3.5 text-ink-soft" />
                  {b.name}
                </button>
              ))}
            </span>
          )}
          {brandless.map((b) => (
            <GiveIt key={b.id} gmb={b} brand={brand} gmail={gmail?.gmail ?? ""} category={category} />
          ))}
        </section>
      </div>
    </section>
  );
}

function More({ brand }: { brand: Brand }) {
  const t = useT();
  const button = useButtonSize();
  const brands = useBrands();
  const [removing, setRemoving] = useState(false);
  const title = t(brandKeys.remove, "Remove {brand}", { brand: brand.name });
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size={button("sm")} icon aria-label={t(brandKeys.more, "More")}>
            <Ellipsis aria-hidden />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem variant="destructive" disabled={brand.gmbs > 0} onClick={() => setRemoving(true)}>
            <Trash2 aria-hidden />
            <span className="flex flex-col">
              {title}
              {brand.gmbs > 0 && <span className="text-xs text-accent-warn">{t(brandKeys.remove.held, "{n} GBPs hold it: give them another first", { n: brand.gmbs })}</span>}
            </span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <ConfirmDialog
        open={removing}
        onOpenChange={setRemoving}
        tone="error"
        title={title}
        description={t(brandKeys.remove.body, "Its photos leave every offer. The GBPs they went to keep them as used.")}
        confirmLabel={t(kit.remove, "Remove")}
        labels={{ cancel: t(move.cancel, "Cancel") }}
        onConfirm={async () => {
          await ok(api.DELETE("/brands/{id}", { params: { path: { id: brand.id } } }));
          openTrade(brand.trade_id, brand.country);
          brands.reload();
        }}
      />
    </>
  );
}

function Identity({ brand, example }: { brand: Brand; example: string | undefined }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const brands = useBrands();
  const sites = useSites();
  const [name, setName] = useState(brand.name);
  const [site, setSite] = useState(brand.site);
  const [refused, setRefused] = useState<string | null>(null);
  const suffix = example?.startsWith(brand.name) === true ? example.slice(brand.name.length).trim() : "· …";
  const save = async (body: { name?: string; site?: string }) => {
    try {
      await ok(api.PATCH("/brands/{id}", { params: { path: { id: brand.id } }, body }));
      setRefused(null);
      brands.reload();
    } catch (e) {
      if (!(e instanceof Refused)) throw e;
      if (e.failure.code !== "unauthenticated") setRefused(failureText(e.failure, t));
    }
  };
  return (
    <section className={SECTION}>
      <h3 className={SECTION_HEAD}>
        {t(brandKeys.identity, "Identity")}
        <span className="flex-1" />
        <span className={HINT}>{t(brandKeys.identity.what, "what every GBP of it shares")}</span>
      </h3>
      <div className="flex items-center gap-4">
        <BrandMark brand={brand} className="size-14 text-2xl" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="text-sm font-medium text-ink">{t(brandKeys.logo, "Logo")}</span>
          <span className="text-xs text-ink-soft">{t(brandKeys.logo.what, "on the review kit · square, PNG, JPEG or WebP")}</span>
        </span>
        <label className={cn("inline-flex cursor-pointer items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-sm text-ink hover:bg-hover focus-within:ring-2 focus-within:ring-ring")}>
          <UploadIcon aria-hidden className="size-4" />
          {brand.logo === null ? t(brandKeys.logo.upload, "Upload") : t(brandKeys.logo.replace, "Replace")}
          <input
            type="file"
            accept="image/*"
            className="sr-only"
            onChange={(e) => {
              const file = e.target.files?.[0];
              e.target.value = "";
              if (file !== undefined)
                void uploadLogo(brand.id, file)
                  .then(brands.reload)
                  .catch((err: unknown) => notifyFailure(err, t));
            }}
          />
        </label>
      </div>
      <label className="flex flex-col gap-1">
        <span className="flex items-baseline text-sm text-ink">
          {t(brandKeys.name, "Name")}
          <span className="flex-1" />
          <span className="text-xs text-ink-soft">{t(brandKeys.name.what, "every GBP's name starts with it")}</span>
        </span>
        <span className="flex items-center gap-2">
          <Input size={size} value={name} onChange={(e) => setName(e.target.value)} onBlur={() => name.trim() !== brand.name && void save({ name: name.trim() })} className="min-w-0 flex-1" />
          <span className="shrink-0 text-sm text-ink-soft">{suffix}</span>
        </span>
      </label>
      <label className="flex flex-col gap-1">
        <span className="flex items-baseline text-sm text-ink">
          {t(brandKeys.site, "Site")}
          <span className="flex-1" />
          <span className="text-xs text-ink-soft">{t(brandKeys.site.what, "its GBPs' pages and QRs are on it")}</span>
        </span>
        <span className="flex items-center gap-2">
          <Input size={size} list={`sites-${brand.id}`} value={site} onChange={(e) => setSite(e.target.value)} className="min-w-0 flex-1 font-mono" />
          <datalist id={`sites-${brand.id}`}>
            {sites.map((s) => (
              <option key={s} value={s} />
            ))}
          </datalist>
          <Button variant="outline" size={button("sm")} disabled={site.trim() === brand.site || site.trim() === ""} onClick={() => void save({ site: site.trim() })}>
            {t(kit.save, "Save")}
          </Button>
        </span>
      </label>
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
    </section>
  );
}

function Photos({ brand, category }: { brand: Brand; category: string }) {
  const t = useT();
  const brands = useBrands();
  const photos = useResource(api, "/brands/{id}/photos", { params: { path: { id: brand.id } } });
  const reload = () => {
    photos.reload();
    brands.reload();
  };
  const list = photos.status === "ok" ? [...photos.data.photos].sort((a, b) => a.uses - b.uses) : [];
  return (
    <section className={SECTION}>
      <h3 className={SECTION_HEAD}>
        {t(kit.photos, "Photos")} <span className="font-normal tabular-nums">· {list.length}</span>
        <span className="flex-1" />
        <span className={HINT}>{t(brandKeys.photos.order, "least used first · a GBP gets each at most once")}</span>
      </h3>
      <ul className="flex flex-wrap gap-2">
        <Upload brand={brand.id} reload={reload} />
        {list.map((p) => (
          <PhotoTile key={p.id} brand={brand.id} photo={p} reload={reload} />
        ))}
      </ul>
      <p className="text-xs text-ink-soft">{t(brandKeys.photos.why, "The review kit offers them with {category}'s drafts. A GBP without a brand is offered drafts only.", { category })}</p>
    </section>
  );
}

function PhotoTile({ brand, photo, reload }: { brand: number; photo: KitPhoto; reload: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const title = t(kit.photos.remove, "Remove the photo");
  const [shows, setShows] = useState(photo.shows ?? "");
  const label = () =>
    shows.trim() !== (photo.shows ?? "") &&
    void ok(api.PATCH("/brands/{id}/photos/{file}", { params: { path: { id: brand, file: `${photo.id}.jpg` } }, body: { shows: shows.trim() } })).then(reload, (e: unknown) => notifyFailure(e, t));
  return (
    <li className="group relative flex w-14 flex-col gap-1">
      {/* eslint-disable-next-line @next/next/no-img-element -- the API's own JPEG, behind the session */}
      <img src={photoSrc(brand, photo.id)} alt="" loading="lazy" className="size-14 rounded-md object-cover" />
      <span className="absolute bottom-1 left-1 rounded bg-black/70 px-1 font-mono text-xs text-white tabular-nums">{photo.uses}×</span>
      <span className="absolute top-0 right-0 hidden rounded-md bg-background/80 group-focus-within:block group-hover:block">
        <ConfirmDialog
          trigger={
            <Button variant="ghost" size={button("sm")} icon aria-label={title} title={title}>
              <Trash2 aria-hidden />
            </Button>
          }
          tone="error"
          title={title}
          description={t(kit.remove.body, "Out of every offer from now on. The GBPs it went to keep it as used.")}
          confirmLabel={t(kit.remove, "Remove")}
          labels={{ cancel: t(move.cancel, "Cancel") }}
          onConfirm={async () => {
            await ok(api.DELETE("/brands/{id}/photos/{file}", { params: { path: { id: brand, file: `${photo.id}.jpg` } } }));
            reload();
          }}
        />
      </span>
      <Input
        size="sm"
        value={shows}
        maxLength={60}
        onChange={(e) => setShows(e.target.value)}
        onBlur={label}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        placeholder={t(brandKeys.photos.shows, "shows…")}
        aria-label={t(brandKeys.photos.showsLabel, "What it shows: its caption for the reviewer")}
        title={t(brandKeys.photos.showsLabel, "What it shows: its caption for the reviewer")}
        className="h-6 px-1 text-xs"
      />
    </li>
  );
}

/** One request per photo, in turn; one past the server's size is refused here, the rest still go. */
function Upload({ brand, reload }: { brand: number; reload: () => void }) {
  const t = useT();
  const [sent, setSent] = useState<number | null>(null);
  const upload = async (files: File[]) => {
    for (const [i, file] of files.entries()) {
      const max = photoMax(file.type);
      if (max !== undefined && file.size > max) {
        toast.error(t(kit.photos.tooLarge, "{name}: a photo is {mib} MiB at most", { name: file.name, mib: max / 2 ** 20 }));
        continue;
      }
      try {
        await uploadPhoto(brand, file, (f) => setSent((i + f) / files.length));
      } catch (e) {
        notifyFailure(e, t);
      }
    }
    setSent(null);
    reload();
  };
  return (
    <li>
      <label className={cn("flex size-14 cursor-pointer flex-col items-center justify-center gap-0.5 rounded-md border border-dashed border-border text-xs text-ink-soft hover:bg-hover hover:text-ink focus-within:ring-2 focus-within:ring-ring", sent !== null && "pointer-events-none")}>
        <ImagePlus aria-hidden className="size-4" />
        {sent === null ? t(kit.photos.upload, "Upload") : `${Math.round(sent * 100)}%`}
        <input
          type="file"
          accept="image/*"
          multiple
          className="sr-only"
          onChange={(e) => {
            const files = [...(e.target.files ?? [])];
            e.target.value = "";
            if (files.length > 0) void upload(files);
          }}
        />
      </label>
    </li>
  );
}

function GiveIt({ gmb, brand, gmail, category }: { gmb: Gmb; brand: Brand; gmail: string; category: string }) {
  const t = useT();
  const button = useButtonSize();
  const brands = useBrands();
  const gmails = useGmails();
  return (
    <p className="flex items-center gap-3 rounded-lg border border-accent-warn/40 bg-accent-warn/5 px-3 py-2 text-sm text-ink">
      <span className="min-w-0 flex-1">{t(brandKeys.brandless, "{gbp} is {gmail}'s and {category}, without a brand", { gbp: gmb.name, gmail, category })}</span>
      <Button
        variant="outline"
        size={button("sm")}
        onClick={() =>
          void ok(api.PUT("/gmbs/brand", { body: { gmbs: [gmb.id], brand: brand.id } }))
            .then(() => {
              gmails.reload();
              brands.reload();
            })
            .catch((e: unknown) => notifyFailure(e, t))
        }
      >
        <Plus aria-hidden />
        {t(brandKeys.give, "Give it {brand}", { brand: brand.name })}
      </Button>
    </p>
  );
}
