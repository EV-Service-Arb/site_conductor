#set page(width: 297mm, height: auto, margin: 14mm)
#set text(size: 9.5pt)
#set par(justify: false)
#show raw.where(block: true): it => block(
  fill: luma(248),
  inset: 8pt,
  radius: 3pt,
  width: 100%,
  text(size: 7.6pt, it),
)
#import "@preview/fletcher:0.5.8" as fletcher: diagram, edge, node

= Review kit — from a scanned QR to a posted Google review

The operator shows a place's QR on their phone; the reviewer scans it with theirs. What the
reviewer's phone does next depends on its OS and on whether the Google Maps app is installed.
Findings come from decompiling Google Maps Android 26.45.01, Google's live app-site-association
files and headless Chrome 156 (2026-11-12, `tmp/review_kit/feasibility.md`). UNVERIFIED marks
what no real device has confirmed.

== 1. What Google's review form takes from outside

```
                         Android · Maps app                      iPhone · Maps app
 open the form by link   writereview/mobile?placeid=<id>         none: /local/writereview is not among
                         (an App Link: Maps claims the path)     the paths Maps claims (/maps, /maps/place/*,
                                                                 /maps/reviews/*, …)
 nearest link we have    the form itself                         maps/place//data=!4m3!3m2!1s<fid>!12e1
                                                                 → Maps, the place; whether !12e1 opens the
                                                                 form or only the place sheet: UNVERIFIED
 text, stars, photos     the link handler reads `placeid` and    no route in at all
   passed in             `source`, nothing else
 without the Maps app    writereview/mobile is a 404 on the web  Maps web, signed out: drops !12e1,
                         → writereview?placeid= in the browser   shows the place + "open the app?"
                         → Google sign-in                        → Google sign-in
```

Text, rating and photos cannot be put into the form on either OS; the page hands the text over
through the clipboard. The sign-in wall without the app is the same on both.

#align(center, diagram(
  spacing: (14mm, 9mm),
  node-corner-radius: 2pt,
  node-stroke: 0.7pt,
  label-size: 7.5pt,
  node((0, 0), [our page, a tap], fill: luma(235), name: <tap>),
  node((-1.6, 1.2), [Android \ Maps app?], name: <a>),
  node((1.6, 1.2), [iPhone \ Maps app?], name: <i>),
  node((-2.4, 2.4), [Maps review form], fill: rgb("#dcfce7"), name: <af>),
  node((2.6, 2.4), [Maps place sheet \ #text(7pt)[form: UNVERIFIED]], fill: rgb("#fef9c3"), name: <ip>),
  node((0, 3.6), [browser form \ Google sign-in], fill: rgb("#fee2e2"), name: <web>),
  edge(<tap>, <a>, "->"),
  edge(<tap>, <i>, "->"),
  edge(<a>, <af>, "->", label: [yes · `intent://…/mobile`]),
  edge(<a>, <web>, "->", label: [no · fallback url], label-side: left),
  edge(<i>, <ip>, "->", label: [yes · `!12e1`], label-side: left),
  edge(<i>, <web>, "->", label: [no · Maps web], label-side: right),
))

== 2. The link our page emits

One QR serves both OSes, so the QR points at our page and the page picks the link by user agent.

```
 Android   intent://search.google.com/local/writereview/mobile?placeid=<id>
             #Intent;scheme=https;package=com.google.android.apps.maps;
             S.browser_fallback_url=<https://search.google.com/local/writereview?placeid=<id>>;end
           the intent names Maps, so a browser that claims the https link cannot take it;
           no Maps → the fallback
 iPhone    https://www.google.com/maps/place//data=!4m3!3m2!1s<fid>!12e1
           (Places API `writeAReviewUri`): a universal link Maps claims
 never     g.page/r/<id>/review — it redirects to the browser form tagged
           laa=merchant-review-solicitation
```

A QR straight at Google's form (the "Google's form" target) cannot be one URL: `/mobile` is a 404
on an iPhone, and the plain form opens in the browser on both. It goes through our page too,
which redirects with no tap. Whether Chrome follows an `intent://` redirect that no tap started:
UNVERIFIED.

== 3. The copy

