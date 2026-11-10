"use client";

import { Button, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger, InputGroup, InputGroupAddon, InputGroupInput, Skeleton, Switch, cn } from "@evinvest/uikit";
import { ChevronDown, CircleMinus, CirclePlus, Search, Tag as TagIcon, X } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { type ReactNode, useEffect, useState } from "react";

import {
  COUNTRIES,
  type Gmb,
  ISSUES,
  type Listed,
  MUST_HAVES,
  type SiteTag,
  type State,
  TABS,
  type Tags,
  byView,
  chooseGmailScope,
  issueCounts,
  issueOf,
  issueText,
  open,
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
} from "@/entities/gmb";
import { may, useMe } from "@/entities/session";
import { AddGmbButton } from "@/features/add-gmb";
import { EditGbpButton } from "@/features/edit-gbp";
import { LinkPlaceButton } from "@/features/link-gmb-place";
import { PlaceButton } from "@/features/place-gmb";
import { ROUTES } from "@/shared/config/routes";
import { useT } from "@/shared/i18n";
import { ErrorState } from "@/shared/ui/error-state";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

import { HEADING, ON_HOVER, ROW, Remove } from "./outline-parts";
import { ScopePicker } from "./scope-picker";

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
  const { gmails: scope, categories } = useScope();
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
    // NOT SELECTED and the footer stay in reach under a long SELECTED, which scrolls alone
    <section aria-label={t("gmbs.title")} className="flex min-h-0 flex-1 flex-col gap-1">
      <ScopePicker data={gmails.data} selection={selection} onFlip={flip} onChanged={changed} />
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
                only !== i && (i === "off" || i === "draft" || i === "no_category" ? "text-ink-soft" : "text-accent-error"),
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
            className="min-h-0 flex-1 overflow-y-auto"
            heading={t("gmbs.selected")}
            count={inSelection.length}
            empty={t("gmbs.selected.empty")}
            onMove={move}
            action={inSelection.length > 0 && <SetTrade gmbs={inSelection.map((l) => l.gmb)} onSet={changed} />}
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
            className="max-h-2/5 shrink-0 overflow-y-auto border-t border-border"
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
          <AddGmbButton gmailId={scoped[0]?.id ?? null} trade={categories?.length === 1 && categories[0]!.trade !== "*" ? categories[0]!.trade : null} onAdded={changed} />
          {t("gmbs.gmb.add")}
        </div>
      )}
    </section>
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
  const give = async (trade: number | null) => {
    try {
      await setTrades(
        gmbs.map((b) => b.id),
        trade,
      );
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
        {t("trades.set", { n: gmbs.length })}
        <ChevronDown aria-hidden className="size-3" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-96">
        <DropdownMenuLabel className="flex text-xs tracking-wide text-ink-soft uppercase">
          <span className="flex-1">{t("trades.set.title", { n: gmbs.length })}</span>
          <span className="font-normal normal-case">{[...byCountry.map(([c, n]) => `${c} ${n}`), ...(nowhere > 0 ? [t("trades.set.nowhere", { n: nowhere })] : [])].join(" · ")}</span>
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
                        {setUp ? `${tr.name} · ${c} ${n}` : t("trades.set.notSetUp", { category: `${tr.name} · ${c}`, n })}
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
            {t("trades.none")}
            {untraded && <span className="text-xs text-ink-soft">{t("trades.set.untraded", { n: gmbs.length })}</span>}
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
  const ready = gmb.state === "draft" ? readiness({ gmb, gmail }, tags).filter((r) => r.ready).length : null;
  const key =
    ready !== null
      ? { text: issueText("draft", { gmb, gmail }, tags, t), title: t("gmbs.readiness.title"), tone: ready === MUST_HAVES.length ? "text-positive" : "text-accent-warn" }
      : tags.sites !== null
        ? site && { text: String(site.leads_7d), title: t("gmbs.leads7d.title"), tone: "text-ink-soft" }
        : archive?.rating != null && { text: `★ ${archive.rating.toFixed(1)}`, title: t("gmbs.rating.title"), tone: "text-ink-soft" };
  const shown = issue === "draft" ? null : issue;
  const where = [gmb.address, showGmail ? gmail?.gmail : null].filter(Boolean).join(" · ");
  const label = (
    <>
      <span className="flex items-baseline gap-2">
        <span className={cn("min-w-0 flex-1 truncate", gmb.enabled ? "text-ink" : "text-ink-soft")}>{name}</span>
        {key && (
          <span className={cn("shrink-0 text-xs tabular-nums", key.tone)} title={key.title}>
            {key.text}
          </span>
        )}
      </span>
      <span className="block truncate text-xs text-ink-soft">
        {gmb.state !== "verified" && <span className={cn("font-medium", gmb.state === "suspended" && "text-accent-error")}>{t(`gmbs.state.${gmb.state}`)}</span>}
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
        <button type="button" className={body} title={gmb.address ? `${name}\n${gmb.address}` : name} onClick={() => open(gmb)}>
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
