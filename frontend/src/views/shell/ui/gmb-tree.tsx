"use client";

import { Button, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger, InputGroup, InputGroupAddon, InputGroupInput, Skeleton, Switch, cn } from "@evinvest/uikit";
import { ChevronDown, CircleMinus, CirclePlus, Link2, Pencil, Search, Tag as TagIcon, Trash2, X } from "lucide-react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { type ReactNode, useEffect, useState } from "react";

import {
  COUNTRIES,
  type Gmb,
  ISSUES,
  type Listed,
  MUST_HAVES,
  LiveReviews,
  type SiteTag,
  type State,
  TABS,
  type Tags,
  byView,
  chooseGmailScope,
  issueCounts,
  issueOf,
  issueText,
  openOrFocus,
  openTrash,
  readiness,
  removeGmb,
  select,
  setFilter,
  setOnMap,
  setOnly,
  setTrades,
  switchGmbs,
  useGmails,
  useGmbTags,
  useOnMap,
  usePeeked,
  useScope,
  useSelection,
  useTrades,
  useTrash,
  useUndoable,
  gmbs as gmbsKeys,
  trash as trashKeys,
  trades as tradesKeys,
  STATE_NAME,
  ISSUE_NAME,
} from "@/entities/gmb";
import { may, useMe } from "@/entities/session";
import { AddGmbButton } from "@/features/add-gmb";
import { EditGbp } from "@/features/edit-gbp";
import { LinkPlace } from "@/features/link-gmb-place";
import { PlaceButton } from "@/features/place-gmb";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { HEADING, ROW, RemoveDialog, ResetSelection, useKept } from "./outline-parts";
import { ScopePicker } from "./scope-picker";

