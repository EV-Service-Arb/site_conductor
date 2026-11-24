"use client";

import { Button, Field, FieldDescription, FieldLabel, Input, NativeSelect, NativeSelectOption, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";
import { useEffect, useId, useState } from "react";

import {
  type Adding,
  STATE_NAME,
  TradePicker,
  gmbs,
  closeWindow,
  setOnMap,
  trades,
} from "@/entities/gmb";
import { api, ok } from "@/shared/api";
import { type Country, type Place, type Spot, type State, countryValues } from "@/shared/api/schema";
import { type T, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { add as addKeys } from "../model/i18n";
import { useLanded } from "../model/use-landed";
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
 * A GBP added, of a trade, under a gmail if given. Its state comes first: a verified one is found on Google
 * (its place, point and country Google's); a draft is where its spots are, in order; the others are not on Maps, so they are added by name,
 * at a point or none. What is picked is drawn on the map; on its page there is no map to pick on.
 */
export function AddPanel({ at, page }: { at: Adding; page: boolean }) {
  const { gmail: scopedGmail, trade: scopedTrade } = at;
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const id = useId();
  const landed = useLanded();
  const [state, setState] = useState<State>("draft");
  const [trade, setTrade] = useState<string | null>(scopedTrade ?? null);
  const [name, setName] = useState("");
  const [gmail, setGmail] = useState<number | null>(scopedGmail ?? null);
  const [picked, setPicked] = useState<Place | null>(null);
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
  // a GBP on Google takes a trade later; one made here needs it now
  const ready = (verified || trade !== null) && named.trim() !== "" && (draft || gmail !== null) && (verified ? picked !== null : !draft || spots.length > 0) && (draft || where !== null);
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
        ? await ok(api.POST("/gmails/{id}/gmbs", { params: { path: { id: gmail! } }, body: { place_id: picked!.place_id, state, ...(trade !== null && { trade }), ...(picked!.name === null && { name: name.trim() }), ...(picked!.country === null && country !== null && { country }) } }))
        : await ok(api.POST("/gmbs", {
            body: {
              name: name.trim(),
              trade: trade!, // ready holds one for a GBP made here
              state,
              ...(gmail !== null && { gmail_id: gmail }),
              ...(where !== null && { country: where }),
              ...(draft ? { spots } : first !== undefined && { location: { lat: first.lat, lng: first.lng } }),
            },
          }));
      closeWindow({ kind: "add", at });
      landed(added);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  const title = t(gmbs.gmb.add, "Add a GBP");
  return (
    <section aria-label={title} className="flex flex-col">
      <p className="px-6 pt-4 pb-3 text-sm text-ink-soft">{STATE_HINT[state](t)}</p>
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
          <TradePicker value={trade} onPick={setTrade} label={t(trades.trade, "Trade")} country={where} className="w-full" />
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
                onMap={!page}
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
              {countryValues.map((c) => (
                <NativeSelectOption key={c} value={c}>
                  {c}
                </NativeSelectOption>
              ))}
            </NativeSelect>
            <FieldDescription>{verified ? t(addKeys.country.none, "Google gives this place no country.") : t(addKeys.country.byHand, "Not on Maps: its country is said by hand.")}</FieldDescription>
          </Field>
        )}
        {scopedGmail == null && (
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
