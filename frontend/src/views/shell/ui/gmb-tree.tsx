"use client";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
  Button,
  InfoTip,
  InfoTipContent,
  InfoTipTrigger,
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Skeleton,
  Switch,
  buttonVariants,
  cn,
} from "@evinvest/uikit";
import { Check, ChevronDown, ChevronRight, CircleMinus, CirclePlus, Mail, Search, Trash2 } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { type ReactNode, useEffect, useState } from "react";

import {
  type Gmail,
  type Gmails,
  type Gmb,
  ISSUES,
  type Listed,
  type Selection,
  type SiteTag,
  type State,
  type Tags,
  byView,
  chooseGmailScope,
  issueCounts,
  issueOf,
  issueText,
  peek,
  removeGmail,
  removeGmb,
  select,
  setFilter,
  setOnMap,
  setOnly,
  switchGmbs,
  under,
  useGmailScope,
  useGmails,
  useGmbTags,
  useOnMap,
  usePeeked,
  useSelection,
} from "@/entities/gmb";
import { may, useMe } from "@/entities/session";
import { AddGmailButton } from "@/features/add-gmail";
import { AddGmbButton } from "@/features/add-gmb";
import { EditGbpButton } from "@/features/edit-gbp";
import { LinkPlaceButton } from "@/features/link-gmb-place";
import { PlaceButton } from "@/features/place-gmb";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

const ROW = "group flex min-w-0 items-center gap-1 rounded-md pr-1 text-sm hover:bg-hover/50";
/** Shown on the row's hover or focus only, so the names keep the width. */
const ON_HOVER = "hidden group-hover:flex group-focus-within:flex";
const HEADING = "flex items-center gap-1 px-2 pt-2 pb-1 text-xs font-medium tracking-wide text-ink-soft uppercase";
const SUBHEADING = "flex items-center gap-1 px-2 pt-1.5 pb-0.5 text-xs text-ink-soft";
const DRAGGED = "application/x-sa-gmb";
/** The status dot by state: hollow while a draft. */
const DOT: Record<State, string> = {
  draft: "border border-positive",
  verifying: "bg-accent-warn",
  verified: "bg-positive",
  suspended: "bg-accent-error",
};

/** The GMB the review archive's page is on, if any. */
function pointedAt(pathname: string): number | null {
  const m = /^\/review_archive\/gmbs\/(\d+)/.exec(pathname);
  return m === null ? null : Number(m[1]);
}

/**
 * The caller's GMBs, the outline every page sits beside: the selection every app narrows to, and the
 * rest, rows dragged between them; the gmail, search and issue narrow the rows only. On the map the
 * selection is split by its viewport, and a row opens the GMB's card; elsewhere a row opens the GMB
 * in the review archive. A GMB's switch is the conductor's; the services inherit it.
 */
