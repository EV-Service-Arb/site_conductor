# Architecture

The conductor is Service-Arb's host on `sa.evinvest.ltd`: who a person is (sessions through
concierge), what they hold across the services (the gmails and GMBs they manage, their tokens),
who a machine is (the source keys), the shell the browser loads, and the forward to the services
behind the origin — sites_panel (the sites business: leads, places, pricing, experiments,
bookings, invoices), review_archive, playbook and placement (gmaps_optimal_placement: where
demand sits and how contested it is, drawn over the home map) — and each managing gmail's antidetect
browser or cloud phone, run by sa_browsers and driven through the conductor. Each service trusts only the assertion the
conductor signs for a request; none of them sees a session, and none calls the conductor but its
queries cache: it reads them. Live updates and alerts are its hubs, fed by what the services tell it; tokens are
charged from what review_archive tells it.

**The unit is the GBP** (Google Business Profile; a GMB in code): a site, its leads, its reviews,
a walk's charge are each about one. A gmail is a managing gmail, a scope of GBPs: several sit on
one, a GBP on one.

```text
browser ─session──▶ conductor ─x-sa-assertion (User)──────────────▶ sites /api/v1, review_archive, playbook,
                                                                     placement
landings/bots ─HMAC▶ conductor ─x-sa-assertion (Source) + raw body ▶ sites /api/ingest/v1
conductor ◀── pulls (Conductor) ── sites /api/conductor/{changes, notifications, sources/received,
                                                          internal-reads}
conductor ◀── pulls (Conductor) ── review_archive /api/conductor/{charges, notifications,
                                                                   captures/<sha>.png}
conductor ─── pushes (Conductor) ─▶ review_archive /api/conductor/gmbs   GMBs, balances, the cursor
conductor ── GET <origin>/.well-known/sa-site, public :443 ──────▶ each site key's landing
landings ──────────────────────── direct, in-cluster ────────────▶ sites /api/internal
review_archive, placement, laptops ─────── open, rate-limited ─▶ conductor /api/queries_cache: what was paid for
conductor ─── (Conductor) launch, stop, devtools ─────────────────▶ sa_browsers /profiles/{gmail}/…
```

A cargo workspace; dependencies point inwards only.

```text
crates/core/             no I/O: the queries cache's wire, a GBP's states and moves, its country and language, and the
                         snapshot the services are told (`gbp`; review_archive takes it too),
                         brand ids, the HMAC of a signed request and its window, the
                         conductor's own alert messages, the buttons' signed data, retry and
                         pacing, tokens' renewal and a charge's split, the charge feed's
                         page (`tokens::ChargePage`; review_archive takes it too), and what
                         costs tokens (`prices`: every tool that charges, its items)
crates/panel/            the engine: `Panel` (sources and their authentication, sessions, access
                         requests, the hubs' state), `seal` (XChaCha20-Poly1305 at rest), `store`
                         (SQLite, migrations/ applied on open, writes BEGIN IMMEDIATE; the queries
                         cache among it), `alerts`
                         (the hub: access, destinations, rules, the feeds fanned out; the
                         Directory and Feeds ports), `telegram` (the method: links, chats, outbox,
                         delivery, buttons; the Bot port), `live` (the in-process bus), `gmbs`,
                         `connections` (a gmail's inbox and Maps key; who pays a Places request),
                         `browsers` (a gmail's browser: its proxy checked and sealed, Multilogin and
                         GoLogin, a launch),
                         `tokens` (the ledger, review_archive's charges, the snapshot), `import`
                         (review_archive's, once), `assistant` (the assistant's chats), `trash`
                         (what was removed, restored until purged)
crates/alerts/           no I/O: the alert rules, and the notes a service publishes to the
                         conductor and their check; sites and review_archive take it too
crates/auth/             the `sa` permissions (concierge_iam derives), the aliases `sa:operator` /
                         `sa:admin`, and the assertion: a User, a Source or the Conductor, for one
                         request of one service
crates/browsers/         sa_browsers: the devices a browser poses as, a launch's wire and the viewer's
                         frames (lib); the runner (`sa_browsers serve`): Orbita headless, Mimic through
                         the Multilogin app's launcher, a browser's devtools passed to the conductor
                         alone, a film made the camera; a cloud phone's life and its viewer over adb
crates/places/           Google Places: a place from what a person pastes (an id, a Maps URL, a
                         search), its point; suggestions as one types (Autocomplete, no session
                         token); every request through the queries cache (in process,
                         or `Remote` over /api/queries_cache), then the payer's own keys, then the
                         rotation its purse pays for; review_archive takes it too
crates/ui/               Dioxus components the services' front ends share
crates/contracts/        concierge's gRPC contract (tonic, from contracts/proto)
crates/server/           the `panel` binary: CLI and HTTP, thin over `Panel`
  src/signin.rs          /auth/login, /auth/callback, /auth/logout; the /api/v1 gate
  src/concierge.rs       concierge over gRPC: ExchangeCode, RefreshClientToken, GetMe,
                         PublishCatalog; its development stand-in (PANEL_DEV_SIGN_IN)
  src/cookies.rs         __Host- cookies, the double-submit CSRF check
  src/http.rs            the router; the /api/v1 route table and the section each sits under
  src/api.rs             GET /me, /sources
  src/connected.rs       GET /sources/sites: each site key's landing report beside sites' view
  src/access.rs          /api/v1/access/requests
  src/ingest.rs          the signed requests: authenticated here, forwarded to sites
  src/queries_cache.rs   /api/queries_cache: the queries cache, open
  src/forward.rs         the services behind the origin; `Services`, the one client and signing
                         key every call to them goes through
  src/grafana.rs         Grafana: its process, /grafana/, the viewers' tokens and the loopback
                         listener they work on, the services' dashboards and its own (dashboards/)
  src/live.rs            GET /api/v1/live, and the follower of sites' change feed
  src/gmbs.rs            /api/v1/gmails, /api/v1/gmbs, /api/v1/trades
  src/connections.rs     /api/v1/gmails/{id}/{connections,imap,maps_key}
  src/browser.rs         /api/v1/gmails/{id}/browser{,/test,/start,/stop,/film}; the runner; the viewer's
                         socket, the relay between it and a browser's devtools, a phone's passed through
  src/tokens.rs          /api/v1/tokens, /api/v1/admin/tokens; review_archive followed and told
  src/trash.rs           /api/v1/trash; the hourly purge
  src/assistant.rs       /api/v1/assistant; the turns' shape
  src/alerts.rs          /api/v1/alerts; the services' feeds as the hub reads them; the hub's
                         work in `serve`
  src/telegram.rs        the Bot API, its long poll
  src/web.rs             the front end's static export (PANEL_WEB_DIR), behind every other route
  src/settings.rs        the environment (ev_lib `settings!`)
crates/gen/              `nix run .#gen`: the front end's mirror of Rust types (ev_lib `ts_gen`),
                         into frontend/src/entities/{session,source,gmb,tokens,alerts,assistant}/model/generated.ts,
                         committed;
                         the pre-commit hook re-runs it
contracts/proto/concierge/v1/  concierge's auth and directory protos, vendored at the commit in
                               REV (`sync.sh` refreshes them)
