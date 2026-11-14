"use client";

import {
  Button,
  Command,
  CommandItem,
  CommandList,
  CommandInput,
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
  NativeSelect,
  NativeSelectOption,
  ToggleGroup,
  ToggleGroupItem,
} from "@evinvest/uikit";
import { MapPin, Plus, X } from "lucide-react";
import { useRouter } from "next/navigation";
import { useEffect, useId, useState } from "react";

import { COUNTRIES, type Candidate, type Country, type State, type Suggestion, TradeSelect, addGmail, addGmb, createGmb, resolvePlace, setOnMap, suggestPlaces, useGmails } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const STATES: readonly State[] = ["draft", "verifying", "verified", "suspended"];
/** Each distinct input is a billed request: asked only once typing pauses. */
const PAUSE_MS = 350;
const MIN_INPUT = 3;
const NEW = "new";

/**
 * A GBP added, of a trade (`trade` first, the outline's scope's), under `gmailId` if given. Its state comes first: a verified one is found on
 * Google (its place, point and country Google's); the others are not on Maps, so they are added by name, at a point picked off Google's
 * suggestions or none. One without a point is then placed on the home map.
 */
export function AddGmbButton({ gmailId, trade: scoped, labeled, onAdded }: { gmailId: number | null; trade: number | null; labeled?: boolean; onAdded: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const id = useId();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [state, setState] = useState<State>("draft");
  const [trade, setTrade] = useState<number | null>(scoped);
  const [name, setName] = useState("");
  const [gmail, setGmail] = useState<number | null>(gmailId);
  const [picked, setPicked] = useState<Candidate | null>(null);
  const [country, setCountry] = useState<Country | null>(null);
  const [busy, setBusy] = useState(false);

  const reset = () => {
    setState("draft");
    setTrade(scoped);
    setName("");
    setGmail(gmailId);
    setPicked(null);
    setCountry(null);
  };

  const verified = state === "verified";
  const draft = state === "draft";
  const named = verified ? (picked?.name ?? name) : name;
  const where = picked?.country ?? country;
  const ready =
    trade !== null &&
    named.trim() !== "" &&
    (draft || gmail !== null) &&
    (verified ? picked !== null : draft || where !== null);

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
            ...(picked?.location != null && { location: picked.location }),
          });
      setOpen(false);
      reset();
      onAdded();
      if (added.location === null) {
        router.push("/");
        setOnMap({ placing: added.id });
      }
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  const label = t("gmbs.gmb.add");
  return (
    <>
      <Button
        variant="ghost"
        size={button("sm")}
        icon={!labeled}
        aria-label={label}
        title={labeled ? undefined : label}
        className={labeled ? "justify-start text-ink-soft" : undefined}
        onClick={() => {
          reset();
          setOpen(true);
        }}
      >
        <Plus aria-hidden />
        {labeled && label}
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
            <DialogTitle>{label}</DialogTitle>
            <DialogDescription>{t(`add.state.${state}.hint`)}</DialogDescription>
          </DialogHeader>
          <form
            className="flex flex-col gap-4"
            onSubmit={(e) => {
              e.preventDefault();
              if (ready && !busy) void submit();
            }}
          >
            <ToggleGroup
              type="single"
              variant="outline"
              size={button("sm")}
              value={state}
              aria-label={t("gmbs.gbp.state")}
              onValueChange={(v) => {
                if (v === "") return;
                setState(v as State);
                setPicked(null);
              }}
              className="w-full"
            >
              {STATES.map((s) => (
                <ToggleGroupItem key={s} value={s} className="flex-1">
                  {t(`gmbs.state.${s}`)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <Field className="flex flex-col gap-1">
              <FieldLabel>{t("trades.trade")}</FieldLabel>
              <TradeSelect value={trade} onPick={setTrade} label={t("trades.trade")} country={where} className="w-full" />
            </Field>
            {verified ? (
              <Field className="flex flex-col gap-1">
                <FieldLabel>{t("add.find")}</FieldLabel>
                <PlaceSearch key={state} gmail={gmail} placeholder={t("add.find.placeholder")} picked={picked} onPick={setPicked} />
              </Field>
            ) : (
              <>
                <Field className="flex flex-col gap-1">
                  <FieldLabel htmlFor={`${id}-name`}>{t("gmbs.gbp.name")}</FieldLabel>
                  <Input id={`${id}-name`} size={size} value={name} onChange={(e) => setName(e.target.value)} />
                </Field>
                <Field className="flex flex-col gap-1">
                  <FieldLabel>
                    {t("add.where")}
                    {draft && <span className="font-normal text-ink-faint">{t("add.optional")}</span>}
                  </FieldLabel>
                  <PlaceSearch key={state} gmail={gmail} placeholder={t("add.where.placeholder")} picked={picked} onPick={setPicked} />
                  <FieldDescription>{t("add.where.hint")}</FieldDescription>
                </Field>
              </>
            )}
            {verified && picked !== null && picked.name === null && (
              <Field className="flex flex-col gap-1">
                <FieldLabel htmlFor={`${id}-name`}>{t("gmbs.gbp.name")}</FieldLabel>
                <Input id={`${id}-name`} size={size} value={name} onChange={(e) => setName(e.target.value)} />
                <FieldDescription>{t("add.name.unnamed")}</FieldDescription>
              </Field>
            )}
            {picked !== null && picked.country === null && (verified || !draft) && (
              <Field className="flex flex-col gap-1">
                <FieldLabel>{t("gmbs.gbp.country")}</FieldLabel>
                <NativeSelect size={size} wrapperClassName="w-full" aria-label={t("gmbs.gbp.country")} value={country ?? ""} onChange={(e) => setCountry(e.target.value === "" ? null : (e.target.value as Country))}>
                  <NativeSelectOption value="">{t("add.country.pick")}</NativeSelectOption>
                  {COUNTRIES.map((c) => (
                    <NativeSelectOption key={c} value={c}>
                      {c}
                    </NativeSelectOption>
                  ))}
                </NativeSelect>
                <FieldDescription>{t("add.country.none")}</FieldDescription>
              </Field>
            )}
            {gmailId === null && (
              <Field className="flex flex-col gap-1">
                <FieldLabel>
                  {t("gmbs.gbp.gmail")}
                  {draft && <span className="font-normal text-ink-faint">{t("add.optional")}</span>}
                </FieldLabel>
                <GmailSelect value={gmail} onPick={setGmail} />
              </Field>
            )}
            <DialogFooter>
              <Button type="submit" size={button()} disabled={busy || !ready}>
                {t(draft ? "gmbs.gbp.create" : "gmbs.gmb.add")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}

/** Google's suggestions as one types, as Maps' own box: a pick is resolved to its place (point, address, country); Enter on nothing picked resolves what was typed, so a Maps link or a place id works too. */
function PlaceSearch({ gmail, placeholder, picked, onPick }: { gmail: number | null; placeholder: string; picked: Candidate | null; onPick: (c: Candidate | null) => void }) {
  const t = useT();
  const button = useButtonSize();
  const [input, setInput] = useState("");
  const [answered, setAnswered] = useState<{ q: string; found: Suggestion[] } | null>(null);
  const [resolving, setResolving] = useState(false);
  const q = input.trim();
  const found = answered !== null && answered.q === q ? answered.found : null;

  useEffect(() => {
    if (q.length < MIN_INPUT) return;
    let live = true;
    const timer = setTimeout(() => {
      suggestPlaces(q, gmail).then(
        (found) => live && setAnswered({ q, found }),
        (e: unknown) => live && notifyFailure(e, t),
      );
    }, PAUSE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [q, gmail, t]);

  const resolve = async (place: string) => {
    setResolving(true);
    try {
      const [first, ...rest] = await resolvePlace(place, gmail);
      if (first === undefined || rest.length > 0) {
        setAnswered({ q, found: [first, ...rest].filter((c) => c !== undefined).map((c) => ({ place_id: c.place_id, main: c.name ?? c.place_id, secondary: c.address })) });
        return;
      }
      onPick(first);
      setInput("");
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setResolving(false);
    }
  };

  if (picked !== null)
    return (
      <div className="flex items-center gap-3 rounded-md border border-border bg-hover/40 px-3 py-2">
        <MapPin aria-hidden className="size-4 shrink-0 text-ink-soft" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-sm text-ink">{picked.name ?? picked.address ?? picked.place_id}</span>
          {picked.name !== null && picked.address !== null && <span className="truncate text-xs text-ink-soft">{picked.address}</span>}
          <span className="text-xs text-ink-faint">
            {[picked.location === null ? t("add.picked.noPoint") : `${picked.location.lat.toFixed(4)}, ${picked.location.lng.toFixed(4)}`, picked.country].filter(Boolean).join(" · ")}
          </span>
        </span>
        <Button type="button" variant="ghost" size={button("sm")} icon aria-label={t("add.picked.clear")} title={t("add.picked.clear")} onClick={() => onPick(null)}>
          <X aria-hidden />
        </Button>
      </div>
    );

  return (
    <Command shouldFilter={false} search={input} onSearchChange={setInput} className="h-auto overflow-visible rounded-md border border-border">
      <CommandInput
        placeholder={placeholder}
        autoCapitalize="none"
        onKeyDown={(e) => {
          // cmdk takes Enter for the highlighted item; with none listed, the typed text is resolved as it is
          if (e.key === "Enter" && (found === null || found.length === 0) && q !== "") {
            e.preventDefault();
            void resolve(q);
          }
        }}
      />
      {(found !== null || resolving) && (
        <CommandList>
          {resolving && <p className="px-3 py-2 text-sm text-ink-soft">{t("state.loading")}</p>}
          {found?.length === 0 && <p className="px-3 py-2 text-sm text-ink-soft">{t("gmbs.gmb.none")}</p>}
          {found?.map((s) => (
            <CommandItem key={s.place_id} value={s.place_id} onSelect={() => void resolve(s.place_id)}>
              <MapPin aria-hidden className="text-ink-soft" />
              <span className="flex min-w-0 flex-col">
                <span className="truncate text-sm text-ink">{s.main}</span>
                {s.secondary !== null && <span className="truncate text-xs text-ink-soft">{s.secondary}</span>}
              </span>
            </CommandItem>
          ))}
          {found !== null && found.length > 0 && <p className="border-t border-border px-3 py-1.5 text-xs text-ink-faint">{t("add.suggestions")}</p>}
        </CommandList>
      )}
    </Command>
  );
}

/** The caller's gmails, then "New gmail…", which adds one and picks it. */
function GmailSelect({ value, onPick }: { value: number | null; onPick: (gmail: number) => void }) {
  const t = useT();
  const size = useControlSize();
  const button = useButtonSize();
  const gmails = useGmails();
  const [naming, setNaming] = useState(false);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);

  const add = async () => {
    setBusy(true);
    try {
      const made = await addGmail(typed.trim());
      gmails.reload();
      onPick(made.id);
      setNaming(false);
      setTyped("");
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  if (naming)
    return (
      <span className="flex items-center gap-2">
        <Input
          size={size}
          type="email"
          autoFocus
          autoCapitalize="none"
          value={typed}
          placeholder={t("add.gmail.new.placeholder")}
          aria-label={t("add.gmail.new")}
          onChange={(e) => setTyped(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              if (typed.trim() !== "") void add();
            }
            if (e.key === "Escape") {
              e.stopPropagation();
              setNaming(false);
            }
          }}
          className="min-w-0 flex-1"
        />
        <Button type="button" size={button("sm")} disabled={busy || typed.trim() === ""} onClick={() => void add()}>
          {t("add.gmail.add")}
        </Button>
        <Button type="button" variant="ghost" size={button("sm")} onClick={() => setNaming(false)}>
          {t("move.cancel")}
        </Button>
      </span>
    );

  return (
    <NativeSelect
      size={size}
      wrapperClassName="w-full"
      aria-label={t("gmbs.gbp.gmail")}
      disabled={gmails.status !== "ok"}
      value={value === null ? "" : String(value)}
      onChange={(e) => {
        const v = e.target.value;
        if (v === NEW) setNaming(true);
        else if (v !== "") onPick(Number(v));
      }}
    >
      {value === null && <NativeSelectOption value="">{t("add.gmail.pick")}</NativeSelectOption>}
      {gmails.status === "ok" &&
        gmails.data.gmails.map((g) => (
          <NativeSelectOption key={g.id} value={String(g.id)}>
            {g.gmail}
          </NativeSelectOption>
        ))}
      <NativeSelectOption value={NEW}>{t("add.gmail.new")}</NativeSelectOption>
    </NativeSelect>
  );
}
