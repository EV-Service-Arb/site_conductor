# Architecture

The conductor is Service-Arb's host on `sa.evinvest.ltd`: who a person is (sessions through
concierge), what they hold across the services (the gmails and GMBs they manage, their tokens),
who a machine is (the source keys), the shell the browser loads, and the forward to the services
behind the origin — sites_panel (the sites business: leads, places, pricing, experiments,
bookings, invoices), review_archive and playbook — and each managing gmail's antidetect
browser or cloud phone, run by sa_browsers and driven through the conductor. Placement
(gmaps_optimal_placement: where demand sits and how contested it is, drawn over the home map) is
a library it links and runs in process. Each service trusts only the assertion the
conductor signs for a request; none of them sees a session, and none calls the conductor but its
queries cache: it reads them. Live updates and alerts are its hubs, fed by what the services tell it; tokens are
charged from what review_archive tells it.

**The unit is the GBP** (Google Business Profile; a GMB in code): a site, its leads, its reviews,
a walk's charge are each about one. A gmail is a managing gmail, a scope of GBPs: several sit on
one, a GBP on one.

```text
browser ─session──▶ conductor ─x-sa-assertion (User)──────────────▶ sites /api/v1, review_archive, playbook
landings/bots ─HMAC▶ conductor ─x-sa-assertion (Source) + raw body ▶ sites /api/ingest/v1
landings ─HMAC▶ conductor /api/kit/v1: the review kit, answered here
conductor ◀── pulls (Conductor) ── sites /api/conductor/{changes, notifications, sources/received,
                                                          internal-reads, places}
conductor ◀── pulls (Conductor) ── review_archive /api/conductor/{charges, notifications,
                                                                   captures/<sha>.png}
conductor ─── pushes (Conductor) ─▶ review_archive /api/conductor/gmbs   GMBs, balances, the cursor
conductor ── GET <origin>/.well-known/sa-site, public :443 ──────▶ each site key's landing
landings ──────────────────────── direct, in-cluster ────────────▶ sites /api/internal
review_archive, laptops ────────────────── open, rate-limited ─▶ conductor /api/queries_cache: what was paid for
review_archive ── its request's User assertion, handed back ──▶ conductor /api/queries_cache/places/{key}/buy
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
                         costs (`prices`: every tool a vendor bills us or we bill a person for, its
                         items), a service's feed of what it spent (`usage`), and the vendors'
                         meters each item draws on, their free units, list prices and stops (`meters`), and a
                         Google account's definition less its secrets (`google`): its slices, credits, digest
crates/panel/            the engine: `Panel` (sources and their authentication, sessions, access
                         requests, the hubs' state), `seal` (XChaCha20-Poly1305 at rest), `store`
                         (SQLite, migrations/ applied on open, writes BEGIN IMMEDIATE; the queries
                         cache among it), `alerts`
                         (the hub: access, destinations, rules, the feeds fanned out; the
                         Directory and Feeds ports), `telegram` (the method: links, chats, outbox,
                         delivery, buttons; the Bot port), `live` (the in-process bus), `gmbs`,
                         `connections` (a gmail's inbox, Maps key and Business Profile client and grant; who pays a Places request), `inbox` (its mail
                         with Google followed into its GBPs' logs; the Mailbox port), `claude` (a person's own
                         Claude),
                         `browsers` (a gmail's browser: its proxy checked and sealed, Multilogin and
                         GoLogin, a launch),
                         `tokens` (the ledger, review_archive's charges, the snapshot), `street_view`
                         (a panorama bought), `meters` (the vendors' readings, synced; a meter's
                         use on an account, an account's standing; Google's billing export read), `import`
                         (review_archive's, once), `assistant` (the assistant's chats), `trash`
                         (what was removed, restored until purged), `oauth` (MCP clients, the
                         sessions granted them), `brands` (a gmail's in a category: name, logo,
                         site, the kit's photos), `categories` (a person's: versions, moves, lineage), `library`
                         (listings, their versions, published studies), `kit` (the review kit: a category's drafts, the offer, an owner's kits and their tally, the posted reviews matched to copies), `areas` (geography,
                         everyone's: the kit's links and studies' frames)
crates/alerts/           no I/O: the alert rules, and the notes a service publishes to the
                         conductor and their check; sites and review_archive take it too
crates/auth/             the `sa` permissions (concierge_iam derives), the aliases `sa:operator` /
                         `sa:admin`, and the assertion: a User, a Source or the Conductor, for one
                         request of one service
crates/browsers/         sa_browsers: the devices a browser poses as, a launch's wire and the viewer's
                         frames (lib); the runner (`sa_browsers serve`): Orbita headless, Mimic through
                         the Multilogin app's launcher, a browser's devtools passed to the conductor
                         alone, a film made the camera; a cloud phone's life and its viewer over adb
crates/sandbox/          sa_sandbox: the worker (Starlark, sealed by rlimits and seccomp before it reads a
                         cell), the wire between it and the conductor, the limits; the conductor's binary
                         is the worker when started as one
crates/places/           Google Places: a place from what a person pastes (an id, a Maps URL, a
                         search), its point; suggestions as one types (Autocomplete, no session
                         token); a locality's viewport, the one around a point (Geocoding), a
                         place's review link; every request through the queries cache (in process,
                         or `Remote` over /api/queries_cache), then the payer's own keys, then the
                         rotation its purse pays for; the Google accounts (`Account`) the rotation's
                         keys are on; review_archive takes it too, buying through the conductor (`Remote`)
crates/guard/            sa_guard, the guard: every enabled API of each account's billed projects held by a
                         spend cap, a daily quota or disabling, or left to its `unguarded`; billing unlinked
                         past `allow`, from Cloud Run (Google accounts)
crates/ui/               Dioxus components the services' front ends share
crates/placement_overlay/  placement's overlay re-exported (wasm, imported by the front end), so Cargo.lock
                         pins it to the builder `placement.rs` links: a renderer, the host fetches and pulls
crates/trace/            `SA_TRACE`: the spans a run closes, as Chrome trace events, for `test-timeline`'s chart
crates/contracts/        concierge's gRPC contract (tonic, from contracts/proto)
crates/server/           the `panel` binary: `serve`, `migrate`, `gen-data-key`; thin over `Panel`
  src/signin.rs          /auth/login, /auth/callback, /auth/logout; the /api/v1 gate
  src/oauth.rs           /mcp/{register,authorize,token} and their metadata: OAuth 2.1 for MCP clients
  src/mcp.rs             /mcp: MCP sessions; `search`, `call`, `execute` and `poll` to an MCP client
  src/concierge.rs       concierge over gRPC: ExchangeCode, RefreshClientToken, GetMe,
                         PublishCatalog; its development stand-in (PANEL_DEV_SIGN_IN)
  src/cookies.rs         __Host- cookies, the double-submit CSRF check
  src/http.rs            the router; the /api/v1 route table and each row's gate
  src/admin.rs           the admin operations: a site key's origin, a gmail for a person, the imports
  src/api.rs             GET /me, /sources
  src/connected.rs       GET /sources/sites: each site key's landing report beside sites' view
  src/access.rs          /api/v1/access/requests
  src/ingest.rs          the signed requests: authenticated here, forwarded to sites
  src/queries_cache.rs   /api/queries_cache: the queries cache, open
  src/forward.rs         the services behind the origin; `Services`, the one client and signing
                         key every call to them goes through
  src/grafana.rs         Grafana: its process, /grafana/, the viewers' tokens and the loopback
                         listener they work on, the services' dashboards; its own in dashboards.rs
  src/live.rs            GET /api/v1/live, and the follower of sites' change feed
  src/gmbs.rs            /api/v1/gmails, /api/v1/gmbs, /api/v1/google-categories
  src/categories.rs      /api/v1/categories, /api/v1/category-versions
  src/library.rs         /api/v1/library: listings, their versions, the standard library
  src/connections.rs     /api/v1/gmails/{id}/{connections,imap,maps_key,business}; each gmail's inbox followed
  src/browser.rs         /api/v1/gmails/{id}/browser{,/test,/start,/stop,/film,/thread}; the runner; the viewer's
                         socket, the relay between it and a browser's devtools, a phone's passed through
  src/tokens.rs          /api/v1/tokens, /api/v1/admin/tokens; review_archive followed and told
  src/trash.rs           /api/v1/trash; the hourly purge
  src/usage.rs           /api/v1/map/load, /api/v1/admin/usage; the services' usage feeds followed
  src/meters.rs          /api/v1/admin/meters{,/readings,/bill,/accounts/{name}/hold}; each account's definition
                         published when it changes; each account's meters and bill read at start
                         and hourly
  src/config.rs          the config file (`--config`): the Google accounts and their slices
  src/assistant.rs       /api/v1/assistant; the turns' shape
  src/kit.rs             the review kit: its /api/v1 routes, the landings' /api/kit/v1
  src/brands.rs          /api/v1/brands, /api/v1/gmbs/brand
  src/catalog.rs         every operation behind the origin: the conductor's own and the services'
                         documents, pulled and merged under their forward prefixes
  src/tools.rs           an agent's `search`, `call`, `execute` and `poll` over the catalog: a call runs through
                         the app with the caller's credentials, as their own request would
  src/execute.rs         `execute`, `poll`: the sessions, each a worker of this binary, driven in `serve`; a
                         cell's calls relayed as its caller's, held as the level holds them
  src/claude.rs          the Claude API; /api/v1/me/claude: a person's own Claude, connected
  src/ask.rs             the in-page assistant's loop (`POST /assistant/chats/{id}/ask`): Claude with
                         `search`/`call`/`execute`/`poll`, `open` (a view) and `run` (a binding); what the
                         level holds is proposed, the user confirms it, and a cell waiting on it goes on
  src/areas.rs           /api/v1/areas, and the cities and localities they are drawn from
  src/placement.rs       /api/v1/placement, /api/v1/library/studies: a study's Definition built from an area
                         and a category version, or kept frozen when published, and built by placement's
                         library in process; a pull held, what the study lacks bought, charged
  src/alerts.rs          /api/v1/alerts; the services' feeds as the hub reads them; the hub's
                         work in `serve`
  src/telegram.rs        the Bot API, its long poll
  src/web.rs             the front end's static export (PANEL_WEB_DIR), behind every other route
  src/settings.rs        the environment (ev_lib `settings!`)
crates/gen/              `nix run .#gen`: the `/api/v1` document into frontend/src/shared/api/openapi.json,
                         and the front end's client types from it (schema.ts), committed; the
                         pre-commit hook re-runs it
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
/api/v1/*            Authorization: Bearer, an MCP client's session (Agents over MCP); else, not
                     GET: x-sa-csrf must equal sa_csrf; the session (access token rotated
                     when within 30 s of expiry, see below); GetMe (cached ≤ 60 s per session,
                     asked afresh for POST/DELETE /sources, the Telegram link and a chat added,
                     one retry when concierge does not answer) → the caller's permissions, else 401 (cookies
                     cleared) / 503; then the row's permission, else 403
POST /auth/logout    CSRF; every session of the user is closed
```

- **Permissions, no admission.** Any active concierge account signs in. `GetMe` answers the
  concrete `sa` permissions the user holds (no alias, no wildcard; anything outside `sa` is a
  failed answer). Every `/api/v1` row names at most one permission, its `x-permission` and
  exactly what its gate checks (`sa_auth::Gate`, the conductor's `Vouch` the session):
  `sa:admin:sources:manage` for the source keys, `sa:admin:tokens:grant` for everyone's tokens,
  `sa:placement:study:read` / `:spend` for placement's studies and pulls,
  `sa:own:payments` / `:credentials` for what only the caller's own sign-in does (claiming,
  spending settings and the wallet; their Claude, linking Telegram, authorizing an MCP client),
  `sa:placement:library:curate` for the standard library's ranks, none for the rest (the caller's
  own gmails, GMBs, categories, tokens, alerts, chats and review kit, and the library). Each fresh
  `GetMe` upserts the user (`users`: id, email, name), whom tokens and gmails belong to.
- **The catalog.** `serve` publishes `Catalog::collect("sa", PANEL_BUILD_EPOCH)` — the whole `sa`
  namespace, every service's permissions — with `PublishCatalog` before it serves (the image
  sets `PANEL_BUILD_EPOCH` to the commit's unix seconds, so an older replica never supersedes a
  newer one); concierge refusing it fails the boot. Not under dev sign-in. Its `sa:everyone`
  alias is held by every member of the `sa` scope with no grant.
- **Acting as another** (`sa:members:act_as`): `PUT /me/acting {user_id}` (fresh; never oneself,
  while acting, or on an MCP client's session) has the browser's session act in that user's
  account (`sessions.acting_as`) until `DELETE /me/acting`. The gate then answers the subject
  (id, email and name from `users`) holding the actor's permissions `lent`: all but those
  `sa_auth` derives `bound` (`sa:own:*`, held through `sa:everyone` only). `/me`'s `acting` and
  the assertion's `actor` name the actor; every service, Grafana viewer and live socket follows
  the subject, who pays for what is done in their account. `GetMe` stays the actor's; one without
  `act_as` ends acting.
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
service, both ways (hyper-util, no buffering), one table for all of it (`ROUTES`).
With the conductor's own prefixes (`OWN`) it is every prefix of the origin and what the edge does
with each: the front end's reserved paths (`web.rs`, a JSON 404 below any of them) and the contract's
ingress lists (nix/contract.nix, held to `--print-paths`) are derived from it, and a test holds the
second block below to it.

```text
/api/sites/*                     → sites /api/v1/*              session + User; CSRF on writes (a bearer: none)
/api/hooks/booking/*             → sites (same path)            open: the providers' webhooks
/mfe/sites/*                     → sites /mfe/*                 open: the sites page's bundle
/api/review_archive/*            → review_archive /*            session + User; CSRF on writes
/mfe/review_archive/*            → review_archive /mfe/*        open: the dashboard's bundle
/api/playbook/*                  → playbook /*                  session + User; CSRF on writes
```

```text
/api/sites                public
/api/hooks/booking        public, rate-limited
/mfe/sites                public
/api/review_archive       public
/mfe/review_archive       public
/api/playbook             public
/api                      public                  the conductor's own, and nothing else's
/api/v1                   public
/api/queries_cache        public, rate-limited    open to everyone
/api/ingest               in-cluster
/api/kit                  in-cluster
/auth                     public, rate-limited
/mcp                      public
/mcp/register             public, rate-limited    open (RFC 7591)
/mcp/authorize            public, rate-limited
/mcp/token                public, rate-limited
/grafana                  public
/mfe                      public                  the services' bundles, forwarded or not
/health                   public
/cabinet                  public                  once the cabinet's account chip
```

- **The assertion** (`sa_auth`, header `x-sa-assertion`) is signed for one request: Ed25519
  (`PANEL_ASSERTION_KEY`), `aud` the service, the method and the upstream path (without its
  query), alive 60 s, and a principal: a **User** (`sub`, email, `email_verified`, name, and
  their `sa:<service>:*` permissions only, and the `actor` acting in `sub`'s account), a **Source** (a key the conductor authenticated:
  `key_id`, kind, brands, and the SHA-256 of the body its MAC covered), or the **Conductor**
  itself (its hubs). The services verify it with the public half (`PANEL_ASSERTION_KEYS`,
  several while a key rotates) and trust nothing else.
- **Hygiene:** the browser's `Cookie`, any inbound `x-sa-assertion` and `x-sa-csrf`, an MCP
  client's bearer, and hop-by-hop headers (`Upgrade` included: nothing is upgraded through the
  forward) never go up; `Set-Cookie` never comes down. Everything else passes (`Idempotency-Key`).
- **Gates:** a gated read takes the cached `GetMe`, a gated write asks concierge afresh. A write
  under a service's API needs the conductor's CSRF header, or an MCP client's bearer.
- **A path stays under its upstream prefix:** a `.` or `..` segment, `%2e`, `%2f`, `%5c` and `\`
  decoded, is a 400.
- **The outline.** Every signed-in page sits beside it: the open service's own views (the sites:
  `<mfe-sites-nav>`, mounted on every page and shown on theirs; the review archive: its places,
  Telegram and settings), then the caller's GMBs under a scope every app shares (gmails × categories × brands, kept in the
  browser), with their switches, each row, pin and card with what needs the operator, from the
  services' tags on the GMBs (review_archive `/me/gmbs`, sites `/gmbs`). `/` is the map of them:
  one Google map per page session, kept while the page is away (each built is a billed load), its
  viewport splitting the outline's list; placing a GMB's point is a mode of it.
- **The pages.** `/sites` mounts `<mfe-sites-panel api="/api/sites" base="/sites" csrf-cookie>`
  from `/mfe/sites/`; `/reviews/archive` mounts
  `<mfe-review-archive-dashboard grafana="/grafana/" ui>` from `/mfe/review_archive/`, `ui` its
  default (`reviewArchiveUi` in `frontend/panel.config.ts`). Every view (`View`, the services'
  nested under their pages) is the front end's one page, which shows the view the location
  parses as; below a nest the path is the element's to route. The bundles run on this origin:
  their builds are trusted as the conductor's own code is.
- **Windows are routes, pages and hotbar entries.** Every window (a GBP, a gmail, a category, the library, a brand,
  the categories, the trash, Add a GBP, a review kit) is a key of one closed union (`WinKey`,
  `entities/gmb/model/windows.ts`: how it reads from and writes to Home's params, its page, its
  entry), drawn by one registry (`views/windows`: title, icon, body, the same over the map and as
  a page). One floats at a time; a phone only ever gets its page. The tab's open set
  (sessionStorage) is the hotbar's entries after the app's tools; × or Escape takes one out. One
  floated and since hidden stays mounted in the shell, its effects off, until it is taken out.
  Map modes (the kit's editor, a study's area) draw on the map: no window, no page.

  ```
  Home ?gmb=12&tab=log ─┐                         open set [kit:lyon, gbp:12, account:3]
  /gbps/12?tab=log ─────┴─▶ shown: WinKey ──▶ hotbar: shown ▸ pressed · hidden ▸ plain · page ▸ ⤢
                              │                  click: hide / show / float back · × closes
         desktop: floating panel (⤢ its page, ×) · phone: its page · page: ⤡ over the map · leader z flips
  ```
- **The elements route by the location.** The front end's location store moves it with
  `pushState`, which fires nothing: after each move, its own or an element's (it wraps
  `pushState`/`replaceState`), it dispatches a stateless `popstate`, as back and forward do
  (Next's own handler ignores it), and puts back the carried keys a move dropped. An element is created once per tag, attributes and language
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

## Placement

gmaps_optimal_placement is a library `placement.rs` links: the builder, its vocabulary (`_core`:
`Definition`, `Category`, `Ask`, `Country`) and the overlay that draws its payload
(`crates/placement_overlay` re-exports it) come from one pin, so `Model::try_new` checks a payload
against the model that built it. It builds over its work dir (`PANEL_PLACEMENT_DIR`: the public
archives, and every studied definition for its `fit`), on a blocking thread, and reads a billed
answer only through its `Held`: here, this conductor's queries cache.

```text
GET  /placement/study?area&version ── the area, a category version the caller reads (or none) → Definition ─▶ build, kept per id
GET  /placement/studies/{sha} ─────── a published study's frozen Definition ─▶ the same build, the same keeping
     ◀── the payload ── the front end hands it to the overlay (wasm, built with the front end)
POST /placement/study/pull?…        ─ since: when the first pull since the study's last whole one began
     asks ─▶ [Ask], each bought into the queries cache (`gop`), asked again until none ─▶ build ◀── the payload
     a whole pull forgets since; a stopped one keeps it, so its retry buys only what it did not
```

A study is an area and a category version: the version is its whole definition, the area applies
it, and one `define` builds the `Definition` for every study route, naming each gcid in the
country's language. Its competitors are tiered by which of its Google categories' searches found
them, first match wins: the trade's (its name and each keyword), then one per additional category
(its name alone), Excluded checked before any. Each additional tier weighs against the trade's 1,
as each keyword against the trade's name. A published study keeps its definition, the area's
geometry frozen in it (areas move); loading it builds over the same queries cache and buys nothing.

The conductor decides what cannot be studied, by why (`another_country`,
`unstudied`: an area in a country placement holds no base for); the front end only renders it. A study
never spends; a pull (`sa:placement:study:spend`) buys the Places searches it lacks at the puller's
tokens (Tokens and GBPs). Placement buys nothing: the conductor is the one center of data
ingestion, and what placement derives, for a map or for training, it derives from what the
conductor holds. Unset, those routes answer 503 and the map says placement is not connected.

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
  watches, or sooner the minutes its launch says the person's balance affords.
- **The runner owns a phone's life**, so a dead conductor leaves none billing: it is recorded on the
  runner's disk before `phone/start`, stopped on every way its run ends (a failed step, a step past
  the time it should take, stop, removal, idle, the cap, SIGTERM), the stop retried until `statuses` reads it stopped, and a recorded one
  stopped before the runner serves again. The launch brings a 3-hour token, past the 180 billed
  minutes Multilogin caps a session at: the runner holds no long-lived one.
- **What it spent, the runner tells** (`GET /usage`, `sa_core::usage`): a phone's run at its stop,
  its minutes from just before `phone/start` (on disk with its record), a started one whole; it is
  counted if its start was answered or it was read running at the stop, so one stopped already
  when a restarted runner reads it is not counted twice. A browser's start, once up. Each names
  whom its launch was for, who is charged as the conductor takes it.
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
             Service::dashboards ──▶ <dir>/dashboards/<service>/<uid>.json ──▶ Grafana
```

- **Datasources are derived:** one Infinity datasource per service's API (`/api/<service>`), its
  uid the service's name, and `conductor` for the conductor's own GETs (`/api/v1`, each row's
  gate taking the viewer's token on a request the loopback listener marked); its url that API on
  the loopback listener and nothing
  beyond it allowed; written, with the dashboards' provider and the conductor's own dashboards,
  before Grafana starts.
- **Dashboards are the services':** coupled to a service's API, they ship with its release, built
  over its document (`sa_core::dashboards`): a query names an operation, and a field its JSONata
  reads that the answer lacks fails the build. A
  service that has them says so (`sa_auth::Service::dashboards`), and its `GET /dashboards` answers a JSON
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

**Keys** (`/api/v1/sources`): one kind and a set of
brands each, the secret shown once. Kinds `panel` and `booking` are written by sites itself: no
key is issued for them. A `site` key carries its landing's origin (`https://<domain>[:port]`,
nothing after it; plain http on `localhost` / `127.0.0.1` for a landing on this machine): given
when the key is issued, or once with `PUT /sources/{key_id}/origin` for a key issued before origins.

## Queries cache

What a paid query was answered — Places searches and details, placement's search volumes — is
kept here once per query, for anyone who might ask it again: sa_places and placement's studies in
this process, review_archive in the cluster, a laptop's placement CLI over the public origin.

```text
GET    /api/queries_cache/{scheme}/{key}         200 {answer, fetched_at} | 404
POST   /api/queries_cache/{scheme}/{key}/claim   {fresh_since?} → 200 {state: stored, answer, fetched_at}
                                                 | {state: granted, until} | {state: held, until}
DELETE /api/queries_cache/{scheme}/{key}/claim   204: the buy failed
PUT    /api/queries_cache/{scheme}/{key}         {answer, request, bought: {item, bill: ours|theirs} | null}
                                                 → 204; 400 unless key = SHA-256(request)
```

- **A key is its query's SHA-256**, under its client's scheme: `places` (sa_places: method,
  URL, field mask, body) and `gop` (placement: Google's URL, body; a study's Places searches the
  conductor buys for a pull, `sa_places::Google::asked`). The cache derives none.
- **Buying is claim, buy, PUT.** A claim holds the key 30 s; whoever else asks waits on it and is
  served what was bought. A failed buy gives the claim up; a holder that dies lets it lapse.
- **A PUT says what it bought** (`bought`: the catalog item and whose key, null for an answer kept
  from another bought): counted as a use in the write that keeps it.
- **Every version is kept, the latest served.** The latest answer per key is `queries_cache`'s
  row in `panel.db`; every older version is in the history, `queries_history.db`
  (`PANEL_HISTORY_DB_PATH`), keyed by when it was fetched and never changed. A version arriving
  newer moves the row it replaces there first; one arriving older goes there; the same one again
  is nothing. Nothing expires:
  `fresh_since` is a reader's age limit, or its re-ask.
- **Every version keeps its request**, and the store refuses one whose key is not its SHA-256,
  whoever writes it.
- **Open to everyone**, reads and writes: bounded as ingest, a body ≤ 8 MiB, and rate-limited per
  client at the edge.
- **review_archive buys through the conductor** (`sa_places::Remote`), holding no key and no
  list: `POST /places/{key}/buy`, `{request, fresh_since?, gbp?}`, carries the User assertion the
  conductor signed on the request it serves (`x-sa-assertion`, `sa_auth::vouched`). The conductor
  buys as it would for that person's own request: the GBP's gmail's key, their other gmails', then
  the rotation at the price; `{answer}`, or why not (`BuyRefused`: 400 not a request sa_places
  makes, its SKU read off the field mask and never the caller's; 401, 402, 502 Places refused it,
  503). The trade-off: for its 60 s the assertion is good for any buy, not only the request's
  own, so review_archive, which already charges the person for its walks, can spend their tokens
  on Places for that long.
- **Prod's is the one authority; every other conductor works on a copy and gives back what it
  bought.** `panel queries-cache index` lists every version held, `export --except <index>` the
  ones that index lacks, `import` keeps those, each as of its own `fetched_at`: run at both ends
  by `scripts/queries-cache-sync.sh` (docs/LOCAL.md, Prod's data). A copy of prod's `panel.db`
  needs none of its history: what it lacks of it is excepted by prod's index.

## The conductor's API, `/api/v1`

JSON; timestamps RFC 3339; every refusal the conductor writes itself, the gate's and the forward's
included, is `{"code", "error"}`: `code` a closed enum (`ErrorCode`) a client acts on, `error` for a
person.

**One table, everything derived** (ev_lib `api`): `http::api_routes` is what the router is built
from; each operation's request and answer are its handler's signature, its description the
handler's `///`, its id the handler's name, its permission its gate's, and what it spends its
`sa_core::prices::Spends<Tool, _>` (`x-spends`: the tool's items), which hands the handler the
`Spender` that whatever buys takes. `GET /openapi.json` is that document, narrowed to the operations the caller's
permissions open; the front end's client and types are generated from it (`nix run .#gen`), and
an agent's tools read it. Below, the overview (a test holds it to the table), its permissions
`sa:admin:…`; what each answers is in the document. Slow: 8 at once, 10 min each, not the bounds
above; a socket's handshake: as `/live`'s. Every refusal is the row's `x-errors`; every answer
`no-store`, and a 401 clears the cookies.

**The catalog** (`catalog.rs`): sites (`/api/v1/openapi.json`), review_archive and playbook
(`/openapi.json`) derive theirs the same way; the conductor pulls each as it pulls `/dashboards` (Conductor, every
60 s, conditional; a failed pull reported, the last kept), puts its paths under its forward
prefix and its ids and schemas under `<service>.`, and merges them with its own. `GET
/openapi.json` and an agent's `search` show a caller only what their permissions open: context,
not authority — each service still authorizes the call, which goes through the forward as a User.

```text
GET    /me
PUT    /me/acting                           members:act_as, fresh
DELETE /me/acting
GET    /members                             members:act_as
GET    /me/claude                           own:credentials
POST   /me/claude/connect                   own:credentials
PUT    /me/claude                           own:credentials
DELETE /me/claude                           own:credentials
GET    /openapi.json
GET    /live                                socket
GET    /sources                             sources:manage
POST   /sources                             sources:manage, fresh
GET    /sources/sites                       sources:manage
DELETE /sources/{key_id}                    sources:manage, fresh
PUT    /sources/{key_id}/origin             sources:manage, fresh
POST   /access/requests
GET    /access/requests/mine
GET    /alerts
POST   /alerts/telegram/{link,chats}        own:credentials, fresh
PUT    /alerts/destinations/{id}/{rules,gmail}
POST   /alerts/destinations/{id}/test
DELETE /alerts/destinations/{id}
GET    /gmails
POST   /gmails
DELETE /gmails/{id}
GET    /gmails/{id}/connections
GET    /gmails/{id}/browser                 socket: the viewer
PUT    /gmails/{id}/{imap,maps_key,browser,business}
POST   /gmails/{id}/{imap,maps_key,browser,business}/test
DELETE /gmails/{id}/{imap,maps_key,browser,business}
POST   /gmails/{id}/business/consent
POST   /gmails/{id}/browser/{start,stop,thread}  start: slow
PUT    /gmails/{id}/browser/film            slow
DELETE /gmails/{id}/browser/film
GET    /gmbs/{resolve,suggest}
POST   /gmbs
POST   /gmails/{id}/gmbs
PATCH  /gmbs/{id}
PUT    /gmbs/{id}/state
POST   /gmbs/{id}/sync
GET    /gmbs/{id}/photos/{photo}
GET    /gmbs/{id}/log
DELETE /gmbs/{id}
PUT    /gmbs/{enabled,trade,brand}
PUT    /gmbs/{id}/spots
PUT    /gmbs/{id}/location
DELETE /gmbs/{id}/location
PUT    /gmbs/{id}/country
GET    /map
POST   /map/street_view
POST   /map/load
GET    /categories
PUT    /categories/{trade}/{country}
DELETE /categories/{trade}/{country}
POST   /categories/{trade}/{country}/head
GET    /categories/{trade}/{country}/history
GET    /category-versions/{id}
GET    /categories/{trade}/{country}/kit
POST   /categories/{trade}/{country}/kit/drafts
PATCH  /categories/{trade}/{country}/kit/drafts/{draft}
DELETE /categories/{trade}/{country}/kit/drafts/{draft}
GET    /library
POST   /library
GET    /library/{id}
POST   /library/{id}/versions
POST   /library/{id}/withdraw
PUT    /library/{id}/standard               placement:library:curate
POST   /library/studies
GET    /library/studies
POST   /library/studies/{sha}/withdraw
GET    /brands
POST   /brands
PATCH  /brands/{id}
DELETE /brands/{id}
PUT    /brands/{id}/logo                    slow
GET    /brands/{id}/logo
GET    /brands/{id}/photos
POST   /brands/{id}/photos                  slow
GET    /brands/{id}/photos/{file}
PATCH  /brands/{id}/photos/{file}
DELETE /brands/{id}/photos/{file}
GET    /gmbs/{id}/kit
PUT    /gmbs/{id}/kit
POST   /gmbs/{id}/kit/shown
GET    /kits
GET    /kits/{slug}
PUT    /kits/{slug}
DELETE /kits/{slug}
GET    /areas
GET    /areas/{slug}
PUT    /areas/{slug}
GET    /placement/catalog                   study:read
GET    /placement/study                     study:read, slow
POST   /placement/study/pull                study:spend, fresh, slow
GET    /placement/studies/{sha}             study:read, slow
POST   /placement/studies/{sha}/pull        study:spend, fresh, slow
GET    /kit/cities
GET    /kit/cities/{place_id}
GET    /kit/locality
GET    /google-categories/{country}
GET    /trash
POST   /trash/{gmails,gmbs}/{id}/restore
DELETE /trash/{gmails,gmbs}/{id}
PUT    /trash/settings
GET    /tokens
GET    /tokens/ledger
GET    /tokens/usage
POST   /tokens/claim                        own:payments
PUT    /tokens/settings                     own:payments
GET    /admin/tokens                        tokens:grant
POST   /admin/tokens/{user_id}              tokens:grant, fresh
GET    /admin/tokens/prices                 tokens:grant
PUT    /admin/tokens/prices                 tokens:grant, fresh
GET    /admin/tokens/flows                  tokens:grant
GET    /admin/usage                         tokens:grant
GET    /admin/meters                        tokens:grant
GET    /admin/meters/readings               tokens:grant
GET    /admin/meters/bill                   tokens:grant
PUT    /admin/meters/accounts/{name}/hold   tokens:grant, fresh
POST   /admin/gmails                        tokens:grant, fresh
POST   /admin/imports/review-archive        tokens:grant, fresh
GET    /assistant/chats
POST   /assistant/chats
GET    /assistant/chats/{id}
PUT    /assistant/chats/{id}/{turns,title,pinned,archived,general}
DELETE /assistant/chats/{id}
DELETE /assistant/archived
GET    /assistant/chats/{id}/access
POST   /assistant/chats/{id}/members
DELETE /assistant/chats/{id}/members/{user}
GET    /assistant/people
POST   /assistant/chats/{id}/ask            slow
POST   /assistant/chats/{id}/act
```

An id in a path that is not the caller's is a 404.
**The assistant** answers on the conductor (`ask.rs`) on the asker's own Claude if they connected
one (`claude.rs`: `claude setup-token`'s OAuth PKCE, the code pasted back from Anthropic's page,
the token proved, then sealed), else on the panel's (`CLAUDE_CODE_OAUTH_TOKEN`), chosen once per
answer; their own refused (401) stops the answer and is marked, never the panel's in its place.
On the panel's, each round is charged at the `claude` items' prices (per million tokens), none
starting once their balance is spent. Each answer carries its `cost`: the model's tokens, whose
Claude, the tokens charged. Its
calls take the `/api/v1` gate on the asker's own session, never a side door to `Panel`. An
answer's action is a view (`href`), an operation of the catalog (`call`: its id and arguments, run
when the person confirms it) or a binding of the page's keymap (`binding`), nothing else; the page
tells it what is open, its views (`ROUTES`) and the bindings bound now.
**The assistant's chats** are the conductor's own; nothing forwards them, no service stores their
ids, so they are deleted, with their members and marks. Owner and members write (turns, a title,
invites; a member leaves); `general: view` lets anyone holding a `sa` permission read one, 403 on
any write; anyone else is answered 404. Turns are written whole from an index, on the version
read; a body ≤ 4 MiB (images ride as data URLs), a chat's turns ≤ 16 MiB, else 413. Before the import (below) the gmail, GMB and category
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

## Agents over MCP

`<origin>/mcp` serves MCP (Streamable HTTP: `initialize` opens an MCP session, `Mcp-Session-Id`, which
every later request names and `DELETE` ends; JSON answers, SSE where it asks the user) as the
signed-in user, the only MCP of the services: `search` and `call` over the catalog (`tools.rs`),
`execute` and `poll` (Code mode, below), and each operation
a service marks its own tool (`x-tool`, `sa_core::agent::Tool`) as `<service>_<operation_id>`
(`playbook_search`), its parameters and body the tool's input, a text answer handed on as it
came. Its instructions are the documents' descriptions, the conductor's then each service's. A
call runs as their own request would, through the gate or the forward, unless the approval level
holds it (below): then a client that declared `elicitation` asks its user (`elicitation/create`, a
form of one boolean, on the answer's stream) and runs it on their yes; any other answers what it
would do until it comes with `confirmed: true`, which the client sets once its user agreed.

```text
POST /mcp/register    open (RFC 7591): redirect_uris (https, or http on loopback, any port), a name
GET  /mcp/authorize   client + redirect_uri registered, PKCE S256, else a 400 page; signed out →
                      /auth/login?return_to= and back; the consent page, its form carrying sa_csrf
POST /mcp/authorize   the form's CSRF, the session → concierge's sign-in again, the grant sealed in
                      sa_prelogin → /auth/callback: a session of the client's (sessions.client_id),
                      the same user's or refused → redirect_uri?code&state
POST /mcp/token       code (60 s, once; its hash, the bearer sealed) + verifier → {access_token}
POST /mcp             Bearer, else 401 with WWW-Authenticate's resource_metadata; initialize → Mcp-Session-Id,
                      named by every later request (none 400, unknown or ended 404); a client's
                      answer to what it was asked → 202
DELETE /mcp           its session ends, the sandbox's with it
/.well-known/oauth-{protected-resource,authorization-server}/mcp   RFC 9728, 8414
```

- **One approval level, at the gate.** `ask` proposes what destroys or spends, `spend` runs what
  spends, `act` runs both (`sa_core::agent::Approval`; `Hints::held_by`, which `call`, an
  operation's own tool and a cell's relay all ask). The in-page assistant is told it with each
  question (the composer's chip); an MCP client's is its grant's scope, chosen on the consent page
  and kept on its session: `spend`, `act`, or neither for `ask` (`scopes_supported`). A call that
  destroys or spends and ran on its own is an action already done in the answer.
- **A grant is a session**, its id the bearer: rotated, refused and closed as a browser's, so a
  family revoked at concierge or a sign-out (every session of the user) ends it, and it lives as
  long as the session does: no refresh token. A browser's id is no bearer, nor a client's a cookie.
- **Bounded:** the OAuth routes as `/auth/*`; `/mcp` 32 at once, 10 min each (a call waits on its
  operation).

## Code mode

`execute { code }` and `poll { cell }`, on both transports: Starlark over the catalog's operations, for
work that takes several calls. It runs in a sealed child of the conductor, a worker, as a cell of a
session that outlives a reply.

```text
conductor (holds the secrets)
  Sandbox: sessions by Key (chat+asker | MCP session), driven by one future `serve` joins
    │  spawn <conductor's own binary> __sa_sandbox_worker: env cleared, stdin/stdout/stderr only
    ▼
  worker (one per session, kept while the session lives)
    1. rlimits: data (its binary's + twice HEAP), CPU (LIMIT), open files (3)
    2. no_new_privs + seccomp allowlist: read fd 0, write fds 1 and 2, its memory, futex, clocks,
       getrandom, signals' masks, exit. Anything else kills it (SIGSYS). Unsealed, it runs nothing.
    3. Starlark: each cell a Module seeded with what earlier Done cells bound (frozen)
       call(operation, path=, query=, body=) ─ Up::Call, a JSON line ─┐
       print(...)                            ─ Up::Print ─────────────┤
                                                                      ▼
  Sandbox: CALLS, the level (held: the person, PAUSE), then Tools::call as the caller's credentials
           ─ Down::Answer {status, body}, a JSON line on the worker's stdin
```

- **A session is its caller's alone**: in-page, a chat's as one person asks in it (acting as another,
  as that pair); on MCP, the MCP session's. Chats are shared, and a session's names hold what its
  caller's permissions fetched. Each session is a worker of its own, so no session's names reach another's.
- **A cell sees what the cells before it bound**: the top-level names of every cell that ended Done,
  frozen (to change one, bind it again; a name bound again is the later cell's); a cell that failed
  hands on nothing. It runs only alone: one at a time.
- **The language is Starlark** (starlark-rust): no `load`, no `while`, recursion bounded by the
  evaluator's call stack and time by `LIMIT`; the standard library, `json` and `print`. Its one global
  is `call`, the `call` tool's arguments (`sa_sandbox::Call`: an operation id as `search` names it, path,
  query, body), returning its answer as `{status, body}`. Starlark has no try/except, so every outcome is
  a value: the conductor's own refusals (not an operation, past `CALLS`, declined, no answer within
  `PAUSE`) come back as status 403, `{code: "refused", error}`, for the code to decide on.
- **Bounded:** a cell's output is kept to `OUTPUT` (64 KiB) a stream, its calls to `CALLS` (100); it is
  answered after `WAIT` (20 s) and killed at `LIMIT` (10 min), its session with it (`Killed`). Past
  `HEAP` of Starlark's own heap accounting the session dies, told why, and the data rlimit is the
  kernel's backstop (`Died`, as any signal). A session idle `IDLE` (30 min) is let go, and past `MAX`
  sessions the idlest; none idle, a new one is refused as busy. A session whose worker ended answers
  the next execute that its state is gone, once; it is never made anew empty under the same name. The
  limits are `sa_sandbox`'s.
- **A cell's calls are its caller's own**: each runs through the app with the caller's credentials as
  of their latest request about the session, so a rotated session is followed, and a 401 ends the
  session (`SignedOut`).
- **A held call waits in the relay, and the cell with it.** In-page, it is an action in the answer;
  `act` lets it run there and resumes the answer: a new turn, the model told what the code did next.
  A new question in the chat declines it. On MCP, a client that declared `elicitation` is asked on
  the answer's stream; one that did not sees it refused at once, its proposal in the error. Past
  `PAUSE` (10 min) unanswered, it is refused.

## Live updates, `/api/v1/live`

A screen reads through the API and is told, over one WebSocket, when what it read may have
changed. Nothing is sent but the hint.

```text
GET /api/v1/live    the session, its row's gate (cached GetMe)               else 401 / 503
                    Origin = PANEL_PUBLIC_ORIGIN exactly                     else 403
                    ≤ 5 sockets per user, ≤ 200 in all                       else 429
                    → 101; any signed-in user
server → client     {"type":"hello","at","user_id"}                          at once
                    {"type":"changed","topic","at"}                          a table committed to
                    {"type":"resync"}                                        read everything again
                    WS Ping every 25 s; no Pong within 60 s → dropped
closes              4401 the session ended   1001 the server is stopping
```

- **A topic is a table** (`<service>.<table>`), and everyone signed in is told every one: what
  they may read is judged when they read it. A screen follows the topics its GET's `x-sa-reads`
  named.
- **The conductor's own writes** are told by its store's pool (`ev_lib::sqlite`): every committed
  write, a request's, the bot's or a background task's, as `conductor.<table>` once its connection
  goes back; a rolled-back one never, nor one that changed no row (an upsert that writes the same
  values changes it).
- **What a service changed** comes from its feed, `GET /api/conductor/changes` (NDJSON, a
  Conductor assertion; sites and review_archive): one follower per service puts each `changed`
  line (`sa_core::feeds::Line`) on the bus as it came, and each `resync`. A service sends
  `resync` first on every connection and a `ping` every 25 s; a feed that ends, fails, or says
  nothing for 60 s is followed again after 1 s, doubling to 30 s, so what was missed meanwhile is
  read again. One that answers 404 has none yet: said once, asked again every 60 s.
- **The shell relays.** The front end holds the one socket (`@evinvest/api`'s `useLiveSocket`)
  and re-dispatches on `window`, as `CustomEvent`s: `ev:live` (each server message as is, and a
  `{type:"resync"}` after the shell reconnects), `ev:live-status` (`{status}`; answered to an
  element's `ev:live-status-request`). Each read follows the tables its last answer named
  (`x-sa-reads`); one that named none re-reads when the window regains focus. The sites elements dispatch `sa:title-count` (`{count}`, the nav
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
other gmails' keys, free; then the Google accounts' Places keys (below) at the admin's price per
request kind (Places' SKU), held under one lock per person from the balance read to the charge, so a
request below the price is refused before Google is asked. A cached answer costs
nothing.
review_archive's lookups are its caller's too: the conductor buys them (Queries cache, the buy).

**A study's pull is sold per search** (`placement.search`) the conductor buys for it, on the
caller's own keys first, then the house's rotation, under the same lock:

```text
POST /api/v1/placement/study/pull ─ price set, budget = balance ÷ price ─▶ the study's asks: [Ask]
  ├─ the first round needs more than n: 402, nothing bought ─▶ the person told the tokens it needs
  ├─ each Ask bought (the pull its purse: none past n, else 402), asked again until none is lacking
  └─ ─▶ built ─▶ the ledger: the searches bought × price; a pull that failed
                    owes it all back, as a failed walk does: the failure is the house's
```

A person's front end reads the prices with their balance, and an action costing more than
their `confirm_above` (100 unsaid) asks them first.

**A price is a cost times a multiplier.** A token is worth what we spend for it
(`sa_core::prices::TOKEN_USD_MICROS`: 1 000 tokens a dollar), so a balance reads as money. Every costly action
is an item of `sa_core::prices`: each tool declares its items there once (an enum, a variant's message
its name and its doc what it is) with where it happens, its unit and how many its cost is for (a
model's tokens: a million). Both halves are the admin's:

```text
cost        what its units cost us, dated (`unit_costs`, append-only): a use costs what its unit did
            when it was done, the first cost ever set for a use before it
multiplier  the premium over it (`token_multipliers`): 1 at cost, 0 free
price       the cost now, in tokens, × the multiplier, rounded up (`Rate::price`); never stored
```

Every use of every item is charged to whom it was made for at its price, and the charge keeps what
it cost us (`token_ledger.cost_usd_micros`; review_archive's charge names what its run used), so a
spend reads as charged or at cost. The admin's page, both tables and the snapshot are walked from the
catalog. Each item the catalog gains starts at cost 0, ×1 (`init_rates`, on open): free until
priced, never refused for want of a price.

**Usage is what vendors bill us.** Each use is a row of `usage` (append-only: item, units, whose
bill, who told), counted where it happens:

```text
a Google answer bought, any service ─ PUT /api/queries_cache, `bought` ─ in the write keeping it
a map, before it is built ─ POST /map/load        a panorama ─ POST /map/street_view   (each charged in its write)
review_archive's walk steps ─ GET /api/conductor/usage ┐ followed every 10 s, a page, its charges
the runner's phone minutes, browser starts ─ GET /usage ┘ and its cursor in one write (`sa_core::usage`)
the assistant's model tokens, by kind ─ each round of POST /assistant/chats/{id}/ask, its model the key,
                                        on whose Claude it ran
```

**Street View is bought before it opens.** Google bills each panorama opened on a browser key
(Dynamic Street View) past 5 000 free a month on its account. The map has no Google pegman: its own shows the
coverage (a press toggles it, a drag shows it while held), and dropped on a panorama Street View
finds (free) asks `POST /map/street_view` on the page's account, which records it and charges
`street_view.panorama` in one write, and refuses whoever asks once its meter there stops (below).
Only then does the browser open it.

**Google accounts.** Free units, credits and the money kept are per billing account, so each is
declared once, in `nix/google.nix`, which the config file (`--config`, `google`:
`sa_places::Account`, and `slices`) imports whole: its export (`null` until its billing account is
open: then nothing of ours uses it), its reader and the day it is read from, and what it `provides`,
each slot optional: `places` (Places, Geocoding),
`browser` (Maps JS). What it is **held** to an admin sets in the meters card (`PUT
/admin/meters/accounts/{name}/hold`, `sa_core::google::Hold`, kept in `google_holds`, every one):
`allow` for every slice, a Pacific day's and month's real money (`{ day; month; }`, 0 for none; the
conductor holds the month of `maps`, `sa_core::meters::SLICE`, which every meter falls in, at rank
1; the guard holds both of every slice at Google), and `unguarded`, the APIs the guard leaves alone
(below), and its credits as the console's Credits page read them (`id` once the export names it).
`Hold::check` refuses a slice missing or unknown, an amount below 0 or not finite, a day past its
month, and a credit with no name, no amount or another's id. An account with no export or no hold is not used. What of it is no secret is
`sa_core::google`'s, which the conductor and the guard both read. Every key bills on its account (docs/spec/google.md); each use of ours is
stored with the account that paid (`usage.account`). N accounts are one with N× the free units,
the money capped per account.

```text
nix/google.nix  accounts = [ { name, export { table, reader, since } | null, provides { places?, browser?, ads? } } … ]
                slices = [ { name, service, sku? } … ]
the admin       PUT …/accounts/{name}/hold { allow { <slice> = { day, month } }, unguarded [ <api> ], credits [ … ] } ─▶ google_holds
the conductor   each minute, a definition (sa_core::google::Definition: the account less its secrets, its hold,
                the slices) changed ─▶ <export dataset>.definition, with its reader
  ├─ the guard (crates/guard, Cloud Run): the exports baked into its image; each tick reads each one's latest
  │    definition, and holds every enabled API of each billed project by its class (below), each slice's real
  │    money to `allow`; an account it cannot read or hold is reported, the others held all the same
  ├─ the conductor: `import` (~/.config/sa_conductor.nix), or devops' literal of google_env.nix,
  │    each secret slot `{ env = "SA_GOOGLE_<NAME>_<SLOT>"; }`
  each account ─ its Cloud Billing export, read at boot and hourly ─▶ meter_readings (account, meter)
                                                                  ─▶ google_bills: its bill from `since`,
                                                                     and the guard's last tick, whole
a buy of an item on a meter ─ each account's standing (`meters::standing`):
     0  its free units of that meter left
     1  its net spend this period (the export's, after credits) under `allow.maps.month`
     ─  neither: not asked
  the lowest, config order breaking ties: Places' rotation (sa_places `Rank`); a page's map
  (`GET /map`: free map loads and panoramas both, its key and account for the page's life)
```

**The guard holds the money at Google, out of the cluster.** A Cloud Run job every 15 minutes
(`nix run .#guard`: a dry run; `.#guard-deploy`: from this commit, as the billing owner, after a change
to crates/guard or an account added), so it holds while the cluster is down, to the definition last
published, and billing.admin stays out of it. Every API enabled on a billed
project falls in one class, which the guard derives and the config never restates
(docs/spec/google.md, `google.guarded`):

```text
class       recognised by                                        lever, set before spend
unguarded   named in the account's `unguarded`                   none: left alone, the backstop only
free        the guard's `FREE`: Google's own that bill nothing   none
capped      its billing service is one Google spend-caps        a spend cap: this month's gross + its share of the
            (`CAPPABLE`: Vertex AI, Gemini API, Cloud Run,        credits left + what its slices may still spend today
            Cloud Run functions, each with its API)
metered     `sa_core::meters` prices it, and it has daily quotas  each daily quota: its meters' least free units over
                                                                 31 days, split over the projects it is on, + what
                                                                 `maps` may still spend today at their dearest price
quota'd     project-wide daily quotas, no meter                  each daily quota at 0: no price turns money into requests
else        –                                                    disabled
```

What a slice may still spend today is `max(0, min(day − real today, month − real this month))`,
spread over the caps it draws through. The guard never caps, zeroes or disables what it runs and
reads through (`OWN` on its own project: Cloud Run, Scheduler, Artifact Registry, Monitoring,
BigQuery; BigQuery on each export's project): each enabled there and not free must be in its account's
`unguarded`, or the tick errors before it acts. `unguarded` is the account's, so naming an API
leaves it alone on every project the account bills. The **backstop** unlinks billing from every
project a slice spent real money in past its day's or its month's allowance by 0.10, hours late.
A metered API's pace holds every SKU through it in its free units: the meters must carry its
least-free SKU, and a SKU of a metered billing service whose priced tier starts below its API's
pace stops the tick. Each tick lands in `<export dataset>.guard`, with the definition's digest,
which the next read of the bill brings back: the admin's meters card and sa-meters show each
slice's day and month, its levers, the APIs spending with no lever and those disabled, and
whether it holds the definition the conductor publishes now. To add an account: its *Standard usage cost* export
into a dataset, a service account with BigQuery Data Viewer on it and Job User on its project (its
key the `reader`), an entry here with its `since`, then `guard-deploy` (it makes the dataset's
`definition` table, the reader its editor); then its hold in the meters card: its credits as the
console's Credits page reads them, `unguarded` from the guard's dry run (`nix run .#guard`: what it
would disable and what it refuses, never guessed).

**A meter is the vendor's count, plus ours since it, on one account.** dev, prod, placement and
anything else billed on an account draw on its free units, and each database sees only its own.
The account's export holds them all, hours behind. A meter (`sa_core::meters`: a SKU, its free
units a period, whether it stops there) is named by the items drawing on it; nothing is
reconciled between databases.

```text
used = baseline   the account's export's count this period (Google's: the calendar month, Pacific
                  time), as of `through`, the export's own progress
     + delta      `usage` of the meter's items on the account after `through`, in the buy's write
a reading of a period before: baseline 0, ours from the period's start
```

Only Dynamic Street View stops, at an account's free ones; the rest are watched (Places is paid
back in tokens). `allow.maps` is held against the export's net as of `through`: our spend since is not
priced in money, so an account overshoots by its hours behind.

**A GBP's life** (`sa_core::gbp::State::nexts`; the schema checks each move again):

```text
             name (unique)    + gmail, country     + place, if none yet   walked
 ┌───────┐  ─────────────▶ ┌───────────┐ ─────────▶ ┌──────────┐ ◀────────▶ ┌───────────┐
 │ Draft │                 │ Verifying │            │ Verified │            │ Suspended │
 └───────┘                 └───────────┘ ◀───────── └──────────┘            └───────────┘
  no place;                 place optional  Google re-asks │  ▲ reinstated    place optional
  spots (the first its      (off Maps)                     └──┼──────────────────┘
  point); phone, gmail,                                       └ re-verify ◀──────┘
  country optional
                            place certain only once Verified (on Maps); with one, the
                            snapshot → archive: a track, walked only while Verified
```

- A name, a place and a phone (`sa_core::gbp::phone`'s spelling) are one live GBP's. The name is a
  draft's to change. A GBP verifying or suspended is not on Maps, so Places may not find it: its
  place is set once, on any move while it has none (with Google's address, point and country),
  and must be by the time it is verified. A GBP is created with its trade.
- **A draft's must-haves** are what Google's form asks and what stands behind it: name, category
  (a trade, a country, and the owner's category there), service areas (≤ 20), phone, a site page (sites' link), the hidden address with whether
  customers visit, the verification film, the managing gmail, its browser (Browsers: set up, its last check
  through). The film is a reference set by hand until verif_vid fills it. How many are ready
  is advice the front end computes; the move onto Google never checks it.
- **A GBP's role** is its gmail's on it (`sa_core::gbp::Role`): owner, created on or transferred to
  it, or manager, with its owner gmail an address recorded for reference only; none without a gmail,
  unknown until said, cleared by a transfer. A gmail usually owns up to 3 GBPs and manages up to 6
  before Google grows wary: its count shown, never refused.
- **A GBP's trade** is one of Google's categories (a gcid, global): the primary it is meant to
  have. `google_primary` is what Google says after a Sync, a fact of its own: a Sync gives a GBP
  with no trade Google's primary, and one whose trade differs from it is an issue, never
  overwritten. A trade is valid in a country only where Google's list has it there.
- **A GBP's category** is its owner's category of its trade in its own country, derived, never
  stored: Plombier · FR. A category is a person's settings for a trade in a country: its
  additional Google categories, its keywords (placement's `keywords`, the trade's name among
  them at weight 1), the terms every GBP of the category is compared on, in the country's
  language, and what its studies read (placement's `Category`, Placement); any other term is one
  location's own and stays out of comparisons. Not set up there is shown so, never filled from
  another country. Its settings are versions and its head a pointer (Library); it is deleted only
  while no live GBP, review draft or brand holds it, its history kept.
- **A GBP's brand** is under its gmail and its category, at most one, optional: no brand, no kit
  photos and no logo. A gmail, trade or country moved drops it in the same write; the schema
  refuses any other. Removed, never deleted, and only while no live GBP holds it. Its own scope
  facet and window, never a setting of its gmail's or category's.

  ```text
  Google's list (gcid) ── trade ──┐
  user ─┬─ category (user × trade × country) ── head ─▶ a version ─┐
        └─ gmail ──────────────────────────────────────────────────┴─▶ brand: name prefix, logo, site, photos
                                                      GBP ── 0..1 ───┘
  brand.site ─▶ sites' BrandId (a site key's brands ─▶ its origin); many brands, one site
  ```
- **Google's categories** are a closed list: a gcid (`gcid:plumber`) and a name per language. A
  GBP takes a primary one and up to 9 additional ones, entered into Google's form in order; a
  name is shown in the reader's language and copied in the country's, as the form in the GBP's
  browser (its country's locale) names it. The list is `crates/panel/google_categories.json`, one
  gcid a line: `{countries, names: {lang: name}}`, named in its countries' languages and the
  front end's; `GET /google-categories/{country}` serves it. It is drawn from pulls of Google's Business Information API:
  jepto/google-business-profile-categories `data/categories.json` (every gcid, its English name,
  its regions) and hypetrix.com's `wp-json/hypetrix/v1/gbp-categories?region=US&lang=<fr|ru>`
  (US only: the 25 GB/CA-only gcids carry their English name as `ru`). A refresh dropping a gcid a GBP, version or listing holds makes reading it fail until a migration moves
  it to what Google replaced it with.
- **Past a draft a GBP has a gmail and a country** (the schema's CHECK on live rows): the archive
  reads its language from it. One on Google is added verifying, verified or suspended: by its
  place, or without one (not verified) with its country and point by hand.
- A transfer moves it to another gmail of its owner's. Removed is apart from the state; a
  gmail removed removes its GBPs.
- **The trash.** A removed gmail or GBP is its owner's to restore (a gmail with the GBPs removed
  at its `removed_at`) until purged: `keep_days` after its removal (30 unsaid; 1, 7, 30 or 90), never with
  `never_purge` (an admin's say; unsaid, on under dev sign-in); `never_purge` turned off counts
  the kept days from then, so nothing goes the moment it may. The owner may purge one at once: a
  gmail with every GBP of it in the trash. Purged is gone for good: its gmail's connections
  deleted, its row kept.

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
- **The import opens it.** `POST /admin/imports/review-archive` (review_archive's
  `scripts/export_members.sql`'s output as the body) writes its people, gmails, GMBs (ids kept; with no country, so
  only an empty list passes), ledger and pending kick-backs in one transaction, refused unless
  the tables are empty, and creates the cursor. Until then tokens and GMBs are closed and `serve` neither follows nor pushes: a balance read
  writes a renewal, which would make the import refuse. A fresh install imports an empty
  document.
- **A place's site** is sites': its `places.gmb_id`, linked from the outline through the forward;
  sites down or refusing leaves the GMBs without the link.
- **A GBP's point** is on its row: Google's, else one the operator places on the map (a
  service-area place has none on Google, a GBP without a place none at all). Google's replaces a placed one, a
  placed one never replaces Google's. Google's arrives with the answer that named the GBP, or by
  **Sync** (`POST /gmbs/{id}/sync`): its owner's, a verified GBP's only, paid, asked afresh, it
  overwrites the name, address, phone, website, hours, primary category (`gcid:<primaryType>` the
  catalog knows), country, point and photos (`gmb_photos`, at most 10) with Places (New)'s. A GBP
  added without a key, or imported, has no point until synced. Categories as configured (the
  additional ones) and the owner's own photos are the Business Profile API's, which is not asked.
- **A GBP's log** (`gmb_events`, `GET /gmbs/{id}/log`): what changed of it, each field before and
  after (a Sync's too, one that changed nothing included), its moves, gmail, trade, brand, film,
  switch, trash, and its gmail's browser launched or stopped; written in the transaction it records,
  with the gmail it was under and who did it. A browser run for the gmail alone is the gmail's, in
  the log of each GBP while under it. Its gmail's mail with Google (`email`): `serve` reads each
  set inbox's All Mail over IMAP every 60 s past its cursor (account, UIDVALIDITY, last UID), a batch
  a write with the cursor; a message is logged once, on each GBP whose name, place id or address's
  first line it holds, else the gmail's. Its thread opens in a new tab of the gmail's own running
  browser (`POST /gmails/{id}/browser/thread`); the conductor never reads Gmail on the web. review_archive's events of the GBP (reviews appeared, gone,
  back, appeals, failed scans) are its own (`/me/gmbs/{gmb}/events`): the page merges both by time.
- **The review kit** is below.
- **A GBP's country** (FR, US, CA, GB: `sa_core::gbp::Country`, the one country → review
  language table) follows the point's rule: Google's (its address), else set by hand
  (`PUT /gmbs/{id}/country`); a Sync brings Google's over one set by hand.

## Review kit

The operator shows a GBP's QR on their phone; the reviewer scans it and lands on the GBP's page on
its landing, which offers drafts to copy into Google's form (what that form takes, per OS:
docs/review_kit.typ). Drafts are a category's, photos its GBP's brand's. A **kit** is an owner's
named selection of their GBPs (`kits`, `kit_members`): it owns no content, only which GBPs and
since when each; its area is a reference, never its membership.

```text
operator (conductor, signed in)                reviewer phone (landing, public)
/kit/<slug>/<gbp>/qr ──▶ QR ───────scan──────▶ <site key origin>/avis/<slug>?qr
   │ POST /gmbs/{id}/kit/shown                     │ the landing's route handlers, HMAC (its site key)
   ▼                                               ▼
conductor /api/v1 (session)                conductor /api/kit/v1 (in-cluster, as ingest)
  kits, drafts, photos, target, areas        offer: a view, ≤ 6 drafts leased to it
                                             copied / saved → a use
              └──────────── kit_* tables ─────────────┘
conductor ── GET /api/conductor/places (Conductor), kept 60 s ──▶ sites: GBP ↔ (brand, slug)
conductor ── GET /api/conductor/reviews (Conductor), followed ──▶ review_archive: reviews seen, gone, back
```

```text
offer, per view of GBP g:
  the drafts of g's category, not removed, in g's places (none tagged, or an area holding g's point)
    − copied on g                 − offered to another view of g whose 15-min lease runs
  order: copies ↑, then the oldest
  take 6 (one_tap: 1), lease them to this view; each 2 of g's brand's photos not saved on g,
  saves ↑, on round them (no brand: none)
  none left: the view is answered as `google`
copy tap      → a use (draft, g), once     the reviewer's tap, before Google's form opens
photos saved  → a use (photo, g), once
```

```text
a review of g first seen at s (review_archive's feed) → kit_posted, decided once:
  the oldest copy (draft d on g at t) not matched yet, t ≤ s ≤ t + 14 d,
  carrying ≥ ½ of d's words (≥ 3 letters, folded)  → on Google from d; else a review not from drafts
  after: only its gone_at follows the feed
```

- **A use is the copy**, not the post: a copy never posted still keeps the draft off g.
- **An event is stored once, tagged with the kit it came from**: a QR shown from a kit's page, a
  page opened from a kit's QR (`<page>?qr&kit=<id>`); a copy, a save and a review matched to a copy
  through their view. A kit is removed, never deleted, so its tags keep pointing at it; its slug is
  free again.
- **A tally** is per GBP, since the period the client sends (`since`, its local midnight for a
  day) or since it joined the kit, the later; a kit's is its GBPs' summed, so a GBP in two counts
  in both.
- **The target**: `google` (no draft, the page redirects), `drafts`, `one_tap`; none is the kit
  off. A GBP with no landing page has only `google`, and its QR is Google's review link itself
  (Places' `writeAReviewUri`, asked when the target is set).
- **The QR's URL** is `<origin>/avis/<slug>?qr`: the origin of the one active site key holding the
  place's sites brand (two origins: 409; the GBP's brand on another site: 409), the slug sites' page of it.
- **The landings' calls** are signed as ingest's (a GET over `GET <path?query>`); the key's brands
  must hold the place's brand. A view's photos are fetched through the landing, same-origin, so a
  download works.
- **Photos** are re-encoded on the way in (JPEG, ≤ 2048 px, upright, no EXIF): what a reviewer
  saves carries nothing of where it was taken; what one shows is its alt and caption there.
- **Areas** are geography, everyone's, and carry their country (ISO 3166-1 alpha-2, Places'): a circle
  around a centre, and the locality its centre lies in, if any, an outline to see it by. One is named
  `<city> × <radius>` while its centre is in the city's circle, else by hand; a kit made from one takes
  its name and link. A kit is at `/reviews/kit/<slug>`, its slug the owner's, never `new`.

## Library

What is global and what is a person's:

```text
global     Google's list (the trades) · category versions · listings and their versions · published studies
personal   categories: (owner, trade, country) ─ head ─▶ a version · their moves · GBPs, brands, kit drafts
```

```text
Apply ─▶ a version (immutable) ◀─ head ── category ── moves: edit | adopt | update | revert (append-only)
                 ▲                                       │ lineage: the last adopt/update since its creation
listing ── versions n=1,2,… (each releases one) ─────────┘ upstream: a newer n than the lineage took
published study (sha = its Definition's id) ── on a listing version
```

- **A version is shared content; history is personal.** Two people stand on one version by
  adopting it, so lineage lives in each one's moves, never on the version. A version body names
  nothing: gcids only, named when a study is built. An Apply equal to the head moves nothing.
- **Who reads a version:** whoever's moves went to it, or anyone once a listing version releases
  it. Anyone publishes, a version of their own moves; a listing releases only versions of its trade
  and country. Ranking the standard library is `sa:placement:library:curate`'s.
- **Released is forever.** A release is readable by everyone, for good: withdrawn hides a listing
  or study from browsing and from whoever never used it (`library_seen`: adopted, or a study of it
  loaded), and leaves it readable to whoever did.
- **Never moved silently.** A newer listing version is shown (`upstream`); updating is the
  person's move, after the diff. Adopting over an existing category is a move too; the old head
  stays in history, and revert is a move.
- **A study is published only on a released version**: the one its definition is of, or released
  by the same request (a new listing, or the next version of one of the author's), else 422
  `unreleased_category`. Loading it costs nothing for what is held; a pull is the loader's, under
  No buy on credit and A Google answer is paid for once.

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
  buttons (`{id, label, pressed, call: {operation, path: its parameters, body}, leaves}`, sites'; the
  operation `op_id` of the service's table, resolved in the catalog as `<service>.<operation>`, a write),
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
  its service through the forward as that user (a User assertion, that service's slice) with `Idempotency-Key: tg/<outbox
  id>/<button id>`, so a press repeated, or its update redelivered, records nothing twice. 201:
  the message gains "{pressed}: {name}" and keeps the buttons the press `leaves`; 200, 400, 409
  "already recorded"; 404 "not found"; 403 "no access"; anything else "try again".
- Language of the conductor's own texts: `TELEGRAM_LOCALE` (`ru` default, `en`).

## Invariants

- **Acting as another lends the actor's permissions into the subject's account, never a bound
  one; a bound permission is held only by its subject's own session.**
- **A service trusts the conductor's assertion and nothing else**, and one assertion opens one
  request: its audience, its method, its path, 60 s; a User's permissions only of that service;
  a Source's over the body its MAC covered. The conductor never forwards a cookie, nor an
  assertion it did not sign. The one it takes back is a User's it signed for review_archive,
  alive: that person's say for the Places buys that request makes (docs/spec/google.md).
- **The conductor authenticates, sites authorizes.** A signed request reaches sites only with a
  verified MAC over its timestamp and body (the 5-minute window cannot be dodged by re-stamping);
  what the key may write is never judged here.
- **Grafana holds no identity.** It signs no one in: its fetches reach a service as a viewer only
  through a token the conductor wrote into that viewer's `/grafana/api/ds/query`, on the loopback
  listener, on GETs, for that query's life. The public listener never takes a token.
- **A source is only ever revoked.** `sources` refuses DELETE and any UPDATE but of `revoked_at`
  and a site's `origin` set once from NULL (only a `site` has one); a key id is never reused.
- **Secrets never sit in the clear.** Sources' HMAC secrets, sessions' tokens, an MCP code's bearer, queued Telegram
  texts, a gmail's IMAP password, Maps key, proxy and Business Profile client secret and grant, and a person's Claude token are sealed (XChaCha20-Poly1305, what they belong
  to as associated data) under `PANEL_DATA_KEY`, whose fingerprint is stored beside each. A
  person's are never read back out: the browser sees a key's last four.
- **No price beside its use.** What a person is charged is an item of `sa_core::prices` at its
  cost times its multiplier, both the admin's; no price is written where it is spent.
- **A use is counted once, where it happens**: a Google answer bought in the write that keeps it, a
  service's through its feed, taken only from where its cursor stands.
- **No buy on credit.** A Places request on the shared keys, a Street View panorama, a map, a
  browser's start or a study's pull for a person is charged at its price, and refused before it is
  sent when their balance is below it; a pull buys no search past what the balance affords. Their
  own keys and the queries cache cost nothing. A round of the assistant on the panel's Claude starts
  only on a balance above 0, a cloud phone on one minute's; their cost, known once done, is charged
  up to what they hold, and a phone runs no longer than its launch's balance affords.
- **No Street View past the free ones.** No panorama opens on a browser key but one
  `POST /map/street_view` bought, and none is bought past its meter's free ones on that key's
  account.
- **A Google key bills only on an account whose export we read** (docs/spec/google.md).
- **A meter is counted per billing account: its vendor's count plus ours on its keys since; a
  stop never opens on an account without its reading.**
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
- **Model-written code runs only in a sandbox worker**: Starlark in a child of the conductor with an
  empty environment, no syscall but its pipes and its own memory (seccomp), its memory and time capped.
  It holds no secret and reaches nothing but the catalog's operations, as their caller, through that
  pipe while its cell lasts, and none the caller's approval level holds until the person agrees.
- **A cloud phone bills while it runs**: nothing a test runs starts one, and one that started stops
  on every way its run ends (docs/spec/browsers.md).
- **No two gmails share a proxy**: an exit two accounts share is a link Google can draw.
- **The ledger is append-only, nothing a service stores an id of is deleted, and what is
  released is released for good.** `token_ledger`, `gmb_events`, `category_versions`,
  `category_moves`, `listing_versions` and `published_studies` refuse UPDATE and DELETE (a
  published study and a listing get `withdrawn_at` once, a listing its curated rank); gmails and GBPs get `removed_at`, which a restore
  clears until `purged_at`, set once; kick-backs only `claimed_at`, once.
- **A GBP with a place is tracked in review_archive; it is walked only while Verified.** The
  snapshot carries every GBP with a place, its state and country; the archive derives the track.
- **Two files, migrated on open.** `PANEL_DB_PATH` names the panel's (`/data/panel.db` in the
  image), `PANEL_HISTORY_DB_PATH` the queries cache's history (`/data/queries_history.db`), each
  with its own migrations (`migrations/`, `history_migrations/`); every command creates them if
  missing and applies the migrations each lacks first (`panel migrate` does only that). A migration newer than the build is let be. Tables are STRICT, foreign keys
  enforced, timestamps INTEGER microseconds since the epoch, UUIDs 16-byte blobs.
- **The conductor's secrets come from the environment only** (`PANEL_DATA_KEY`, `SENTRY_DSN`,
  `RP_CLIENT_SECRET_SA`, `PANEL_ASSERTION_KEY`, `TELEGRAM_BOT_TOKEN`, `GOLOGIN_TOKEN`,
  `MULTILOGIN_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`), through
  `ev_lib::settings`, and the Google accounts' keys and readers, which the config file names
  (`{ env = "…" }`); a person's (a gmail's IMAP password, its Maps key, its proxy, its Business Profile client and grant, their Claude token) through the API, sealed at
  once; with `APP_ENV=production` the database, the data key, the four sign-in
  variables, the forward's four and `TOKENS_START_CAP_MULTIPLE` are required at boot
  (`panel --print-required-vars` lists them), and `serve` a Google account in the config file.

## Deploy requirements

- **One image, one origin.** The image carries the binary and the front end's static export,
  and sets `PANEL_WEB_DIR` to it; `serve` answers `/api`, `/auth`, `/mcp` (and its `/.well-known/oauth-*/mcp`) and `/health` first, then the
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
  `PANEL_DB_PATH`, the volume (`/data`) and the SQLite files on it to replicate, what the ingress
  must not publish or must rate-limit, and the egress; what is not `container.implement`'s is
  nix/contract.nix. `crates/server/tests/it/contract.rs` fails when its required variables,
  `ingress.rateLimitPathPrefixes`, `ingress.excludePathPrefixes`, `consumes` or `provides` and the
  binary's `--print-required-vars`, `--print-paths rate-limited|unpublished|consumed` or
  `--print-provided-paths` disagree.
- **What it provides and consumes, by contract name.** `provides.ingest` is
  `/api/ingest/v1/events`, `provides.kit` `/api/kit/v1`, `provides.queries_cache` `/api/queries_cache`; `consumes` names the peers behind `PANEL_SITES_URL` (`sites_panel`),
  `PANEL_REVIEW_ARCHIVE_URL` (`review_archive`) and `PANEL_PLAYBOOK_URL` (`playbook_web`), whose
  URLs and egress devops derives.
- **The landings, outbound on :443.** The Connected sites page asks each site key's origin; the
  origins are data, so the egress is public :443, not a host list.
- **Ingest and the kit stay inside the cluster.** Their senders reach them by service DNS;
  the IngressRoute that publishes `sa.evinvest.ltd` must not route `/api/ingest` nor `/api/kit`,
  and the NetworkPolicy lets in only the pods that send.
- **sites_panel within reach, and only through here.** The conductor calls `PANEL_SITES_URL` for
  everything it forwards and for its hubs; sites has no public route of its own (the landings
  reach its `/api/internal` in-cluster).
- **concierge within reach.** The pods call concierge's gRPC (`CONCIERGE_GRPC_ADDR`) for every
  sign-in, rotation and `GetMe`. concierge must register client `sa` with redirect URI
  `<PANEL_PUBLIC_ORIGIN>/auth/callback` exactly and hold the hash of `RP_CLIENT_SECRET_SA`.
- **The config file.** The image runs `panel --config /etc/settings/sa_conductor.json`: the
  deploy's literal `sa_conductor.json`, built from `nix/google_env.nix` (the accounts and their
  slices), each slot a sops key of the Secret the file names by its variable.
- **Places, outbound.** With an account's Places key the pods need egress to
  `places.googleapis.com:443` and `geocode.googleapis.com:443` (the keys need the Geocoding API
  on) (a pasted URL without a place id, a GMB's name, address and point).
  A person's own keys go first (Who pays, above), then the accounts' by their standing;
  one Google says is spent for a quota window sits out until the window resets, one whose billing
  is off until restart (a person's, until it answers a test). Every answer is kept in the queries
  cache, in the database. Set the prices (`/admin/tokens`) after the first deploy: until then a
  person without a key of their own is refused. Only a conductor started with
  `--spend-real-quota-i-am-certain` (the image's entrypoint) buys one: without it a miss is refused.
- **The billing exports, outbound.** `bigquery.googleapis.com:443` and `oauth2.googleapis.com:443`:
  each account's `export.table` (`project.dataset.table`, its Cloud Billing "detailed usage cost"
  export) read with its `export.reader`, a service account's JSON key (BigQuery Data Viewer on the dataset, Job
  User on its project). An export starts the day it is turned on: until the next month, that
  account's baseline misses what came before.
- **IMAP, outbound.** A gmail's inbox is dialled on :993 at a public address when it is set or
  tested, and read every 60 s: egress to public :993. The browser loads Maps JS straight
  from Google with the browser key `GET /map` names (referrer-restricted to the public origin); the
  server's keys never reach it.
- **Browsers.** sa_browsers is its own pod (`PANEL_BROWSERS_URL`, in-cluster only): `BROWSERS_DIR` on
  a volume of its own (the gmails' sessions: signed-in Google lives there; `phones.json`, the phones it
  may have started; `usage.jsonl`, the feed, a line an id) and `HOME` on it too (the Multilogin app's `~/mlx`, Mimic's profiles),
  `BROWSERS_MAX`, the conductor's public keys (`PANEL_ASSERTION_KEYS`), Orbita (`.#orbita`), ffmpeg,
  the Multilogin app under Xvfb (`.#mlx`, `BROWSERS_MLX`; its launcher `launcher.mlx.yt:45001`, a
  public name for 127.0.0.1), `BROWSERS_MULTILOGIN_API`, and a phone's tools (`.#phone-tools`: adb,
  an aarch64 busybox, ADBKeyBoard). Its TERMination grace covers stopping every phone (≥ 2 min).
  Egress to `api.gologin.com:443`, `api.multilogin.com:443` and the app's own Multilogin hosts, the
  phones' adb at whatever `ip:port` Multilogin hands out, and through the gmails' proxies: any public
  address. The conductor too dials those proxies (a set-up's check, to `ip-api.com:80` through them),
  `api.gologin.com:443` and `api.multilogin.com:443`. Multilogin's account auto-stop (Account
  settings) at 40 min is the backstop by hand.