const SUBHEADING = "flex items-center gap-1 px-2 pt-1.5 pb-0.5 text-xs text-ink-soft";
const DRAGGED = "application/x-sa-gmb";
/** The status dots by state: hollow while a draft. */
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
  const { gmails: scope, categories } = useScope();
  const inView = useOnMap((s) => s.inView);
  const peeked = usePeeked();
  const pathname = usePathname();
  const { sites, tags } = useGmbTags();
  const linked = may(me, "sa:sites:work:places:edit") ? tags.sites : null;
  const selection = useSelection();
  const undoable = useUndoable();

  // a scoped gmail removed, here or in another tab: out of the scope
  const known = gmails.status === "ok" ? gmails.data.gmails : null;
  useEffect(() => {
    if (known !== null && scope !== null && [...scope].some((id) => !known.some((g) => g.id === id))) chooseGmailScope([...scope].filter((id) => known.some((g) => g.id === id)));
  }, [known, scope]);

  const flip = async (gmbs: Gmb[], enabled: boolean) => {
    try {
      undoable(await switchGmbs(gmbs, enabled, t));
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
      peeked={l.gmb.id === peeked}
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
    // NOT SELECTED and the footer stay in reach under a long SELECTED, which scrolls alone
    <section aria-label={t(gmbsKeys.title, "Managing gmails")} className="flex min-h-0 flex-1 flex-col gap-1">
      <ScopePicker data={gmails.data} selection={selection} onFlip={flip} onChanged={changed} />
      <InputGroup className="mx-0.5 w-auto">
        <InputGroupAddon>
          <Search aria-hidden />
        </InputGroupAddon>
        <InputGroupInput type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t(gmbsKeys.filter, "Name, address, place id, gmail")} aria-label={t(gmbsKeys.filter, "Name, address, place id, gmail")} />
      </InputGroup>
      {counts.size > 0 && (
        <div role="group" aria-label={t(gmbsKeys.issues, "What needs you")} className="flex flex-wrap gap-1 px-0.5 pt-1">
          {ISSUES.filter((i) => counts.has(i)).map((i) => (
            <button
              key={i}
              type="button"
              aria-pressed={only === i}
              onClick={() => setOnly(only === i ? null : i)}
              className={cn(
                "rounded-full border px-2 py-0.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring",
                only === i ? "border-ink bg-ink text-background" : "border-border hover:bg-hover/50",
                only !== i && (i === "off" || i === "draft" || i === "no_category" ? "text-ink-soft" : "text-accent-error"),
              )}
            >
              {ISSUE_NAME[i](t)} <span className="tabular-nums">{counts.get(i)}</span>
            </button>
          ))}
        </div>
      )}
      {rows.length === 0 ? (
        <p className="px-2 py-1.5 text-xs text-ink-soft">{t(gmbsKeys.none, "No GMBs yet.")}</p>
      ) : (
        <>
          <Group
            into
            className="min-h-0 flex-1 overflow-y-auto"
            heading={t(gmbsKeys.selected, "Selected")}
            count={inSelection.length}
            empty={t(gmbsKeys.selected.empty, "Drag locations here: every app shows only these.")}
            onMove={move}
            action={
              <>
                {inSelection.length > 0 && <SetTrade gmbs={inSelection.map((l) => l.gmb)} onSet={changed} />}
                {chosen !== null && <ResetSelection />}
              </>
            }
          >
            {split ? (
              <>
                <h4 className={SUBHEADING}>
                  {t(gmbsKeys.inView, "In view")} <span className="tabular-nums">{split.inView.length + split.offInView.length}</span>
                </h4>
                <ul className="flex flex-col">{split.inView.map(row)}</ul>
                {split.offInView.length > 0 && (
                  <>
                    <h4 className={cn(SUBHEADING, "mx-2 border-t border-border px-0")}>
                      {t(gmbsKeys.issue.off, "Off")} <span className="tabular-nums">{split.offInView.length}</span>
                    </h4>
                    <ul className="flex flex-col">{split.offInView.map(row)}</ul>
                  </>
                )}
                <h4 className={SUBHEADING}>
                  {t(gmbsKeys.elsewhere, "Elsewhere")} <span className="tabular-nums">{split.elsewhere.length}</span>
                </h4>
                <ul className="flex flex-col">{split.elsewhere.map(row)}</ul>
              </>
            ) : (
              <ul className="flex flex-col">{inSelection.map(row)}</ul>
            )}
          </Group>
          <Group
            into={false}
            className="max-h-2/5 shrink-0 overflow-y-auto border-t border-border"
            heading={t(gmbsKeys.unselected, "Not selected")}
            count={outOfSelection.length}
            empty={t(gmbsKeys.unselected.empty, "Drag a location here to leave it out of every app.")}
            onMove={move}
            action={chosen !== null && <HeadAction onClick={() => select(null)}>{t(gmbsKeys.selectAll, "Select all")}</HeadAction>}
          >
            <ul className="flex flex-col">{outOfSelection.map(row)}</ul>
          </Group>
        </>
      )}
      <footer className="flex items-center">
        {scoped.length <= 1 && (
          <AddGmbButton labeled gmailId={scoped[0]?.id ?? null} trade={categories?.length === 1 && categories[0]!.trade !== "*" ? categories[0]!.trade : null} onAdded={changed} />
        )}
        <span className="flex-1" />
        <TrashButton />
      </footer>
    </section>
  );
}

/** The trash, with how much waits in it. */
function TrashButton() {
  const t = useT();
  const trash = useTrash();
  const router = useRouter();
  const pathname = usePathname();
  const button = useButtonSize();
  const n = trash.status === "ok" ? trash.data.gmails.length + trash.data.gmbs.length : null;
  return (
    <Button
      variant="ghost"
      size={button("sm")}
      className="text-ink-soft"
      onClick={() => {
        if (pathname === "/") openTrash(true);
        else router.push("/?trash");
      }}
    >
      <Trash2 aria-hidden />
      {t(trashKeys.open, "Trash")}
      {n !== null && n > 0 && <span className="tabular-nums">{n}</span>}
    </Button>
  );
}

