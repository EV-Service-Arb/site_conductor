# Alerts

A **note** says something happened. Its **rule** says what kind, who may hear of it and how much it
matters (**tier**). A person reads their notes in the panel (the **inbox**) and is reached outside it
at **destinations**, each saying *when* it is told of each tier (**cadence**), *which rules* differ
and *which GBPs* it is for (**filter**).

```text
 sites ───────┐ notes            ┌─▶ inbox ── bell · /user/alerts · toast · urgent takes the hotbar
 review_archive ├──▶ the hub ────┤
 conductor ───┘  tier, audience  └─▶ each destination: filter ─▶ cadence ─▶ outbox ─▶ transport
                                                                  │
                             now · unread · hourly · daily · off ─┘   quiet hours, snooze hold all but urgent
```

## Tiers

| tier | means | shown |
|---|---|---|
| `urgent` | minutes cost money: call now | red; takes the hotbar; pierces quiet hours and snooze |
| `important` | today, by you | amber |
| `info` | nice to know | neutral |

A note's tier: the person's own for the rule, else the note's (a lead that only compares prices is
`important`, one for today `urgent`), else the rule's.

## Rules

| family | rule | from | audience | tier |
|---|---|---|---|---|
| leads | new_lead, contact_overdue | sites | sa:sites:work:leads:edit | urgent |
| leads | booked | sites | sa:sites:work:leads:edit | important |
| leads | payment_received | sites | sa:admin:sources:manage | info |
| reviews | review_gone | review_archive | the GBPs' owners | important |
| reviews | review_new, review_changed (≤ 3★: important), review_reappeared, scan_failed | review_archive | the GBPs' owners | info |
| profiles | google_mail (a suspension: urgent; routine: info) | conductor | the gmail's owner | important |
| profiles | connection_lost (its inbox fails) | conductor | the gmail's owner | important |
| money | tokens_low, browser_stopped | conductor | the person | important |
| money | kickback | conductor | the person | info |
| panel | assistant_waiting | conductor | the person | important |
| panel | access_granted, release_out, trash_purge | conductor | the person | info |
| team | access_requested | conductor | every permission of sa:admin | important |
| team | source_silent, feed_stalled, meter_held (billing unlinked: urgent) | conductor | sa:admin:sources:manage | important |

## Destinations

| method | address | private | proved by |
|---|---|---|---|
| telegram | own chat, channel, group, topic | own chat | /start; admin + a test |
| email | the account's verified address, or another | yes | concierge; a link mailed there |
| discord | a DM from the bot; a channel's webhook | DM | OAuth; the webhook answers + a test |
| slack | a channel's incoming webhook | no | a test |
| push | one browser, one phone (Web Push) | yes | its subscription |
| webhook | an https URL of theirs: JSON, HMAC-signed | no | a test |

PII and buttons go only to a private destination, and PII only to a holder of
`sa:sites:work:pii:see`. Every secret part of an address is sealed.

**Cadence**, one per tier, and one per rule where it differs:

| cadence | |
|---|---|
| now | at once |
| unread | if still unread in the panel 10 min later |
| hourly | one message an hour, what came in it |
| daily | one message a day, at the person's hour |
| off | never |

| default | urgent | important | info |
|---|---|---|---|
| own chat, DM, push | now | now | off |
| email | unread | hourly | daily |
| channel, group, slack | now | now | daily |
| webhook | now | now | now |

**Filter**: gmails, trades (gcid), brands, GBPs. A facet left empty takes all; a note passes when one
GBP it names is in every facet set. A note naming no GBP passes.

**Quiet hours** (the person's, in their time zone) and **snooze** hold everything but urgent, then
send it as one message.

Reading a note anywhere (the inbox, its link, a button pressed) marks it read everywhere; what was
still waiting for it as `unread` is dropped.

## Transports

Every network's endpoints, payloads and failures are `social_networks_adapters::notify`'s
(valeratrades/social_networks, feature `notify`); the conductor decides who, when and what, and
keeps the outbox, the pace and the retries.