- **The sandbox's workers, in the conductor's pod.** Up to `MAX` (4) children of the conductor's own
  binary, each capped by its data rlimit at what the binary holds plus twice `HEAP` (24 MiB): the pod's
  memory limit has `MAX` × that (≤ 200 Mi) above the conductor. A worker idles at ~5 MiB of its own (its binary's
  pages are the conductor's), ~9 MiB holding 10 000 GBP-sized dicts, ~30 MiB at `HEAP`. The sessions live in the conductor's
  memory, so a rollout ends every one, each then answered as gone. The runtime must let a process set
  no_new_privs and install a seccomp filter (`seccomp(2)` not denied by the container's own profile):
  a worker that cannot seal itself runs nothing, and `execute` says why. Nothing new in the image.
- **Telegram, outbound only.** With `TELEGRAM_BOT_TOKEN` the pods need egress to
  `api.telegram.org:443`; no webhook is set on the bot (`getUpdates` refuses to run while one is).
- **`/api/v1/live` through the public IngressRoute, as a WebSocket.** Nothing in front may buffer
  the response or strip `Upgrade` / `Connection` / `Origin`, and an idle timeout on the way must
  be longer than the 25 s ping.
- **Rate-limit `/auth`, `/api/hooks`, `/api/queries_cache` and `/mcp/{register,authorize,token}` per client IP at Traefik**; the
  conductor bounds how many run at once, not who starts them. `/api/queries_cache` is public and
  open: laptops buy through it, and this is its one guard.
- **One pod, a volume, litestream.** One writer: one replica, rolled out with `Recreate`. No
  init container: `serve` migrates on start.