```

## Signing in

The conductor is a relying party of concierge, client id `sa`, with its own origin and its own
cookies; the browser never holds a concierge token.

```text
GET /auth/login      ?return_to=<path> (optional; else 400), ?prompt=select_account (optional;
                     anything else 400): state + PKCE verifier + return_to, sealed into
                     sa_prelogin (10 min)
                     → 302 <concierge>/api/auth/authorize?client_id=sa&redirect_uri=…&state&code_challenge
                       [&prompt=select_account: Google's chooser even with a live evinvest.ltd session]
GET /auth/callback   state = the cookie's (constant time), and not redeemed before
                     (consumed_states), else 400 and the code is never presented;
                     ExchangeCode(code, redirect_uri, verifier, client secret); the browser's
                     previous session, if any, closed → a session row (tokens sealed under
                     PANEL_DATA_KEY, keyed by the hash of a random id) → sa_session (HttpOnly)
                     + sa_csrf → 303 return_to, else /
/api/v1/*            not GET: x-sa-csrf must equal sa_csrf; the session (access token rotated
                     when within 30 s of expiry, see below); GetMe (cached ≤ 60 s per session,
                     asked afresh for POST/DELETE /sources, the Telegram link and a chat added,
                     one retry when concierge does not answer) → the caller's permissions, else 401 (cookies
                     cleared) / 503; then the route's section, else 403
POST /auth/logout    CSRF; every session of the user is closed
```

- **Permissions, no admission.** Any active concierge account signs in. `GetMe` answers the
  concrete `sa` permissions the user holds (no alias, no wildcard; anything outside `sa` is a
  failed answer). Every `/api/v1` route sits under one section — Admin
  (`sa:admin:sources:manage`), Tokens (`sa:admin:tokens:grant`) — or none (`/me`,
  `/access/requests*`, `/alerts*`, `/assistant*`, the caller's own gmails, GMBs, trades and tokens). Each fresh
  `GetMe` upserts the user (`users`: id, email, name), whom tokens and gmails belong to.
  `http::api_routes` is the table the router is built from and the tests walk.
- **The catalog.** `serve` publishes `Catalog::collect("sa", PANEL_BUILD_EPOCH)` — the whole `sa`
  namespace, every service's permissions — with `PublishCatalog` before it serves (the image
  sets `PANEL_BUILD_EPOCH` to the commit's unix seconds, so an older replica never supersedes a
  newer one); concierge refusing it fails the boot. Not under dev sign-in. Its `sa:everyone`
  alias is held by every member of the `sa` scope with no grant.
- **`return_to`** is a path on this origin: it starts with `/`, the next character is not `/`,
  it has no `\`, only visible ASCII, ≤ 512 bytes. Anything else is the 400 sign-in page.
- **Revocation.** concierge refusing a refresh or `GetMe` closes the session here; concierge
  unreachable is a 503 and keeps it. A permission revoked at concierge is seen within the 60 s
  of the cache (at once by every write: see Forward). Signing out of evinvest.ltd revokes the
  concierge token family: the session ends at its next rotation.
- **Rotation, a refresh token presented once.** No pool connection is held while concierge is
  asked. In one process, a single flight per session. Across processes, a lease on the row
  (`sessions.rotating_until`, 15 s): only its holder asks, the others poll the row for up to
  6 s, then answer 503; a lease whose holder died lapses. The rotated pair is written only if
  the row still has the pair the rotation started from.
- **Bounded.** `/auth/*`: at most 8 at once (else 503), 10 s each. `/api/v1`: 32, 15 s. The pool
  gives a connection within 3 s. Every answer carries `X-Content-Type-Options: nosniff`; the
  sign-in's HTML pages a CSP of `default-src 'self'; frame-ancestors 'none'; base-uri 'none';
  form-action 'self'`.
- **Cookies** are `__Host-` and `Secure` when `PANEL_PUBLIC_ORIGIN` is https, bare over plain
  http (development only), all `SameSite=Lax`.
- **Dev sign-in** (`PANEL_DEV_SIGN_IN=sa:admin|sa:operator|<permission>,…|none`, docs/LOCAL.md):
  `Concierge::dev` answers for `PANEL_DEV_SIGN_IN_EMAIL`'s user, or `/auth/login?as=<email>`'s (the database's of that email, else one named by it), holding that set; the dev tokens name their user. The pre-login, the state, the
  session and the gate run unchanged. Refused at boot (exit 78) in any profile but development,
  beside any concierge variable, and unless `PANEL_PUBLIC_ORIGIN` is loopback http.
- `serve` needs a sign-in: concierge's four variables (all or none), or dev sign-in.

## Forward

The services live under the conductor's origin; `forward.rs` streams each prefix to its
service, both ways (hyper-util, no buffering), one table for all of it — the front end's
reserved paths (`web.rs`) are derived from it.

```text
/api/sites/*                     → sites /api/v1/*              session + User; CSRF on writes
/api/hooks/booking/*             → sites (same path)            open: the providers' webhooks
/sites/mfe/*                     → sites /mfe/*                 open: the sites page's bundle
/api/review_archive/*            → review_archive /*            session + User; CSRF on writes
/review_archive/mfe/*            → review_archive /mfe/*        open: the dashboard's bundle
/playbook_mcp/authorize          → playbook (same path)         session + User; no conductor CSRF
/playbook_mcp/*, /.well-known/oauth-{authorization-server,protected-resource}/playbook_mcp
                                 → playbook (same path)         open: OAuth and bearer clients
/api/placement/*                 → placement /*                 session + User; CSRF on writes
/placement/mfe/*                 → placement /mfe/*             open: the map overlay's module
```

- **The assertion** (`sa_auth`, header `x-sa-assertion`) is signed for one request: Ed25519
  (`PANEL_ASSERTION_KEY`), `aud` the service, the method and the upstream path (without its
  query), alive 60 s, and a principal: a **User** (`sub`, email, `email_verified`, name, and
  their `sa:<service>:*` permissions only), a **Source** (a key the conductor authenticated:
  `key_id`, kind, brands, and the SHA-256 of the body its MAC covered), or the **Conductor**
  itself (its hubs). The services verify it with the public half (`PANEL_ASSERTION_KEYS`,
  several while a key rotates) and trust nothing else.
- **Hygiene:** the browser's `Cookie`, any inbound `x-sa-assertion` and `x-sa-csrf`, and
  hop-by-hop headers (`Upgrade` included: nothing is upgraded through the forward) never go up;
  `Set-Cookie` never comes down. Everything else passes (`Idempotency-Key`, `X-Member`, a
  client's `Authorization`).
- **Gates:** a gated read takes the cached `GetMe`, a gated write asks concierge afresh. A write
  under `/api/sites` and `/api/review_archive` needs the conductor's CSRF header. playbook's
  consent form at `/playbook_mcp/authorize` posts without it: the `SameSite=Lax` session and
  playbook's own single-use nonce guard it; a signed-out `GET` of that page goes to
  `/auth/login?return_to=` and comes back.
- **A path stays under its upstream prefix:** a `.` or `..` segment, `%2e`, `%2f`, `%5c` and `\`
  decoded, is a 400.
- **The outline.** Every signed-in page sits beside it: the open service's own views (the sites:
  `<mfe-sites-nav>`, mounted on every page and shown on theirs; the review archive: its places,
  Telegram and settings), then the caller's GMBs under a gmail scope every app shares (kept in the
  browser), with their switches, each row, pin and card with what needs the operator, from the
  services' tags on the GMBs (review_archive `/me/gmbs`, sites `/gmbs`). `/` is the map of them:
  one Google map per page session, kept while the page is away (each built is a billed load), its
  viewport splitting the outline's list; placing a GMB's point is a mode of it.
- **The pages.** `/sites` mounts `<mfe-sites-panel api="/api/sites" base="/sites" csrf-cookie>`
  from `/sites/mfe/`; `/review_archive` mounts
  `<mfe-review-archive-dashboard grafana="/grafana/" ui>` from `/review_archive/mfe/`, `ui` its
  default (`reviewArchiveUi` in `frontend/panel.config.ts`). Any `/sites/<view>` or
  `/review_archive/<view>` without a dot in its last segment is that page, for the element to
  route, and a file below a view (`/sites/<view>/index.txt`, `…/__next.*.txt`) is the page's
  own, so Next's client-side navigation to a view stays soft. The bundles run on this origin:
  their builds are trusted as the conductor's own code is.
- **The elements route by the location.** Next moves it with `pushState`, which fires nothing:
  after each of its moves the shell dispatches a stateless `popstate`, as back and forward do
  (Next's own handler ignores it). An element is created once per tag, attributes and language
  and re-inserted after: a Dioxus one (ev_lib's `mfe`) cannot be torn down, so it keeps running
  while its page is not shown, and holds its last route while the location is not under its base.
- **One keymap and one undo history per page, the shell's.** An element listens for no key: it
  hands its bindings over with `ev:keymap`, and they sit under the leader in the scope
  `elementKeys` (`frontend/panel.config.ts`) assigns its tag (`mfe-sites-nav`: `V`, its views),
  bound while it is connected. A change it can take back goes to the page's history with
  `ev:undo`, as the conductor's own GMB writes do (a point placed, a switch, a trade); an entry
  writes back only while the state is still what it left, else the history is dropped.
- **The platform's elements are not forwarded.** The account chip, the sign-in element and the
  wallet load straight from `https://evinvest.ltd`. `/cabinet` stays reserved: a JSON 404, never
  the front end's page.
- Without `PANEL_ASSERTION_KEY`, `PANEL_REVIEW_ARCHIVE_URL`, `PANEL_PLAYBOOK_URL` and
  `PANEL_SITES_URL` (all four, or none), nothing is forwarded, nothing ingested, the hubs hear
  nothing of sites, and review_archive is neither followed nor told.
- **Placement** (`PANEL_PLACEMENT_URL`, optional, needs the four) is gmaps_optimal_placement's
  `serve --behind-conductor`. The home map imports its `/placement/mfe/overlay.js` once per page
  session and hands it the map: the heat, the pressure field and the competitors are its drawing,
  the model's wasm shipped with the payload it reads. A GET there never spends; a POST (Pull
  latest, `sa:placement:study:spend`) buys the Places searches it lacks. Unset, `/api/placement`
  answers 503 and the map says placement is not connected.

## Browsers

Each managing gmail has one antidetect browser: one fingerprint, one session, one proxy, whichever
of its GBPs it acts for. The product is Multilogin's **cloud phone** (real ARM Android: Google's device
checks see a phone); Multilogin's browser (Mimic posing as Android) and GoLogin's (Orbita) are
browsers. sa_browsers runs them apart from the conductor (a browser browsing the web never shares a
process tree with the conductor's secrets); the operator drives them from the conductor's window.

```text
browser ──WS /api/v1/gmails/{id}/browser (Origin, session, owner)──▶ conductor
   ▲  {frame b64, tabs, dialog}       ▼ {touch, wheel, key, text, navigate, tab}  │ a browser: the relay, the only CDP client
   └──────────────────────────────────────────────────────────────────────────────┤ a phone: each frame checked, passed through
conductor ── MULTILOGIN_TOKEN: phones, Mimic profiles, the Library, a run's 3 h token ──▶ api.multilogin.com
conductor ── GOLOGIN_TOKEN: the profile, its 3-minute token ──▶ api.gologin.com
conductor ── POST /profiles/{gmail}/launch {Launch}, Conductor ──▶ sa_browsers
   sa_browsers: Orbita headless | the Multilogin app on Xvfb, its launcher → Mimic  ── /cdp → devtools
                a phone: phone/start ▸ adb + glogin ▸ geo, zone, exit from inside ▸ /view ▸ stop
```

- **Set up once, its proxy replaceable.** Provider and device are a fingerprint: another is a new
  browser. A phone's model is drawn from Multilogin's Google and Samsung ones; a browser poses as a
  measured phone (`sa_browsers::Device`). A proxy is kept only once one request through it (ip-api)
  comes out in the country of every GBP of the gmail that has one, at a public address; sealed, and
  compared with the others by a keyed tag. Multilogin's phone or profile is made at the set-up, its
  proxy replaced after; GoLogin's at the first start, its noise drawn by GoLogin then and kept.
- **A start is for a GBP or the gmail.** At a GBP: its point is the position, its zone and its
  country's locale the browser's; for the gmail alone the position is blocked and the exit's zone
  stands. A phone is placed from inside (mock location on every provider, the zone read back) and its
  exit read from inside must be in the GBP's country. A start for another while one runs is refused.
  Ten minutes without a viewer stop a browser; a phone, three, and thirty after its start whoever
  watches.
- **The runner owns a phone's life**, so a dead conductor leaves none billing: it is recorded on the
  runner's disk before `phone/start`, stopped on every way its run ends (a failed step, a step past
  the time it should take, stop, removal, idle, the cap, SIGTERM), the stop retried until `statuses` reads it stopped, and a recorded one
  stopped before the runner serves again. The launch brings a 3-hour token, past the 180 billed
  minutes Multilogin caps a session at: the runner holds no long-lived one.
- **The relay** never sends `Runtime.enable` (what pages detect devtools by): `Target.*`, `Page.*`
  (the screencast, navigation, dialogs) and `Input.*`, a finger's touches for the pointer. A phone is
  not devtools: the runner speaks the viewer's frames itself (screencap, `input`, ADBKeyBoard's
  broadcast for text). One viewer per gmail; a new one closes the old (4409).
- **A film as the camera** (`PUT …/browser/film`): for Orbita the runner cuts it (first frame still,
  the film, the last held, its sound over a faint noise) for Chrome's file capture, and its next
  launch plays it, `facing.js` before every page's scripts; the file route's label and
  per-`getUserMedia` restart are known tells (emulate_phone_cam). A phone's goes to Multilogin's
  Library, picked in the Multilogin app's Camera panel: no API binds it. Mimic takes none.
- Without `PANEL_BROWSERS_URL` (and `GOLOGIN_TOKEN` or `MULTILOGIN_TOKEN`, or both; they need
  `PANEL_ASSERTION_KEY`) every browser route answers 503 "browsers not connected"; a provider without
  its token, "<provider> not connected".

## Grafana

One Grafana for every service, at `/grafana/` on this origin: an element steers its frame through
`history` and checks the origin of its messages, which a subdomain would break. `serve` runs it
(`PANEL_GRAFANA_SERVER` and `PANEL_GRAFANA_DIR`, or neither: no Grafana) on a unix socket in its
dir, its root URL `<PANEL_PUBLIC_ORIGIN>/grafana/`.

```text
browser ── /grafana/* ────────────▶ conductor (session; a POST is a read) ──socket──▶ Grafana
browser ── /grafana/api/ds/query ─▶ conductor: a viewer token into each query whose datasource
                                    uid is a service or the conductor; dead once Grafana answers
Grafana (Infinity) ── GET 127.0.0.1:<any>/api/<service>/… + token ──▶ the forward, as that viewer
Grafana (Infinity) ── GET 127.0.0.1:<any>/api/v1/… + token ──▶ the conductor's own GETs, as that viewer
conductor ── GET /dashboards (Conductor), every 60 s, conditional ──▶ each service of
             forward::DASHBOARDS ──▶ <dir>/dashboards/<service>/<uid>.json ──▶ Grafana
```

- **Datasources are derived:** one Infinity datasource per service's API (`/api/<service>`), its
  uid the service's name, and `conductor` for the conductor's own GETs (`/api/v1`, each route's
  section checked against the viewer); its url that API on the loopback listener and nothing
  beyond it allowed; written, with the dashboards' provider and the conductor's own dashboards,
  before Grafana starts.
- **Dashboards are the services':** coupled to a service's API, they ship with its release. A
  service that has them is in `forward::DASHBOARDS`, and its `GET /dashboards` answers a JSON
  array of them, each with its `uid`, and an ETag. A pull that fails — no answer, a 404, a uid
  another service serves or Grafana refuses — is reported, and what was served before stays.
- Grafana's dashboards query by POST, so every method under `/grafana/` takes the cached `GetMe`
  and no CSRF.
- Cookies, assertions, `x-sa-csrf` and the viewer header never reach Grafana, its `Set-Cookie`
  never comes down, nothing is upgraded. A Grafana that fails is reported and `/grafana/`
  answers 502; the conductor goes on.

## Ingest

The landings, the bots and the other sources sign what they send with a key the conductor
holds; the conductor authenticates, sites authorizes.

```text
POST /api/ingest/v1/events        at most 32 at once (else 503), 30 s in all (else 408)
  │ headers present, |now − x-sa-timestamp| ≤ 5 min, x-sa-key-id a slug     else 401, body unread
  │ body read: ≤ 4 MiB within 10 s                                          else 413 / 408
  │ x-sa-key-id → sources (not revoked) → secret, unsealed
  │ HMAC over "sa-ingest/v1.<x-sa-timestamp>.<body>"                        else 401
  ▼
sites POST /api/ingest/v1/events, the body as it came, Source{key_id, kind, brands, body digest}
  → its answer as it came: 207 a verdict per event, 409 + Retry-After, 400 …
GET /api/ingest/v1/leads/by-ref/{brand}/{ref}   the same, the MAC over "GET <path?query>" in
  place of a body → sites, the same path and query, Source over the empty body
```

What a key may write — its kind's types, its brands, a bot's lookup — is sites' to judge, per
event. An unknown key and a bad signature answer the same 401, so key ids cannot be probed for;
only a timestamp outside the window is told apart.

**Keys** (`/api/v1/sources`, `panel source add|list|set-origin|revoke`): one kind and a set of
brands each, the secret shown once. Kinds `panel` and `booking` are written by sites itself: no
key is issued for them. A `site` key carries its landing's origin (`https://<domain>[:port]`,
nothing after it; plain http on `localhost` / `127.0.0.1` for a landing on this machine): given
when the key is issued, or once with `set-origin` for a key issued before origins.

## Queries cache

What a paid query was answered — Places searches and details, placement's search volumes — is
kept here once per query, for anyone who might ask it again: sa_places in this process,
review_archive and placement in the cluster, a laptop's gmaps_optimal_placement over the public
origin.

```text
GET    /api/queries_cache/{scheme}/{key}         200 {answer, fetched_at} | 404
POST   /api/queries_cache/{scheme}/{key}/claim   {fresh_since?} → 200 {state: stored, answer, fetched_at}
                                                 | {state: granted, until} | {state: held, until}
DELETE /api/queries_cache/{scheme}/{key}/claim   204: the buy failed
PUT    /api/queries_cache/{scheme}/{key}         {answer, request} → 204; 400 unless key = SHA-256(request)
```

- **A key is its query's SHA-256**, under its client's scheme: `places` (sa_places: method,
  URL, field mask, body) and `gop` (placement: URL, body). The cache derives none.
- **Buying is claim, buy, PUT.** A claim holds the key 30 s; whoever else asks waits on it and is
  served what was bought. A failed buy gives the claim up; a holder that dies lets it lapse.
- **The latest answer per key stays**; an older PUT changes nothing, and nothing expires:
  `fresh_since` is a reader's age limit, or its re-ask.
- **Open to everyone**, reads and writes: bounded as ingest, a body ≤ 8 MiB, and rate-limited per
  client at the edge.
- `panel import queries-cache <scheme> <dir>` takes the files a client kept, `<key>.json` each, as of
  when each was written.

## The conductor's API, `/api/v1`

JSON; timestamps RFC 3339; errors `{"error": "…"}` with 400 / 403 / 404 / 409 / 413 / 503.

```text
GET    /me                                  {user_id, email, preferred_name, permissions,
                                            dev_sign_in}: every sa permission, every service's
GET    /sources              Admin          {sources: [{key_id, kind, brands, created_at, revoked_at,
                                            origin}]}
POST   /sources              Admin, fresh   {key_id, kind, brands, origin} → 201 {key_id, secret}
                                            (shown once), 409 if taken; kind panel or booking, a
                                            site without an origin or another kind with one → 400
GET    /sources/sites        Admin          the Connected sites table, below
DELETE /sources/{key_id}     Admin, fresh   204, 404
POST   /access/requests                     {need} → 201 {need, requested_at}, made now; 200 the one
                                            standing (made within 24 h: nobody told again);
                                            400 a need neither a permission nor an alias of the
                                            sa catalog; 409 a need held already
GET    /access/requests/mine                {requests: [{need, requested_at}]}; a need is held
                                            when every permission it stands for is, and both
                                            routes drop the caller's requests held
GET    /alerts                              {enabled, bot, rules, destinations: [{id, kind, title,
                                            topic_id, gmail_id, dead, rules: {rule: bool}}]} (the
                                            rules open to the caller)
POST   /alerts/telegram/link fresh          → 201 {url: "https://t.me/<bot>?start=<token>"};
                                            503 without a bot
POST   /alerts/telegram/chats fresh         {chat, gmail_id} → 201 as GET; 400 refused (nothing
                                            stored), 409 a destination already
PUT    /alerts/destinations/{id}/rules      {rules: {new_lead: false, …}} → 200 as GET; 400 for a
                                            rule not open to them
PUT    /alerts/destinations/{id}/gmail      {gmail_id} → 200 as GET; 404 not theirs
POST   /alerts/destinations/{id}/test       204; 400 with what Telegram said
DELETE /alerts/destinations/{id}            204, 404
GET    /gmails                              {gmails: [{id, gmail, role, cap, created_at, browser}], gmbs: [{id,
                                            gmail_id, place_id, name, address, state, next (the
                                            states it may move to), country: {code, by_hand} | null,
                                            phone, trade_id, areas, hidden_address, customers_visit,
                                            film, created_at, enabled, location: {lat, lng,
                                            placed} | null}]} the caller's, removed ones left out; a
                                            draft's gmail_id may be null, place_id a verified one's
                                            only is certain; cap null while the role is unknown
POST   /gmails                              {gmail} → 201 the gmail; 409 held already
PATCH  /gmails/{id}                         {role: owner|manager} → 200 the gmail
DELETE /gmails/{id}                         204: to the trash, its GBPs with it; 409 while it has a
                                            browser
GET    /gmails/{id}/connections             {imap: {host, username, set_at, checked: {at, error}} | null,
                                            maps_key: {last4, set_at, checked, out} | null, prices:
                                            [the Places items of /admin/tokens/prices], browser: {provider, device, endpoint,
                                            exit, set_at, checked, running: {gmb} | null, film} | null}:
                                            no secret, ever
PUT    /gmails/{id}/imap                    {host, username, password} → 200 as GET, once it logs in
                                            (IMAPS :993, a public address only); 400 refused, nothing
                                            stored
PUT    /gmails/{id}/maps_key                {key} → 200 as GET, once Google answers for it (an
                                            unbilled id-only details); 400 refused, nothing stored
POST   /gmails/{id}/{imap,maps_key}/test    200 as GET, the check recorded; 404 none set
DELETE /gmails/{id}/{imap,maps_key}         204
PUT    /gmails/{id}/browser                 {provider, device?, proxy} → 200 as connections, once the proxy
                                            comes out in the GBPs' country (Browsers); 400 refused,
                                            nothing stored; 409 another provider or device; a phone
                                            names no device
POST   /gmails/{id}/browser/test            200 as connections, the exit checked again
DELETE /gmails/{id}/browser                 204: stopped, the provider's profile (a phone's film) and session gone
POST   /gmails/{id}/browser/{start,stop}    {gmb?} → 204 once up; 409 it runs for another
PUT    /gmails/{id}/browser/film            an mp4 ≤ 512 MiB → 204; a phone's ≤ 100 MiB, to the Library;
                                            Mimic's 409
                                            (start and film: 8 at once, 10 min each, not the bounds above)
DELETE /gmails/{id}/browser/film            204
GET    /gmails/{id}/browser                 the viewer's WebSocket: outside the gate, as /live
GET    /gmbs/resolve?place=&gmail=          {candidates: [{place_id, name, address, location: {lat, lng}
                                            | null, country | null}]} (sa_places; `gmail`'s key
                                            first); 400 one in a country not served
GET    /gmbs/suggest?input=&gmail=          {suggestions: [{place_id, main, secondary | null}]}: Places
                                            Autocomplete in the countries served, paid as resolve
POST   /gmbs                                {name, trade, state, gmail_id?, country?, location?} → 201 a
                                            GBP no place is known of: draft, verifying or suspended;
                                            past a draft gmail_id and country (by hand) required;
                                            location placed by hand; 409 the name a live GBP's
POST   /gmails/{id}/gmbs                    {place_id, state, name?, country?, trade} → 201 a GBP on Google
                                            already (state not draft; name and country fill what
                                            Google lacks, else 400); 409 the name or place taken
PATCH  /gmbs/{id}                           {name?, gmail_id?, phone?, trade?, areas?,
                                            hidden_address?, customers_visit?, film?} →
                                            200 the GBP: name a draft's only (409), gmail_id another
                                            of the caller's (a transfer), null clears the rest; 409
                                            a name or a phone another live GBP's
PUT    /gmbs/{id}/state                     {state, place?} → 200 the GBP; 400 a move the drawing
                                            below lacks; `place` taken while it has none, needed to
                                            become verified
DELETE /gmbs/{id}                           204: to the trash
PUT    /gmbs/enabled                        {gmbs: [id], enabled} → 204: every one switched, or none
                                            (404 one not the caller's)
PUT    /gmbs/trade                          {gmbs: [id], trade: id | null} → 204: every one, or none
PUT    /gmbs/{id}/location                  {lat, lng} → 204, placed by hand; 409 Google has its point
DELETE /gmbs/{id}/location                  → 204, the point placed by hand taken back; 409 none is
PUT    /gmbs/{id}/country                   {country} → 204, set by hand; 409 Google has its country
GET    /map                                 {key}: the browser's Maps JS key; 404 without
                                            GOOGLE_MAPS_BROWSER_KEY
GET    /trades                              {trades: [{id, name, created_at, categories: [{country, lang,
                                            keywords, google_primary: {id, name} | null,
                                            google_additional: [{id, name}], study}]}]}
POST   /trades                              {name} → 201 the trade; 409 the caller's already, any case
PATCH  /trades/{id}                         {name} → 200 the trade; 409
DELETE /trades/{id}                         204, its categories with it; 409 while a live GBP holds it
PUT    /trades/{id}/{country}               {keywords, google_primary: gcid | null, google_additional:
                                            [gcid], study} → 200 the trade: set up there, or edited;
                                            400 a keyword twice once normalized, a gcid not Google's
                                            there or twice, additional ones past 9 or without a primary
DELETE /trades/{id}/{country}               204
GET    /google-categories/{country}         {categories: [{id, name}]}: Google's list there
GET    /trash                               {settings: {keep_days, never_purge}, gmails: [{id, gmail,
                                            removed_at, purge_at, gmbs, blocked}], gmbs: [{id, name,
                                            state, gmail, removed_at, purge_at, blocked}]}: gmbs those
                                            removed on their own; blocked why a restore is refused
POST   /trash/{gmails,gmbs}/{id}/restore    204, a gmail with the GBPs removed with it; 409 blocked: a
                                            GBP's gmail in the trash, a live gmail's address, a live
                                            GBP's name, place or phone
PUT    /trash/settings                      {keep_days: 1–90, never_purge?} → 204; never_purge needs
                                            sa:admin:trash:keep (403)
GET    /tokens                              {balance, daily, cap, kickbacks}: renewed, the
                                            kick-backs not claimed
GET    /tokens/ledger?before=&limit=        {entries, next}: newest first, limit 1–200
GET    /tokens/usage                        {days: [{day, charges, tokens}]}: 30 UTC days
POST   /tokens/claim                        → 200 as GET /tokens
GET    /admin/tokens         Tokens         {users: [{user_id, email, name, balance}]}
POST   /admin/tokens/{user}  Tokens, fresh  {set: n, note?} | {add: n, note?} | {grant: n, note}
                                            → 200 {balance}; a grant is owed until claimed
GET    /admin/tokens/prices  Tokens         {prices: [{item, scope, tool, tool_title, title, description,
                                            tokens, set_by, set_at}]}: every item of `sa_core::prices`,
                                            in its order; tokens null, not sold
PUT    /admin/tokens/prices  Tokens, fresh  {prices: {item: n | null}} (n 0 or more; the others stay)
                                            → 200 as GET; 400 an item not priced
GET    /admin/tokens/flows?from=&to=  Tokens  {flows: [{at, email, name, kind, tokens, service, label, by,
                                            note}]}: everyone's ledger and grants given, Unix ms, ≤ 400
                                            days; kind renewal, purchase, set, spend, refund,
                                            grant_claimed, granted
GET    /assistant/chats                     {chats}: owned and joined, the caller's marks, newest
                                            first
POST   /assistant/chats                     {id, title, turns} → 201 the chat (version 1); 409 the
                                            id taken
GET    /assistant/chats/{id}                the chat: its turns, version, the caller's role
PUT    /assistant/chats/{id}/turns          {base, from, turns} → 200 the chat: turns[..from] kept,
                                            the rest replaced; 409 `base` not the version, 400
                                            `from` past the end
PUT    /assistant/chats/{id}/title          {title} → 200 the chat; empty unnames it
PUT    /assistant/chats/{id}/{pinned,archived}  {on} → 204, the caller's own
DELETE /assistant/chats/{id}                204; the owner's
DELETE /assistant/archived                  204: the caller's archived chats deleted, or left
GET    /assistant/chats/{id}/access         {owner, members, general}
POST   /assistant/chats/{id}/members        {users} → 200 as access; 400 an unknown user
DELETE /assistant/chats/{id}/members/{user} 204; the owner anyone, a member themselves
PUT    /assistant/chats/{id}/general        {general: none|view} → 200 as access; the owner's
GET    /assistant/people?q=                 {people}: whom the caller shares a chat with, by name
                                            or email, and whoever has `q` as their whole email
```

An id in a path that is not the caller's is a 404.
**The assistant's chats** are the conductor's own; nothing forwards them, no service stores their
ids, so they are deleted, with their members and marks. Owner and members write (turns, a title,
invites; a member leaves); `general: view` lets anyone holding a `sa` permission read one, 403 on
any write; anyone else is answered 404. Turns are written whole from an index, on the version
read; a body ≤ 4 MiB (images ride as data URLs), a chat's turns ≤ 16 MiB, else 413. Before the import (below) the gmail, GMB and trade
writes and every tokens route answer 503.
**Connected sites** (`GET /sources/sites`): for every active site key at once, its landing's own
report and sites' view of it, all asked together (joined, nothing spawned):

```text
{sites: [{key_id, brands, origin, report}], received, internal_reads}
  report          {state: "reported", report: <the landing's /.well-known/sa-site body>}
                  | {state: "failed", detail} | {state: "no_origin"}
                  GET <origin>/.well-known/sa-site: no redirect followed, 5 s, ≤ 64 KiB, 200 and
                  the report's shape, its brand one of the key's — else failed
  received        sites GET /api/conductor/sources/received  {state: "read", value: {key_id: at}}
  internal_reads  sites GET /api/conductor/internal-reads    {state: "read", value: {since, reads:
                  {brand: {locations, pricing, experiments}}}}; either {state: "failed", detail}
                  (5 s; or no forward)
```

The page `/access?need=&continue=` (outside the shell) is where a service sends a signed-in user
lacking `need`: it asks, then reads `/me` every 30 s and goes on to `continue` (a path, the
`return_to` rule) once held.

## Live updates, `/api/v1/live`

A screen reads through the API and is told, over one WebSocket, when what it read may have
changed. Nothing is sent but the hint.

```text
GET /api/v1/live    Origin = PANEL_PUBLIC_ORIGIN exactly                     else 403
                    the session, as the /api/v1 gate (cached GetMe)          else 401 / 503
                    ≤ 5 sockets per user, ≤ 200 in all                       else 429
                    → 101; any signed-in user: what they are told is judged per message
server → client     {"type":"hello","at","user_id"}                          at once
                    {"type":"changed","topic","brand_id"?,"id"?,"at"}        a change they may read
                    {"type":"resync"}                                        read everything again
                    WS Ping every 25 s; no Pong within 60 s → dropped
closes              4401 the session ended   1001 the server is stopping
```

- **What sites changed** comes from its feed, `GET /api/conductor/changes` (NDJSON, a Conductor
  assertion): one follower per process puts each `changed` line on the bus with the permission
  it names — told to whoever holds it — and each `resync` to everyone. sites sends `resync`
  first on every connection and a `ping` every 25 s; a feed that ends, fails, or says nothing
  for 60 s is followed again after 1 s, doubling to 30 s, so what was missed meanwhile is read
  again.
- **The conductor's own topics:** `sources` (a key minted or revoked; holders of
  `sa:admin:sources:manage`), `alerts` (the user's destinations added, removed, found dead
  or revived; their rules; that user only), `browsers` (`id` a gmail: its browser set, checked,
  started, stopped, its film; its owner) and `assistant` (`id` a chat: any change, to its owner
  and members, those it removed too; a pin or archive to whose it is; viewers are not told).
- **The shell relays.** The front end holds the one socket and re-dispatches on `window`, as
  `CustomEvent`s: `sa:live` (each server message as is, and a `{type:"resync"}` after the shell
  reconnects), `sa:live-status` (`{status}`; answered to an element's
  `sa:live-status-request`). The sites elements dispatch `sa:title-count` (`{count}`, the nav
  only) and `sa:unauthenticated` (the shell opens its sign-in, returning to the current page).
- **The bus is in-process** (`tokio::sync::broadcast`, one pod), 1024 messages per subscriber; a
  socket further behind skips the backlog and is sent `resync`. A frame that does not go out
  within 10 s drops the socket; its write buffer is capped at 256 KiB, a client frame at 4 KiB.
  Handshakes: 16 at once, 15 s each.
- **The session is asked again** every 60 s, and at once when a session ends: a sign-out (every
  socket of the user), a sign-in replacing the browser's session, concierge refusing a rotation
  or `GetMe`. Concierge unreachable keeps the socket.

## Tokens and GBPs

A person's gmails and the GBPs each manages are the conductor's; the services store their ids. A
GBP is on or off by its owner's switch (`gmbs_off`), which the services inherit for their own
work: off, review_archive walks none of its tracks. Tokens are a ledger: a balance is its rows'
sum, renewed a day's worth when read (`TOKENS_DAILY` up to `TOKENS_CAP`; a first balance
`TOKENS_START_CAP_MULTIPLE` caps). review_archive's walks charge after the fact; the conductor's
Places requests charge at the buy.

**Who pays a Places request** about a GBP or a gmail: its gmail's own Maps key, then its owner's
other gmails' keys, free; then `GOOGLE_MAPS_KEYS` at the admin's price per request kind (Places'
SKU), held under one lock per person from the balance read to the charge, so a
request below the price, or unpriced, is refused before Google is asked. A cached answer costs
nothing. The operator's commands (`gmb add`, `locate`) try the gmail's keys, then the house pays.
review_archive's lookups and placement's sweeps are the house's for now.

**Prices are the admin's.** Whatever charges tokens charges items of `sa_core::prices`: each tool
that charges declares its items there once (an enum, a variant's message its name and its doc
what it is), and the admin's page, `token_prices` and the snapshot are walked from it. An item
without a price is not sold: a Places request is refused, a walk step not walked.

**A GBP's life** (`sa_core::gbp::State::nexts`; the schema checks each move again):

```text
             name (unique)    + gmail, country     + place, if none yet   walked
 ┌───────┐  ─────────────▶ ┌───────────┐ ─────────▶ ┌──────────┐ ◀────────▶ ┌───────────┐
 │ Draft │                 │ Verifying │            │ Verified │            │ Suspended │
 └───────┘                 └───────────┘ ◀───────── └──────────┘            └───────────┘
  no place;                 place optional  Google re-asks │  ▲ reinstated    place optional
  phone, point,             (off Maps)                     └──┼──────────────────┘
  gmail/country optional                                      └ re-verify ◀──────┘
                            place certain only once Verified (on Maps); with one, the
                            snapshot → archive: a track, walked only while Verified
```

- A name, a place and a phone (`sa_core::gbp::phone`'s spelling) are one live GBP's. The name is a
  draft's to change. A GBP verifying or suspended is not on Maps, so Places may not find it: its
  place is set once, on any move while it has none (with Google's address, point and country),
  and must be by the time it is verified. A GBP is created with its trade.
- **A draft's must-haves** are what Google's form asks and what stands behind it: name, category
  (set up, with its primary Google category), service areas (≤ 20), phone, a site page (sites' link), the hidden address with whether
  customers visit, the verification film, the managing gmail, its browser (Browsers: set up, its last check
  through). The film is a reference set by hand until verif_vid fills it. How many are ready
  is advice the front end computes; the move onto Google never checks it.
- **A gmail's role** on its GBPs, owner or manager, unknown until said, caps what it holds before
  Google grows wary (`sa_core::gbp::GmailRole::cap`): room shown, never refused.
- **A GBP's category** is its trade (its owner's: Plumbing) in its own country, derived, never
  stored: Plumbing · FR. The trade set up for a country holds its Google categories there, placement's
  study of it, and its keywords (`sa_core::gbp::keywords`), the terms every GBP of the category is
  compared on, in the country's language; any other term is one study's or one location's own and
  stays out of comparisons. Not set up there is shown so, never filled from another country. A
  trade is deleted only once no live GBP holds it; no service stores its id.
- **Google's categories** are a closed list: a gcid (`gcid:plumber`) and a name per language. A
  GBP takes a primary one and up to 9 additional ones, entered into Google's form in order; a
  name is shown in the reader's language and copied in the country's, as the form in the GBP's
  browser (its country's locale) names it. The list is `crates/panel/google_categories.json`, one
  gcid a line: `{countries, names: {lang: name}}`, named in its countries' languages and the
  front end's. It is drawn from pulls of Google's Business Information API:
  jepto/google-business-profile-categories `data/categories.json` (every gcid, its English name,
  its regions) and hypetrix.com's `wp-json/hypetrix/v1/gbp-categories?region=US&lang=<fr|ru>`
  (US only: the 25 GB/CA-only gcids carry their English name as `ru`). A refresh dropping a gcid a trade holds makes reading it fail until a migration moves
  it to what Google replaced it with.
- **Past a draft a GBP has a gmail and a country** (the schema's CHECK on live rows): the archive
  reads its language from it. One on Google is added verifying, verified or suspended: by its
  place, or without one (not verified) with its country and point by hand.
- A transfer moves it to another gmail of its owner's. Removed is apart from the state; a
  gmail removed removes its GBPs.
- **The trash.** A removed gmail or GBP is its owner's to restore (a gmail with the GBPs removed
  at its `removed_at`) until purged: `keep_days` after its removal (30 unsaid, 1–90), never with
  `never_purge` (an admin's say; unsaid, on under dev sign-in). Purged is gone for good: its
  gmail's IMAP and Maps key deleted, its row kept.

```text
follow (10 s)   GET review_archive /api/conductor/charges?after=<cursor>&limit=500 (Conductor)
                → an `sa_core::tokens::ChargePage`
                → one write transaction, taken only if the cursor is still `after`: each charge's
                payers (subs) renewed, its cost split over them within their balances, a
                `charge` row each (service, key `run:<id>`, label), what a failed walk owes back
                as kick-backs, the cursor moved; an unknown sub fails the page, the cursor stays
push (a change, a page, 60 s)
                PUT review_archive /api/conductor/gmbs (Conductor), an `sa_core::gbp::Snapshot`: every GBP that ever had a place
                ({id, gmail_id, gmail, owner, place_id, state, country, name, address,
                removed, enabled}), every user's balance renewed, the cursor they were read at and
                every price, from one transaction; review_archive gives each live one its track
                and meters its walks at those prices
```

- **The archive's allowance** is that snapshot's balance less its own charges past the cursor,
  so a walk is neither counted twice nor missed, and walks go on while the conductor is away.
  Two services spending one balance could overlap by at most a walk each; holds come with a
  second spender.
- **A 404 on `/api/conductor/*`** is an archive not cut over yet: told once at info, asked again
  every 60 s.
- **The import opens it.** `panel import review-archive <file>` (review_archive's
  `scripts/export_members.sql`) writes its people, gmails, GMBs (ids kept; with no country, so
  only an empty list passes), ledger and pending kick-backs in one transaction, refused unless
  the tables are empty, and creates the cursor. Until then tokens and GMBs are closed and `serve` neither follows nor pushes: a balance read
  writes a renewal, which would make the import refuse. A fresh install imports an empty
  document.
- **A place's site** is sites': its `places.gmb_id`, linked from the outline through the forward;
  sites down or refusing leaves the GMBs without the link.
- **A GBP's point** is on its row: Google's, else one the operator places on the map (a
  service-area place has none on Google, a GBP without a place none at all). Google's replaces a placed one, a
  placed one never replaces Google's. Google's is written with the GBP from the answer that named
  it; a GBP added without a key, or imported, has none until `panel gmb locate` asks for the
  missing ones and lists those Google gives no point: a command, so the spend is explicit; run
  after a deploy that brings points, and after `local-stack --from-prod`.
- **A GBP's country** (FR, US, CA, GB: `sa_core::gbp::Country`, the one country → review
  language table) follows the point's rule: Google's (its address), else set by hand
  (`PUT /gmbs/{id}/country`, `panel gmb country`); `locate` asks for the missing ones too and
  lists those Google gives none.

## Alerts

One hub: rules, their audiences, destinations and the fan-out are the conductor's; the services
only publish notes (`sa_alerts`), and Telegram is one method of reaching a destination
(`TELEGRAM_BOT_TOKEN`; it needs the forward). Without a bot, nothing is sent.

```text
rule               feed            audience                                         default
new_lead           sites           sa:sites:work:leads:edit                         on
contact_overdue    sites           sa:sites:work:leads:edit                         on
booked             sites           sa:sites:work:leads:edit                         on
payment_received   sites           sa:admin:sources:manage                          off
source_silent      conductor       sa:admin:sources:manage                          off
access_requested   conductor       every permission of sa:admin                     on
review_gone        review_archive  the owners of the GMBs the note names            on
review_new, review_changed, review_reappeared, scan_failed   (the same)             off
```

```text
feed (2 s)        GET <service>/api/conductor/notifications?after=<cursor>&limit=500 (Conductor), each
                  service under its own cursor → one write transaction, taken only from where the
                  cursor stands: each note checked against its feed (its rule's, its GMBs and photo
                  as its rule says, its buttons callable), claimed (rule, event) and queued for its
                  audience, the cursor moved to the page's `next`; a note that fails fails the page,
                  the cursor stays (loud, never skipped); 404: a service not cut over, said once,
                  asked again every 60 s
own (2 s)         sources silent ≥ 24 h (sites' last receipt per key, else the key's creation; once
                  per full day of it), access requests (once a day per user and need)
audience          a destination: the rule on there, its user's access confirmed within the hour;
                  a permission rule: their permissions open it; a GMB rule: they own a GMB the note
                  names, the GMB on, it and its gmail not removed, the destination's gmail theirs if
                  it is scoped to one
delivery (0.5 s)  lead messages noted > 1 h ago dead ("stale"); > 20 of one rule waiting for one
                  destination folded into one summary without PII; claim in one write transaction
                  (nothing while the outbox is paused): ≤ 25 tries started in any second, one per
                  private chat per second, one per channel or group per 3 s, none to a chat with a
                  send in flight; leased 60 s → the user's access checked again → sendMessage, or
                  sendPhoto with the photo fetched from the note's service (Conductor) → sent |
                  retry | dead
```

- **Destinations.** A user's own private chat (one), and channels, groups and forum topics they
  add. Each has its rules (a rule without a row is at its default) and may be scoped to one of
  the user's gmails for the GMB rules.
- **Private chat.** `POST /api/v1/alerts/telegram/link` (GetMe asked afresh; 256 random bits,
  base64url; SHA-256 stored with the caller's permissions, 10 min) → `t.me/<bot>?start=<token>`;
  `/start <token>`, in a private chat, not forwarded: the token redeemed once, the chat the user's
  private destination (a chat linked before is let go, the rules kept); a chat that is another
  account's is refused ("/stop first"), the token kept. `/stop` removes it.
- **Channels, groups, topics.** A paste (`@channel`, `-100<id>`, `<group>/<topic>`) is taken
  only after `getChat` sees it, `getChatMember` says the user's own Telegram account (their
  private chat's id; linked first) administers it, and a test went out there: a refusal stores
  nothing. They get the note's non-PII text, never buttons, never the conductor's access
  requests. One destination per chat and topic.
- **A note is its service's rendering**: its text and Telegram entities, a PII text sent only to
  private chats of holders of `sa:sites:work:pii:see`, the user who is not told (`skip_user`), its
  buttons (`{id, label, pressed, call: {method, path under /api/v1/, body}, leaves}`, sites'),
  the GMBs it names, a photo path (`review_gone`'s capture, ≤ 1024 UTF-16 of caption, no
  buttons). Queued texts are sealed under `PANEL_DATA_KEY` per chat and topic and dropped once
  sent or dead; no parse mode, link previews off.
- **Updates by long polling.** One process polls, under a lease in `telegram_poller` (60 s,
  renewed every poll); the offset is stored there after each update, and every update is
  idempotent, so one handled twice across a takeover does nothing twice.
- **Retries.** 429 pauses the whole outbox for `retry_after` and costs the message no try; 5xx,
  timeouts and a photo not fetched back off from 10 s, doubling, to 15 min; 10 tries, then dead.
  403 and 400 "chat not found" / "user is deactivated" mark every destination in the chat dead
  until a send there succeeds again (a link, a test). Any other 4xx is dead at once; so is a
  queued message that no longer opens.
- **Quiet.** The bot answers commands in private chats only; its unsolicited replies go at most
  once per chat per 10 min.
- **Who gets a message: access concierge confirmed within the hour.** The hub keeps per user the
  set last confirmed and when: the `/api/v1` gate records every `GetMe` answer, and every 15 min
  the hub asks again for a user with a destination reached, not confirmed since, through their
  newest session used within 7 days. The set is checked again when a message is sent. A grant
  revoked at concierge, or a session concierge refuses to rotate (or whose `GetMe` it refuses
  twice running), stops messages at the next check and drops what was queued; no session used
  for a week, or concierge unreachable, stops them after an hour.
- **Buttons.** The callback data is `<button id>.<outbox id>.<HMAC-SHA256, 80 bits>` under a key
  derived from `PANEL_DATA_KEY`, bound to the chat; a message sent with one-letter buttons
  (`t`, `n`) presses as `take`, `no_answer`. The message must be that chat's, the chat the
  private chat of the message's user, and the user's access is asked of concierge at the press —
  with no live session to ask with, the answer is "open the panel". The button's call goes to
  sites as that user (a User assertion, the sites slice) with `Idempotency-Key: tg/<outbox
  id>/<button id>`, so a press repeated, or its update redelivered, records nothing twice. 201:
  the message gains "{pressed}: {name}" and keeps the buttons the press `leaves`; 200, 400, 409
  "already recorded"; 404 "not found"; 403 "no access"; anything else "try again".
- Language of the conductor's own texts: `TELEGRAM_LOCALE` (`ru` default, `en`).

## Invariants

- **A service trusts the conductor's assertion and nothing else**, and one assertion opens one
  request: its audience, its method, its path, 60 s; a User's permissions only of that service;
  a Source's over the body its MAC covered. The conductor never forwards a cookie, nor an
  assertion it did not sign.
- **The conductor authenticates, sites authorizes.** A signed request reaches sites only with a
  verified MAC over its timestamp and body (the 5-minute window cannot be dodged by re-stamping);
  what the key may write is never judged here.
- **Grafana holds no identity.** It signs no one in: its fetches reach a service as a viewer only
  through a token the conductor wrote into that viewer's `/grafana/api/ds/query`, on the loopback
  listener, on GETs, for that query's life. The public listener never takes a token.
- **A source is only ever revoked.** `sources` refuses DELETE and any UPDATE but of `revoked_at`
  and a site's `origin` set once from NULL (only a `site` has one); a key id is never reused.
- **Secrets never sit in the clear.** Sources' HMAC secrets, sessions' tokens, queued Telegram
  texts and a gmail's IMAP password, Maps key and proxy are sealed (XChaCha20-Poly1305, what they belong
  to as associated data) under `PANEL_DATA_KEY`, whose fingerprint is stored beside each. A
  person's are never read back out: the browser sees a key's last four.
- **No price beside its use.** What a person is charged is an item of `sa_core::prices` at the
  admin's price; no cost is written where it is spent.
- **No buy on credit.** A Places request on the shared keys for a person is charged at its price,
  and refused before it is sent when their balance is below it; their own keys and the queries
  cache cost nothing.
- **Alerts are the conductor's.** A service publishes notes and never reaches a person itself;
  who is told, where and how is decided here.
- **A note is in the outbox before the cursor passes it**: the page and the cursor move in one
  transaction. **So is a charge in the ledger**, and a page is taken only from where the cursor
  stands.
- **A Google answer is paid for once.** Every billed answer, from any service or machine, is
  looked up in the queries cache before it is bought, claimed while it is bought and kept after;
  a place a search returned is not asked about again. Placement's requests sit in their own scheme.
- **A gmail's browser is driven only through the conductor**, its devtools no one else's, a phone's
  viewer only through it; the runner holds none of the conductor's secrets: a launch brings a token
  for that run alone (GoLogin's 3 minutes, Multilogin's 3 hours) and the proxy.
- **A cloud phone bills while it runs**: nothing a test runs starts one, and one that started stops
  on every way its run ends (docs/spec/browsers.md).
- **No two gmails share a proxy**: an exit two accounts share is a link Google can draw.
- **The ledger is append-only, and nothing a service stores an id of is deleted.**
  `token_ledger` refuses UPDATE and DELETE; gmails and GBPs get `removed_at`, which a restore
  clears until `purged_at`, set once; kick-backs only `claimed_at`, once.
- **A GBP with a place is tracked in review_archive; it is walked only while Verified.** The
  snapshot carries every GBP with a place, its state and country; the archive derives the track.
- **One file, migrated on open.** `PANEL_DB_PATH` names it (`/data/panel.db` in the image); every
  command creates it if missing and applies the migrations it lacks first (`panel migrate` does
  only that). A migration newer than the build is let be. Tables are STRICT, foreign keys
  enforced, timestamps INTEGER microseconds since the epoch, UUIDs 16-byte blobs.
- **The conductor's secrets come from the environment only** (`PANEL_DATA_KEY`, `SENTRY_DSN`,
  `RP_CLIENT_SECRET_SA`, `PANEL_ASSERTION_KEY`, `TELEGRAM_BOT_TOKEN`, `GOOGLE_MAPS_KEYS`, `GOLOGIN_TOKEN`,
  `MULTILOGIN_TOKEN`), through
  `ev_lib::settings`; a person's (a gmail's IMAP password, its Maps key, its proxy) through the API, sealed at
  once; with `APP_ENV=production` the database, the data key, the four sign-in
  variables, the forward's four and `TOKENS_START_CAP_MULTIPLE` are required at boot
  (`panel --print-required-vars` lists them).

## Deploy requirements

- **One image, one origin.** The image carries the binary and the front end's static export,
  and sets `PANEL_WEB_DIR` to it; `serve` answers `/api`, `/auth` and `/health` first, then the
  forward's prefixes, then the files (a directory by its `index.html`, anything else `404.html`
  with a 404). `/cabinet` is held, and `/grafana` without Grafana: JSON 404s. `/_next/static/*` is `immutable`
  for a year, everything else `no-cache`; every page carries a CSP of `frame-ancestors 'none';
  base-uri 'none'; form-action 'self'`, and every answer `Referrer-Policy:
  strict-origin-when-cross-origin` (Maps JS checks its referrer-restricted key by the origin).
- **Grafana's memory.** ~340 MB at its peak, ~185 of it its binary's pages, and its Infinity plugin
  server ~80 MB at a board's load, ~110 under a sustained one: the pod's limit has room for both
  above the conductor. Its dir is `/data/grafana`, unreplicated: provisioned again on every start.
- **The contract** (`nix eval .#containers.<system>.panel.contract`) names the port (the flake's `port`),
  `/health`, `APP_ENV=production`, the variables required, secret and optional,
  `PANEL_DB_PATH`, the volume (`/data`) and the SQLite file on it to replicate, what the ingress
  must not publish or must rate-limit, and the egress. `checks.contract-env` fails the flake
  when its required variables and the binary's `--print-required-vars` disagree.
- **What it provides and consumes, by contract name.** `provides.ingest` is
  `/api/ingest/v1/events`, `provides.queries_cache` `/api/queries_cache`; `consumes` names the peers behind `PANEL_SITES_URL` (`sites_panel`),
  `PANEL_REVIEW_ARCHIVE_URL` (`review_archive`) and `PANEL_PLAYBOOK_URL` (`playbook_web`), whose
  URLs and egress devops derives. `checks.provided-paths` fails the flake when `provides` and the
  binary's `--print-provided-paths` (the constants its router mounts) disagree.
- **The landings, outbound on :443.** The Connected sites page asks each site key's origin; the
  origins are data, so the egress is public :443, not a host list.
- **Ingest stays inside the cluster.** Its sources reach it by service DNS; the IngressRoute that
  publishes `sa.evinvest.ltd` must not route `/api/ingest`, and the NetworkPolicy lets in only
  the pods that send.
- **sites_panel within reach, and only through here.** The conductor calls `PANEL_SITES_URL` for
  everything it forwards and for its hubs; sites has no public route of its own (the landings
  reach its `/api/internal` in-cluster).
- **concierge within reach.** The pods call concierge's gRPC (`CONCIERGE_GRPC_ADDR`) for every
  sign-in, rotation and `GetMe`. concierge must register client `sa` with redirect URI
  `<PANEL_PUBLIC_ORIGIN>/auth/callback` exactly and hold the hash of `RP_CLIENT_SECRET_SA`.
- **Places, outbound.** With `GOOGLE_MAPS_KEYS` the pods need egress to
  `places.googleapis.com:443` (a pasted URL without a place id, a GMB's name, address and point).
  A person's own keys go first (Who pays, above), then these in their order, free ones first;
  one Google says is spent for a quota window sits out until the window resets, one whose billing
  is off until restart (a person's, until it answers a test). Every answer is kept in the queries
  cache, in the database. Set the prices (`/admin/tokens`) after the first deploy: until then a
  person without a key of their own is refused.
- **IMAP, outbound.** A gmail's inbox is dialled on :993 at a public address when it is set or
  tested: egress to public :993. The browser loads Maps JS straight
  from Google with `GOOGLE_MAPS_BROWSER_KEY` (referrer-restricted to the public origin); the
  server's keys never reach it.
- **Browsers.** sa_browsers is its own pod (`PANEL_BROWSERS_URL`, in-cluster only): `BROWSERS_DIR` on
  a volume of its own (the gmails' sessions: signed-in Google lives there; `phones.json`, the phones it
  may have started) and `HOME` on it too (the Multilogin app's `~/mlx`, Mimic's profiles),
  `BROWSERS_MAX`, the conductor's public keys (`PANEL_ASSERTION_KEYS`), Orbita (`.#orbita`), ffmpeg,
  the Multilogin app under Xvfb (`.#mlx`, `BROWSERS_MLX`; its launcher `launcher.mlx.yt:45001`, a
  public name for 127.0.0.1), `BROWSERS_MULTILOGIN_API`, and a phone's tools (`.#phone-tools`: adb,
  an aarch64 busybox, ADBKeyBoard). Its TERMination grace covers stopping every phone (≥ 2 min).
  Egress to `api.gologin.com:443`, `api.multilogin.com:443` and the app's own Multilogin hosts, the
  phones' adb at whatever `ip:port` Multilogin hands out, and through the gmails' proxies: any public
  address. The conductor too dials those proxies (a set-up's check, to `ip-api.com:80` through them),
  `api.gologin.com:443` and `api.multilogin.com:443`. Multilogin's account auto-stop (Account
  settings) at 40 min is the backstop by hand.
- **Telegram, outbound only.** With `TELEGRAM_BOT_TOKEN` the pods need egress to
  `api.telegram.org:443`; no webhook is set on the bot (`getUpdates` refuses to run while one is).
- **`/api/v1/live` through the public IngressRoute, as a WebSocket.** Nothing in front may buffer
  the response or strip `Upgrade` / `Connection` / `Origin`, and an idle timeout on the way must
  be longer than the 25 s ping.
- **Rate-limit `/auth`, `/api/hooks` and `/api/queries_cache` per client IP at Traefik**; the
  conductor bounds how many run at once, not who starts them. `/api/queries_cache` is public and
  open: laptops buy through it, and this is its one guard.
- **One pod, a volume, litestream.** One writer: one replica, rolled out with `Recreate`. No
  init container: `serve` migrates on start.
