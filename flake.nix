{
  nixConfig = {
    extra-substituters = [ "https://valeratrades.cachix.org" ];
    extra-trusted-public-keys = [ "valeratrades.cachix.org-1:gXVwhzO5YB+BaiEJYT48qZgzdaErGQew6xtZcz4Fo1Q=" ];
  };

  inputs = {
    v_flakes.url = "github:valeratrades/v_flakes?ref=v1.6";
  };

  outputs = { self, v_flakes }:
    let
      # docs/port_range.md; this repository's ten are its first
      SA_PORT_RANGE = { from = 52800; to = 52900; };
      inherit (v_flakes) flake-utils pre-commit-hooks;
      manifest = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package;
      # The binary (crates/server, `[[bin]] name`), the image and the release all go
      # by this name; the workspace root has no package of its own.
      pname = "panel";
    in
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import v_flakes.default_nixpkgs { inherit system; };
        lib = pkgs.lib;
        rust = v_flakes.rs.default_nightly system;
        # mold is Linux-only — the adapter throws at *eval* time on Darwin, which
        # would break every target on a mac, not just the ones that link.
        stdenv = if pkgs.stdenv.hostPlatform.isDarwin then pkgs.stdenv else pkgs.stdenvAdapters.useMoldLinker pkgs.stdenv;

        # Single source for the container's exposed port and the prod bind.
        port = 59120;

        local = n: toString (SA_PORT_RANGE.from + n);
        localEnv = {
          SA_PORT_RANGE = "${toString SA_PORT_RANGE.from}-${toString SA_PORT_RANGE.to}";
          PANEL_BIND = "127.0.0.1:${local 0}";
          PANEL_WEB_PORT = local 1; # the front end's `npm run dev`
          FIGMA_STACK_PORT = local 3; # `nix run .#e2e`/`.#figma`'s own stack (`dev --port`), never a running one's
          FIGMA_WEB_PORT = local 4; # its next dev: FIGMA_STACK_PORT + 1
          BROWSERS_BIND = "127.0.0.1:${local 5}"; # sa_browsers' runner
          SANDBOX_BIND = "127.0.0.1:${local 6}"; # sa_sandbox's runner
        };

        # `nix run .#gen`: the conductor's OpenAPI document (crates/gen) and the front end's client
        # types generated from it, committed.
        gen = pkgs.writeShellApplication {
          name = "gen";
          runtimeInputs = [ rust pkgs.git pkgs.nodejs ];
          text = ''
            cd "$(git rev-parse --show-toplevel)"
            # resolved on a copy: under a [patch.crates-io] path override cargo would rewrite the committed
            # lock into the path form the Nix build cannot vendor, from inside the pre-commit hook
            lock="$(mktemp -d)"
            trap 'rm -r "$lock"' EXIT
            cp Cargo.lock "$lock/"
            cargo --config "resolver.lockfile-path=\"$lock/Cargo.lock\"" run -q -p sa_gen "$@"
            cd frontend && npm run -s gen:api
          '';
        };

        pre-commit-check = pre-commit-hooks.lib.${system}.run (
          let base = v_flakes.files.preCommit { inherit pkgs; stripClaudeSignature = true; }; in
          base // {
            hooks = base.hooks // {
              # Regenerate and re-stage rather than assert: a stale mirror type-checks against
              # itself and fails at runtime, in the browser.
              generated = {
                enable = true;
                name = "regenerate derived files";
                entry = "bash -c '${gen}/bin/gen && git add -A frontend/src/shared/api/openapi.json frontend/src/shared/api/schema.ts'";
                pass_filenames = false;
                require_serial = true;
                files = "^(crates/.*\\.(rs|toml)|Cargo\\.(toml|lock)|contracts/.*)$"; # what the document is built from: a front-end-only commit skips the build
              };
              # A conductor release ships against its peers' vendored documents: each must already be released.
              peers-released = {
                enable = true;
                name = "peers' vendored documents are released";
                entry = "bash -c 'case \"$PRE_COMMIT_REMOTE_BRANCH\" in refs/tags/v*) ${gen}/bin/gen --released;; esac'";
                pass_filenames = false;
                always_run = true;
                stages = [ "pre-push" ];
              };
              # English from the code, stale and orphaned translations dropped, the untranslated asked of ask_llm:
              # nothing in messages/ is edited but ru overrides.
              i18n = {
                enable = true;
                name = "regenerate i18n catalogues";
                entry = "bash -c 'npm --prefix frontend run --silent i18n && npm --prefix frontend run --silent i18n:translate && git add -A frontend/messages'";
                pass_filenames = false;
                require_serial = true;
                files = "^frontend/(src|messages)/"; # the code the catalogues are read from, and the catalogues
              };
            };
          }
        );
        rs = v_flakes.rs {
          inherit pkgs rust;
          build.workspace = {
            "./crates/server" = [ "git_version" "log_directives" "api_docs" ];
          };
        };
        js = v_flakes.js { inherit pkgs; packages = [ "frontend" ]; };
        # A job of its own, so the run's graph shows each one's time; the rest of the errors share one `Particles` job.
        own = p: p // {
          ci = {
            job = {
              name = p.name;
              runs-on = "ubuntu-latest";
              timeout-minutes = 45;
              steps = [{ uses = "actions/checkout@v5"; }]
                ++ lib.optionals (p ? ciCache && p.ciCache != null) (
                let
                  cacheWith = {
                    path = lib.concatStringsSep "\n" p.ciCache.paths;
                    key = "particle-${p.name}-\${{ runner.os }}-\${{ hashFiles(${lib.concatMapStringsSep ", " (f: "'${f}'") p.ciCache.hashFiles}) }}";
                    restore-keys = "particle-${p.name}-\${{ runner.os }}-";
                  };
                in
                [
                  { "if" = "github.ref_name != github.event.repository.default_branch"; uses = "actions/cache/restore@v5"; "with" = cacheWith; }
                  { "if" = "github.ref_name == github.event.repository.default_branch"; uses = "actions/cache@v5"; "with" = cacheWith; }
                ]
              );
            };
            work = [{ run = "nix run .#test -- --severity errors --serial ${p.name}"; }];
          };
        };
        github = v_flakes.github {
          inherit pkgs pname rs js;
          enable = true;
          lastSupportedVersion = "nightly-2026-09-24";
          containerRelease = { registry = "ghcr.io/ev-service-arb"; };
          figma = { };
          jobs.default = true;
          jobs.errors.install.packages = [ "ffmpeg" "deno" ]; # crates/browsers/tests/runner.rs cuts its films with ffmpeg; crates/sandbox's tests run Deno
          jobs.errors.exclude = [ "frontend-typecheck" "frontend-test" ];
          jobs.errors.augment = map own [ (js.script "frontend" "typecheck") (js.script "frontend" "test") e2e { name = "checks"; builds = checks; } ];
          jobs.warnings.exclude = [ "code-duplication" ];
          jobs.warnings.augment = [{ name = "code-duplication"; args.excludePatterns = [ "frontend/src/shared/api/schema.ts" "frontend/src/shared/api/services/**" "frontend/src/shared/api/views.ts" "crates/sandbox/js/catalog.ts" ]; }]; # generated
          lfs = false;
        };
        readme = v_flakes.readme-fw {
          inherit pkgs;
          pname = "site_conductor"; # CI keys the badge gist by the repo name, not the binary's
          defaults = true;
          lastSupportedVersion = "nightly-1.100";
          rootDir = ./.;
          badges = [ "msrv" "loc" "ci" ];
        };
        combined = v_flakes.utils.combine { inherit rust; modules = [ rs js github readme ]; };

        build_rust = v_flakes.rs.build_nightly system;
        rustPlatform = pkgs.makeRustPlatform { rustc = build_rust; cargo = build_rust; inherit stdenv; };
        # `.cargo` holds dev-only accelerators (sccache rustc-wrapper, cranelift,
        # mold) the hermetic sandbox lacks — drop it so the pure build uses nix's
        # own toolchain instead of failing on a missing `sccache` on PATH.
        pureSrc = lib.cleanSourceWith {
          src = lib.cleanSource ./.;
          filter = path: _type: baseNameOf path != ".cargo";
        };

        # placement's core is a git dependency (crates/placement_overlay)
        cargoLock = {
          lockFile = ./Cargo.lock;
          allowBuiltinFetchGit = true;
        };

        bin = rustPlatform.buildRustPackage {
          inherit pname cargoLock;
          version = manifest.version;
          src = pureSrc;
          cargoBuildFlags = [ "-p" "sa_server" ];
          nativeBuildInputs = with pkgs; [ pkg-config ];
          # ev_lib's `sentry` turns on reqwest's native-tls, which is OpenSSL on Linux
          # (Security.framework on Darwin, which needs nothing here).
          buildInputs = lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.openssl ];
          # `cargo test` runs in the devShell and CI; the tests bind loopback servers (fake
          # Telegram, PostHog and concierge), which the Darwin sandbox refuses unless asked
          doCheck = false;
          auditable = false; # cargo-auditable doesn't support edition 2024
          meta.description = "the panel binary";
        };
        # sa_sandbox's runner, built alone: its image carries nothing of the conductor's
        sandboxBin = rustPlatform.buildRustPackage {
          pname = "sa_sandbox";
          version = manifest.version;
          src = pureSrc;
          inherit cargoLock;
          cargoBuildFlags = [ "-p" "sa_sandbox" ];
          doCheck = false; # its tests run Deno against loopback stand-ins, in the devShell and CI
          auditable = false;
          meta.description = "the runner of the agents' code";
        };
        # what a run imports (crates/sandbox/js): the prelude, the catalog and its types, @evinvest/api installed
        sandboxJs = pkgs.buildNpmPackage {
          pname = "${pname}-sandbox-js";
          version = manifest.version;
          src = lib.cleanSourceWith {
            src = lib.cleanSource ./crates/sandbox/js;
            filter = path: _type: baseNameOf path != "node_modules";
          };
          npmDepsHash = "sha256-dwu/lQeOn2Lh03/7W8RMMX90E0vIstOF9dXaafdk4lQ=";
          dontNpmBuild = true;
          installPhase = ''
            runHook preInstall
            cp -r . "$out"
            runHook postInstall
          '';
        };
        sandboxPort = 59121;

        # The CLI must be the lock's `wasm-bindgen`: a skew is a hard error at bindgen time.
        wasmBindgen = (lib.findFirst (p: p.name == "wasm-bindgen") (throw "Cargo.lock holds no wasm-bindgen") (lib.importTOML ./Cargo.lock).package).version;
        wasm-bindgen-cli =
          let cli = pkgs."wasm-bindgen-cli_${lib.replaceStrings [ "." ] [ "_" ] wasmBindgen}" or (throw "nixpkgs has no wasm-bindgen-cli ${wasmBindgen}, the lock's wasm-bindgen"); in
          assert lib.assertMsg (cli.version == wasmBindgen) "wasm-bindgen-cli ${cli.version} is not the lock's wasm-bindgen ${wasmBindgen}";
          cli;
        # crates/placement_overlay as the front end imports it: wasm-bindgen's module, its .d.ts, the wasm
        placementOverlay = rustPlatform.buildRustPackage {
          pname = "placement_overlay";
          version = manifest.version;
          inherit cargoLock;
          src = pureSrc;
          nativeBuildInputs = [ wasm-bindgen-cli pkgs.binaryen ];
          buildPhase = ''
            runHook preBuild
            cargo build --release -p placement_overlay --target wasm32-unknown-unknown
            wasm-bindgen --target web --out-dir "$out" --out-name placement_overlay target/wasm32-unknown-unknown/release/placement_overlay.wasm
            wasm-opt -Oz "$out/placement_overlay_bg.wasm" -o "$out/placement_overlay_bg.wasm"
            runHook postBuild
          '';
          installPhase = "true";
          doCheck = false;
          auditable = false; # cargo-auditable doesn't support edition 2024
          meta.description = "the home map's placement overlay";
        };
        # The same into the checkout, unoptimised: next dev imports it (local-stack --dev).
        placementOverlayDev = pkgs.writeShellApplication {
          name = "placement-overlay";
          runtimeInputs = [ rust wasm-bindgen-cli pkgs.git ];
          text = ''
            cd "$(git rev-parse --show-toplevel)"
            cargo build -q -p placement_overlay --target wasm32-unknown-unknown
            wasm-bindgen --target web --out-dir frontend/src/shared/placement_overlay --out-name placement_overlay target/wasm32-unknown-unknown/debug/placement_overlay.wasm
          '';
        };

        # The front end's static export (`next build` → out/), served by `panel serve` from
        # PANEL_WEB_DIR on the same origin as /api and /auth.
        frontend = pkgs.buildNpmPackage {
          pname = "${pname}-frontend";
          version = manifest.version;
          src = lib.cleanSourceWith {
            src = lib.cleanSource ./frontend;
            filter = path: _type: !(builtins.elem (baseNameOf path) [ "node_modules" ".next" "out" ]);
          };
          npmDepsHash = "sha256-rny9u1FSldhSYbkRjMijxRaOJtnE76Ql5aCg+apy/d8=";
          env = {
            NEXT_TELEMETRY_DISABLED = "1";
          };
          # Turbopack talks to its node workers over a loopback socket, which the Darwin
          # sandbox refuses unless asked.
          __darwinAllowLocalNetworking = true;
          # Turbopack's build stalls in the sandbox after "Compiled successfully" (its
          # PostCSS worker never hands back); webpack makes the same export.
          npmBuildFlags = [ "--" "--webpack" ];
          # next writes its cache under HOME, which the sandbox does not have
          # the image is the release: every key translated, as committed
          preBuild = ''
            export HOME="$TMPDIR"
            cp -r --no-preserve=mode ${placementOverlay} src/shared/placement_overlay
            npm run i18n:release
          '';
          installPhase = ''
            runHook preInstall
            cp -r out "$out"
            runHook postInstall
          '';
          meta.description = "the front end's static export";
        };

        # Grafana (docs/ARCHITECTURE.md, Grafana): the tilers plugin lays out the services' boards
        tilersPlugin = pkgs.fetchzip {
          url = "https://github.com/EV-Main/tilers/releases/download/grafana-v0.6.0/evmain-tilers-app-0.6.0.zip";
          hash = "sha256-NDxPRBj29ZmAb5ROp9SxPCSvrG0C+CVDWYPljcBeW4k=";
        };

        # Infinity's package for this platform alone: the catalog's default zip carries every platform's backend
        infinityPlugin = pkgs.grafanaPlugins.grafanaPlugin {
          pname = "yesoreyeram-infinity-datasource";
          version = "4.1.0";
          zipHash = {
            x86_64-linux = "sha256-pzgHZ2abzFzyKvBIQOTzlirus+AUTE0/VfP2zYnL06I=";
            aarch64-linux = "sha256-1fZ7v7mkleROOessTSbLB6wQH2HLMbhJdVGiqaaC/pg=";
          };
        };

        # Grafana without what no browser here loads: its source maps and API explorer
        grafanaPruned = pkgs.runCommand "grafana-pruned" { } ''
          cp -r --no-preserve=mode ${pkgs.grafana} $out
          chmod +x $out/bin/grafana
          find $out/share/grafana/public -name '*.map' -delete
          rm -r $out/share/grafana/public/build-swagger
        '';

        # PANEL_GRAFANA_SERVER: `serve` gives it its socket, data dir, provisioning, root URL and sign-in
        # settings; the rest is fixed here, cut to what the dashboards use (docs/ARCHITECTURE.md, Deploy
        # requirements). Warnings and up on serve's stderr.
        grafanaServer = pkgs.writeShellScript "grafana-server" ''
          export GF_PATHS_LOGS="$GF_PATHS_DATA/log"
          export GF_PATHS_PLUGINS="${pkgs.linkFarm "grafana-plugins" [
            { name = "yesoreyeram-infinity-datasource"; path = infinityPlugin; }
            { name = "marcusolsson-dynamictext-panel"; path = pkgs.grafanaPlugins.marcusolsson-dynamictext-panel; }
            { name = "evmain-tilers-app"; path = tilersPlugin; }
          ]}"
          export GF_PLUGINS_ALLOW_LOADING_UNSIGNED_PLUGINS=evmain-tilers-app
          export GF_SECURITY_ALLOW_EMBEDDING=true
          export GF_PLUGINS_PREINSTALL_DISABLED=true # its plugin dir is the store's
          export GF_LIVE_MAX_CONNECTIONS=0
          export GF_ANALYTICS_REPORTING_ENABLED=false GF_ANALYTICS_CHECK_FOR_UPDATES=false GF_ANALYTICS_CHECK_FOR_PLUGIN_UPDATES=false
          export GF_UNIFIED_ALERTING_ENABLED=false GF_NEWS_NEWS_FEED_ENABLED=false GF_PUBLIC_DASHBOARDS_ENABLED=false GF_SNAPSHOTS_ENABLED=false
          export GF_QUERY_HISTORY_ENABLED=false GF_EXPLORE_ENABLED=false GF_CLOUD_MIGRATION_ENABLED=false GF_RECORDED_QUERIES_ENABLED=false
          export GF_PROFILE_ENABLED=false GF_HELP_ENABLED=false
          # its own, not the pod's; Infinity inherits both.
          # The heap is kept by GOGC: a limit near the live heap (~70 MiB idle, ~105 after a while of loads) spins the GC
          export GOGC=50 GOMEMLIMIT=200MiB
          export GF_LOG_MODE=console GF_LOG_LEVEL=warn
          exec ${grafanaPruned}/bin/grafana server --homepath ${grafanaPruned}/share/grafana
        '';

        # The database: one SQLite file on the pod's volume, created and migrated by `serve` on
        # start (no init container, no migrate step), replicated off the pod by the cluster's
        # litestream — which is what `sqlite` below asks for. One writer: one pod, replaced
        # (Recreate), never two at once on the volume.
        dbPath = "/data/panel.db";
        # the config file, in the image the deploy's literal `sa_conductor.json` mounted (docs/ARCHITECTURE.md, Deploy requirements)
        configPath = "/etc/settings/sa_conductor.json";

        # sa_browsers' browser: Chrome 151 is what the measured devices' UA says (crates/browsers)
        orbita = import ./nix/orbita.nix { inherit pkgs; } "151";
        multilogin = import ./nix/multilogin.nix { inherit pkgs; };

        # What the deploy must provide, beyond what `container.implement` records. Plain
        # data on the contract, read by the devops generator; `checks.contract-env` holds
        # `requiredEnv` to the binary's own `--print-required-vars`.
        deploy = {
          # Settings::required_var_names("production"): the pod exits 78 without any of them.
          # PANEL_DB_PATH is the container's own env (below); the rest the deploy supplies.
          requiredEnv = [ "PANEL_DB_PATH" "PANEL_DATA_KEY" "PANEL_PUBLIC_ORIGIN" "CONCIERGE_PUBLIC_ORIGIN" "CONCIERGE_GRPC_ADDR" "RP_CLIENT_SECRET_SA" "PANEL_ASSERTION_KEY" "PANEL_REVIEW_ARCHIVE_URL" "PANEL_PLAYBOOK_URL" "PANEL_SITES_URL" "TOKENS_START_CAP_MULTIPLE" ];
          # The Google accounts (docs/ARCHITECTURE.md, Google accounts): a literal mounted here, each
          # account's secrets named in it as `{ env = "…"; }`, from the sops-backed Secret.
          configFile = configPath;
          # from the sops-backed Secret, never literal env
          secretEnv = [
            "PANEL_DATA_KEY"
            "SENTRY_DSN"
            "RP_CLIENT_SECRET_SA"
            "PANEL_ASSERTION_KEY"
            "TELEGRAM_BOT_TOKEN"
            "GOLOGIN_TOKEN"
            "MULTILOGIN_TOKEN"
            "CLAUDE_CODE_OAUTH_TOKEN"
          ];
          # TELEGRAM_BOT_TOKEN needs the forward (it sends sites' notes).
          optionalEnv = [
            "SENTRY_DSN"
            "TELEGRAM_BOT_TOKEN"
            "TELEGRAM_BOT_USERNAME"
            "TELEGRAM_LOCALE"
            # 15 and 500 by default
            "TOKENS_DAILY"
            "TOKENS_CAP"
            # the gmails' browsers: the runner (sa_browsers, below) with a provider's token or both; unset,
            # their routes answer 503, and a provider without its token too
            "GOLOGIN_TOKEN"
            "MULTILOGIN_TOKEN"
            "PANEL_BROWSERS_URL"
            # the agents' `execute`: sa_sandbox's runner (below); unset, there is none
            "PANEL_SANDBOX_URL"
            # the in-page assistant's model (a Claude subscription); unset, asking it answers 503
            "CLAUDE_CODE_OAUTH_TOKEN"
          ];
          ingress = {
            # in-cluster only, by service DNS: the landings' and bots' ingest, the landings' review kit, the sandbox's runs
            # (docs/ARCHITECTURE.md, Deploy requirements)
            excludePathPrefixes = [ "/api/ingest" "/api/kit" "/api/run" ];
            # per client IP at the edge; the conductor bounds concurrency, not who calls. /api/hooks/booking:
            # the booking providers' webhooks, forwarded to sites (public, signature-checked there).
            # /api/queries_cache: open to everyone, so this is its only guard. /mcp/register: open too.
            # `checks.paths` holds both lists to the binary's path table (forward::ROUTES and OWN).
            rateLimitPathPrefixes = [ "/auth" "/api/hooks/booking" "/api/queries_cache" "/mcp/register" "/mcp/authorize" "/mcp/token" ];
          };
          egress = {
            # concierge's gRPC, at the address it names
            grpcEnv = [ "CONCIERGE_GRPC_ADDR" ];
            # the Bot API; the Places API (the accounts' Places keys); GoLogin's (GOLOGIN_TOKEN) and
            # Multilogin's (MULTILOGIN_TOKEN) APIs; the Claude API (CLAUDE_CODE_OAUTH_TOKEN)
            # the meters' baseline: each account's billing export, its service account's token
            hosts = [ "api.telegram.org:443" "places.googleapis.com:443" "geocode.googleapis.com:443" "api.gologin.com:443" "api.multilogin.com:443" "bigquery.googleapis.com:443" "oauth2.googleapis.com:443" "api.anthropic.com:443" ];
          };
        };

        # What the conductor serves to its peers, by endpoint: devops builds a consumer's URL from it.
        # `checks.provided-paths` holds it to the routes the binary mounts.
        provides = {
          ingest = "/api/ingest/v1/events";
          # the landings' review kit pages (docs/ARCHITECTURE.md, Review kit)
          kit = "/api/kit/v1";
          # the queries cache (docs/ARCHITECTURE.md, Queries cache): review_archive's and placement's
          queries_cache = "/api/queries_cache";
          # a sandbox's run, back to its caller's operations (docs/ARCHITECTURE.md, Code mode)
          run = "/api/run";
        };
        # The services the forward reaches (docs/ARCHITECTURE.md, Forward), by contract name: devops
        # sets each variable to the peer's in-cluster origin and opens the egress to it.
        consumes = {
          PANEL_SITES_URL = "sites_panel";
          PANEL_REVIEW_ARCHIVE_URL = "review_archive";
          PANEL_PLAYBOOK_URL = "playbook_web";
          PANEL_PLACEMENT_URL = "gmaps_optimal_placement";
          PANEL_SANDBOX_URL = "${pname}-sandbox";
        };

        containerStd = v_flakes.container.implement {
          inherit pkgs pname;
          containers."" = {
            inherit port;
            mounts = [ "/data" ];
            sqlite = [ dbPath ];
            # Grafana's own sqlite: its state is provisioned again on every start
            unreplicated = [ "/data/grafana" ];
            healthPath = "/health";
            # a source that gets a 5xx retries from its outbox; nothing is lost while down
            criticality = "normal";
            entrypoint = [ "${bin}/bin/${pname}" "--config" configPath "--spend-real-quota-i-am-certain" "serve" "--bind" "0.0.0.0:${toString port}" ];
            # /bin/panel, for `kubectl exec … panel migrate` and the like
            contents = [ bin ];
            # the production guards (`#[required_in("production")]`) are armed wherever it runs
            env = {
              APP_ENV = "production";
              PANEL_DB_PATH = dbPath;
              PANEL_GRAFANA_DIR = "/data/grafana";
            };
            # the epoch versions the sa catalog `serve` publishes to concierge: it must travel with
            # the build, or a tag whose catalog changed boots on the previous epoch and is refused
            imageEnv = [ "PANEL_WEB_DIR=${frontend}" "PANEL_GRAFANA_SERVER=${grafanaServer}" "PANEL_BUILD_EPOCH=${toString self.lastModified}" ];
          };
          # the agents' code (docs/ARCHITECTURE.md, Code mode): stateless, reached by the conductor alone
          containers.sandbox = {
            port = sandboxPort;
            healthPath = "/health";
            criticality = "normal";
            withCacert = false; # it dials the conductor alone, in plain http
            entrypoint = [ "${sandboxBin}/bin/sa_sandbox" "serve" "--bind" "0.0.0.0:${toString sandboxPort}" ];
            # each run's type check holds ~300 MB: the pod's memory is this many of them
            env.SANDBOX_MAX = "2";
            imageEnv = [ "SANDBOX_DENO=${pkgs.deno}/bin/deno" "SANDBOX_JS=${sandboxJs}" ];
          };
        };
        # The sandbox's deploy: the conductor's public keys (its assertion is all it trusts), and the conductor's
        # `/api/run` its only peer; no host off-cluster.
        sandboxDeploy = {
          requiredEnv = [ "PANEL_ASSERTION_KEYS" "SANDBOX_CONDUCTOR_URL" ];
          egress.hosts = [ ];
          consumes.SANDBOX_CONDUCTOR_URL = "${pname}.run";
        };
        # `implement` knows no key for these, and throws on one it does not know.
        extra = {
          ${pname} = deploy // { inherit provides consumes; };
          "${pname}-sandbox" = sandboxDeploy;
        };
        containers = lib.mapAttrs (name: c: c // { contract = c.contract // extra.${name}; }) containerStd.containers;

        contractEnv = pkgs.runCommand "${pname}-contract-env" { } ''
          ${bin}/bin/${pname} --print-required-vars production | sort > got
          printf '%s\n' ${lib.escapeShellArgs deploy.requiredEnv} | sort > want
          diff -u want got || { echo "flake.nix deploy.requiredEnv disagrees with the binary" >&2; exit 1; }
          touch "$out"
        '';

        providedPaths = pkgs.runCommand "${pname}-provided-paths" { } ''
          ${bin}/bin/${pname} --print-provided-paths | sort > got
          printf '%s\n' ${lib.escapeShellArgs (lib.attrValues provides)} | sort > want
          diff -u want got || { echo "flake.nix provides disagrees with the routes the binary mounts" >&2; exit 1; }
          touch "$out"
        '';

        paths = pkgs.runCommand "${pname}-paths" { } ''
          check() {
            ${bin}/bin/${pname} --print-paths "$1" | sort > got
            printf '%s\n' $2 | sort > want
            diff -u want got || { echo "flake.nix $3 disagrees with the binary's --print-paths $1" >&2; exit 1; }
          }
          check rate-limited ${lib.escapeShellArg (lib.concatStringsSep " " deploy.ingress.rateLimitPathPrefixes)} ingress.rateLimitPathPrefixes
          check unpublished ${lib.escapeShellArg (lib.concatStringsSep " " deploy.ingress.excludePathPrefixes)} ingress.excludePathPrefixes
          check consumed ${lib.escapeShellArg (lib.concatStringsSep " " (lib.attrNames consumes))} consumes
          touch "$out"
        '';

        # The front end's own screens into Figma (frontend/tests/e2e), by `@evinvest/figma-render`'s bin;
        # the loop is the `figma-render` skill.
        figmaFile = "G9JfrDvGd0BKUdOKYg61Nn"; # "panel — renders"
        figma = (js.playwright {
          name = "figma";
          package = "frontend";
          config = "tests/e2e";
          env = localEnv;
          runtimeInputs = [ dev ];
          run = ''exec node_modules/.bin/evinvest-figma-render "$@" -c tests/e2e --file ${figmaFile}'';
        }).program;
        # The front end's behaviour over the same stack: every spec in frontend/tests/e2e but the renders.
        e2e = js.playwright {
          name = "e2e";
          package = "frontend";
          config = "tests/e2e";
          env = localEnv;
          runtimeInputs = [ dev ];
          run = ''exec playwright test -c tests/e2e --grep-invert @figma "$@"'';
        };
        checks = {
          contract-env = contractEnv;
          provided-paths = providedPaths;
          inherit paths;
        };
        # In the devShell, not an app: built in another environment, every crate's fingerprint is stale.
        testTimeline = pkgs.writeShellApplication {
          name = "test-timeline";
          runtimeInputs = with pkgs; [ coreutils gawk gnugrep gnused git jq cargo-nextest ];
          text = builtins.readFile ./scripts/test.sh;
        };

        # The conductor (dev sign-in), sites_panel and the landings beside its checkout, wired
        # together on this machine; the script is the source, shellchecked here (docs/LOCAL.md). It builds the
        # binary and the front end itself, unless PANEL_BIN / PANEL_WEB_DIR name others: the
        # app does not depend on them, so naming a cargo build skips the Nix one.
        localStack = pkgs.writeShellApplication {
          name = "local-stack";
          runtimeInputs = with pkgs; [ coreutils curl gawk jq git gnused iproute2 procps util-linux nodejs_22 watchexec litestream sops yq-go openssh gnutar ];
          text = lib.concatStrings (lib.mapAttrsToList (k: v: "export ${k}=${v}\n") localEnv) + builtins.readFile ./scripts/local-stack.sh;
        };

        # `nix run .#dev`'s peers, from the checkouts _.siblings-local.json names; a user's entry may name a `command` (docs/LOCAL.md)
        devPeers = [ "sites_panel" "review_archive" "gmaps_optimal_placement" ];
        devUsers = { };
        devPrelude = lib.concatStrings (lib.mapAttrsToList
          (user: peers:
            "if [ \"$(id -un)\" = ${lib.escapeShellArg user} ]; then\n"
            + lib.concatStrings (lib.mapAttrsToList
              (peer: o:
                assert lib.assertMsg (builtins.elem peer devPeers) "devUsers.${user}.${peer}: not one of devPeers";
                assert lib.assertMsg (lib.subtractLists [ "command" ] (lib.attrNames o) == [ ]) "devUsers.${user}.${peer}: only command";
                lib.concatStrings (lib.mapAttrsToList (k: v: "export DEV_${lib.toUpper k}_${peer}=${lib.escapeShellArg v}\n") o))
              peers)
            + "fi\n")
          devUsers);
        dev = pkgs.writeShellApplication {
          name = "dev";
          runtimeInputs = [ pkgs.coreutils ];
          text = devPrelude + ''exec ${lib.getExe localStack} --dev "$@"'';
        };
      in
      {
        apps = {
          help = v_flakes.utils.help {
            inherit pkgs self system;
            about = "  cargo test                      every test; the database ones on throwaway SQLite files\n"
              + "  panel --help                    the CLI itself";
          };
          publish = v_flakes.utils.publish pkgs;
          gen = { type = "app"; program = lib.getExe gen; meta.description = "the OpenAPI document and the front end's client types from it (pre-commit runs it too)"; };
          dev = { type = "app"; program = lib.getExe dev; meta.description = "the conductor and its peers under watch: a conductor already serving is replaced, a peer used (docs/LOCAL.md)"; };
          e2e = { type = "app"; program = lib.getExe e2e.program; meta.description = "the front end's behaviour over a stack of its own (frontend/tests/e2e, the renders aside); args go to `playwright test`"; };
          figma = { type = "app"; program = lib.getExe figma; meta.description = "the front end's screens into Figma over a stack of its own, gridded on a new page of figmaFile; `-- frames` lists them"; };
          local-stack = { type = "app"; program = lib.getExe localStack; meta.description = "the conductor, its Grafana, sites_panel, review_archive and the landings, wired together on this machine (docs/LOCAL.md)"; };
        };

        packages = {
          default = bin;
          inherit bin frontend placementOverlay;
        } // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (containerStd.packages // { grafana-server = grafanaServer; inherit orbita; mlx = multilogin.mlx; phone-tools = multilogin.phoneTools; });

        containers = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux containers;

        inherit checks;

        devShells.default =
          with pkgs;
          mkShell {
            inherit stdenv;
            shellHook =
              pre-commit-check.shellHook
              + combined.shellHook
              + ''
                cp -f ${(v_flakes.files.treefmt) { inherit pkgs; }} ./.treefmt.toml
                cp -f ${(v_flakes.files.gitattributes) { inherit pkgs; lfs = false; }} ./.gitattributes
              '';

            packages = [
              mold
              pkg-config
              openssl # sentry's native-tls
              sqlite # inspecting the database
              deno # sa_sandbox's runs
              rust
              testTimeline
              placementOverlayDev
            ] ++ pre-commit-check.enabledPackages ++ combined.enabledPackages;

            env = localEnv // {
              RUST_BACKTRACE = 1;
              RUST_LIB_BACKTRACE = 0;
            };
          };
      }
    );
}
