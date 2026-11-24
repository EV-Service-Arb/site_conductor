import type { Tone } from "@evinvest/uikit";

import { type MailKind, type MailTopic, type MailWho, type VerificationMethod, type VerificationOutcome, mailKindValues } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

import { mail } from "./i18n";

type Named<K extends string> = Record<K, (t: T) => string>;

export const TOPIC_NAME: Named<MailTopic> = {
  verification: (t) => t(mail.topic.verification, "Verification"),
  suspension: (t) => t(mail.topic.suspension, "Suspension"),
  reviews: (t) => t(mail.topic.reviews, "Reviews"),
  activity: (t) => t(mail.topic.activity, "Activity"),
  access: (t) => t(mail.topic.access, "Access"),
  profile: (t) => t(mail.topic.profile, "Profile"),
  other: (t) => t(mail.topic.other, "Other"),
};

export const TOPIC_TONE: Record<MailTopic, Tone> = { verification: "warn", suspension: "error", reviews: "info", activity: "warn", access: "neutral", profile: "neutral", other: "neutral" };

export const WHO_NAME: Named<MailWho> = {
  us: (t) => t(mail.message.us, "You"),
  support: (t) => t(mail.who.support, "Google support"),
  google: (t) => t(mail.who.google, "Google"),
  unproven: (t) => t(mail.kind.impersonation, "Not from Google"),
};

/** A topic's kinds, as a person picks one; `null`: of any topic. */
export const KINDS: readonly { topic: MailTopic | null; kinds: readonly MailKind[] }[] = [
  { topic: null, kinds: ["asks", "acknowledged", "routed", "closed", "unclear"] },
  { topic: "verification", kinds: ["video_rejected", "video_in_review", "verified", "reverification_required", "self_serve", "call_offered", "call_failed", "options_exhausted"] },
  { topic: "suspension", kinds: ["suspended", "account_restricted", "appeal_received", "appeal_approved", "appeal_denied"] },
  { topic: "reviews", kinds: ["reviews_paused", "reviews_restricted", "restriction_lifted", "review_removed", "reinstatement_refused", "reinstatement_done", "reviews_live_already", "reviews_no_data"] },
  { topic: "activity", kinds: ["still_active"] },
  { topic: "access", kinds: ["invited", "access_granted", "access_lost", "ownership_requested", "management_requested"] },
  { topic: "profile", kinds: ["edit_suggested", "edit_rejected", "changed_by_google", "content_removed", "policy_warning", "duplicate"] },
  { topic: "other", kinds: ["noise", "not_business", "impersonation"] },
];

if (KINDS.flatMap((g) => g.kinds).length !== mailKindValues.length) throw new Error("a kind of mail has no place among those a person picks");

export const KIND_NAME: Named<MailKind> = {
  video_rejected: (t) => t(mail.kind.video_rejected, "Video rejected"),
  video_in_review: (t) => t(mail.kind.video_in_review, "Video in review"),
  verified: (t) => t(mail.kind.verified, "Verified"),
  reverification_required: (t) => t(mail.kind.reverification_required, "To verify again"),
  self_serve: (t) => t(mail.kind.self_serve, "Pointed back to the video"),
  call_offered: (t) => t(mail.kind.call_offered, "Live call offered"),
  call_failed: (t) => t(mail.kind.call_failed, "Live call failed"),
  options_exhausted: (t) => t(mail.kind.options_exhausted, "No way to verify left"),
  suspended: (t) => t(mail.kind.suspended, "Suspended"),
  account_restricted: (t) => t(mail.kind.account_restricted, "Account restricted"),
  appeal_received: (t) => t(mail.kind.appeal_received, "Appeal received"),
  appeal_approved: (t) => t(mail.kind.appeal_approved, "Appeal approved"),
  appeal_denied: (t) => t(mail.kind.appeal_denied, "Appeal denied"),
  reviews_paused: (t) => t(mail.kind.reviews_paused, "Reviews paused"),
  reviews_restricted: (t) => t(mail.kind.reviews_restricted, "Reviews restricted"),
  restriction_lifted: (t) => t(mail.kind.restriction_lifted, "Restriction lifted"),
  review_removed: (t) => t(mail.kind.review_removed, "Review removed"),
  reinstatement_refused: (t) => t(mail.kind.reinstatement_refused, "Reinstatement refused"),
  reinstatement_done: (t) => t(mail.kind.reinstatement_done, "Reviews reinstated"),
  reviews_live_already: (t) => t(mail.kind.reviews_live_already, "Sees the reviews live"),
  reviews_no_data: (t) => t(mail.kind.reviews_no_data, "Which reviews?"),
  still_active: (t) => t(mail.kind.still_active, "Still open?"),
  invited: (t) => t(mail.kind.invited, "Invited to manage"),
  access_granted: (t) => t(mail.kind.access_granted, "Access granted"),
  access_lost: (t) => t(mail.kind.access_lost, "Access lost"),
  ownership_requested: (t) => t(mail.kind.ownership_requested, "Ownership requested"),
  management_requested: (t) => t(mail.kind.management_requested, "Management requested"),
  edit_suggested: (t) => t(mail.kind.edit_suggested, "Edit suggested"),
  edit_rejected: (t) => t(mail.kind.edit_rejected, "Edit rejected"),
  changed_by_google: (t) => t(mail.kind.changed_by_google, "Changed by Google"),
  content_removed: (t) => t(mail.kind.content_removed, "Content removed"),
  policy_warning: (t) => t(mail.kind.policy_warning, "Policy warning"),
  duplicate: (t) => t(mail.kind.duplicate, "Marked a duplicate"),
  acknowledged: (t) => t(mail.kind.acknowledged, "Received"),
  routed: (t) => t(mail.kind.routed, "Passed to a specialist"),
  asks: (t) => t(mail.kind.asks, "Asks for something"),
  closed: (t) => t(mail.kind.closed, "Closed"),
  unclear: (t) => t(mail.kind.unclear, "Unclear"),
  noise: (t) => t(mail.kind.noise, "Routine"),
  not_business: (t) => t(mail.kind.not_business, "Not about a GBP"),
  impersonation: (t) => t(mail.kind.impersonation, "Not from Google"),
};

