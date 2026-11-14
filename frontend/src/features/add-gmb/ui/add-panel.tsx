"use client";

import { Button, Field, FieldDescription, FieldLabel, Input, NativeSelect, NativeSelectOption, ToggleGroup, ToggleGroupItem, cn } from "@evinvest/uikit";
import { X } from "lucide-react";
import { useEffect, useId, useState } from "react";

import {
  type Adding,
  COUNTRIES,
  type Candidate,
  type Country,
  STATE_NAME,
  type Spot,
  type State,
  TradeSelect,
  addGmb,
  createGmb,
  gmbs,
  open,
  openAdd,
  setOnMap,
  trades,
  useGmails,
  useOnMap,
} from "@/entities/gmb";
import { type T, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { add as addKeys } from "../model/i18n";
import { GmailSelect } from "./gmail-select";
import { PlaceSearch } from "./place-search";
import { SpotsEditor, spotKey } from "./spots-editor";

const STATES: readonly State[] = ["draft", "verifying", "verified", "suspended"];

const STATE_HINT = {
  draft: (t) => t(addKeys.state.draft.hint, "Not on Google yet. Everything Google's form asks is filled on its Launch tab."),
  verifying: (t) => t(addKeys.state.verifying.hint, "Sent to Google, not published yet, so not findable: its place comes once verified."),
  verified: (t) => t(addKeys.state.verified.hint, "Public on Maps: Google has its place, its point and its country."),
  suspended: (t) => t(addKeys.state.suspended.hint, "Hidden from Maps, so Google won't find it: its place comes once it is reinstated."),
} satisfies Record<State, (t: T) => string>;

/**
 * `?add`: a GBP added, over the map beside it, of a trade, under a gmail if given. Its state comes first: a verified one is found on Google
 * (its place, point and country Google's); a draft is where its spots are, in order; the others are not on Maps, so they are added by name,
 * at a point or none. What is picked is drawn on the map.
 */
export function AddPanel({ gmail: scopedGmail, trade: scopedTrade }: Adding) {
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const id = useId();
  const gmails = useGmails();
  const picking = useOnMap((s) => s.picking !== null);
  const [state, setState] = useState<State>("draft");
  const [trade, setTrade] = useState<number | null>(scopedTrade);
  const [name, setName] = useState("");
  const [gmail, setGmail] = useState<number | null>(scopedGmail);
  const [picked, setPicked] = useState<Candidate | null>(null);
  const [spots, setSpots] = useState<Spot[]>([]);
  // the country Google gave each spot added off its suggestions
  const [google, setGoogle] = useState<ReadonlyMap<string, Country | null>>(new Map());
  const [country, setCountry] = useState<Country | null>(null);
  const [busy, setBusy] = useState(false);

  const verified = state === "verified";
  const draft = state === "draft";
  const first = spots[0];
  const googles = verified ? (picked?.country ?? null) : first === undefined ? null : (google.get(spotKey(first)) ?? null);
  const where = googles ?? country;
  const named = verified ? (picked?.name ?? name) : name;
  const ready = trade !== null && named.trim() !== "" && (draft || gmail !== null) && (verified ? picked !== null : !draft || spots.length > 0) && (draft || where !== null);
  const countryAsked = verified ? picked !== null && picked.country === null : !draft && googles === null;

  useEffect(() => {
    setOnMap({ marks: verified ? (picked?.location ? [picked.location] : []) : spots.map(({ lat, lng }) => ({ lat, lng })) });
  }, [verified, picked, spots]);
  useEffect(() => () => setOnMap({ marks: [], picking: null }), []);

  const submit = async () => {
    if (!ready) throw new Error("submitted before ready");
    setBusy(true);
    try {
      const added = verified
        ? await addGmb(gmail!, { place_id: picked!.place_id, state, trade, ...(picked!.name === null && { name: name.trim() }), ...(picked!.country === null && country !== null && { country }) })
        : await createGmb({
            name: name.trim(),
            trade,
            state,
            ...(gmail !== null && { gmail_id: gmail }),
            ...(where !== null && { country: where }),
            ...(draft ? { spots } : first !== undefined && { location: { lat: first.lat, lng: first.lng } }),
          });
      gmails.reload();
      openAdd(null);
      if (added.location === null) setOnMap({ placing: added.id });
      else open(added);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  const title = t(gmbs.gmb.add, "Add a GBP");
  return (
    <section aria-label={title} className={cn("absolute inset-y-0 right-0 flex w-full flex-col overflow-y-auto border-l border-border bg-background md:w-md", picking && "max-md:hidden")}>
      <header className="flex items-start gap-3 px-6 pt-4 pb-3">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 className="text-lg font-semibold text-ink">{title}</h2>
          <p className="text-sm text-ink-soft">{STATE_HINT[state](t)}</p>
        </div>
        <Button variant="ghost" size={button("sm")} icon aria-label={t(addKeys.close, "Close")} title={t(addKeys.close, "Close")} onClick={() => openAdd(null)}>
          <X aria-hidden />
        </Button>
      </header>
      {/* no <form>: the trade select holds its own, for a new trade */}
      <div className="flex flex-1 flex-col gap-4 px-6 pb-6">
        <ToggleGroup
          type="single"
          variant="outline"
          size={button("sm")}
          value={state}
          aria-label={t(gmbs.gbp.state, "Its state on Google")}
          onValueChange={(v) => {
            if (v === "") return;
            setState(v as State);
            setPicked(null);
            if (v !== "draft") setSpots((s) => s.slice(0, 1));
          }}
          className="w-full"
        >
          {STATES.map((s) => (
            <ToggleGroupItem key={s} value={s} className="flex-1">
              {STATE_NAME[s](t)}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <Field className="flex flex-col gap-1">
          <FieldLabel>{t(trades.trade, "Trade")}</FieldLabel>
          <TradeSelect value={trade} onPick={setTrade} label={t(trades.trade, "Trade")} country={where} className="w-full" />
        </Field>
        {verified ? (
          <Field className="flex flex-col gap-1">
            <FieldLabel>{t(addKeys.find, "Find it on Google")}</FieldLabel>
            <PlaceSearch key={state} gmail={gmail} placeholder={t(addKeys.find.placeholder, "Its name and city, a Maps link or a place id")} picked={picked} onPick={setPicked} />
          </Field>
        ) : (
          <>
            <Field className="flex flex-col gap-1">
              <FieldLabel htmlFor={`${id}-name`}>{t(gmbs.gbp.name, "Name")}</FieldLabel>
              <Input id={`${id}-name`} size={size} value={name} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Field className="flex flex-col gap-1">
              <FieldLabel>
                {t(addKeys.where, "Where")}
                {draft ? (
                  spots.length > 1 && <span className="font-normal text-ink-faint">{t(addKeys.spots.order, "its point is the first · drag to reorder")}</span>
                ) : (
                  <span className="font-normal text-ink-faint">{t(addKeys.where.optional, "optional")}</span>
                )}
              </FieldLabel>
              <SpotsEditor
                spots={spots}
                gmail={gmail}
                single={!draft}
                onChange={(next, added) => {
                  setSpots(next);
                  if (added) setGoogle((g) => new Map(g).set(spotKey(added.spot), added.country));
                }}
              />
              <FieldDescription>
                {draft ? t(addKeys.spots.hint, "Where it could stand, each with a note. Its country, which its reviews' language follows, is the first spot's.") : t(addKeys.where.hint, "Its point on the map, and its country, which its reviews' language follows.")}
              </FieldDescription>
            </Field>
          </>
        )}
        {verified && picked !== null && picked.name === null && (
          <Field className="flex flex-col gap-1">
            <FieldLabel htmlFor={`${id}-name`}>{t(gmbs.gbp.name, "Name")}</FieldLabel>
            <Input id={`${id}-name`} size={size} value={name} onChange={(e) => setName(e.target.value)} />
            <FieldDescription>{t(addKeys.name.unnamed, "Google gives this place no name.")}</FieldDescription>
          </Field>
        )}
        {countryAsked && (
          <Field className="flex flex-col gap-1">
            <FieldLabel>{t(gmbs.gbp.country, "Country")}</FieldLabel>
            <NativeSelect size={size} wrapperClassName="w-full" aria-label={t(gmbs.gbp.country, "Country")} value={country ?? ""} onChange={(e) => setCountry(e.target.value === "" ? null : (e.target.value as Country))}>
              <NativeSelectOption value="">{t(addKeys.country.pick, "Pick its country")}</NativeSelectOption>
              {COUNTRIES.map((c) => (
                <NativeSelectOption key={c} value={c}>
                  {c}
                </NativeSelectOption>
              ))}
            </NativeSelect>
            <FieldDescription>{verified ? t(addKeys.country.none, "Google gives this place no country.") : t(addKeys.country.byHand, "Not on Maps: its country is said by hand.")}</FieldDescription>
          </Field>
        )}
        {scopedGmail === null && (
          <Field className="flex flex-col gap-1">
            <FieldLabel>
              {t(gmbs.gbp.gmail, "Gmail")}
              {draft && <span className="font-normal text-ink-faint">{t(addKeys.optional, "optional for a draft")}</span>}
            </FieldLabel>
            <GmailSelect value={gmail} onPick={setGmail} />
          </Field>
        )}
        <div className="mt-auto flex justify-end pt-2">
          <Button size={button()} disabled={busy || !ready} onClick={() => void submit()}>
            {draft ? t(gmbs.gbp.create, "Create the draft") : t(gmbs.gmb.add, "Add a GBP")}
          </Button>
        </div>
      </div>
    </section>
  );
}