export function GmbTree({ onNavigate }: { onNavigate?: () => void }) {
  const t = useT();
  const me = useMe();
  const gmails = useGmails();
  const scope = useGmailScope();
  const { inView, hovered } = useOnMap();
  const peeked = usePeeked();
  const pathname = usePathname();
  const { sites, tags } = useGmbTags();
  const linked = may(me, "sa:sites:work:places:edit") ? tags.sites : null;
  const selection = useSelection();

  // a scoped gmail removed, here or in another tab: out of the scope
  const known = gmails.status === "ok" ? gmails.data.gmails : null;
  useEffect(() => {
    if (known !== null && scope !== null && [...scope].some((id) => !known.some((g) => g.id === id))) chooseGmailScope([...scope].filter((id) => known.some((g) => g.id === id)));
  }, [known, scope]);

  const flip = async (ids: number[], enabled: boolean) => {
    try {
      await switchGmbs(ids, enabled);
    } catch (e) {
      notifyFailure(e, t);
    }
    gmails.reload();
  };
  const changed = () => {
    gmails.reload();
    sites.reload();
  };

  if (gmails.status === "loading") return <Skeleton className="mx-2 h-24" />;
  if (gmails.status === "error") return <ErrorState failure={gmails.failure} onRetry={gmails.reload} />;
  if (selection === null) throw new Error("the gmails are read, so is the selection");
  const { scoped, filter, only, found, rows, chosen, isSelected, move } = selection;
  const counts = issueCounts(
    found.map((l) => l.gmb),
    tags,
  );
  const onMap = pathname === "/" && inView !== null;
  const row = (l: Listed) => (
    <GmbRow
      key={l.gmb.id}
      listed={l}
      tags={tags}
      showGmail={scoped.length !== 1}
      onMap={onMap}
      lit={l.gmb.id === hovered || l.gmb.id === peeked}
      current={l.gmb.id === pointedAt(pathname)}
      selected={isSelected(l.gmb.id)}
      linked={linked}
      onMove={move}
      onFlip={flip}
      onChanged={changed}
      {...(onNavigate && { onNavigate })}
    />
  );
  const inSelection = rows.filter((l) => isSelected(l.gmb.id));
  const outOfSelection = rows.filter((l) => !isSelected(l.gmb.id));
  const split = onMap ? byView(inSelection, inView) : null;

  return (
    <section aria-label={t("gmbs.title")} className="flex flex-col gap-1">
      <Scope data={gmails.data} selection={selection} onFlip={flip} onChanged={changed} />
      <InputGroup className="mx-0.5 w-auto">
        <InputGroupAddon>
          <Search aria-hidden />
        </InputGroupAddon>
        <InputGroupInput type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t("gmbs.filter")} aria-label={t("gmbs.filter")} />
      </InputGroup>
      {counts.size > 0 && (
        <div role="group" aria-label={t("gmbs.issues")} className="flex flex-wrap gap-1 px-0.5 pt-1">
          {ISSUES.filter((i) => counts.has(i)).map((i) => (
            <button
              key={i}
              type="button"
              aria-pressed={only === i}
              onClick={() => setOnly(only === i ? null : i)}
              className={cn(
                "rounded-full border px-2 py-0.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring",
                only === i ? "border-ink bg-ink text-background" : "border-border hover:bg-hover/50",
                only !== i && (i === "off" ? "text-ink-soft" : "text-accent-error"),
              )}
            >
              {t(`gmbs.issue.${i}`)} <span className="tabular-nums">{counts.get(i)}</span>
            </button>
          ))}
        </div>
      )}
      {rows.length === 0 ? (
        <p className="px-2 py-1.5 text-xs text-ink-soft">{t("gmbs.none")}</p>
      ) : (
        <>
          <Group
            into
            heading={t("gmbs.selected")}
            count={inSelection.length}
            empty={t("gmbs.selected.empty")}
            onMove={move}
            action={null}
          >
            {split ? (
              <>
                <h4 className={SUBHEADING}>
                  {t("gmbs.inView")} <span className="tabular-nums">{split.inView.length + split.offInView.length}</span>
                </h4>
                <ul className="flex flex-col">{split.inView.map(row)}</ul>
                {split.offInView.length > 0 && (
                  <>
                    <h4 className={cn(SUBHEADING, "mx-2 border-t border-border px-0")}>
                      {t("gmbs.issue.off")} <span className="tabular-nums">{split.offInView.length}</span>
                    </h4>
                    <ul className="flex flex-col">{split.offInView.map(row)}</ul>
                  </>
                )}
                <h4 className={SUBHEADING}>
                  {t("gmbs.elsewhere")} <span className="tabular-nums">{split.elsewhere.length}</span>
                </h4>
                <ul className="flex flex-col">{split.elsewhere.map(row)}</ul>
              </>
            ) : (
              <ul className="flex flex-col">{inSelection.map(row)}</ul>
            )}
          </Group>
          <Group
            into={false}
            heading={t("gmbs.unselected")}
            count={outOfSelection.length}
            empty={t("gmbs.unselected.empty")}
            onMove={move}
            action={chosen !== null && <HeadAction onClick={() => select(null)}>{t("gmbs.selectAll")}</HeadAction>}
          >
            <ul className="flex flex-col">{outOfSelection.map(row)}</ul>
          </Group>
        </>
      )}
      {scoped.length <= 1 && (
        <div className="flex items-center gap-1 px-1 text-sm text-ink-soft">
          <AddGmbButton gmailId={scoped[0]?.id ?? null} onAdded={changed} />
          {t("gmbs.gmb.add")}
        </div>
      )}
    </section>
  );
}

