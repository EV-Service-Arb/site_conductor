import { Bot, Euro, Mail, Phone, Smartphone, Star, Store, Users, Webhook } from "lucide-react";
import type { ComponentType } from "react";

import { type Cadence, type Family, type Method, type Rule, type Tier, cadenceValues, familyValues, tierValues } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

import { DiscordIcon, SlackIcon, TelegramIcon } from "../ui/brand-icons";
import { alerts } from "./i18n";

type Icon = ComponentType<{ className?: string }>;
type Named<K extends string> = Record<K, (t: T) => string>;

/** Most pressing first. */
export const TIERS = ["urgent", "important", "info"] as const satisfies readonly Tier[];
export const CADENCES = ["now", "unread", "hourly", "daily", "off"] as const satisfies readonly Cadence[];
export const FAMILIES = ["leads", "reviews", "profiles", "money", "panel", "team"] as const satisfies readonly Family[];
if (TIERS.length !== tierValues.length || CADENCES.length !== cadenceValues.length || FAMILIES.length !== familyValues.length) throw new Error("a tier, cadence or family has no place in the page's order");

export const TIER_NAME: Named<Tier> = {
  urgent: (t) => t(alerts.tier.urgent, "Urgent"),
  important: (t) => t(alerts.tier.important, "Important"),
  info: (t) => t(alerts.tier.info, "Info"),
};

export const TIER_MEANS: Named<Tier> = {
  urgent: (t) => t(alerts.tier.urgent.means, "minutes cost money"),
  important: (t) => t(alerts.tier.important.means, "today, by you"),
  info: (t) => t(alerts.tier.info.means, "nice to know"),
};

/** A tier's ink, its dot and the circle behind a row's icon. */
export const TIER_LOOK: Record<Tier, { ink: string; dot: string; wash: string }> = {
  urgent: { ink: "text-accent-error", dot: "bg-accent-error", wash: "bg-tint-error-fill text-accent-error" },
  important: { ink: "text-accent-warn", dot: "bg-accent-warn", wash: "bg-tint-warn-fill text-accent-warn" },
  info: { ink: "text-ink", dot: "bg-ink-soft", wash: "bg-secondary text-ink-soft" },
};

export const FAMILY_NAME: Named<Family> = {
  leads: (t) => t(alerts.family.leads, "Leads"),
  reviews: (t) => t(alerts.family.reviews, "Reviews"),
  profiles: (t) => t(alerts.family.profiles, "Profiles"),
  money: (t) => t(alerts.family.money, "Money"),
  panel: (t) => t(alerts.family.panel, "Panel"),
  team: (t) => t(alerts.family.team, "Team"),
};

export const FAMILY_ICON: Record<Family, Icon> = { leads: Phone, reviews: Star, profiles: Store, money: Euro, panel: Bot, team: Users };

export const RULE_NAME: Named<Rule> = {
  new_lead: (t) => t(alerts.rule.new_lead, "New leads"),
  contact_overdue: (t) => t(alerts.rule.contact_overdue, "Leads waiting too long"),
  booked: (t) => t(alerts.rule.booked, "Bookings"),
  payment_received: (t) => t(alerts.rule.payment_received, "Payments"),
  review_new: (t) => t(alerts.rule.review_new, "New reviews"),
  review_changed: (t) => t(alerts.rule.review_changed, "Edited reviews"),
  review_gone: (t) => t(alerts.rule.review_gone, "Removed reviews"),
  review_reappeared: (t) => t(alerts.rule.review_reappeared, "Reviews back"),
  scan_failed: (t) => t(alerts.rule.scan_failed, "Failed scans"),
  google_mail: (t) => t(alerts.rule.google_mail, "Mail from Google"),
  connection_lost: (t) => t(alerts.rule.connection_lost, "Lost connections"),
  tokens_low: (t) => t(alerts.rule.tokens_low, "Low tokens"),
  browser_stopped: (t) => t(alerts.rule.browser_stopped, "Stopped browsers"),
  kickback: (t) => t(alerts.rule.kickback, "Kickbacks"),
  assistant_waiting: (t) => t(alerts.rule.assistant_waiting, "Assistant waiting for you"),
  access_granted: (t) => t(alerts.rule.access_granted, "Access granted"),
  release_out: (t) => t(alerts.rule.release_out, "New releases"),
  trash_purge: (t) => t(alerts.rule.trash_purge, "Trash purges"),
  access_requested: (t) => t(alerts.rule.access_requested, "Access requests"),
  source_silent: (t) => t(alerts.rule.source_silent, "Silent sources"),
  feed_stalled: (t) => t(alerts.rule.feed_stalled, "Stalled feeds"),
  meter_held: (t) => t(alerts.rule.meter_held, "Held meters"),
};

/** The rules whose notes carry a tier of their own, and how; the others come at their rule's. */
export const VARIES: Partial<Named<Rule>> = {
  new_lead: (t) => t(alerts.varies.new_lead, "for today Urgent, comparing prices Info"),
  review_new: (t) => t(alerts.varies.review, "★3 or less Important, else Info"),
  review_changed: (t) => t(alerts.varies.review, "★3 or less Important, else Info"),
  google_mail: (t) => t(alerts.varies.google_mail, "a suspension Urgent, else Important"),
};

export const RULE_HINT: Partial<Named<Rule>> = {
  connection_lost: (t) => t(alerts.hint.connection_lost, "inbox, Business Profile, proxy"),
};

export const CADENCE_NAME: Named<Cadence> = {
  now: (t) => t(alerts.cadence.now, "Now"),
  unread: (t) => t(alerts.cadence.unread, "If unread"),
  hourly: (t) => t(alerts.cadence.hourly, "Hourly"),
  daily: (t) => t(alerts.cadence.daily, "Daily"),
  off: (t) => t(alerts.cadence.off, "Off"),
};

export const METHOD_NAME: Named<Method> = {
  telegram: () => "Telegram",
  email: (t) => t(alerts.method.email, "Email"),
  push: (t) => t(alerts.method.push, "Push"),
  discord: () => "Discord",
  discord_channel: () => "Discord",
  slack: () => "Slack",
  webhook: () => "Webhook",
};

export const METHOD_ICON: Record<Method, Icon> = { telegram: TelegramIcon, email: Mail, push: Smartphone, discord: DiscordIcon, discord_channel: DiscordIcon, slack: SlackIcon, webhook: Webhook };
