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
          FIGMA_STACK_PORT = local 9; # `nix run .#figma`'s (`dev --port --fixed`), the next one its own too
          E2E_STACK_PORT = local 7; # `nix run .#e2e`'s (`dev --port`), its next dev on the next
          BROWSERS_BIND = "127.0.0.1:${local 5}"; # sa_browsers' runner
          SANDBOX_BIND = "127.0.0.1:${local 6}"; # sa_sandbox's runner
          PROD_SSH = prodSsh;
          PROD_RESTORE = lib.getExe prodRestore;
          QUERIES_CACHE_SYNC = lib.getExe queriesCacheSync;
        };

        # prod's host, as ~/.ssh/config names it
        prodSsh = "rpi5-ts";
        # prod's SQLite files from their litestream replicas; local-stack's --from-prod, and whoever needs a copy
        prodRestore = pkgs.writeShellApplication {
          name = "prod-restore";
          runtimeInputs = with pkgs; [ coreutils git litestream sops yq-go ];
          text = builtins.readFile ./scripts/prod-restore.sh;
        };
        # a state's answers into prod's queries cache; PANEL_BIN the caller's, so local-stack builds no conductor of Nix's for it
        queriesCacheSync = pkgs.writeShellApplication {
          name = "queries-cache-sync";
          runtimeInputs = with pkgs; [ coreutils openssh ];
          text = "export PROD_SSH=${prodSsh}\n" + builtins.readFile ./scripts/queries-cache-sync.sh;
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
              # `test = false` keeps a crate's harnesses from building; a test written there would never run
              untested = {
                enable = true;
                name = "no tests in a crate built with test = false";
                entry = toString (pkgs.writeShellScript "untested" ''
                  for m in crates/*/Cargo.toml; do
                    grep -q '^test = false' "$m" || continue
                    if grep -rnE '#\[(tokio::)?test' "$(dirname "$m")/src"; then echo "$m: test = false, so the above never run" >&2; exit 1; fi
                  done
                '');
                pass_filenames = false;
                files = "^crates/.*\\.(rs|toml)$";
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
          pinDeps = true;
          build.workspace = {
            "./crates/server" = [ "git_version" "log_directives" "api_docs" ];
          };
        };
        js = v_flakes.js { inherit pkgs; packages = [ "frontend" ]; };
        github = v_flakes.github {
          inherit pkgs pname rs js;
          enable = true;
          lastSupportedVersion = "nightly-2026-09-24";
          containerRelease = { registry = "ghcr.io/ev-service-arb"; };
          figma = { on = "publish"; }; # the render needs .local's data, the peers and a browser key, which only a developer's machine has
          jobs.default = true;
          jobs.errors.install.packages = [ "ffmpeg" "deno" ]; # crates/browsers/tests/runner.rs cuts its films with ffmpeg; crates/sandbox's tests run Deno
          jobs.errors.exclude = [ "frontend-typecheck" ];
          jobs.errors.augment = [ (e2e // { ciCache = [ e2e.ciCache rs.ciCache ]; }) frontendTypecheck ]; # e2e: local-stack's conductor build
          jobs.warnings.exclude = [ "code-duplication" ];
          jobs.warnings.augment = [{ name = "code-duplication"; args.excludePatterns = [ "frontend/src/shared/api/schema.ts" "frontend/src/shared/api/services/**" "frontend/src/shared/api/views.ts" "crates/sandbox/js/catalog.ts" "crates/server/src/connections.rs" ]; }]; # generated; connections.rs: its route table, alike by nature to kit.rs'
          lfs = false;
          envrc.extra = "watch_file scripts/local-stack.sh"; # baked into the shell's `figma`
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

        # placement's crates are a git dependency (crates/server, crates/placement_overlay)
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
        # what a session runs (crates/sandbox/js): the host, the prelude, the catalog and its types, @evinvest/api and typescript installed
        sandboxJs = pkgs.buildNpmPackage {
          pname = "${pname}-sandbox-js";
          version = manifest.version;
          src = lib.cleanSourceWith {
            src = lib.cleanSource ./crates/sandbox/js;
            filter = path: _type: baseNameOf path != "node_modules";
          };
          npmDepsHash = "sha256-5AWfHRXALyxr0HUUZhdRwSGd8g9/URACBbnq1DUIvO8=";
          dontNpmBuild = true;
          # the runner checks a cell alone, never the js it imports
          doCheck = true;
          nativeCheckInputs = [ pkgs.deno ];
          checkPhase = ''
            runHook preCheck
            DENO_DIR="$TMPDIR/deno" deno check --no-remote --cached-only --config deno.json host.ts
            runHook postCheck
          '';
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
          runtimeInputs = [ wasm-bindgen-cli pkgs.gitMinimal pkgs.jq ]; # the shell's cargo
          text = ''
            cd "$(git rev-parse --show-toplevel)"
            cargo build -q -p placement_overlay --target wasm32-unknown-unknown
            target="$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)"
            wasm-bindgen --target web --out-dir frontend/src/shared/placement_overlay --out-name placement_overlay "$target/wasm32-unknown-unknown/debug/placement_overlay.wasm"
          '';
        };

        # The front end's typecheck, its overlay's module built first where the checkout has none (gitignored)
        frontendTypecheck =
          let p = js.script "frontend" "typecheck"; in
          p // {
            prepare = pkgs.writeShellApplication {
              name = "frontend-typecheck-install";
              runtimeInputs = [ rust pkgs.stdenv.cc pkgs.mold pkgs.gitMinimal ]; # placementOverlayDev's cargo
              text = ''
                ${lib.getExe p.prepare}
                cd "$(git rev-parse --show-toplevel)"
                [ -d frontend/src/shared/placement_overlay ] || ${lib.getExe placementOverlayDev}
              '';
            };
            ciCache = [ p.ciCache rs.ciCache ];
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
          npmDepsHash = "sha256-KAGMYGVbF42nZabqpVnXz+mCnprkMlPS8TgC9yFnT8E=";
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
        # the queries cache's every version of an answer but the latest: its own file, replicated beside it
        historyDbPath = "/data/queries_history.db";
        # the config file, in the image the deploy's literal `sa_conductor.json` mounted (docs/ARCHITECTURE.md, Deploy requirements)
        configPath = "/etc/settings/sa_conductor.json";

        # sa_browsers' browser: Chrome 151 is what the measured devices' UA says (crates/browsers)
        orbita = import ./nix/orbita.nix { inherit pkgs; } "151";
        multilogin = import ./nix/multilogin.nix { inherit pkgs; };

        contract = import ./nix/contract.nix { inherit pname; };
        inherit (contract) provides consumes sandboxDeploy;
        deploy = contract.deploy // {
          # The Google accounts (docs/ARCHITECTURE.md, Google accounts): a literal mounted here, each
          # account's secrets named in it as `{ env = "…"; }`, from the sops-backed Secret.
          configFile = configPath;
        };
        containerStd = v_flakes.container.implement {
          inherit pkgs pname;
          containers."" = {
            inherit port;
            mounts = [ "/data" ];
            sqlite = [ dbPath historyDbPath ];
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
              PANEL_HISTORY_DB_PATH = historyDbPath;
              PANEL_GRAFANA_DIR = "/data/grafana";
            };
            # the epoch versions the sa catalog `serve` publishes to concierge: it must travel with
            # the build, or a tag whose catalog changed boots on the previous epoch and is refused
            imageEnv = [ "PANEL_WEB_DIR=${frontend}" "PANEL_GRAFANA_SERVER=${grafanaServer}" "PANEL_BUILD_EPOCH=${toString self.lastModified}" ];
          };
          # the agents' code (docs/ARCHITECTURE.md, Code mode), reached by the conductor alone. Its sessions are held in
          # memory, and the conductor names them by the runner's id: one replica (v_flakes' Deployment), and a rollout
          # ends every session, each answered as gone
          containers.sandbox = {
            port = sandboxPort;
            healthPath = "/health";
            criticality = "normal";
            withCacert = false; # it dials the conductor alone, in plain http
            entrypoint = [ "${sandboxBin}/bin/sa_sandbox" "serve" "--bind" "0.0.0.0:${toString sandboxPort}" ];
            # sessions held at once, each a Deno process (its heap capped at 256 MB), beside one type checker for them
            # all (~0.7 GB): the pod's memory is this many sessions and the checker; past it the idlest session is let go
            env.SANDBOX_MAX = "2";
            imageEnv = [ "SANDBOX_DENO=${pkgs.deno}/bin/deno" "SANDBOX_JS=${sandboxJs}" ];
          };
        };
        # `implement` knows no key for these, and throws on one it does not know.
        extra = {
          ${pname} = deploy // { inherit provides consumes; };
          "${pname}-sandbox" = sandboxDeploy;
        };
        containers = lib.mapAttrs (name: c: c // { contract = c.contract // extra.${name}; }) containerStd.containers;

        # The front end's own screens into Figma (frontend/tests/e2e), by `@evinvest/figma-render`'s bin;
        # the loop is the `figma-render` skill.
        figmaFile = "8Z2zYr2Mwq4qmnjgp19kuS"; # "panel — renders", in the Pro team "EV Invest"
        # a stack outliving its run (tests/e2e/stack.ts), by its state dir: `--stop` stops it, `--restart` starts the run on a new one
        stackRun = { state }: run: ''
          stop() {
            local pid
            pid="$(cat "${state}/pid" 2>/dev/null)" || return 0
            ps -o args= -p "$pid" | grep -q local-stack || return 0
            kill -TERM "$pid"
            for _ in $(seq 60); do kill -0 "$pid" 2>/dev/null || return 0; sleep 0.5; done
            echo "local-stack $pid did not stop in 30s" >&2
            exit 1
          }
          case "''${1:-}" in
          --stop) stop; exit 0 ;;
          --restart) stop; shift ;;
          esac
          ${run}
        '';
        figma = (js.playwright {
          name = "figma";
          package = "frontend";
          config = "tests/e2e";
          env = localEnv;
          runtimeInputs = [ dev pkgs.procps pkgs.gnugrep ];
          run = stackRun { state = "../.local-port-$FIGMA_STACK_PORT"; } ''exec node_modules/.bin/evinvest-figma-render -c tests/e2e/figma.config.ts --file ${figmaFile} "$@"'';
        }).program;
        # The front end's behaviour over the same stack: every spec in frontend/tests/e2e but the renders.
        e2e = js.playwright {
          name = "e2e";
          package = "frontend";
          config = "tests/e2e";
          env = localEnv;
          runtimeInputs = [ dev pkgs.procps pkgs.gnugrep ];
          run = stackRun { state = "../.local-port-$E2E_STACK_PORT"; } ''exec playwright test -c tests/e2e "$@"'';
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
          runtimeInputs = with pkgs; [ coreutils curl gawk jq git gnused iproute2 procps util-linux nodejs_22 watchexec sops openssh gnutar sqlite ];
          text = lib.concatStrings (lib.mapAttrsToList (k: v: "export ${k}=${v}\n") localEnv) + builtins.readFile ./scripts/local-stack.sh;
        };

        # `nix run .#dev`'s peers, from the checkouts _.siblings-local.json names; a user's entry may name a `command` (docs/LOCAL.md)
        devPeers = [ "sites_panel" "review_archive" ];
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
        # what local-stack builds and runs with, in either dev shell
        stackPackages = with pkgs; [
          mold
          pkg-config
          openssl # sentry's native-tls
          deno # sa_sandbox's runs
          placementOverlayDev
        ];
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
          publish = github.publish;
          gen = { type = "app"; program = lib.getExe gen; meta.description = "the OpenAPI document and the front end's client types from it (pre-commit runs it too)"; };
          dev = { type = "app"; program = lib.getExe dev; meta.description = "the conductor and its peers under watch: a conductor already serving is replaced, a peer used (docs/LOCAL.md)"; };
          e2e = { type = "app"; program = lib.getExe e2e.program; meta.description = "the front end's behaviour over a stack of its own (frontend/tests/e2e, the renders aside); args go to `playwright test`"; };
          figma = { type = "app"; program = lib.getExe figma; meta.description = "the front end's screens into Figma, crawled over a stack of its own on a copy of .local's data, gridded on a new page of figmaFile"; };
          prod-restore = { type = "app"; program = lib.getExe prodRestore; meta.description = "prod's SQLite files from their litestream replicas: <replica>=<path>…, GITOPS_DIR and DEVOPS_DIR set (docs/LOCAL.md)"; };
          queries-cache-sync = {
            type = "app";
            program = lib.getExe (pkgs.writeShellApplication {
              name = "queries-cache-sync";
              text = ''PANEL_BIN="''${PANEL_BIN:-${bin}/bin/panel}" exec ${lib.getExe queriesCacheSync} "$@"'';
            });
            meta.description = "a local state's answers (<state>: its panel.db and queries_history.db) into prod's queries cache (docs/LOCAL.md)";
          };
          local-stack = { type = "app"; program = lib.getExe localStack; meta.description = "the conductor, its Grafana, sites_panel, review_archive and the landings, wired together on this machine (docs/LOCAL.md)"; };
        };

        packages = {
          default = bin;
          inherit bin frontend placementOverlay;
        } // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (containerStd.packages // { grafana-server = grafanaServer; inherit orbita; mlx = multilogin.mlx; phone-tools = multilogin.phoneTools; });

        containers = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux containers;

        # local-stack's under GITHUB_ACTIONS: CI builds and serves the stack, never develops it
        devShells.ci = pkgs.mkShell {
          inherit stdenv;
          inherit (rs.ciShell) shellHook;
          packages = rs.ciShell.packages ++ stackPackages ++ js.enabledPackages;
          env = localEnv;
        };
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
                export CARGO_TARGET_DIR="$(git rev-parse --show-toplevel)/target" # .cargo/config.toml's is for builds outside this shell
              '';

            packages = stackPackages ++ [
              sqlite # inspecting the database
              rust
              testTimeline
              figma # `nix run .#figma` without its ~8s of evaluation
            ] ++ pre-commit-check.enabledPackages ++ combined.enabledPackages;

            env = localEnv // {
              RUST_BACKTRACE = 1;
              RUST_LIB_BACKTRACE = 0;
            };
          };
      }
    );
}