/** A side of the selection, which a row dragged onto it moves to. */
function Group({
  into,
  heading,
  count,
  empty,
  action,
  onMove,
  children,
}: {
  into: boolean;
  heading: string;
  count: number;
  empty: string;
  action: ReactNode;
  onMove: (gmbs: readonly number[], into: boolean) => void;
  children: ReactNode;
}) {
  const [over, setOver] = useState(false);
  return (
    <section
      aria-label={heading}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes(DRAGGED)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        setOver(true);
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setOver(false);
      }}
      onDrop={(e) => {
        e.preventDefault();
        setOver(false);
        const id = Number(e.dataTransfer.getData(DRAGGED));
        if (!Number.isSafeInteger(id)) throw new Error(`dropped a GMB without an id: ${e.dataTransfer.getData(DRAGGED)}`);
        onMove([id], into);
      }}
      className={cn("rounded-md transition-colors", over && "bg-hover/40 ring-1 ring-ring")}
    >
      <h3 className={HEADING}>
        {heading} <span className="tabular-nums">{count}</span>
        <span className="flex-1" />
        {action}
      </h3>
      {count === 0 ? <p className="px-2 py-1.5 text-xs text-ink-soft">{empty}</p> : children}
    </section>
  );
}

function HeadAction({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" onClick={onClick} className="rounded px-1 text-xs font-normal tracking-normal normal-case outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      {children}
    </button>
  );
}

