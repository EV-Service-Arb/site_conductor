"use client";

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Field,
  FieldDescription,
  FieldLabel,
  Input,
  Label,
  RadioGroup,
  RadioGroupItem,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@evinvest/uikit";
import { Plus } from "lucide-react";
import { useRouter } from "next/navigation";
import { useId, useState } from "react";

import { COUNTRIES, type Candidate, type Country, ON_GOOGLE, type OnGoogle, addGmb, createDraft, editGmb, resolvePlace, setOnMap, useGmails } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const GOOGLES = "google";

/**
 * A GBP added: a new one, a draft by its name (under `gmailId` if given); or one on Google already, a place or Maps URL pasted,
 * resolved to candidates, one picked with its state, under `gmailId` or a gmail picked. One Google has no point for is then placed
 * on the home map.
 */
export function AddGmbButton({ gmailId, onAdded }: { gmailId: number | null; onAdded: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const id = useId();
  const router = useRouter();
  const gmails = useGmails();
  const [open, setOpen] = useState(false);
  const [onGoogle, setOnGoogle] = useState(gmailId !== null);
  const [name, setName] = useState("");
  const [gmail, setGmail] = useState<number | null>(gmailId);
  const [place, setPlace] = useState("");
  const [candidates, setCandidates] = useState<Candidate[] | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [state, setState] = useState<OnGoogle>("verified");
  const [country, setCountry] = useState<Country | null>(null);
  const [busy, setBusy] = useState(false);

  const reset = () => {
    setName("");
    setGmail(gmailId);
    setPlace("");
    setCandidates(null);
    setPicked(null);
    setState("verified");
    setCountry(null);
  };

  const act = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  const done = () => {
    setOpen(false);
    reset();
    onAdded();
  };

  const draft = () =>
    act(async () => {
      const made = await createDraft(name.trim());
      if (gmailId !== null) await editGmb(made.id, { gmail_id: gmailId });
      done();
    });

  const find = () =>
    act(async () => {
      const found = await resolvePlace(place.trim());
      setCandidates(found);
      setPicked(found.length === 1 ? found[0]!.place_id : null);
    });

  const add = (under: number, placeId: string) =>
    act(async () => {
      const added = await addGmb(under, { place_id: placeId, state, ...(country !== null && { country }) });
      done();
      if (added.location === null) {
        router.push("/");
        setOnMap({ placing: added.id });
      }
    });

  const mode = (google: boolean, label: string) => (
    <Button variant={onGoogle === google ? "primary" : "outline"} size={button("sm")} aria-pressed={onGoogle === google} onClick={() => setOnGoogle(google)}>
      {label}
    </Button>
  );

  return (
    <>
      <Button variant="ghost" size={button("sm")} icon aria-label={t("gmbs.gmb.add")}
        title={t("gmbs.gmb.add")}
        onClick={() => {
          reset();
          setOnGoogle(gmailId !== null);
          setOpen(true);
        }}
      >
        <Plus aria-hidden />
      </Button>
      <Dialog
        open={open}
        onOpenChange={(o) => {
          setOpen(o);
          if (!o) reset();
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("gmbs.gmb.add")}</DialogTitle>
            <DialogDescription>{t(onGoogle ? "gmbs.gmb.place.hint" : "gmbs.gbp.new.hint")}</DialogDescription>
          </DialogHeader>
          <div role="group" className="flex gap-2">
            {mode(false, t("gmbs.gbp.new"))}
            {mode(true, t("gmbs.gbp.existing"))}
          </div>
          {!onGoogle ? (
            <form
              className="flex flex-col gap-3"
              onSubmit={(e) => {
                e.preventDefault();
                if (name.trim()) void draft();
              }}
            >
              <Field className="flex flex-col gap-1">
                <FieldLabel htmlFor={`${id}-name`}>{t("gmbs.gbp.name")}</FieldLabel>
                <Input id={`${id}-name`} size={size} value={name} onChange={(e) => setName(e.target.value)} />
              </Field>
              <DialogFooter>
                <Button type="submit" size={button()} disabled={busy || !name.trim()}>
                  {t("gmbs.gbp.create")}
                </Button>
              </DialogFooter>
            </form>
          ) : (
            <>
              {gmailId === null && gmails.status === "ok" && (
                <Field className="flex flex-col gap-1">
                  <FieldLabel htmlFor={`${id}-gmail`}>{t("gmbs.gbp.gmail")}</FieldLabel>
                  <Select value={gmail === null ? "" : String(gmail)} onValueChange={(v) => setGmail(Number(v))}>
                    <SelectTrigger id={`${id}-gmail`} size={size} className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {gmails.data.gmails.map((g) => (
                        <SelectItem key={g.id} value={String(g.id)}>
                          {g.gmail}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              )}
              <form
                className="flex items-end gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (place.trim()) void find();
                }}
              >
                <Field className="flex min-w-0 flex-1 flex-col gap-1">
                  <FieldLabel htmlFor={`${id}-place`}>{t("gmbs.gmb.place")}</FieldLabel>
                  <Input
                    id={`${id}-place`}
                    size={size}
                    autoCapitalize="none"
                    value={place}
                    onChange={(e) => {
                      setPlace(e.target.value);
                      setCandidates(null);
                      setPicked(null);
                    }}
                  />
                </Field>
                <Button type="submit" variant="outline" size={button()} disabled={busy || !place.trim()}>
                  {t("gmbs.gmb.find")}
                </Button>
              </form>
              {candidates !== null && candidates.length === 0 && <FieldDescription>{t("gmbs.gmb.none")}</FieldDescription>}
              {candidates !== null && candidates.length > 0 && (
                <RadioGroup value={picked ?? ""} onValueChange={setPicked} aria-label={t("gmbs.gmb.pick")} className="flex flex-col gap-2">
                  {candidates.map((c) => (
                    <div key={c.place_id} className="flex items-start gap-3 rounded-md border border-border p-3">
                      <RadioGroupItem id={`${id}-${c.place_id}`} value={c.place_id} className="mt-0.5" />
                      <Label htmlFor={`${id}-${c.place_id}`} className="flex min-w-0 flex-col items-start gap-0.5 font-normal">
                        <span className="text-sm text-ink">{c.name ?? c.place_id}</span>
                        {c.address && <span className="text-xs text-ink-soft">{c.address}</span>}
                      </Label>
                    </div>
                  ))}
                </RadioGroup>
              )}
              <div className="flex gap-2">
                <Field className="flex min-w-0 flex-1 flex-col gap-1">
                  <FieldLabel htmlFor={`${id}-state`}>{t("gmbs.gbp.state")}</FieldLabel>
                  <Select value={state} onValueChange={(v) => setState(v as OnGoogle)}>
                    <SelectTrigger id={`${id}-state`} size={size} className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {ON_GOOGLE.map((s) => (
                        <SelectItem key={s} value={s}>
                          {t(`gmbs.state.${s}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
                <Field className="flex min-w-0 flex-1 flex-col gap-1">
                  <FieldLabel htmlFor={`${id}-country`}>{t("gmbs.gbp.country")}</FieldLabel>
                  <Select value={country ?? GOOGLES} onValueChange={(v) => setCountry(v === GOOGLES ? null : (v as Country))}>
                    <SelectTrigger id={`${id}-country`} size={size} className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={GOOGLES}>{t("gmbs.gbp.country.google")}</SelectItem>
                      {COUNTRIES.map((c) => (
                        <SelectItem key={c} value={c}>
                          {c}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              </div>
              <DialogFooter>
                <Button size={button()} disabled={busy || picked === null || gmail === null} onClick={() => picked !== null && gmail !== null && void add(gmail, picked)}>
                  {t("gmbs.gmb.add")}
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