/** A side of the selection, which a row dragged onto it moves to. */
function Group({
  into,
  className,
  heading,
  count,
  empty,
  action,
  onMove,
  children,
}: {
  into: boolean;
  className: string;
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
      className={cn("rounded-md transition-colors", className, over && "bg-hover/40 ring-1 ring-ring")}
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

/** The selected GBPs given one trade, or none: every one, or none of them. Each trade previews where they land, per country. */
function SetTrade({ gmbs, onSet }: { gmbs: Gmb[]; onSet: () => void }) {
  const t = useT();
  const trades = useTrades();
  const undoable = useUndoable();
  const give = async (trade: number | null) => {
    try {
      undoable(await setTrades(gmbs, trade, t));
      onSet();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  const byCountry = COUNTRIES.map((c) => [c, gmbs.filter((b) => b.country?.code === c).length] as const).filter(([, n]) => n > 0);
  const nowhere = gmbs.filter((b) => b.country === null).length;
  const untraded = gmbs.every((b) => b.trade_id === null);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="flex items-center gap-0.5 rounded px-1 text-xs font-normal tracking-normal normal-case outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
        {t(tradesKeys.set, "Set trade · {n}", { n: gmbs.length })}
        <ChevronDown aria-hidden className="size-3" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-96">
        <DropdownMenuLabel className="flex text-xs tracking-wide text-ink-soft uppercase">
          <span className="flex-1">{t(tradesKeys.set.title, "Set the trade of {n}", { n: gmbs.length })}</span>
          <span className="font-normal normal-case">{[...byCountry.map(([c, n]) => `${c} ${n}`), ...(nowhere > 0 ? [t(tradesKeys.set.nowhere, "{n} no country", { n: nowhere })] : [])].join(" · ")}</span>
        </DropdownMenuLabel>
        {trades.status === "ok" &&
          trades.data.map((tr) => (
            <DropdownMenuItem key={tr.id} onClick={() => void give(tr.id)}>
              <TagIcon aria-hidden />
              <span className="flex min-w-0 flex-col">
                {tr.name}
                <span className="truncate text-xs text-ink-soft">
                  {byCountry.map(([c, n], i) => {
                    const setUp = tr.categories.some((k) => k.country === c);
                    return (
                      <span key={c} className={cn(!setUp && "text-accent-warn")}>
                        {i > 0 && " · "}
                        {setUp ? `${tr.name} · ${c} ${n}` : t(tradesKeys.set.notSetUp, "{category} {n} · not set up", { category: `${tr.name} · ${c}`, n })}
                      </span>
                    );
                  })}
                </span>
              </span>
            </DropdownMenuItem>
          ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem disabled={untraded} onClick={() => void give(null)}>
          <X aria-hidden />
          <span className="flex flex-col">
            {t(tradesKeys.none, "No trade")}
            {untraded && <span className="text-xs text-ink-soft">{t(tradesKeys.set.untraded, "all {n} already have no trade", { n: gmbs.length })}</span>}
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function HeadAction({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" onClick={onClick} className="rounded px-1 text-xs font-normal tracking-normal normal-case outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      {children}
    </button>
  );
}

function GmbRow({
  listed: { gmb, gmail },
  tags,
  showGmail,
  onMap,
  peeked,
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
  peeked: boolean;
  current: boolean;
  selected: boolean;
  linked: ReadonlyMap<number, SiteTag> | null;
  onMove: (gmbs: readonly number[], into: boolean) => void;
  onFlip: (gmbs: Gmb[], enabled: boolean) => Promise<void>;
  onChanged: () => void;
  onNavigate?: () => void;
}) {
  const t = useT();
  const kept = useKept();
  const hovered = useOnMap((s) => s.hovered === gmb.id);
  const [dialog, setDialog] = useState<"edit" | "link" | "remove" | null>(null);
  const name = gmb.name;
  const move = (selected ? t(gmbsKeys.deselect, "Leave out") : t(gmbsKeys.select, "Select"));
  const issue = issueOf(gmb, tags);
  const archive = tags.archive?.get(gmb.id);
  const ready = gmb.state === "draft" ? readiness({ gmb, gmail }, tags).filter((r) => r.ready).length : null;
  const shown = issue === "draft" ? null : issue;
  const where = [gmb.address, showGmail ? gmail?.gmail : null].filter(Boolean).join(" · ");
  const label = (
    <>
      <span className="flex items-baseline gap-2">
        <span className={cn("min-w-0 flex-1 truncate", gmb.enabled ? "text-ink" : "text-ink-soft")}>{name}</span>
        {ready !== null && (
          <span className={cn("shrink-0 text-xs tabular-nums", ready === MUST_HAVES.length ? "text-positive" : "text-accent-warn")} title={t(gmbsKeys.readiness.title, "Must-haves ready before Google's form")}>
            {issueText("draft", { gmb, gmail }, tags, t)}
          </span>
        )}
      </span>
      <span className="block truncate text-xs text-ink-soft">
        {gmb.state !== "verified" && <span className={cn("font-medium", gmb.state === "suspended" && "text-accent-error")}>{STATE_NAME[gmb.state](t)}</span>}
        {gmb.state !== "verified" && (shown || where) && " · "}
        {shown && <span className={cn("font-medium", shown === "off" || shown === "no_category" ? "text-ink-soft" : "text-accent-error")}>{issueText(shown, { gmb, gmail }, tags, t)}</span>}
        {shown && where && " · "}
        {where}
      </span>
    </>
  );
  const body = "min-w-0 flex-1 py-1.5 pl-2 text-left";
  return (
    <li
      className={cn(ROW, (current || peeked || hovered) && "bg-hover/50")}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData(DRAGGED, String(gmb.id));
        e.dataTransfer.effectAllowed = "move";
      }}
      onMouseEnter={() => setOnMap({ hovered: gmb.id })}
      onMouseLeave={() => setOnMap({ hovered: null })}
    >
      {/* the ⋮'s dots are the state's, so the menu takes no width of its own */}
      <DropdownMenu>
        <DropdownMenuTrigger aria-label={t(gmbsKeys.actions, "{name}: actions", { name })} title={STATE_NAME[gmb.state](t)} className="flex w-4 shrink-0 flex-col items-center justify-center gap-0.5 self-stretch rounded-sm outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-hover">
          {[0, 1, 2].map((i) => (
            <span key={i} aria-hidden className={cn("size-1.5 rounded-full", DOT[gmb.state], !gmb.enabled && "border-ink-soft bg-ink-soft", gmb.location === null && "opacity-40")} />
          ))}
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-48">
          <DropdownMenuItem onClick={() => onMove([gmb.id], !selected)}>
            {selected ? <CircleMinus aria-hidden /> : <CirclePlus aria-hidden />}
            {move}
          </DropdownMenuItem>
          {gmb.state !== "draft" && (
            <DropdownMenuItem onClick={() => setDialog("edit")}>
              <Pencil aria-hidden />
              {t(gmbsKeys.edit, "Edit…")}
            </DropdownMenuItem>
          )}
          <PlaceButton gmb={gmb} item {...(onNavigate && { onNavigate })} />
          {linked && (
            <DropdownMenuItem onClick={() => setDialog("link")}>
              <Link2 aria-hidden />
              {linked.has(gmb.id) ? t(gmbsKeys.tag.site, "site {place}", { place: `${linked.get(gmb.id)!.brand}/${linked.get(gmb.id)!.location}` }) : t(gmbsKeys.tag.site.link, "site: link…")}
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onClick={() => setDialog("remove")}>
            <Trash2 aria-hidden />
            {t(gmbsKeys.remove, "Remove")}…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {dialog === "edit" && <EditGbp gmb={gmb} onClose={() => setDialog(null)} onChanged={onChanged} />}
      {dialog === "link" && linked && <LinkPlace gmbId={gmb.id} linked={linked} onClose={() => setDialog(null)} onDone={onChanged} />}
      {dialog === "remove" && <RemoveDialog title={t(gmbsKeys.gmb.remove.title, "Remove {name}?", { name })} body={t(gmbsKeys.gmb.remove.body, "It goes to the trash{kept}: every service stops working on it until then; its history stays.", { kept })} remove={() => removeGmb(gmb.id)} onRemoved={onChanged} onClose={() => setDialog(null)} />}
      {onMap ? (
        <button type="button" className={body} title={gmb.address ? `${name}\n${gmb.address}` : name} onClick={() => openOrFocus(gmb, peeked)}>
          {label}
        </button>
      ) : TABS[gmb.state][0] !== undefined ? (
        <Link href={`/?gmb=${gmb.id}&tab=${TABS[gmb.state][0]}`} onClick={() => onNavigate?.()} className={body} title={name}>
          {label}
        </Link>
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
      {archive && <LiveReviews days={archive.live_30d} />}
      <Switch aria-label={t(gmbsKeys.switch, "{name} on or off", { name })} checked={gmb.enabled} onCheckedChange={(v) => void onFlip([gmb], v)} />
    </li>
  );
}
