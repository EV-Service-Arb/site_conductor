import { goes } from "@/entities/gmb";
import type { MailAutos, MailCase, MailHalt } from "@/shared/api/schema";
import type { Locale, T } from "@/shared/i18n";
import { when } from "@/shared/lib/time";

import { mail } from "./i18n";
import { TOPIC_NAME } from "./names";

function haltText(h: Exclude<MailHalt, { is: "off" }>, t: T, locale: Locale, now: Date): string {
  switch (h.is) {
    case "uncertain":
      return t(mail.halt.uncertain, "which GBP it is about is not certain");
    case "removed":
      return t(mail.halt.removed, "its GBP is in the trash");
    case "unread":
      return t(mail.halt.unread, "Google's last word is not read");
    case "no_reply":
      return t(mail.halt.no_reply, "nothing in it takes a reply");
    case "elsewhere":
      return t(mail.halt.elsewhere, "its GBP's latest conversation on this asks");
    case "settled":
      return t(mail.halt.settled, "what it asks for is given");
    case "asked":
      return t(mail.halt.asked, "what Google asks is yours to give");
    case "limit":
      return t(mail.halt.limit, "nothing new in {n}: the next is yours to send", { n: h.asked });
    case "google":
      return t(mail.halt.google, "Google's turn until {until}, then it asks again", { until: when(h.until, locale, now) });
    case "operators":
      return t(mail.halt.operators, "what Google said is yours to act on");
    case "account":
      return t(mail.halt.account, "its inbox is two accounts', or it was written to another");
    case "stale":
      return t(mail.halt.stale, "its inbox was not read for an hour");
  }
}

/** What comes next by itself, once nothing halts it. */
function next(c: MailCase, t: T, locale: Locale, now: Date): string {
  switch (c.state.is) {
    case "sending":
      return goes(c) ? t(mail.automation.goes, "goes at {at}", { at: when(c.state.at, locale, now) }) : t(mail.state.sending, "sending");
    case "reply_due":
      return t(mail.automation.due, "asks next once its reply is written");
    case "waiting":
      return t(mail.automation.waiting, "asks again after {until}", { until: when(c.state.until, locale, now) });
    default:
      return c.wanted.some((w) => !w.given) ? t(mail.automation.lacks, "what its reply lacks is yours to give") : t(mail.automation.yours, "this reply is yours to send");
  }
}

/** Whether a case is answered by itself, how often Google was asked, and when it is next, or why not. */
export function automation(c: MailCase, autos: MailAutos, t: T, locale: Locale, now: Date): string {
  const { halt, asked, limit } = c.auto;
  const times = t(mail.automation.asked, "asked {n} of {limit}", { n: asked, limit });
  if (!autos.topics.includes(c.topic)) return [t(mail.automation.never, "Never automatic on {topic}", { topic: TOPIC_NAME[c.topic](t) }), asked > 0 && times].filter(Boolean).join(" · ");
  if (!autos.on.some((x) => x.gmb_id === c.gmb_id && x.topic === c.topic)) return [c.gmb_id === null ? t(mail.automation.unknown, "Automatic replies off: which GBP it is about is not known") : t(mail.automation.off, "Automatic replies off"), times].join(" · ");
  if (!autos.by_itself) return [t(mail.automation.on, "Automatic replies on"), times, t(mail.automation.here, "nothing goes from this conductor")].join(" · ");
  // `off`: the case was read before the switch
  return [t(mail.automation.on, "Automatic replies on"), times, halt === null ? next(c, t, locale, now) : halt.is === "off" ? null : haltText(halt, t, locale, now)].filter(Boolean).join(" · ");
}
