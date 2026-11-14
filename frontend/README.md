# Conductor front end

Next.js (App Router), Feature-Sliced Design, `@evinvest/uikit`. The host of Service-Arb: a top
bar over every screen, the conductor's own screens (Sources for admins, Usage, Profile, Wallet,
access requests), and the services' pages as their elements: `/sites/*` mounts sites_panel's
`<mfe-sites-panel>`, `/review_archive/*` review_archive's dashboard.

```text
[Service-Arb⌄] [Sites⌄] [Review Archive]            [75 tokens] [(V)⌄ account]
                └ <mfe-sites-nav>, mounted while closed      Wallet · Usage · Sources (admin) · Sign out
phone: [☰] [Service-Arb⌄] … [(V)⌄]   ☰ → Sheet: Sites, Review Archive, tokens
```

```text
app/                 routes: (panel)/ is everything behind sign-in, access/ is not
src/views/           one slice per screen, plus shell/ (the top bar, the access states, the window events)
src/features/        account-chip · sign-out · wallet-element · sites-element · manage-sources · request-access
src/entities/        session · source · tokens — types, response checks, requests
src/shared/          api/ (fetch, CSRF, the gate's answers), i18n/, lib/ (live/: the socket), mfe/, ui/
messages/            en.json (the source of keys) and ru.json
scripts/dev-stub.ts  a stand-in backend for local work
tests/               vitest, in Node: the rules the screens obey live in plain modules; e2e/: the Figma frames
```

## Running it

```sh
npm ci
npm run dev:stub     # on STUB_PORT — /api/v1 (me, sources, access) and /auth; STUB_ROLE=admin, STUB_ME=401|403|503,
                     # STUB_LIVE=off for no socket, STUB_LIVE_EVERY=5 for busier live activity
PANEL_DEV_BACKEND=http://127.0.0.1:$STUB_PORT npm run dev
                     # on PANEL_WEB_PORT — proxies /api, /auth and /sites/mfe to PANEL_DEV_BACKEND
```

The ports are the dev shell's (`nix develop`). Against the real backend, point `PANEL_DEV_BACKEND`
at `panel serve` with sign-in configured and `PANEL_PUBLIC_ORIGIN` this server's origin, so the
callback lands here.

The top bar's account chip, the sign-in dialog and the account element on `/account`
load from the platform origin (`evinvest.ltd`), named by its published registry. If the
registry is unreachable, the chip's fallback shows. The chip's header row opens `/account`
(`data-profile-href`); the element there shows who is signed in and links to evinvest.ltd
for what acts on the platform session. The panel's own chip entries are the caller's screens
(wallet, Usage, Sources) and the review archive's (token history, view as member); the chip's
stand-in, while the bundle loads or when it cannot, opens the same entries.

Checks: `npm run typecheck`, `npm run lint`, `npm test`, `npm run build`.
`nix run .#figma` puts the conductor's own screens into Figma, both breakpoints, over the stub as an
admin (`tests/e2e`; the loop is the `figma-render` skill).

## How it is served

`next build` is a static export (`out/`): every screen reads `/api/v1` from the
browser with the session cookie, so a Node server would have nothing to render,
and the session cookie is `HttpOnly` anyway. The Rust binary serves `out/` on the
same origin as `/api` and `/auth` — one image, one Service, one IngressRoute, as
aquafix has one. What that needs of the backend and the flake:

- `panel serve` gets a fallback: files from a directory (say `PANEL_WEB_DIR`),
  a directory answering with its `index.html` (the export writes `sources/index.html`
  and links `/sources/`), and `404.html` with status 404 for anything else. `/api`,
  `/auth` and `/health` keep their routes ahead of it; `/sites/<view>` and
  `/review_archive/<view>` without a dot get their page's `index.html`, the element routing.
- Cache headers: `/_next/static/*` is content-hashed — `public, max-age=31536000,
  immutable`; every HTML file `no-cache`.
- The flake builds `frontend/` with `buildNpmPackage` (needs `npmDepsHash`) and puts
  `out/` in the container beside the binary; the entrypoint sets the directory.
- The IngressRoute for `sa.evinvest.ltd` sends everything to the panel's Service
  except `/api/ingest` (in-cluster only, `docs/ARCHITECTURE.md`).

## Live updates

The shell opens one WebSocket per tab, same origin, `/api/v1/live` (cookie
session), and relays it on `window` (CONTRACT §9): every server message as it came
as `sa:live`, the connection's status as `sa:live-status` (answered again on
`sa:live-status-request`). It closes with 4401 (session gone: the browser goes to
sign-in) or 4403 (access lost: the "no access" screen). `shared/lib/live`
reconnects with exponential backoff and jitter; a hidden tab keeps an open socket
but makes no attempts, and reconnects at once when visible.

The conductor's screens read the same events (`useResource(…, { live })`): a change
on their topic, a `resync`, or the socket back after a drop re-reads them. The sites'
elements do the same for theirs. The elements also tell the host: `sa:title-count`
(the tab title's new leads: `(3) Leads — Service-Arb panel`), `sa:unauthenticated`
(their API's 401: the sign-in), `ev:navigate` (a link the host routes).

## Keys

Everything sits behind one leader, `Alt+S` (`⌥S` on macOS), through `@evinvest/uikit`'s
`KeymapProvider` in the shell; a feature contributes its keys with `useBinding` while it is
mounted, an element with `ev:keymap` under the scope `elementKeys` (`panel.config.ts`) gives it.
The leader opens from a field too, and the scope's next key is kept from it; besides it the page
claims only the undo keys, which leave a field its own text undo.

```text
Alt+S ─┬─ A   assistant: show / hide          (not on its full-screen page)
       ├─ N   assistant: new chat
       ├─ L   assistant: copy the chat's link (once a chat has started)
       ├─ P   map: Google's places on / off   (only on /; the hotbar shows it)
       ├─ B   map: the base layer             (only on /)
       ├─ S   sidebar: fold / unfold
       ├─ G ──┬─ H  home      S  sites       R  review archive
       │      ├─ C  sources   T  tokens      U  usage
       │      └─ W  wallet    E  alerts      Y  account
       │      (each only when the caller may open it)
       ├─ V   sites: its views, as mfe-sites-nav binds them (L leads, P places, …)
       └─ ?   keybinds: every key bound right now
Ctrl+Z           undo    the page's one history: a GMB's point, its switch, its trade
Ctrl+Shift+Z     redo    (⌘Z, ⇧⌘Z on macOS; Ctrl+Y too)
Esc closes the hint and the help.
```

## The selection

The outline holds the selection every app narrows to. Gmails (`?gmail=2,12`), a search
(`?find=`) and an issue (`?issue=`) select what they match, every GMB without them;
rows dragged between "Selected" and "Not selected" edit that, `?gmbs=` (empty: none),
until a filter changes. These are the outline's keys of the query (`shared/lib/query`),
kept from page to page; `?gmb=` (the open card) and `?at=` (`<lat>,<lng>,<zoom>`) are
`/`'s. The host tells the elements the selection as `sa:gmbs` `{ids}` (`null`: every
GMB) on each change and again on `sa:gmbs-request`; an element selects with
`sa:gmbs-select` `{ids}`, which clears the filters (CONTRACT §9).