/** A kind that decides something; any other is neutral. */
export const KIND_TONE: Partial<Record<MailKind, Tone>> = {
  verified: "positive",
  call_offered: "warn",
  appeal_approved: "positive",
  restriction_lifted: "positive",
  reinstatement_done: "positive",
  video_rejected: "error",
  call_failed: "error",
  options_exhausted: "error",
  suspended: "error",
  account_restricted: "error",
  appeal_denied: "error",
  reinstatement_refused: "error",
  impersonation: "error",
};

/** What a kind that is the operator's asks of them. */
export const KIND_ASKS: Partial<Named<MailKind>> = {
  asks: (t) => t(mail.why.asks, "Google asks for something, and the reply is written: check it and send it."),
  video_rejected: (t) => t(mail.why.video_rejected, "Google rejected the recorded video, and no open conversation asks for the live call."),
  reverification_required: (t) => t(mail.why.reverification_required, "Google wants it verified again: start it from the profile."),
  call_offered: (t) => t(mail.why.call_offered, "The live call is enabled: take it from the profile, at the place."),
  call_failed: (t) => t(mail.why.call_failed, "A live call was held and did not verify it."),
  suspended: (t) => t(mail.why.suspended, "Google suspended the profile: the appeal is sent from Google's own form."),
  account_restricted: (t) => t(mail.why.account_restricted, "Google restricted the gmail itself, not one profile."),
  appeal_denied: (t) => t(mail.why.appeal_denied, "Google denied the appeal."),
  reviews_paused: (t) => t(mail.why.reviews_paused, "Google paused new reviews: it saw a spike of spam."),
  reviews_restricted: (t) => t(mail.why.reviews_restricted, "Google found fake or incentivised reviews and restricted the profile's."),
  reviews_live_already: (t) => t(mail.why.reviews_live_already, "Support sees the reviews live: check which are missing on Maps."),
  still_active: (t) => t(mail.why.still_active, "Google asks whether it is still open, and removes it unless answered."),
  ownership_requested: (t) => t(mail.why.ownership_requested, "Somebody asks for its ownership: accept or refuse it on the profile."),
  management_requested: (t) => t(mail.why.management_requested, "Somebody asks to manage it: accept or refuse it on the profile."),
  edit_suggested: (t) => t(mail.why.edit_suggested, "Somebody suggested an edit: accept or refuse it on the profile."),
  policy_warning: (t) => t(mail.why.policy_warning, "Google warns of a policy broken on the profile."),
  duplicate: (t) => t(mail.why.duplicate, "Google marked it another profile's duplicate."),
};

export const METHOD_NAME: Named<VerificationMethod> = {
  video: (t) => t(mail.method.video, "Video"),
  call: (t) => t(mail.method.call, "Live call"),
  unknown: (t) => t(mail.method.unknown, "Verification"),
};

export const OUTCOME_NAME: Named<VerificationOutcome> = {
  in_review: (t) => t(mail.outcome.in_review, "in review"),
  rejected: (t) => t(mail.outcome.rejected, "rejected"),
  offered: (t) => t(mail.outcome.offered, "offered"),
  failed: (t) => t(mail.outcome.failed, "failed"),
  verified: (t) => t(mail.outcome.verified, "verified"),
};

export const OUTCOME_TONE: Record<VerificationOutcome, Tone> = { in_review: "info", rejected: "error", offered: "warn", failed: "error", verified: "positive" };
