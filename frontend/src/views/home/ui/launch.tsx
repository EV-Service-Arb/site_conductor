"use client";

import { Button, Input, NativeSelect, NativeSelectOption, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { Check, Copy, X } from "lucide-react";
import { type ReactNode, useState } from "react";

import { CategoryChip, categoryName, categoryOf, COUNTRIES, type Country, editGmb, GMAIL_ROLE_LABEL, type Gmb, type GmbEdit, type GoogleCategory, googleName, type Listed, moveGmb, MUST_HAVES, type MustHave, openAccount, openTrade, type Placed, readiness, setCountry, TradeSelect, under, useGmails, useGmbTags } from "@/entities/gmb";
import { may, useMe } from "@/entities/session";
import { LinkPlaceButton } from "@/features/link-gmb-place";
import { PlaceButton } from "@/features/place-gmb";
import { ApiError } from "@/shared/api";
import { type T, useLocale, useT } from "@/shared/i18n";
import { failureText } from "@/shared/ui/failure-text";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const MAX_AREAS = 20;
const MUST = {
  name: (t) => t("Name"),
  category: (t) => t("Category"),
  areas: (t) => t("Areas"),
  phone: (t) => t("Phone"),
  site: (t) => t("Site"),
  hidden_address: (t) => t("Address"),
  film: (t) => t("Film"),
  gmail: (t) => t("Gmail"),
  profile: (t) => t("Browser"),
} satisfies Record<MustHave, (t: T) => string>;

/** A draft's tab: what Google's form asks, in its order, then what stands behind it; and the move onto Google once the form is sent. */
export function Launch({ listed: { gmb, gmail } }: { listed: Listed }) {
  const t = useT();
  const locale = useLocale();
  const me = useMe();
  const gmails = useGmails();
  const { sites, tags } = useGmbTags();
  const rows = readiness({ gmb, gmail }, tags);
  const placed = tags.trades === null ? null : categoryOf(gmb, tags.trades);
  const ready = rows.filter((r) => r.ready).length;
  const missing = rows.filter((r) => !r.ready);
  const button = useButtonSize();
  const [failed, setFailed] = useState<{ must: MustHave; text: string } | null>(null);

  const save = (must: MustHave) => async (edit: GmbEdit) => {
    try {
      await editGmb(gmb.id, edit);
      setFailed(null);
      gmails.reload();
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      if (e.failure.kind !== "unauthenticated") setFailed({ must, text: failureText(e.failure, t) });
    }
  };
  const text = (must: MustHave, value: string | null, edit: (v: string | null) => GmbEdit, type?: "tel") => (
    <TextEditor value={value} {...(type && { type })} onSave={(v) => save(must)(edit(v))} />
  );

  const linked = may(me, "sa:sites:work:places:edit") ? tags.sites : null;
  const gmailList = gmails.status === "ok" ? gmails.data.gmails : [];
  const room = (id: number) => {
    const g = gmailList.find((g) => g.id === id);
    if (g === undefined || gmails.status !== "ok") return null;
    return [g.role && GMAIL_ROLE_LABEL[g.role](t), t("{n} GBP", { n: under(gmails.data, id).length })].filter(Boolean).join(" · ");
  };

  const parts: Record<MustHave, { value: ReactNode; editor: ReactNode; why: string | null }> = {
    name: { value: gmb.name, editor: text("name", gmb.name, (v) => ({ name: v ?? "" })), why: t("keyword name: a nom commercial at the top helps an appeal") },
    category: {
      value: placed?.status === "set_up" && [categoryName(placed.trade, placed.category.country), placed.category.google_primary && googleName(placed.category.google_primary, locale)].filter(Boolean).join(" · "),
      editor: (
        <span className="flex flex-col gap-2">
          <TradeSelect value={gmb.trade_id} country={gmb.country?.code ?? null} className="w-80 max-w-full" label={t("Category")} onPick={(trade) => void save("category")({ trade })} />
          {placed !== null && <CategoryFix placed={placed} gmb={gmb} onSet={gmails.reload} />}
        </span>
      ),
      // past a trade, the fix says what is missing
      why: placed === null || placed.status === "no_trade" ? t("its trade, set up in its country with Google's category") : null,
    },
    areas: { value: gmb.areas.join(" · "), editor: <AreasEditor areas={gmb.areas} onSave={(areas) => save("areas")({ areas })} />, why: t("up to 20 · they cover where the film is shot") },
    phone: { value: gmb.phone, editor: text("phone", gmb.phone, (phone) => ({ phone }), "tel"), why: t("routes to the closers · on no other GBP") },
    site: {
      value: tags.sites?.get(gmb.id) && `${tags.sites.get(gmb.id)!.brand}/${tags.sites.get(gmb.id)!.location}`,
      editor: linked ? (
        <span className="flex items-center gap-1 text-sm text-ink-soft">
          <LinkPlaceButton gmbId={gmb.id} linked={linked} onDone={sites.reload} />
          {t("site: link…")}
        </span>
      ) : null,
      why: tags.sites === null ? t("{service} can't be read right now.", { service: t("Sites") }) : linked ? t("its page on our sites; its own domain: a shared site links profiles") : t("linking a site page needs access to the sites"),
    },
    hidden_address: {
      value: gmb.hidden_address && `${(gmb.customers_visit ? t("shown") : t("hidden"))} · ${gmb.hidden_address}`,
      editor: (
        <span className="flex flex-col gap-2">
          <ToggleGroup
            type="single"
            size="sm"
            value={gmb.customers_visit === null ? "" : String(gmb.customers_visit)}
            onValueChange={(v) => typeof v === "string" && v !== "" && void save("hidden_address")({ customers_visit: v === "true" })}
          >
            <ToggleGroupItem value="false">{t("Customers don't visit")}</ToggleGroupItem>
            <ToggleGroupItem value="true">{t("Customers visit")}</ToggleGroupItem>
          </ToggleGroup>
          <TextEditor value={gmb.hidden_address} onSave={(v) => save("hidden_address")({ hidden_address: v })} />
        </span>
      ),
      why: t("customers don't visit → service area; the address is where the film is shot"),
    },
    film: { value: gmb.film, editor: text("film", gmb.film, (film) => ({ film })), why: t("the verification film from the kit (verif_vid)") },
    gmail: {
      value: gmail && `${gmail.gmail} · ${room(gmail.id)}`,
      editor: (
        <NativeSelect
          size="sm"
          wrapperClassName="w-80 max-w-full"
          value={gmb.gmail_id === null ? "" : String(gmb.gmail_id)}
          onChange={(e) => e.target.value !== "" && void save("gmail")({ gmail_id: Number(e.target.value) })}
        >
          {gmb.gmail_id === null && <NativeSelectOption value="">{t("No gmail yet")}</NativeSelectOption>}
          {gmailList.map((g) => (
            <NativeSelectOption key={g.id} value={String(g.id)}>
              {g.gmail} · {room(g.id)}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      ),
      why: t("the gmail that manages it: an owner holds 3, a manager 6"),
    },
    profile: {
      value: gmail?.browser && t("{gmail}'s browser", { gmail: gmail.gmail }),
      editor: gmail ? (
        <span>
          <Button variant="outline" size={button("sm")} onClick={() => openAccount({ gmail: gmail.id, tab: "connections" })}>
            {t("Open {gmail}'s connections", { gmail: gmail.gmail })}
          </Button>
        </span>
      ) : (
        <span className="text-sm text-ink-soft">{t("pick its gmail first: the browser is the gmail's")}</span>
      ),
      why: t("the gmail's antidetect browser: its fingerprint, its proxy in the GBP's country, its session"),
    },
  };

  return (
    <div className="flex max-w-3xl flex-col gap-4">
      <section className={cn("flex flex-col gap-2 rounded-lg border px-4 py-3", missing.length === 0 ? "border-positive/40 bg-positive/5" : "border-accent-warn/40 bg-accent-warn/5")}>
        <div className="flex items-center gap-3">
          <span aria-hidden className="flex gap-1">
            {rows.map((r) => (
              <span key={r.must} className={cn("h-1.5 w-4 rounded-full", r.ready ? "bg-positive" : "bg-ink-soft/40")} />
            ))}
          </span>
          <p className="font-medium text-ink">{t("{n} of {of} ready to open Google's form", { n: ready, of: MUST_HAVES.length })}</p>
        </div>
        <p className="text-sm text-ink-soft">
          {missing.length === 0 ? t("Everything Google's form asks is ready: open it from the profile.") : t("Missing: {what}", { what: missing.map((r) => MUST[r.must](t)).join(" · ") })}
        </p>
      </section>

      <section className="rounded-lg border border-border">
        <h3 className="flex items-baseline gap-2 px-4 pt-3 pb-1 font-medium text-ink">
          {t("The nine, in Google's order")}
          <span className="text-xs font-normal text-ink-soft">{t("then what stands behind it · ready rows fold to one line")}</span>
        </h3>
        <ol className="flex flex-col divide-y divide-border px-4 pb-1">
          {rows.map((r, i) => (
            // a saved value folds its row afresh
            <MustRow key={`${r.must}:${String(parts[r.must].value)}`} n={i + 1} label={MUST[r.must](t)} ready={r.ready} failed={failed?.must === r.must ? failed.text : null} {...parts[r.must]} />
          ))}
        </ol>
      </section>

      <Behind listed={{ gmb, gmail }} />
      <OntoGoogle gmbId={gmb.id} onMoved={gmails.reload} />
    </div>
  );
}

/** A must-have: folded to its value while ready, its editor and why while not, or once unfolded. */
function MustRow({ n, label, ready, value, editor, why, failed }: { n: number; label: string; ready: boolean; value: ReactNode; editor: ReactNode; why: string | null; failed: string | null }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const folded = ready && !open && failed === null;
  return (
    <li className="flex items-start gap-3 py-2.5">
      <span className={cn("mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full font-mono text-xs", ready ? "bg-hover text-ink-soft" : "bg-accent-warn/20 text-accent-warn")}>{n}</span>
      <span className="w-28 shrink-0 pt-0.5 text-sm text-ink-soft">{label}</span>
      {folded ? (
        <button type="button" onClick={() => setOpen(true)} title={t("Edit")} className="min-w-0 flex-1 truncate rounded pt-0.5 text-left text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
          {value}
        </button>
      ) : (
        <span className="flex min-w-0 flex-1 flex-col gap-1">
          {editor}
          {failed !== null ? <span className="text-xs text-accent-error">{failed}</span> : !ready && why !== null && <span className="text-xs text-accent-warn">{why}</span>}
        </span>
      )}
      {ready && (open ? (
        <button type="button" onClick={() => setOpen(false)} aria-label={t("Fold")} title={t("Fold")} className="mt-0.5 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
          <X aria-hidden className="size-4" />
        </button>
      ) : (
        <Check aria-label={t("Ready")} className="mt-0.5 size-4 shrink-0 text-positive" />
      ))}
    </li>
  );
}

/** A text field saved on Enter or its button: empty clears it. */
function TextEditor({ value, type, onSave }: { value: string | null; type?: "tel"; onSave: (v: string | null) => Promise<void> }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const [typed, setTyped] = useState(value ?? "");
  const [busy, setBusy] = useState(false);
  const next = typed.trim() === "" ? null : typed.trim();
  return (
    <form
      className="flex gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        void onSave(next).finally(() => setBusy(false));
      }}
    >
      <Input size={size} type={type ?? "text"} value={typed} onChange={(e) => setTyped(e.target.value)} className="min-w-0 flex-1" />
      <Button type="submit" variant="outline" size={button("sm")} disabled={busy || next === value}>
        {t("Save")}
      </Button>
    </form>
  );
}

/** Service areas as chips, one added on Enter; Google takes 20. */
function AreasEditor({ areas, onSave }: { areas: string[]; onSave: (areas: string[] | null) => Promise<void> }) {
  const t = useT();
  const size = useControlSize();
  const [typed, setTyped] = useState("");
  const write = (next: string[]) => onSave(next.length === 0 ? null : next);
  return (
    <span className="flex flex-col gap-2">
      {areas.length > 0 && (
        <span className="flex flex-wrap gap-1">
          {areas.map((a) => (
            <span key={a} className="flex items-center gap-1 rounded-full border border-border py-0.5 pr-1 pl-2 text-xs text-ink">
              {a}
              <button type="button" aria-label={t("Remove {area}", { area: a })} onClick={() => void write(areas.filter((x) => x !== a))} className="rounded-full text-ink-soft hover:text-ink">
                <X aria-hidden className="size-3" />
              </button>
            </span>
          ))}
        </span>
      )}
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (typed.trim() === "") return;
          void write([...areas, typed]);
        }}
      >
        <Input size={size} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={t("Add an area, Enter")} className="min-w-0 flex-1" />
        <span className="font-mono text-xs text-ink-soft tabular-nums">
          {areas.length}/{MAX_AREAS}
        </span>
      </form>
    </span>
  );
}

