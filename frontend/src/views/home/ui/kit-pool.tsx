"use client";

import { Button, ConfirmDialog, Skeleton, Textarea } from "@evinvest/uikit";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import { kit, move } from "@/entities/gmb";
import { type Failure, Refused, api, ok } from "@/shared/api";
import type { Country, KitDraft, KitPool } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import type { Resource } from "@evinvest/api/react";
import { ErrorState } from "@/shared/ui/error-state";
import { failureText } from "@/shared/ui/failure-text";
import { useButtonSize } from "@/shared/ui/touch";

const SECTION = "flex flex-col gap-2";
const SECTION_HEAD = "flex items-baseline gap-2 text-xs font-semibold tracking-wide text-ink-soft uppercase";
const HINT = "font-normal tracking-normal normal-case";

const leastUsed = <X extends { uses: number }>(xs: readonly X[]) => [...xs].sort((a, b) => a.uses - b.uses);

/** A category's review drafts (13h), each copied to a GBP once; the photos offered with them are each GBP's brand's. */
export function KitPoolPane({ trade, country, pool }: { trade: number; country: Country; pool: Resource<KitPool, Failure> & { reload: () => void } }) {
  const t = useT();
  if (pool.status === "loading") return <Skeleton className="h-64 w-full" />;
  if (pool.status === "error") return <ErrorState failure={pool.failure} onRetry={pool.reload} />;
  const { drafts } = pool.data;
  const at = { trade, country, reload: pool.reload };
  return (
    <div className="flex flex-col gap-6">
      <section className={SECTION}>
        <h3 className={SECTION_HEAD}>
          {t(kit.drafts, "Drafts")} <span className="font-normal tabular-nums">{drafts.length}</span>
          <span className="flex-1" />
          <span className={HINT}>{t(kit.drafts.order, "least used first · a GBP gets each at most once")}</span>
        </h3>
        {drafts.length === 0 && <p className="text-sm text-ink-soft">{t(kit.drafts.none, "No drafts yet: a reviewer's page offers Google's form alone.")}</p>}
        <ul className="flex flex-col divide-y divide-border border-y border-border">
          {leastUsed(drafts).map((d) => (
            <DraftRow key={d.id} draft={d} at={at} />
          ))}
        </ul>
        <NewDraft at={at} />
      </section>
    </div>
  );
}

interface At {
  trade: number;
  country: Country;
  reload: () => void;
}

function DraftRow({ draft, at }: { draft: KitDraft; at: At }) {
  const t = useT();
  const button = useButtonSize();
  const [editing, setEditing] = useState(false);
  if (editing)
    return (
      <li className="py-3">
        <DraftEditor
          at={at}
          initial={draft.text}
          save={(text) => ok(api.PATCH("/trades/{id}/{country}/kit/drafts/{draft}", { params: { path: { id: at.trade, country: at.country, draft: draft.id } }, body: { text } }))}
          onDone={() => setEditing(false)}
        />
      </li>
    );
  return (
    <li className="flex items-center gap-3 py-2.5">
      <p className="line-clamp-2 min-w-0 flex-1 text-sm text-ink">{draft.text}</p>
      <span className="w-16 shrink-0 text-right font-mono text-sm text-ink tabular-nums">{t(kit.uses, "{n} uses", { n: draft.uses })}</span>
      <Button variant="ghost" size={button("sm")} icon aria-label={t(kit.drafts.edit, "Edit the draft")} title={t(kit.drafts.edit, "Edit the draft")} onClick={() => setEditing(true)}>
        <Pencil aria-hidden />
      </Button>
      <Remove at={at} id={draft.id} title={t(kit.drafts.remove, "Remove the draft")} />
    </li>
  );
}

/** Out of every offer, its uses kept: a removed draft still keeps the GBPs it went to. */
function Remove({ at, id, title }: { at: At; id: number; title: string }) {
  const t = useT();
  const button = useButtonSize();
  return (
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
        await ok(api.DELETE("/trades/{id}/{country}/kit/drafts/{draft}", { params: { path: { id: at.trade, country: at.country, draft: id } } }));
        at.reload();
      }}
    />
  );
}

function NewDraft({ at }: { at: At }) {
  const t = useT();
  const button = useButtonSize();
  const [open, setOpen] = useState(false);
  if (!open)
    return (
      <Button variant="outline" size={button("sm")} className="self-start" onClick={() => setOpen(true)}>
        <Plus aria-hidden />
        {t(kit.drafts.add, "Add a draft")}
      </Button>
    );
  return <DraftEditor at={at} initial="" save={(text) => ok(api.POST("/trades/{id}/{country}/kit/drafts", { params: { path: { id: at.trade, country: at.country } }, body: { text } }))} onDone={() => setOpen(false)} />;
}

/** A draft's text; the server's refusal (a copied draft's text) shown under it. */
function DraftEditor({ at, initial, save, onDone }: { at: At; initial: string; save: (text: string) => Promise<unknown>; onDone: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const [text, setText] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    try {
      await save(text.trim());
      at.reload();
      onDone();
    } catch (e) {
      if (!(e instanceof Refused)) throw e;
      if (e.failure.code !== "unauthenticated") setRefused(failureText(e.failure, t));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <Textarea autoFocus rows={3} value={text} onChange={(e) => setText(e.target.value)} placeholder={t(kit.drafts.placeholder, "A review as a customer would write it, in the country's language")} aria-label={t(kit.drafts.text, "The draft's text")} />
      {refused !== null && <p className="text-xs text-accent-error">{refused}</p>}
      <span className="flex gap-2">
        <Button type="submit" size={button("sm")} disabled={busy || text.trim() === ""}>
          {t(kit.save, "Save")}
        </Button>
        <Button type="button" variant="ghost" size={button("sm")} onClick={onDone}>
          {t(move.cancel, "Cancel")}
        </Button>
      </span>
    </form>
  );
}
