"use client";

import { CloseButton, Button, Card, CardContent, CardDescription, CardHeader, CardTitle, buttonVariants, cn } from "@evinvest/uikit";

import { CategoryChip, type Listed, issueOf, issueText, open, useGmbTags, gmbs } from "@/entities/gmb";
import { ViewLink } from "@/shared/lib/location";
import { useT, home } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

/** A GMB's card over the map: its numbers, what needs the operator, and where its services show it. */
export function Peek({ listed, onClose, className }: { listed: Listed; onClose: () => void; className?: string }) {
  const t = useT();
  const button = useButtonSize();
  const { tags } = useGmbTags();
  const { gmb, gmail } = listed;
  const archive = tags.archive?.get(gmb.id);
  const site = tags.sites?.get(gmb.id);
  const issue = issueOf(gmb, tags);
  const name = gmb.name;
  const reviewsNote = archive && (archive.suspended > 0 ? t(gmbs.tag.suspended, "{n} reviews suspended", { n: archive.suspended }) : archive.failing ? t(gmbs.issue.failing, "Failing") : null);
  const numbers = [
    archive?.rating != null && archive.reviews !== null && t(home.peek.rating, "★ {rating} ({n} reviews)", { rating: archive.rating.toFixed(1), n: archive.reviews }),
    site && (site.overdue > 0 ? t(home.peek.leadsOverdue, "{n} leads in 7 days ({overdue} overdue)", { n: site.leads_7d, overdue: site.overdue }) : t(home.peek.leads, "{n} leads in 7 days", { n: site.leads_7d })),
  ].filter(Boolean);
  return (
    <Card className={cn("w-80 gap-3 py-4 shadow-overlay", className)}>
      <CardHeader className="flex items-start gap-2 px-4">
        <div className="min-w-0 flex-1">
          <CardTitle className="truncate" title={name}>
            {name}
          </CardTitle>
          <CardDescription>{[gmb.address, gmail?.gmail].filter(Boolean).join(" · ")}</CardDescription>
          <div className="pt-1">
            <CategoryChip gmb={gmb} />
          </div>
        </div>
        <CloseButton size={button("sm")} label={t(home.peek.close, "Close")} onClose={onClose} />
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-4">
        <div className="flex flex-col gap-0.5 text-sm">
          {numbers.length > 0 && <p className="text-ink tabular-nums">{numbers.join(" · ")}</p>}
          <p className={cn(issue === null || issue === "off" || issue === "draft" || issue === "no_category" ? "text-ink-soft" : "font-medium text-accent-error")}>
            {issue === null ? t(home.peek.fine, "Nothing needs you") : issue === "off" ? t(home.peek.off, "Switched off: the services do no work on it.") : issueText(issue, listed, tags, t)}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {gmb.tabs.length > 0 && (
            <Button size={button("sm")} onClick={() => open(gmb)}>
              {t(home.peek.open, "Open")}
            </Button>
          )}
          <ViewLink to={{ view: "review_archive.mine.gmb", params: { gmb: gmb.id } }} className={buttonVariants({ variant: "outline", size: button("sm") })}>
            {reviewsNote ? `${t(home.peek.reviews, "Reviews")} · ${reviewsNote}` : t(home.peek.reviews, "Reviews")}
          </ViewLink>
          {site && (
            <ViewLink to={{ view: "sites", params: { below: "/places" } }} className={buttonVariants({ variant: "outline", size: button("sm") })}>
              {t(gmbs.tag.site, "site {place}", { place: `${site.brand}/${site.location}` })}
              {site.overdue > 0 && ` · ${t(gmbs.tag.overdue, "{n} overdue", { n: site.overdue })}`}
            </ViewLink>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