/** Where the trade resolves in the GBP's country, and what it lacks with its one fix, or Google's categories to type into its form, in order. */
function CategoryFix({ placed, gmb, onSet }: { placed: Placed; gmb: Gmb; onSet: () => void }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const region = (c: Country) => new Intl.DisplayNames([locale], { type: "region" }).of(c) ?? c;
  if (placed.status === "no_trade") return null;
  if (placed.status === "no_country")
    return (
      <span className="flex flex-col gap-1">
        <span className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
          {t("country not set:")}
          <CountrySelect gmb={gmb.id} value={null} onSet={onSet} />
        </span>
        <span className="text-xs text-accent-warn">{t("the category and Google's categories wait for the country")}</span>
      </span>
    );
  const country = placed.status === "set_up" ? placed.category.country : placed.country;
  const name = categoryName(placed.trade, country);
  const lacking =
    placed.status === "not_set_up"
      ? t("{category} not set up: no Google category to type", { category: name })
      : placed.category.google_primary === null
        ? t("{category} has no primary Google category yet", { category: name })
        : null;
  return (
    <span className="flex flex-col gap-1">
      <span className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
        {t("in {country} →", { country: region(country) })}
        <CategoryChip gmb={gmb} />
        {lacking !== null && (
          <Button variant={placed.status === "not_set_up" ? "primary" : "outline"} size={button("sm")} onClick={() => openTrade(placed.trade.id, country)}>
            {(placed.status === "not_set_up" ? t("Set up {category}", { category: name }) : t("Open {category}", { category: name }))}
          </Button>
        )}
      </span>
      {lacking !== null ? (
        <span className="text-xs text-accent-warn">{lacking}</span>
      ) : (
        placed.status === "set_up" &&
        placed.category.google_primary !== null && (
          <>
            <TypeIn category={placed.category.google_primary} lang={placed.category.lang} />
            {placed.category.google_additional.length > 0 && (
              <span className="flex flex-col gap-1 text-xs text-ink-soft">
                {t("then the additional ones, in this order:")}
                <ol className="flex flex-col gap-0.5">
                  {placed.category.google_additional.map((g, i) => (
                    <li key={g.id} className="flex items-center gap-2">
                      <span className="w-4 text-right font-mono tabular-nums">{i + 1}</span>
                      <Copyable category={g} lang={placed.category.lang} />
                    </li>
                  ))}
                </ol>
              </span>
            )}
          </>
        )
      )}
    </span>
  );
}

