"use client";

import { Button, Card, CardContent, CardHeader, CardTitle, CloseButton, Field, FieldLabel, Input, Textarea, useDismissableLayer } from "@evinvest/uikit";
import { type ReactNode, useEffect, useId, useRef, useState } from "react";

import { NOTE_MAX, TradeName, TradePicker, gmbs, trades, windowKeys } from "@/entities/gmb";
import { api, ok } from "@/shared/api";
import { type Area, type LatLng, countryValues } from "@/shared/api/schema";
import { placement, useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { add as addKeys } from "../model/i18n";
import { useLanded } from "../model/use-landed";

/** A draft started at a point of the map, its first spot: named, noted if wanted, of the trade and in the area the map's study shows; the trade asked only when it shows none. */
export function DraftHere({ at, trade: shown, area, onClose }: { at: LatLng; trade: string | null; area: Area | null; onClose: () => void }) {
  const t = useT();
  const id = useId();
  const size = useControlSize();
  const button = useButtonSize();
  const landed = useLanded();
  const [name, setName] = useState("");
  const [note, setNote] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const trade = shown ?? picked;
  const country = countryValues.find((c) => c === area?.country) ?? null;
  const ready = trade !== null && name.trim() !== "";
  useDismissableLayer({ enabled: true, onDismiss: onClose, pointerOutside: false });

  // the map adds the form to the page once drawn: focusable only then
  const first = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const input = first.current;
    if (input === null) throw new Error("the form renders its name");
    const drawn = new ResizeObserver(() => {
      if (input.offsetHeight === 0) return;
      drawn.disconnect();
      input.focus({ preventScroll: true });
    });
    drawn.observe(input);
    return () => drawn.disconnect();
  }, []);

  const submit = async () => {
    if (trade === null) throw new Error("submitted with no trade");
    setBusy(true);
    try {
      const added = await ok(
        api.POST("/gmbs", {
          body: {
            name: name.trim(),
            trade,
            state: "draft",
            spots: [{ ...at, label: area?.name ?? null, note: null }],
            ...(country !== null && { country }),
            ...(note.trim() !== "" && { note: note.trim() }),
          },
        }),
      );
      onClose();
      landed(added);
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  const title = t(addKeys.draft.here, "A draft here");
  const inherited = (label: string, value: ReactNode) => (
    <>
      <dt className="text-ink-soft">{label}</dt>
      <dd className="min-w-0 truncate text-ink">{value}</dd>
    </>
  );
  return (
    <Card role="dialog" aria-label={title} className="pointer-events-auto w-80 gap-3 py-4 shadow-overlay">
      <CardHeader className="flex items-center gap-2 px-4">
        <CardTitle className="min-w-0 flex-1 truncate">{title}</CardTitle>
        <CloseButton size={button("sm")} label={t(windowKeys.close, "Close the window")} onClose={onClose} />
      </CardHeader>
      <CardContent className="px-4">
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (ready && !busy) void submit();
          }}
        >
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
            {shown !== null && inherited(t(trades.trade, "Trade"), <TradeName gcid={shown} country={country} />)}
            {area !== null && inherited(t(placement.area, "Area"), [area.name, country].filter(Boolean).join(" · "))}
            {inherited(t(addKeys.where, "Where"), <span className="font-mono tabular-nums">{`${at.lat.toFixed(4)}, ${at.lng.toFixed(4)}`}</span>)}
          </dl>
          {shown === null && <TradePicker value={picked} onPick={setPicked} label={t(trades.trade, "Trade")} country={country} className="w-full" />}
          <Field className="flex flex-col gap-1">
            <FieldLabel htmlFor={`${id}-name`}>{t(gmbs.gbp.name, "Name")}</FieldLabel>
            <Input ref={first} id={`${id}-name`} size={size} required value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Textarea size={size} rows={2} maxLength={NOTE_MAX} value={note} onChange={(e) => setNote(e.target.value)} placeholder={t(addKeys.draft.note, "A note, optional: only you see it")} aria-label={t(addKeys.draft.note, "A note, optional: only you see it")} />
          <div className="flex justify-end">
            <Button type="submit" size={button("sm")} disabled={busy || !ready}>
              {t(gmbs.gbp.create, "Create the draft")}
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