/** Which gmails' GMBs are selected, and which of each: a gmail toggles among the filters (or, after a move, all its GMBs), a location opened under it toggles alone. */
function Scope({ data, selection, onFlip, onChanged }: { data: Gmails; selection: Selection; onFlip: (ids: number[], enabled: boolean) => Promise<void>; onChanged: () => void }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [opened, setOpened] = useState<ReadonlySet<number>>(new Set());
  const { scoped, chosen, edited, isSelected, move } = selection;
  const { gmails } = data;
  const total = data.gmbs.length;
  const covered = (gmbs: Gmb[]) => gmbs.filter((b) => isSelected(b.id)).length;
  const pick = (g: Gmail) => {
    const gmbs = under(data, g.id);
    if (edited) return move(gmbs.map((b) => b.id), covered(gmbs) < gmbs.length);
    const next = new Set(scoped.map((s) => s.id));
    if (next.has(g.id)) next.delete(g.id);
    else next.add(g.id);
    chooseGmailScope(next);
  };
  const unfold = (id: number) => {
    const next = new Set(opened);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setOpened(next);
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="mx-0.5 flex items-center gap-2 rounded-md border border-border px-2.5 py-2 text-sm text-ink outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring">
        <Mail aria-hidden className="size-4 text-ink-soft" />
        <span className="min-w-0 flex-1 truncate text-left">
          {scoped.length === 0 ? t("gmbs.scope.all") : scoped.length === 1 ? scoped[0]!.gmail : t("gmbs.scope.many", { n: scoped.length })}
        </span>
        <span className="text-xs text-ink-soft tabular-nums">{selection.selected.length}</span>
        <ChevronDown aria-hidden className="size-4 text-ink-soft" />
      </PopoverTrigger>
      <PopoverContent align="start" className="max-h-[70svh] w-80 overflow-y-auto p-1">
        <div className={cn(HEADING, "pt-1")}>
          {t("gmbs.title")}
          <InfoTip>
            <InfoTipTrigger label={t("gmbs.what")} />
            <InfoTipContent className="font-normal tracking-normal normal-case">{t("gmbs.gmail.hint")}</InfoTipContent>
          </InfoTip>
          <span className="flex-1" />
          <span className="tabular-nums">{t("gmbs.scope.counts", { gmails: gmails.length, gmbs: total })}</span>
        </div>
        <button type="button" onClick={() => select(null)} aria-pressed={chosen === null} className={cn(ROW, "w-full py-1.5 pl-2", chosen === null && "bg-hover font-medium ring-1 ring-accent-info ring-inset")}>
          <span className="flex-1 text-left">{t("gmbs.scope.all")}</span>
          <span className="text-xs text-ink-soft tabular-nums">{total}</span>
        </button>
        <ul className="flex flex-col">
          {gmails.map((g) => {
            const gmbs = under(data, g.id);
            const on = gmbs.length > 0 && gmbs.every((b) => b.enabled);
            const k = chosen === null ? 0 : covered(gmbs);
            const full = k > 0 && k === gmbs.length;
            const unfolded = opened.has(g.id);
            return (
              <li key={g.id}>
                <div className={cn(ROW, full && "bg-hover ring-1 ring-accent-info ring-inset", k > 0 && !full && "bg-hover/40 ring-1 ring-accent-info/40 ring-inset")}>
                  <button
                    type="button"
                    onClick={() => unfold(g.id)}
                    aria-expanded={unfolded}
                    aria-label={t(unfolded ? "gmbs.collapse" : "gmbs.expand", { gmail: g.gmail })}
                    className="rounded p-1 text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <ChevronRight aria-hidden className={cn("size-3.5 transition-transform", unfolded && "rotate-90")} />
                  </button>
                  <button
                    type="button"
                    onClick={() => pick(g)}
                    aria-pressed={full ? true : k > 0 ? "mixed" : false}
                    className={cn("min-w-0 flex-1 truncate py-1.5 text-left", on ? "text-ink" : "text-ink-soft", k > 0 && "font-medium")}
                    title={g.gmail}
                  >
                    {g.gmail}
                  </button>
                  <span className={cn("flex items-center", ON_HOVER)}>
                    <AddGmbButton gmailId={g.id} onAdded={onChanged} />
                    <Remove title={t("gmbs.gmail.remove.title", { gmail: g.gmail })} body={t("gmbs.gmail.remove.body")} remove={() => removeGmail(g.id)} onRemoved={onChanged} />
                  </span>
                  <span className="text-xs text-ink-soft tabular-nums">{k > 0 && !full ? `${k}/${gmbs.length}` : gmbs.length}</span>
                  <Switch aria-label={t("gmbs.switch", { name: g.gmail })} checked={on} disabled={gmbs.length === 0} onCheckedChange={(v) => void onFlip(gmbs.map((b) => b.id), v)} />
                </div>
                {unfolded && (
                  <ul className="ml-4 flex flex-col border-l border-border pl-1">
                    {gmbs.length === 0 && <li className="px-2 py-1.5 text-xs text-ink-soft">{t("gmbs.gmail.empty")}</li>}
                    {gmbs.map((b) => {
                      const picked = chosen !== null && isSelected(b.id);
                      return (
                        <li key={b.id}>
                          <button
                            type="button"
                            onClick={() => move([b.id], !picked)}
                            aria-pressed={picked}
                            title={b.address ? `${b.name}\n${b.address}` : b.name}
                            className={cn(
                              "flex w-full items-center gap-1 rounded-md py-1 pr-1 pl-2 text-left text-sm outline-none hover:bg-hover/50 focus-visible:ring-2 focus-visible:ring-ring",
                              picked && "bg-hover font-medium ring-1 ring-accent-info ring-inset",
                              b.enabled ? "text-ink" : "text-ink-soft",
                            )}
                          >
                            <span className="min-w-0 flex-1 truncate">{b.name}</span>
                            {picked && <Check aria-hidden className="size-3.5 shrink-0" />}
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </li>
            );
          })}
        </ul>
        <div className="border-t border-border pt-1">
          <AddGmailButton onAdded={onChanged} />
        </div>
      </PopoverContent>
    </Popover>
  );
}

function GmbRow({
  listed: { gmb, gmail },
  tags,
  showGmail,
  onMap,
  lit,
  current,
  selected,
  linked,
  onMove,
  onFlip,
  onChanged,
  onNavigate,
}: {
  listed: Listed;
  tags: Tags;
  showGmail: boolean;
  onMap: boolean;
  lit: boolean;
  current: boolean;
  selected: boolean;
  linked: ReadonlyMap<number, SiteTag> | null;
  onMove: (gmbs: readonly number[], into: boolean) => void;
  onFlip: (ids: number[], enabled: boolean) => Promise<void>;
  onChanged: () => void;
  onNavigate?: () => void;
}) {
  const t = useT();
  const button = useButtonSize();
  const name = gmb.name;
  const move = t(selected ? "gmbs.deselect" : "gmbs.select", { name });
  const issue = issueOf(gmb, tags);
  const archive = tags.archive?.get(gmb.id);
  const site = tags.sites?.get(gmb.id);
  const key =
    tags.sites !== null
      ? site && { text: String(site.leads_7d), title: t("gmbs.leads7d.title") }
      : archive?.rating != null && { text: `★ ${archive.rating.toFixed(1)}`, title: t("gmbs.rating.title") };
  const label = (
    <>
      <span className="flex items-baseline gap-2">
        <span className={cn("min-w-0 flex-1 truncate", gmb.enabled ? "text-ink" : "text-ink-soft")}>{name}</span>
        {key && (
          <span className="shrink-0 text-xs text-ink-soft tabular-nums" title={key.title}>
            {key.text}
          </span>
        )}
      </span>
      <span className="block truncate text-xs text-ink-soft">
        {gmb.state !== "verified" && <span className={cn("font-medium", gmb.state === "suspended" && "text-accent-error")}>{t(`gmbs.state.${gmb.state}`)} · </span>}
        {issue && <span className={cn("font-medium", issue === "off" ? "text-ink-soft" : "text-accent-error")}>{issueText(issue, gmb.id, tags, t)}</span>}
        {issue && (gmb.address || (showGmail && gmail)) && " · "}
        {[gmb.address, showGmail ? gmail?.gmail : null].filter(Boolean).join(" · ")}
      </span>
    </>
  );
  const body = "min-w-0 flex-1 py-1.5 pl-2 text-left";
  return (
    <li
      className={cn(ROW, (current || lit) && "bg-hover/50")}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData(DRAGGED, String(gmb.id));
        e.dataTransfer.effectAllowed = "move";
      }}
      onMouseEnter={() => setOnMap({ hovered: gmb.id })}
      onMouseLeave={() => setOnMap({ hovered: null })}
    >
      <span aria-hidden className={cn("ml-1 size-2 shrink-0 rounded-full", DOT[gmb.state], !gmb.enabled && "border-ink-soft bg-ink-soft", gmb.location === null && "opacity-40")} />
      {onMap ? (
        <button type="button" className={body} title={gmb.address ? `${name}\n${gmb.address}` : name} onClick={() => peek(gmb.id)}>
          {label}
        </button>
      ) : gmb.state === "draft" ? (
        <span className={body} title={name}>
          {label}
        </span>
      ) : (
        <Link
          href={`${ROUTES.reviewArchive}gmbs/${gmb.id}`}
          onClick={() => onNavigate?.()}
          aria-current={current ? "page" : undefined}
          className={body}
          title={gmb.address ? `${name}\n${gmb.address}` : name}
        >
          {label}
        </Link>
      )}
      <span className={cn("flex items-center", ON_HOVER)}>
        <Button variant="ghost" size={button("sm")} icon aria-label={move} title={move} onClick={() => onMove([gmb.id], !selected)}>
          {selected ? <CircleMinus aria-hidden /> : <CirclePlus aria-hidden />}
        </Button>
        <EditGbpButton gmb={gmb} onChanged={onChanged} />
        <PlaceButton gmb={gmb} {...(onNavigate && { onNavigate })} />
        {linked && <LinkPlaceButton gmbId={gmb.id} linked={linked} onDone={onChanged} />}
        <Remove title={t("gmbs.gmb.remove.title", { name })} body={t("gmbs.gmb.remove.body")} remove={() => removeGmb(gmb.id)} onRemoved={onChanged} />
      </span>
      <Switch aria-label={t("gmbs.switch", { name })} checked={gmb.enabled} onCheckedChange={(v) => void onFlip([gmb.id], v)} />
    </li>
  );
}

function Remove({ title, body, remove, onRemoved }: { title: string; body: string; remove: () => Promise<void>; onRemoved: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const act = async () => {
    try {
      await remove();
      onRemoved();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="ghost" size={button("sm")} icon aria-label={title} title={title}>
          <Trash2 aria-hidden />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{body}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t("move.cancel")}</AlertDialogCancel>
          <AlertDialogAction className={buttonVariants({ variant: "destructive", size: button() })} onClick={() => void act()}>
            {t("gmbs.remove")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