function TypeIn({ category, lang }: { category: GoogleCategory; lang: string }) {
  const t = useT();
  return (
    <span className="flex items-center gap-2 text-xs text-ink-soft">
      {t("type into Google's form:")}
      <Copyable category={category} lang={lang} />
    </span>
  );
}

/** Shown in the reader's language, copied in `lang`, the one Google's form in its country reads. */
function Copyable({ category, lang }: { category: GoogleCategory; lang: string }) {
  const t = useT();
  const locale = useLocale();
  const button = useButtonSize();
  const typed = googleName(category, lang);
  return (
    <span className="flex items-center gap-1">
      <span className="rounded bg-hover px-1.5 py-0.5 font-mono text-sm text-ink">{googleName(category, locale)}</span>
      <Button variant="ghost" size={button("sm")} icon aria-label={t("Copy {category}", { category: typed })} title={t("Copy {category}", { category: typed })} onClick={() => void navigator.clipboard.writeText(typed)}>
        <Copy aria-hidden />
      </Button>
    </span>
  );
}

/** Set by hand while Google gives none; disabled once it does. */
function CountrySelect({ gmb, value, disabled, onSet }: { gmb: number; value: Country | null; disabled?: boolean; onSet: () => void }) {
  const t = useT();
  const pick = async (c: Country) => {
    try {
      await setCountry(gmb, c);
      onSet();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <NativeSelect size="sm" wrapperClassName="w-32" disabled={disabled ?? false} aria-label={t("Country")} value={value ?? ""} onChange={(e) => e.target.value !== "" && void pick(e.target.value as Country)}>
      {value === null && <NativeSelectOption value="">—</NativeSelectOption>}
      {COUNTRIES.map((c) => (
        <NativeSelectOption key={c} value={c}>
          {c}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}

/** Outside the nine: its point on the map and its country, which its reviews' language follows. */
function Behind({ listed: { gmb } }: { listed: Listed }) {
  const t = useT();
  const gmails = useGmails();
  const googles = gmb.country !== null && !gmb.country.by_hand;
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3 text-sm">
      <h3 className="font-medium text-ink">{t("Behind it")}</h3>
      <div className="flex items-center gap-3">
        <span className="w-28 shrink-0 text-ink-soft">{t("Point")}</span>
        <span className="flex-1 text-ink">{gmb.location ? `${gmb.location.lat.toFixed(4)}, ${gmb.location.lng.toFixed(4)}` : <span className="text-accent-warn">{t("No location")}</span>}</span>
        <PlaceButton gmb={gmb} />
      </div>
      <div className="flex items-center gap-3">
        <span className="w-28 shrink-0 text-ink-soft">{t("Country")}</span>
        <CountrySelect gmb={gmb.id} value={gmb.country?.code ?? null} disabled={googles} onSet={gmails.reload} />
        <span className="text-xs text-ink-soft">{t("its reviews' language follows it")}</span>
      </div>
    </section>
  );
}

/** Once Google's form is sent, the draft is verifying: its place pasted if Maps shows it yet. */
function OntoGoogle({ gmbId, onMoved }: { gmbId: number; onMoved: () => void }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const [place, setPlace] = useState("");
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const move = async () => {
    setBusy(true);
    try {
      await moveGmb(gmbId, "verifying", place.trim() === "" ? undefined : place.trim());
      onMoved();
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      if (e.failure.kind !== "unauthenticated") setFailed(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-border px-4 py-3">
      <h3 className="font-medium text-ink">{t("Its place on Google")}</h3>
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void move();
        }}
      >
        <Input size={size} autoCapitalize="none" value={place} onChange={(e) => setPlace(e.target.value)} className="min-w-0 flex-1" aria-label={t("Its place on Google")} />
        <Button type="submit" size={button("sm")} disabled={busy}>
          {t.ctx("move to", "Verifying")}
        </Button>
      </form>
      <p className={cn("text-xs", failed === null ? "text-ink-soft" : "text-accent-error")}>{failed ?? t("Once created on Google, it is verifying. Paste its Maps link or place id if Maps shows it yet; it is needed once verified.")}</p>
    </section>
  );
}