```
 on load, no tap      refused on both (Chrome 156: NotAllowedError; Safari needs a tap)
 in the tap           the anchor's click handler copies synchronously, then the anchor's own
                      navigation runs; in Chromium the text survives into the next app
                      (tested); iPhone: UNVERIFIED on a device
 paste, Android       Gboard shows the copied text as a chip above the keyboard (kept 1 h): 1 tap
 paste, iPhone        long-press the box → Paste: 2 taps; a paste the person starts brings no
                      "Allow Paste" prompt (iOS 16.1+)
```

Nothing a page does removes the tap: it is what lets the copy happen.

== 4. Photos

```
              save first (they become the newest in the picker)        then, in Google's form
 Android      <a download> per photo: 1 tap (+1 if Chrome asks)         Add photos → tick → Done
 iPhone       navigator.share({files}) → "Save N Images": 2 taps        Add photos → tick → Add
 share sheet  "Add to Maps" / "Post to Google Maps" posts them as a separate contribution,
              not on the review
```

Google's policy prohibits photos that are not the reviewer's own; the kit's photos are the category's (§6), against it.

== 5. Taps after the scan

```
                      Android · Maps        iPhone · Maps                       either · no Maps
 Google's form        stars · type · Post   place · Write a review · stars ·    sign-in, then as with
                      (typed from nothing)  type · Post                         the app
 Drafts page          tap · stars ·         tap · (Write a review?) · stars ·   sign-in + the same
 One tap              paste · Post  = 4     long-press · Paste · Post  = 5–6
 + photos             +3–4                  +5
```

Counted from the scan; the scan itself is one more tap on each (the camera's link banner).

#align(center, diagram(
  spacing: (12mm, 8mm),
  node-corner-radius: 2pt,
  node-stroke: 0.7pt,
  label-size: 7.5pt,
  node((0, 0), [scan], fill: luma(235), name: <s>),
  node((1, 0), [tap: copy + open], name: <t>),
  node((2, -0.7), [form], name: <af>),
  node((2, 0.7), [place sheet], name: <ip>),
  node((3, 0.7), [Write a review], fill: rgb("#fef9c3"), name: <wr>),
  node((4, 0), [stars], name: <st>),
  node((5, -0.7), [paste chip], name: <pa>),
  node((5, 0.7), [long-press · Paste], name: <pi>),
  node((6, 0), [Post], fill: luma(235), name: <p>),
  edge(<s>, <t>, "->"),
  edge(<t>, <af>, "->", label: [Android]),
  edge(<t>, <ip>, "->", label: [iPhone], label-side: right),
  edge(<ip>, <wr>, "->", label: [UNVERIFIED], label-size: 6.5pt, label-side: right),
  edge(<af>, <st>, "->"),
  edge(<wr>, <st>, "->"),
  edge(<st>, <pa>, "->"),
  edge(<st>, <pi>, "->"),
  edge(<pa>, <p>, "->"),
  edge(<pi>, <p>, "->"),
))

== 6. The offer

Drafts and photos are a category's (trade × country); each view of a GBP's page is offered from
them. A draft or a photo goes to one GBP once: the reviewer's copy (or save) is its use there.

```
 a view of GBP g                         excluded                         order
   drafts of g's category ──────────▶  copied on g                    copies ↑
   (not removed)                        held by another live view      saves of its photos still
                                        of g (15-min lease)             offerable on g ↑
                                                                        oldest first
   take 6 (one tap: 1) → leased to this view
   each draft's photos                  saved on g                     saves ↑
```

#align(center, diagram(
  spacing: (14mm, 9mm),
  node-corner-radius: 2pt,
  node-stroke: 0.7pt,
  label-size: 7.5pt,
  node((0, 0), [category pool], fill: luma(235), name: <pool>),
  node((1.4, 0), [view of g \ ≤ 6 leased], name: <view>),
  node((2.8, -0.7), [copy tap \ use (draft, g)], fill: rgb("#dcfce7"), name: <copy>),
  node((2.8, 0.7), [save photos \ use (photo, g)], fill: rgb("#dcfce7"), name: <save>),
  node((4.2, 0), [never offered \ on g again], fill: rgb("#fee2e2"), name: <gone>),
  edge(<pool>, <view>, "->", label: [− used on g, − held]),
  edge(<view>, <copy>, "->"),
  edge(<view>, <save>, "->"),
  edge(<copy>, <gone>, "->"),
  edge(<save>, <gone>, "->"),
))

A copy never posted still keeps the draft off g: posting is confirmed later, from the archive. A
photo saved on g drops out of its draft's photos there; the draft stays offered. The lease is for
people standing together who scan one QR.
