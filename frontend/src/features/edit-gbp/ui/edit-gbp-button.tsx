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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@evinvest/uikit";
import { Pencil } from "lucide-react";
import { useId, useState } from "react";

import { COUNTRIES, type Country, type Gmb, type State, editGmb, moveGmb, setCountry, useGmails } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

const NONE = "none";

/** A GBP on Google's own fields (its gmail, phone, and its country while Google gives none), and the states it may move to; a draft's are its window's Launch tab. */
export function EditGbpButton({ gmb, onChanged }: { gmb: Gmb; onChanged: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const size = useControlSize();
  const id = useId();
  const gmails = useGmails();
  const [open, setOpen] = useState(false);
  const [gmail, setGmail] = useState(gmb.gmail_id);
  const [phone, setPhone] = useState(gmb.phone ?? "");
  const [country, setCountryPicked] = useState<Country | null>(gmb.country?.code ?? null);
  const [busy, setBusy] = useState(false);
  const googles = gmb.country !== null && !gmb.country.by_hand;
  const title = t("gmbs.gbp.edit", { name: gmb.name });

  const reset = () => {
    setGmail(gmb.gmail_id);
    setPhone(gmb.phone ?? "");
    setCountryPicked(gmb.country?.code ?? null);
  };

  const act = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      setOpen(false);
      onChanged();
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    act(async () => {
      const edit = {
        ...(gmail !== null && gmail !== gmb.gmail_id && { gmail_id: gmail }),
        ...(phone.trim() !== (gmb.phone ?? "") && { phone: phone.trim() === "" ? null : phone.trim() }),
      };
      if (Object.keys(edit).length > 0) await editGmb(gmb.id, edit);
      if (country !== null && country !== gmb.country?.code) await setCountry(gmb.id, country);
    });

  const move = (to: State) => act(() => moveGmb(gmb.id, to).then(() => undefined));

  if (gmb.state === "draft") return null;
  return (
    <>
      <Button variant="ghost" size={button("sm")} icon aria-label={title}
        title={title}
        onClick={() => {
          reset();
          setOpen(true);
        }}
      >
        <Pencil aria-hidden />
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
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription>
              {t(`gmbs.state.${gmb.state}`)}
              {gmb.address && ` · ${gmb.address}`}
            </DialogDescription>
          </DialogHeader>
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            {gmails.status === "ok" && (
              <Field className="flex flex-col gap-1">
                <FieldLabel htmlFor={`${id}-gmail`}>{t("gmbs.gbp.gmail")}</FieldLabel>
                <Select value={gmail === null ? NONE : String(gmail)} onValueChange={(v) => setGmail(v === NONE ? null : Number(v))}>
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
            <div className="flex gap-2">
              <Field className="flex min-w-0 flex-1 flex-col gap-1">
                <FieldLabel htmlFor={`${id}-phone`}>{t("gmbs.gbp.phone")}</FieldLabel>
                <Input id={`${id}-phone`} size={size} type="tel" value={phone} onChange={(e) => setPhone(e.target.value)} />
              </Field>
              <Field className="flex min-w-0 flex-1 flex-col gap-1">
                <FieldLabel htmlFor={`${id}-country`}>{t("gmbs.gbp.country")}</FieldLabel>
                <Select value={country ?? NONE} onValueChange={(v) => setCountryPicked(v as Country)}>
                  <SelectTrigger id={`${id}-country`} size={size} className="w-full" disabled={googles}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {country === null && <SelectItem value={NONE}>—</SelectItem>}
                    {COUNTRIES.map((c) => (
                      <SelectItem key={c} value={c}>
                        {c}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {googles && <FieldDescription>{t("gmbs.gbp.country.google")}</FieldDescription>}
              </Field>
            </div>
            <DialogFooter>
              <Button type="submit" size={button()} disabled={busy}>
                {t("gmbs.gbp.save")}
              </Button>
            </DialogFooter>
          </form>
          <div className="flex flex-col gap-2 border-t border-border pt-3">
            <div className="flex flex-wrap items-center gap-2 text-sm text-ink-soft">
              {t("gmbs.gbp.move")}
              {gmb.next.map((to) => (
                <Button key={to} variant="outline" size={button("sm")} disabled={busy} onClick={() => void move(to)}>
                  {t(`gmbs.state.${to}`)}
                </Button>
              ))}
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
