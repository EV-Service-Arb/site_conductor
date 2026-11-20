#!/usr/bin/env bash
# The conductor (its Grafana included), sites_panel, review_archive and the landings on this
# machine, wired together: a lead posted on a landing lands in sites_panel through the conductor's
# ingest, a place edited on the sites page shows on the landing, review_archive's boards draw in
# the conductor's Grafana. See docs/LOCAL.md.
#
#   nix run .#local-stack                  build, seed, start everything; Ctrl-C stops it all
#   nix run .#local-stack -- --reset       wipe .local/ (databases, keys, the landings' leads) first
#   nix run .#local-stack -- --no-sites    the conductor and sites_panel, no landings
#   nix run .#local-stack -- --role operator
#   nix run .#local-stack -- --watch       every part rebuilt and restarted as its checkout is edited
#   nix run .#local-stack -- --port-offset 1000   every port moved by 1000: a second stack, e.g. a worktree's
#   nix run .#local-stack -- --reset --from-prod   prod's databases, from their litestream replicas
#   nix run .#dev [-- --port <n>]          the conductor, sites_panel and review_archive under --watch,
#                                          a peer started unless it already serves, a conductor already
#                                          serving stopped and replaced; the conductor on n
#   nix run .#dev -- --fixed [<rev>]       the same unwatched, the conductor as this checkout is now, or at <rev>;
#                                          beside the watched one, on the first free pair down from SA_PORT_RANGE's top;
#                                          opened in the default browser
#
# The conductor and its front end come from `nix build .#bin` / `.#frontend` of this checkout,
# unless PANEL_BIN / PANEL_WEB_DIR name others (PANEL_BIN=target/debug/panel for a cargo build);
# sites_panel and its bundle from its checkout's, unless SITES_BIN / SITES_MFE_DIR do; review_archive
# likewise (REVIEW_ARCHIVE_BIN / REVIEW_ARCHIVE_MFE_DIR), and Grafana's server (PANEL_GRAFANA_SERVER).
# Under --watch each is cargo's (npm's for the bundles) of its checkout instead, and the *_BIN /
# *_DIR overrides of the builds are refused.
# _.siblings-local.json names the siblings' checkouts; AQUAFIX_DIR, VIFNET_DIR and DEBOUCHAGE_DIR the landings', by
# default beside this one's. Each part's port is what its own dev shell sets (docs/port_range.md).
set -euo pipefail
argv=("$@")

need="run it as \`nix run .#local-stack\`, or inside \`nix develop\`: the flake sets the conductor's ports"
PANEL_PORT="${PANEL_BIND:?$need}"
PANEL_PORT="${PANEL_PORT##*:}"
WEB_DEV_PORT="${PANEL_WEB_PORT:?$need}"
BROWSERS_PORT="${BROWSERS_BIND:?$need}"
BROWSERS_PORT="${BROWSERS_PORT##*:}"
SANDBOX_PORT="${SANDBOX_BIND:?$need}"
SANDBOX_PORT="${SANDBOX_PORT##*:}"
offset=0
# whom dev sign-in is, whatever --role: with --from-prod, prod's own account
DEV_EMAIL="admin@evinvest.ltd"
# prod's host, as ~/.ssh/config names it: --from-prod reads review_archive's blobs off its volume
PROD_SSH="rpi5-ts"
AQUAFIX_PLACES=(royat clermont-ferrand desgenettes lyon-est la-mouche lyon-nord)
VIFNET_PLACES=(vifnet)
DEBOUCHAGE_PLACES=(clermont-ferrand)

reset=0
sites=1
watch=0
# Google answers the caches lack are bought only with it: tests and agents never pass it
spend_flag=""
dev=0
no_peers=0
fixed=0
fixed_rev=""
own_port=""
side=0
from_prod=0
role="admin"
while [ $# -gt 0 ]; do
	case "$1" in
	--reset) reset=1 ;;
	--no-sites) sites=0 ;;
	--watch) watch=1 ;;
	--spend-real-quota-i-am-certain) spend_flag="--spend-real-quota-i-am-certain" ;;
	--from-prod) from_prod=1 ;;
	--dev) dev=1 ;;
	--no-peers) no_peers=1 ;;
	--fixed)
		fixed=1
		if [ $# -gt 1 ] && [[ "$2" != -* ]]; then
			fixed_rev="$2"
			shift
		fi
		;;
	--port)
		own_port="${2:-}"
		side=1
		[[ "$own_port" =~ ^[0-9]+$ ]] || {
			echo "--port takes a number, not '$own_port'" >&2
			exit 2
		}
		shift
		;;
	--port-offset)
		offset="${2:-}"
		[[ "$offset" =~ ^[0-9]+$ ]] || {
			echo "--port-offset takes a number, not '$offset'" >&2
			exit 2
		}
		shift
		;;
	--role)
		role="${2:-}"
		shift
		;;
	-h | --help)
		cat <<'EOF'
local-stack [--reset] [--no-sites] [--watch] [--role admin|operator] [--port-offset <n>] [--from-prod] [--spend-real-quota-i-am-certain]
dev [--port <n>] [--reset] [--role admin|operator] [--from-prod] [--no-peers] [--fixed [<rev>]]

  The conductor (dev sign-in, its Grafana at /grafana/), sites_panel, review_archive, aquafix,
  vifnet and debouchage, wired together, each on the port its dev shell sets. State in .local/
  (--reset wipes it). Ctrl-C stops everything. See docs/LOCAL.md.
  --watch: every part rebuilt and restarted as its checkout is edited, and the conductor's front end
  from `next dev`, whose origin is then the one to open.
  --port-offset <n>: every port moved by n, beside the stack on the usual ones.
  --from-prod: prod's panel.db, sites_panel.db and review_archive.db from their litestream replicas
  (the keys from ../devops's sops), and review_archive's blobs over ssh (PROD_SSH, rsync into
  .local-prod-blobs/, which --reset keeps), into a state with none of them; one with all of them keeps its copy.
  Dev sign-in is one user whatever --role (DEV_EMAIL, at the top of the script): prod's, when copied.
  dev (--dev): --watch without the landings. A conductor or its next dev already on their ports is
  stopped, with the run that started it, and replaced; sites_panel and review_archive are started
  unless they already answer on their ports, a peer without a checkout skipped with a warning.
  --port <n>: the conductor on n and its next dev on n+1, in SA_PORT_RANGE, with a state of its own; no browsers' nor
  sandbox's runner, whose ports are the stack's on the usual ones.
  --no-peers: neither built nor started: sites_panel, review_archive, gmaps_optimal_placement, playbook; the conductor answers 502 for them.
  --fixed [<rev>]: nothing watched; the conductor and its static front end built once from a copy of this checkout
  as it is now, or at <rev> (a commit, a tag), into this checkout's build dir: only what differs compiles. Without --port,
  on the first free pair down from SA_PORT_RANGE's top, beside a watched dev and other --fixed runs, never replacing them.
  Without --port and --port-offset, opened in the default browser (xdg-open).
EOF
		exit 0
		;;
	*)
		echo "unknown argument: $1 (see --help)" >&2
		exit 2
		;;
	esac
	shift
done
case "$role" in
admin | operator) ;;
*)
	echo "--role is admin or operator, not '$role'" >&2
	exit 2
	;;
esac
if [ "$dev" = 1 ]; then
	watch=1
	sites=0
fi
if [ "$fixed" = 1 ]; then
	[ "$dev" = 1 ] || {
		echo "--fixed is dev's" >&2
		exit 2
	}
	watch=0
fi
[ -z "$own_port" ] || [ "$dev" = 1 ] || {
	echo "--port is dev's: local-stack moves every port with --port-offset" >&2
	exit 2
}
# --fixed as a person runs it, opened in their browser; with --port or --port-offset, a harness's, on the ports it gave
window=0
[ "$fixed" = 0 ] || [ -n "$own_port" ] || [ "$offset" != 0 ] || window=1
[ "$window" = 0 ] || command -v xdg-open >/dev/null || {
	echo "--fixed opens the conductor with xdg-open: not on PATH" >&2
	exit 1
}

say() { printf '\033[1m▶ %s\033[0m\n' "$*" >&2; }
die() {
	printf '\033[31m✗ %s\033[0m\n' "$*" >&2
	exit 1
}
warn() { printf '\033[33m! %s\033[0m\n' "$*" >&2; }

root="$(git rev-parse --show-toplevel 2>/dev/null)" || die "run this from inside the conductor's checkout"
[ -f "$root/crates/server/Cargo.toml" ] || die "$root is not the conductor's checkout"
# the root's dev shell entered once for the whole run, not per command: each entry costs seconds
if [ "${LOCAL_STACK_SHELL:-}" != "$root" ]; then
	cd "$root" # its shellHook writes into the working directory
	shell=default
	[ -z "${GITHUB_ACTIONS:-}" ] || shell=ci # no dev tools
	exec nix develop "$root#$shell" -c env LOCAL_STACK_SHELL="$root" "$0" ${argv[@]+"${argv[@]}"}
fi
# The others sit beside the conductor's main checkout (_sa/{site_conductor,review_archive,
# sites/{panel,aquafix,vifnet,debouchage}}), also when this is one of its worktrees.
main_checkout="$(dirname "$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)")"
# Where the conductor's code is built from. --fixed: a copy no one edits, in a dir named by its content, building into this
# checkout's build dir, which tells sources apart by checksum (.cargo/config.toml, whose settings the copy inherits)
src="$root"
if [ "$fixed" = 1 ]; then
	worktree_files() { git -C "$root" ls-files -z -co --exclude-standard --deduplicate | while IFS= read -rd '' f; do [ ! -e "$root/$f" ] || printf '%s\0' "$f"; done; }
	if [ -n "$fixed_rev" ]; then
		commit="$(git -C "$root" rev-parse --verify -q "$fixed_rev^{commit}")" || die "--fixed $fixed_rev: not a commit of $root"
		src="$root/.local/fixed/$(git -C "$root" rev-parse --short=12 "$commit^{tree}")"
		fixed_at="$fixed_rev"
	else
		fixed_at="this checkout's state at the start"
		commit="$(git -C "$root" rev-parse HEAD)"
		src="$root/.local/fixed/$(worktree_files | (cd "$root" && xargs -0 sha256sum) | sha256sum | cut -c1-12)" # frontend/tests/e2e/stack.ts hashes the same way
	fi
	if [ ! -e "$src/.copied" ]; then
		say "copying the conductor at $fixed_at into $src"
		rm -rf "$src"
		mkdir -p "$src"
		if [ -n "$fixed_rev" ]; then
			git -C "$root" archive "$commit" | tar -x -C "$src"
		else
			worktree_files | tar -C "$root" --null -T - -c | tar -x -C "$src"
		fi
		# the build script's GIT_HASH: the commit, its objects the checkout's
		git -C "$src" init -q
		echo "$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)/objects" >"$src/.git/objects/info/alternates"
		git -C "$src" update-ref --no-deref HEAD "$commit"
		# no newer than the checkout's when the same commit: the build script reruns on a newer one, and sa_server with it
		[ -n "$fixed_rev" ] || for f in HEAD logs/HEAD; do touch -r "$(git -C "$root" rev-parse --path-format=absolute --git-path "$f")" "$src/.git/$f"; done
		# gitignored, and read by the builds; the installs hard-linked (npm ci replaces what it changes), the overlay copied (rebuilt in place)
		[ ! -e "$root/frontend/_.js-local.json" ] || cp "$root/frontend/_.js-local.json" "$src/frontend/"
		for d in frontend/node_modules crates/sandbox/js/node_modules; do [ ! -d "$root/$d" ] || cp -al "$root/$d" "$src/$d"; done
		[ ! -d "$root/frontend/src/shared/placement_overlay" ] || cp -a "$root/frontend/src/shared/placement_overlay" "$src/frontend/src/shared/"
		# the newest copy's Turbopack cache: the export compiles only what differs
		prev=""
		for c in "$root"/.local/fixed/*/frontend/.next/cache; do
			if [ -d "$c" ] && { [ -z "$prev" ] || [ "$c" -nt "$prev" ]; }; then prev="$c"; fi
		done
		[ -z "$prev" ] || { mkdir -p "$src/frontend/.next" && cp -a "$prev" "$src/frontend/.next/"; }
		touch "$src/.copied"
	fi
fi
service_arb="$(dirname "$main_checkout")"
# the siblings' checkouts, one gitignored file for gen and this script alike: {"sites", "review_archive",
# "placement", "playbook": <checkout root>}
siblings="$root/_.siblings-local.json"
sibling() { jq -er --arg k "$1" '.[$k]' "$siblings" || die "$siblings names no $1"; }
if [ "$no_peers" = 1 ]; then # no checkouts: a CI runner has none
	SITES_DIR="" REVIEW_ARCHIVE_DIR="" PLACEMENT_DIR="" PLAYBOOK_DIR=""
else
	[ -f "$siblings" ] || die "$siblings names each sibling's checkout root: sites, review_archive, placement, playbook"
	SITES_DIR="$(sibling sites)"
	REVIEW_ARCHIVE_DIR="$(sibling review_archive)"
	PLACEMENT_DIR="$(sibling placement)"
	PLAYBOOK_DIR="$(sibling playbook)"
fi
AQUAFIX_DIR="${AQUAFIX_DIR:-$service_arb/sites/aquafix}"
VIFNET_DIR="${VIFNET_DIR:-$service_arb/sites/vifnet}"
DEBOUCHAGE_DIR="${DEBOUCHAGE_DIR:-$service_arb/sites/debouchage}"

busy() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
# --fixed beside the watched dev, never on its ports: in a window, the first free pair down from the range's top, one not claimed by a starting run's pid
if [ "$window" = 1 ]; then
	range="${SA_PORT_RANGE:?$need}"
	own_port=$((${range#*-} - 1))
	while busy "$own_port" || busy "$((own_port + 1))" || kill -0 "$(cat "$root/.local-port-$own_port/pid" 2>/dev/null)" 2>/dev/null; do
		own_port=$((own_port - 2))
		[ "$own_port" -ge "${range%-*}" ] || die "--fixed: no two free ports left in SA_PORT_RANGE $range"
	done
fi
# a stack on other ports is another stack: its own databases, and its own review_archive serve.lock
state="${LOCAL_STACK_DIR:-$root/.local}"
[ "$offset" = 0 ] || state="${LOCAL_STACK_DIR:-$root/.local-$offset}"
# a conductor on a port of its own beside the usual one: never two on one panel.db
[ -z "$own_port" ] || state="${LOCAL_STACK_DIR:-$root/.local-port-$own_port}"
if [ "$reset" = 1 ] && [ -d "$state" ]; then
	say "--reset: wiping $state"
	rm -rf "$state"
fi
mkdir -p "$state/logs" "$state/secrets"
export STACK_TIMELINE="$state/timeline.tsv"
: >"$STACK_TIMELINE"
# Ignored by its own .gitignore: the repo's is generated, and this is nobody else's business.
printf '*\n' >"$state/.gitignore"

# ── ports ────────────────────────────────────────────────────────────────────────────────

# `shell_port <checkout> <variable>`: the port in what that checkout's dev shell sets the variable
# to (a port, or an address ending in one), moved by --port-offset; nothing for a checkout absent
system="$(nix eval --impure --raw --expr builtins.currentSystem)"
shell_port() {
	[ -f "$1/flake.nix" ] || return 0
	local v
	v="$(nix eval --raw "$1#devShells.$system.default.$2")" || die "$1's dev shell sets no $2"
	echo $((${v##*:} + offset))
}
if [ -n "$own_port" ]; then
	range="${SA_PORT_RANGE:?$need}"
	[ "$own_port" -ge "${range%-*}" ] && [ "$((own_port + 1))" -le "${range#*-}" ] ||
		die "--port $own_port: the conductor and its next dev take $own_port and $((own_port + 1)), both in SA_PORT_RANGE $range (docs/port_range.md)"
	PANEL_PORT="$own_port"
	WEB_DEV_PORT=$((own_port + 1))
else
	PANEL_PORT=$((PANEL_PORT + offset))
	WEB_DEV_PORT=$((WEB_DEV_PORT + offset))
fi
BROWSERS_PORT=$((BROWSERS_PORT + offset)) # a sibling's, as the others are
SANDBOX_PORT=$((SANDBOX_PORT + offset))
SITES_PORT="$(shell_port "$SITES_DIR" PANEL_BIND)"
REVIEW_ARCHIVE_PORT="$(shell_port "$REVIEW_ARCHIVE_DIR" REVIEW_ARCHIVE_SERVER_BIND)"
PLACEMENT_PORT="$(shell_port "$PLACEMENT_DIR" PLACEMENT_PORT)"
PLAYBOOK_PORT="$(shell_port "$PLAYBOOK_DIR" PORT)"
AQUAFIX_PORT=""
VIFNET_PORT=""
DEBOUCHAGE_PORT=""
if [ "$sites" = 1 ] || [ "$dev" = 0 ]; then
	AQUAFIX_PORT="$(shell_port "$AQUAFIX_DIR" PORT)"
	VIFNET_PORT="$(shell_port "$VIFNET_DIR" PORT)"
	DEBOUCHAGE_PORT="$(shell_port "$DEBOUCHAGE_DIR" PORT)"
fi

answers() { curl -fsS -o /dev/null --max-time 5 "http://127.0.0.1:$1/health" 2>/dev/null; }
# What this run starts: under dev, each part not already answering on its port; else all of them.
run_panel=1
run_sites=1
run_review_archive=1
run_placement=1
run_playbook=1
placement_answers() { curl -fsS -o /dev/null --max-time 5 "http://127.0.0.1:$PLACEMENT_PORT/health" 2>/dev/null; }
# `take_over <port>`: a conductor or its next dev listening there stops, with the run that started it
# (whatever else that run serves goes too, and is started here instead); anything else is let be.
take_over() {
	local pid root p
	pid="$(ss -ltnpH "sport = :$1" | sed -n 's/.*pid=\([0-9]*\).*/\1/p' | head -1)"
	[ -n "$pid" ] || return 0
	case "$(ps -o comm= -p "$pid")" in
	panel | next-server*) ;;
	*) return 0 ;;
	esac
	root="$pid"
	for p in $(ancestors "$pid"); do
		[ "$(ps -o comm= -p "$p")" = local-stack ] && root="$p"
	done
	say "stopping the conductor's instance on :$1 (pid $root and what it started) to serve this checkout"
	local pids=("$root")
	tree() {
		local c
		for c in $(pgrep -P "$1"); do
			pids+=("$c")
			tree "$c"
		done
	}
	tree "$root"
	kill -TERM "${pids[@]}" 2>/dev/null || true # some exit on their parent's TERM before theirs
	for _ in $(seq 10); do
		busy "$1" || return 0
		sleep 1
	done
	kill -KILL "${pids[@]}" 2>/dev/null || true # gone already, or the port would still be held
	for _ in $(seq 5); do
		busy "$1" || return 0
		sleep 1
	done
	die ":$1 is still held after killing pid $root's tree"
}
ancestors() {
	local p="$1"
	while p="$(ps -o ppid= -p "$p" | tr -d ' ')" && [ "$p" -gt 1 ]; do echo "$p"; done
}
if [ "$dev" = 1 ]; then
	take_over "$PANEL_PORT"
	take_over "$WEB_DEV_PORT"
	if answers "$PANEL_PORT"; then
		say "the conductor already serves http://127.0.0.1:$PANEL_PORT: using it"
		run_panel=0
	fi
	if [ -z "$SITES_PORT" ]; then
		warn "skipping sites_panel: no checkout at $SITES_DIR (_.siblings-local.json names it)"
		run_sites=0
	elif answers "$SITES_PORT"; then
		say "sites_panel already serves http://127.0.0.1:$SITES_PORT: using it"
		run_sites=0
	fi
	if [ -z "$REVIEW_ARCHIVE_PORT" ]; then
		warn "skipping review_archive: no checkout at $REVIEW_ARCHIVE_DIR (_.siblings-local.json names it)"
		run_review_archive=0
	elif answers "$REVIEW_ARCHIVE_PORT"; then
		say "review_archive already serves http://127.0.0.1:$REVIEW_ARCHIVE_PORT: using it"
		run_review_archive=0
	fi
	if [ -z "$PLACEMENT_PORT" ]; then
		warn "skipping gmaps_optimal_placement: no checkout at $PLACEMENT_DIR (_.siblings-local.json names it); the map says placement is not connected"
		run_placement=0
	elif placement_answers; then
		say "gmaps_optimal_placement already serves http://127.0.0.1:$PLACEMENT_PORT: using it"
		run_placement=0
	fi
	if [ -z "$PLAYBOOK_PORT" ]; then
		warn "skipping playbook: no checkout at $PLAYBOOK_DIR (_.siblings-local.json names it); /mcp lists no playbook tools"
		run_playbook=0
	elif answers "$PLAYBOOK_PORT"; then
		say "playbook already serves http://127.0.0.1:$PLAYBOOK_PORT: using it"
		run_playbook=0
	fi
	if [ "$run_panel$run_sites$run_review_archive" = 000 ]; then
		if busy "$WEB_DEV_PORT"; then say "open http://localhost:$WEB_DEV_PORT"; else say "open http://127.0.0.1:$PANEL_PORT"; fi
		exit 0
	fi
elif answers "$PANEL_PORT"; then
	echo "a stack already serves http://127.0.0.1:$PANEL_PORT (and http://localhost:$WEB_DEV_PORT under --watch): use it, or --port-offset <n> for another" >&2
	exit 0
fi
[ "$dev" = 1 ] || [ -n "$PLAYBOOK_PORT" ] || {
	warn "skipping playbook: no checkout at $PLAYBOOK_DIR (_.siblings-local.json names it); /mcp lists no playbook tools"
	run_playbook=0
}
[ "$no_peers" = 0 ] || { run_sites=0; run_review_archive=0; run_placement=0; run_playbook=0; }
ports=()
[ "$run_panel" = 1 ] && ports+=("$PANEL_PORT")
[ "$run_sites" = 1 ] && ports+=("$SITES_PORT")
[ "$run_review_archive" = 1 ] && ports+=("$REVIEW_ARCHIVE_PORT")
[ "$run_placement" = 1 ] && ports+=("$PLACEMENT_PORT")
[ "$run_playbook" = 1 ] && ports+=("$PLAYBOOK_PORT")
[ "$sites" = 1 ] && ports+=("$AQUAFIX_PORT" "$VIFNET_PORT" "$DEBOUCHAGE_PORT")
[ "$watch" = 1 ] && [ "$run_panel" = 1 ] && ports+=("$WEB_DEV_PORT")
for port in "${ports[@]}"; do
	[ -n "$port" ] || continue # a checkout absent, skipped below
	busy "$port" && die "port $port is taken — another local stack, or \`lsof -nP -iTCP:$port -sTCP:LISTEN\` says who"
done
# whose stack this is, for e2e's stack.ts: reused while these match, stopped by the pid; before the builds, so a second run finds it starting
printf '%s\n' ${argv[@]+"${argv[@]}"} "SA_CONDUCTOR_CONFIG=${SA_CONDUCTOR_CONFIG:-}" >"$state/args"
echo $$ >"$state/pid"
[ "$fixed" = 0 ] || echo "$src" >"$state/src" # the copy served, named by its content: stack.ts's freshness
rm -f "$state/panel.built" "$state/panel.failed" # a previous stack's builds

# ── prod's data ──────────────────────────────────────────────────────────────────────────

REVIEW_ARCHIVE_DATA_DIR="${REVIEW_ARCHIVE_DATA_DIR:-$state/review_archive}"
copies=("$state/panel.db:panel-data/panel.db" "$state/sites_panel.db:sites-panel-data/sites_panel.db"
	"$REVIEW_ARCHIVE_DATA_DIR/review_archive.db:review-archive-data/review_archive.db")
restore=0
if [ "$from_prod" = 1 ]; then
	kept=0
	for c in "${copies[@]}"; do [ ! -e "${c%%:*}" ] || kept=$((kept + 1)); done
	case "$kept" in
	0)
		[ ! -e "$REVIEW_ARCHIVE_DATA_DIR/blobs" ] || die "--from-prod: $REVIEW_ARCHIVE_DATA_DIR has blobs but no databases: add --reset"
		restore=1
		;;
	"${#copies[@]}") say "--from-prod: keeping the copy of prod's databases in $state (--reset for a fresh one)" ;;
	*) die "--from-prod restores into a state with none of its databases, or keeps all of them: add --reset" ;;
	esac
fi
# Beside the builds, waited on before the first reader. Under `timed`, so errexit is off: every step checked.
prod_data() {
	local devops="${DEVOPS_DIR:-$(dirname "$service_arb")/devops}" gitops="${GITOPS_DIR:-$(dirname "$service_arb")/gitops}"
	local cache="$root/.local-prod-blobs" r2_key r2_secret volume c p pids=() failed=0
	# the replicas as gitops main configures them, not as an old checkout of it says
	git -C "$gitops" fetch -q origin main || return 1
	git -C "$gitops" show FETCH_HEAD:clusters/rpi5/tenants/service-arb/manifests.yaml |
		yq eval-all '[select(.kind == "ConfigMap" and .data["litestream.yml"])][0].data["litestream.yml"]' >"$state/litestream.yml" || return 1
	r2_key="$(sops -d --extract '["r2_access_key_id"]' "$devops/nix/secrets/host.json")" || return 1
	r2_secret="$(sops -d --extract '["r2_secret_access_key"]' "$devops/nix/secrets/host.json")" || return 1
	mkdir -p "$REVIEW_ARCHIVE_DATA_DIR" "$cache"
	printf '*\n' >"$cache/.gitignore"
	for c in "${copies[@]}"; do
		(
			unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY # the caller's would win over litestream's own variables
			export LITESTREAM_ACCESS_KEY_ID="$r2_key" LITESTREAM_SECRET_ACCESS_KEY="$r2_secret"
			timed "restore/$(basename "${c%%:*}" .db)" litestream restore -config "$state/litestream.yml" -o "${c%%:*}" "/pvc/${c#*:}"
		) &
		pids+=("$!")
	done
	# not replicated: read off the volume on the host, into a cache outside the state (--reset keeps it), so only what
	# changed comes again; hard-linked into the state, as review_archive replaces a blob by a rename, never in place
	# shellcheck disable=SC2016 # expanded on the host
	volume="$(ssh -o BatchMode=yes "$PROD_SSH" 'pv="$(sudo -n k3s kubectl -n service-arb get pvc review-archive-data -o jsonpath="{.spec.volumeName}")" && sudo -n k3s kubectl get pv "$pv" -o jsonpath="{.spec.local.path}"')" || return 1
	{ timed blobs rsync -a --delete -e "ssh -o BatchMode=yes" --rsync-path="sudo -n rsync" "$PROD_SSH:$volume/blobs/" "$cache/blobs/" &&
		cp -al "$cache/blobs" "$REVIEW_ARCHIVE_DATA_DIR/blobs"; } &
	pids+=("$!")
	for p in "${pids[@]}"; do wait "$p" || failed=1; done
	[ "$failed" = 0 ]
}

# ── the builds ───────────────────────────────────────────────────────────────────────────

# `in_shell <checkout> <command…>`: in that checkout, under its own dev shell's toolchain; under `timed`,
# the shell's realisation and the command are spans of their own
in_shell() {
	local dir="$1"
	shift
	if [ "$dir" = "$root" ] || [ "$dir" = "$src" ]; then # entered at the top; --fixed's copy builds under it too
		if [ -z "${TIMELINE_PARENT:-}" ]; then (cd "$dir" && "$@"); else (cd "$dir" && timed "${1##*/}" "$@"); fi
		return
	fi
	if [ -z "${TIMELINE_PARENT:-}" ]; then
		(cd "$dir" && nix develop -c "$@")
		return
	fi
	local start
	start="$(date +%s%3N)"
	printf '%s\t%s\t\n' "$TIMELINE_PARENT/nix develop" "$start" >>"$STACK_TIMELINE"
	# shellcheck disable=SC2016 # expanded by the shell inside
	(cd "$dir" && nix develop -c bash -c "$(declare -f timed)"'
		printf "%s\t%s\t%s\n" "$TIMELINE_PARENT/nix develop" "$0" "$(date +%s%3N)" >>"$STACK_TIMELINE"
		timed "${1##*/}" "$@"' "$start" "$@")
}
REVIEW_ARCHIVE_WEB_BUILD="nix run .#build-mfe -- '$state/watch/review_archive-mfe'"
# The checkout's binaries linked into the state, its target dir keeping the rest: other sessions
# build their worktrees into this target dir, and the last to link owns target/debug/<bin>.
own_cargo="env CARGO_BUILD_BUILD_DIR='$root/target' CARGO_TARGET_DIR='$state/target' cargo"
# once more on a failure: other sessions sweep target dirs, mid-build too
retried() { "$@" || { warn "retrying: $*"; "$@"; }; }
# `timed <name> <command…>`: a span of $STACK_TIMELINE (name, start, end in epoch ms; a row without end
# on start, the whole row again on end), nested under the `timed` it runs in; the e2e's timeline
# reporter draws it beside the tests
timed() {
	local name="${TIMELINE_PARENT:+$TIMELINE_PARENT/}$1" start rc=0
	shift
	start="$(date +%s%3N)"
	printf '%s\t%s\t\n' "$name" "$start" >>"$STACK_TIMELINE"
	TIMELINE_PARENT="$name" "$@" || rc=$?
	printf '%s\t%s\t%s\n' "$name" "$start" "$(date +%s%3N)" >>"$STACK_TIMELINE"
	return "$rc"
}
# Each line prefixed with whose it is, and kept in .local/logs/<name>.log.
prefixed() {
	local name="$1"
	shift
	"$@" 2>&1 | tee -a "$state/logs/$name.log" | awk -v p="[$name] " '{ print p $0; fflush() }'
}
# `unbuilt <peer> <checkout>`: dev goes on without it, the conductor answering 502 for it; the
# stack needs every part
unbuilt() {
	local locked head hint=""
	locked="$(grep -m1 -o 'site_conductor?branch=main#[0-9a-f]*' "$2/Cargo.lock" | cut -d'#' -f2)" || true # no sa_* locked: no hint
	head="$(git -C "$root" rev-parse HEAD)"
	[ -z "$locked" ] || [ "$locked" = "$head" ] ||
		hint="; its Cargo.lock has sa_* at ${locked:0:7}, this checkout is at ${head:0:7}: [patch.\"https://github.com/EV-Service-Arb/site_conductor\"] in $2/.cargo/config.toml points them at $root/crates"
	[ "$dev" = 1 ] || die "$1 did not build$hint"
	warn "skipping $1: it did not build$hint (see $state/logs/); the conductor answers 502 for it"
}
# `late <peer>`: one not answering in time; under dev it goes on starting, 502 until it answers
late() {
	[ "$dev" = 1 ] || die "$1 did not start"
	warn "$1 is not up yet: the conductor answers 502 for it until it is"
}
# What does not wait on the conductor's build starts beside it, and is waited for where it is first needed.
prod_data_build=""
if [ "$restore" = 1 ]; then
	say "restoring prod's databases from their litestream replicas, and review_archive's blobs from $PROD_SSH, beside the builds"
	timed prod-data prod_data &
	prod_data_build=$!
fi
grafana_build=""
if [ "$run_panel" = 1 ] && [ -z "${PANEL_GRAFANA_SERVER:-}" ]; then
	say "building Grafana (nix build .#grafana-server)"
	PANEL_GRAFANA_SERVER="$state/build/grafana-server"
	timed grafana nix build "$root#grafana-server" --out-link "$PANEL_GRAFANA_SERVER" &
	grafana_build=$!
fi
# The front end's packages and the map's overlay, for the next dev started below.
web_prep() {
	# two --fixed runs of one state of the checkout share its copy, whose next build refuses a second at once; under `&`: held till the job ends
	[ "$fixed" = 0 ] || { exec 9<"$src/.copied" && flock 9; }
	# in the dev shell, whose js-local lays our packages' checkouts over the install (frontend/_.js-local.json);
	# npm ci stamps node_modules/.package-lock.json: a lockfile pulled since is newer
	timed packages prefixed web in_shell "$src" bash -c \
		'cd frontend && if [ node_modules/.package-lock.json -nt package-lock.json ]; then js-local; else npm ci; fi'
	# skipped while its inputs hash as they did at its last build: the wasm takes ~20s
	local out="$src/frontend/src/shared/placement_overlay" inputs
	inputs="$( (cd "$src" && find crates/placement_overlay Cargo.toml Cargo.lock .cargo -type f ! -path '*/target/*'; [ -z "$PLACEMENT_DIR" ] || find "$PLACEMENT_DIR/gmaps_optimal_placement_core" -type f ! -path '*/target/*') | sort | (cd "$src" && xargs -d '\n' sha256sum) | sha256sum)"
	if [ ! -f "$out/placement_overlay_bg.wasm" ] || [ "$(cat "$out/.inputs-hash" 2>/dev/null)" != "$inputs" ]; then
		timed overlay prefixed placement-overlay in_shell "$src" env CARGO_BUILD_BUILD_DIR="$root/target" placement-overlay
		echo "$inputs" >"$out/.inputs-hash"
	fi
	[ "$fixed" = 0 ] || timed export prefixed web in_shell "$src" bash -c 'cd frontend && npm run -s i18n && PANEL_EXPORT_UNCHECKED=1 npx next build'
}
web_prep_build=""
if [ "$run_panel" = 1 ] && { [ "$watch" = 1 ] || [ "$fixed" = 1 ]; }; then
	timed web-prep web_prep &
	web_prep_build=$!
fi
# The conductor's and its runners' binaries in one build, each watched rebuild too: another -p set unifies features
# otherwise, a second variant of shared deps built cold under the build dir's lock.
conductor_packages="-p sa_server -p sa_sandbox"
[ -z "${GOLOGIN_TOKEN:-}${MULTILOGIN_TOKEN:-}" ] || conductor_packages+=" -p sa_browsers"
build_sites() {
	retried in_shell "$SITES_DIR" cargo b -q -p panel_server &&
		(cd "$SITES_DIR/frontend" && { [ node_modules/.package-lock.json -nt package-lock.json ] || npm ci; } && npm run -s build)
}
build_review_archive() {
	retried in_shell "$REVIEW_ARCHIVE_DIR" cargo b -q -p review_archive_server &&
		(cd "$REVIEW_ARCHIVE_DIR" && retried nix run .#build-mfe -- "$state/watch/review_archive-mfe")
}
if [ "$watch" = 1 ] || [ "$fixed" = 1 ]; then
	for v in PANEL_BIN PANEL_WEB_DIR SITES_BIN SITES_MFE_DIR REVIEW_ARCHIVE_BIN REVIEW_ARCHIVE_MFE_DIR; do
		[ -z "${!v:-}" ] || die "--watch and --fixed build every part from its checkout: unset $v"
	done
	say "building the conductor, sites_panel and review_archive from their checkouts, at once"
	# also when another conductor serves: `panel gen-data-key` makes the state's key
	timed conductor retried in_shell "$src" bash -c "$own_cargo b -q $conductor_packages" &
	conductor_build=$!
	sites_build=""
	if [ "$run_sites" = 1 ] && [ -z "${DEV_COMMAND_sites_panel:-}" ]; then
		[ -f "$SITES_DIR/flake.nix" ] || die "no sites_panel checkout at $SITES_DIR: _.siblings-local.json names it"
		timed sites_panel build_sites &
		sites_build=$!
	fi
	review_archive_build=""
	if [ "$run_review_archive" = 1 ] && [ -z "${DEV_COMMAND_review_archive:-}" ]; then
		[ -f "$REVIEW_ARCHIVE_DIR/flake.nix" ] || die "no review_archive checkout at $REVIEW_ARCHIVE_DIR: _.siblings-local.json names it"
		timed review_archive build_review_archive &
		review_archive_build=$!
	fi
	wait "$conductor_build" || die "the conductor did not build"
	PANEL_BIN="$state/target/debug/panel"
	if [ -n "$sites_build" ]; then
		if wait "$sites_build"; then
			SITES_BIN="$SITES_DIR/target/debug/sites_panel"
			SITES_MFE_DIR="$SITES_DIR/frontend/dist"
		else
			unbuilt sites_panel "$SITES_DIR"
			run_sites=0
		fi
	fi
	if [ -n "$review_archive_build" ]; then
		if wait "$review_archive_build"; then
			REVIEW_ARCHIVE_BIN="$REVIEW_ARCHIVE_DIR/target/debug/review_archive"
			REVIEW_ARCHIVE_MFE_DIR="$state/watch/review_archive-mfe"
		else
			unbuilt review_archive "$REVIEW_ARCHIVE_DIR"
			run_review_archive=0
		fi
	fi
fi
if [ -z "${PANEL_BIN:-}" ]; then
	say "building the conductor (nix build .#bin)"
	nix build "$root#bin" --out-link "$state/build/bin"
	PANEL_BIN="$state/build/bin/bin/panel"
fi
if [ "$watch" = 1 ]; then
	PANEL_WEB_DIR=""
elif [ "$fixed" = 1 ]; then
	[ "$run_panel" = 0 ] || wait "$web_prep_build" || die "the front end did not build"
	PANEL_WEB_DIR="$src/frontend/out"
elif [ -z "${PANEL_WEB_DIR:-}" ]; then
	say "building the front end (nix build .#frontend)"
	nix build "$root#frontend" --out-link "$state/build/frontend"
	PANEL_WEB_DIR="$state/build/frontend"
fi
[ -x "$PANEL_BIN" ] || die "PANEL_BIN $PANEL_BIN is not an executable"
if [ "$run_sites" = 1 ] && [ "$watch" = 0 ] && [ -z "${SITES_BIN:-}" ]; then
	[ -f "$SITES_DIR/flake.nix" ] || die "no sites_panel checkout at $SITES_DIR: _.siblings-local.json names it"
	say "building sites_panel ($SITES_DIR#bin)"
	nix build "$SITES_DIR#bin" --out-link "$state/build/sites"
	SITES_BIN="$state/build/sites/bin/sites_panel"
fi
if [ "$run_sites" = 1 ] && [ "$watch" = 0 ] && [ -z "${SITES_MFE_DIR:-}" ]; then
	say "building the sites' bundle ($SITES_DIR#frontend)"
	nix build "$SITES_DIR#frontend" --out-link "$state/build/sites-mfe"
	SITES_MFE_DIR="$state/build/sites-mfe"
fi
[ "$run_sites" = 0 ] || [ -n "${DEV_COMMAND_sites_panel:-}" ] || [ -x "$SITES_BIN" ] || die "SITES_BIN $SITES_BIN is not an executable"
[ -z "$grafana_build" ] || wait "$grafana_build" || die "Grafana did not build"
if [ "$run_review_archive" = 1 ] && [ "$watch" = 0 ] && { [ -z "${REVIEW_ARCHIVE_BIN:-}" ] || [ -z "${REVIEW_ARCHIVE_MFE_DIR:-}" ]; }; then
	if [ -f "$REVIEW_ARCHIVE_DIR/flake.nix" ]; then
		if [ -z "${REVIEW_ARCHIVE_BIN:-}" ]; then
			say "building review_archive ($REVIEW_ARCHIVE_DIR#bin)"
			nix build "$REVIEW_ARCHIVE_DIR#bin" --out-link "$state/build/review_archive"
			REVIEW_ARCHIVE_BIN="$state/build/review_archive/bin/review_archive"
		fi
		if [ -z "${REVIEW_ARCHIVE_MFE_DIR:-}" ]; then
			say "building review_archive's bundle ($REVIEW_ARCHIVE_DIR#mfe)"
			nix build "$REVIEW_ARCHIVE_DIR#mfe" --out-link "$state/build/review_archive-mfe"
			REVIEW_ARCHIVE_MFE_DIR="$state/build/review_archive-mfe"
		fi
	else
		echo "skipping review_archive: no checkout at $REVIEW_ARCHIVE_DIR (_.siblings-local.json names it); /api/review_archive answers 502" >&2
		run_review_archive=0
	fi
fi

if [ "$run_playbook" = 1 ] && [ -z "${PLAYBOOK_BIN:-}" ]; then
	# cargo in its checkout, which patches sa_auth to the local one: Nix would fetch the git dep
	say "building playbook_web from $PLAYBOOK_DIR"
	in_shell "$PLAYBOOK_DIR" cargo b -q -p playbook_web
	PLAYBOOK_BIN="$PLAYBOOK_DIR/target/debug/playbook_web"
fi

# ── the environments ─────────────────────────────────────────────────────────────────────

[ -z "$prod_data_build" ] || wait "$prod_data_build" || die "prod's data did not restore (see the lines above)"
[ -s "$state/data-key" ] || "$PANEL_BIN" gen-data-key >"$state/data-key"
origin="http://127.0.0.1:$PANEL_PORT"
# The browser's origin: the sign-in sends it back there, and the live socket must come from it.
public_origin="http://localhost:$PANEL_PORT" # localhost as under --watch: one host for the Maps key's referrers
if [ "$watch" = 1 ] && { [ "$run_panel" = 1 ] || busy "$WEB_DEV_PORT"; }; then
	public_origin="http://localhost:$WEB_DEV_PORT"
fi
sites_origin="http://127.0.0.1:$SITES_PORT"
review_archive_origin="http://127.0.0.1:$REVIEW_ARCHIVE_PORT"
placement_origin="${PLACEMENT_PORT:+http://127.0.0.1:$PLACEMENT_PORT}"
# playbook_web where it runs, else nowhere: the conductor's variable is required, and the forward answers 502 for it
playbook_origin="http://127.0.0.1:9"
if [ -n "$PLAYBOOK_PORT" ] && { [ "$run_playbook" = 1 ] || answers "$PLAYBOOK_PORT"; }; then
	playbook_origin="http://127.0.0.1:$PLAYBOOK_PORT"
fi
# The gmails' browsers only with the caller's GOLOGIN_TOKEN or MULTILOGIN_TOKEN: neither runs without a plan's.
browsers_origin=""
[ -z "${GOLOGIN_TOKEN:-}${MULTILOGIN_TOKEN:-}" ] || [ "$run_panel" = 0 ] || [ "$side" = 1 ] || browsers_origin="http://127.0.0.1:$BROWSERS_PORT"
# a runner answers one conductor: under dev, another stack's (a --port one beside it) leaves this one without
if [ -n "$browsers_origin" ] && [ "$dev" = 1 ] && busy "$BROWSERS_PORT"; then
	warn "port $BROWSERS_PORT is another stack's browsers' runner: none here, the gmails' browsers say they are not connected"
	browsers_origin=""
fi
# The agents' `execute` once crates/sandbox/js has its packages: @evinvest/api comes through its _.js-local.json until published.
sandbox_origin=""
if [ "$run_panel" = 1 ] && [ "$side" = 0 ]; then
	sandbox_js="$src/crates/sandbox/js"
	[ "$sandbox_js/node_modules/.package-lock.json" -nt "$sandbox_js/package.json" ] || timed sandbox-js in_shell "$root" bash -c "cd crates/sandbox/js && npm install --no-audit --no-fund >&2"
	if [ -d "$sandbox_js/node_modules/@evinvest/api" ]; then
		sandbox_origin="http://127.0.0.1:$SANDBOX_PORT"
	fi
	if [ -n "$sandbox_origin" ] && [ "$dev" = 1 ] && busy "$SANDBOX_PORT"; then
		warn "port $SANDBOX_PORT is another stack's sandbox runner: none here, the agents have no execute"
		sandbox_origin=""
	fi
fi
# The Google accounts, their keys named in it from the environment (docs/ARCHITECTURE.md, Google accounts).
conductor_config="${SA_CONDUCTOR_CONFIG:-$HOME/.config/sa_conductor.nix}"
[ -f "$conductor_config" ] || { echo "no conductor config at $conductor_config (SA_CONDUCTOR_CONFIG): its \`google\` accounts, \`[]\` for none" >&2; exit 1; }
# A key for this machine alone: the 32 bytes 0..31, and its public half for sites_panel.
assertion_key="local:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
assertion_public="local:A6EHv/POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg="
# Whatever the calling shell exports for a deployed conductor must not leak in: concierge's
# variables would refuse the dev sign-in, a bot token would start the bot.
panel() {
	env -u CONCIERGE_PUBLIC_ORIGIN -u CONCIERGE_GRPC_ADDR -u RP_CLIENT_SECRET_SA \
		-u TELEGRAM_BOT_TOKEN -u SENTRY_DSN \
		APP_ENV=development \
		PANEL_DB_PATH="$state/panel.db" \
		PANEL_DATA_KEY="$(cat "$state/data-key")" \
		PANEL_PUBLIC_ORIGIN="$public_origin" \
		PANEL_DEV_SIGN_IN="sa:$role" \
		PANEL_DEV_SIGN_IN_EMAIL="$DEV_EMAIL" \
		PANEL_WEB_DIR="$PANEL_WEB_DIR" \
		PANEL_ASSERTION_KEY="$assertion_key" \
		PANEL_SITES_URL="$sites_origin" \
		PANEL_REVIEW_ARCHIVE_URL="$review_archive_origin" \
		PANEL_PLACEMENT_URL="$placement_origin" \
		PANEL_BROWSERS_URL="$browsers_origin" \
		PANEL_SANDBOX_URL="$sandbox_origin" \
		GOLOGIN_TOKEN="${browsers_origin:+${GOLOGIN_TOKEN:-}}" \
		MULTILOGIN_TOKEN="${browsers_origin:+${MULTILOGIN_TOKEN:-}}" \
		PANEL_GRAFANA_SERVER="$PANEL_GRAFANA_SERVER" \
		PANEL_GRAFANA_DIR="$state/grafana" \
		PANEL_PLAYBOOK_URL="$playbook_origin" \
		TOKENS_START_CAP_MULTIPLE=1 \
		"$PANEL_BIN" --config "$conductor_config" ${spend_flag:+"$spend_flag"} "$@"
}
sites_panel() {
	env -u POSTHOG_PROJECT_API_KEY -u POSTHOG_PROJECT_ID -u SENTRY_DSN \
		APP_ENV=development \
		PANEL_DB_PATH="$state/sites_panel.db" \
		PANEL_DATA_KEY="$(cat "$state/data-key")" \
		PANEL_ASSERTION_KEYS="$assertion_public" \
		PANEL_MFE_DIR="$SITES_MFE_DIR" \
		"$SITES_BIN" "$@"
}

# playbook_web in a dir of its own (it keeps mcp.db in its working directory), trusting the
# conductor's assertions; concierge is never asked: its projection only fails to sync.
playbook() {
	mkdir -p "$state/playbook"
	cd "$state/playbook"
	env -u SENTRY_DSN \
		APP_ENV=development \
		PORT="$PLAYBOOK_PORT" \
		PUBLIC_URL="$playbook_origin" \
		PANEL_ASSERTION_KEYS="$assertion_public" \
		CONCIERGE_GRPC_ADDR="http://127.0.0.1:9" \
		CONCIERGE_CLIENT_SECRET="local" \
		"$PLAYBOOK_BIN" "$@"
}

# review_archive on its data dir (REVIEW_ARCHIVE_DATA_DIR, e.g. a copy of its checkout's data/),
# trusting the conductor's assertions, HTTP only: the stack holds the dir's serve.lock, so nothing
# walks Maps and the boards draw what is stored.
review_archive() {
	mkdir -p "$state/review_archive" "$REVIEW_ARCHIVE_DATA_DIR"
	cat >"$state/review_archive/config.toml" <<EOF
data_dir = "$REVIEW_ARCHIVE_DATA_DIR"
mfe_dir = "$REVIEW_ARCHIVE_MFE_DIR"
bind = "127.0.0.1:$REVIEW_ARCHIVE_PORT"
queries_cache = "http://127.0.0.1:$PANEL_PORT/api/queries_cache"
[browser]
lean = []
EOF
	env -u SENTRY_DSN -u TELEGRAM_BOT_TOKEN \
		APP_ENV=development \
		PANEL_ASSERTION_KEYS="$assertion_public" \
		"$REVIEW_ARCHIVE_BIN" --config "$state/review_archive/config.toml" "$@"
}

# ── seed: idempotent, so every run may do it ─────────────────────────────────────────────

# a conductor or sites_panel already serving has its own state, seeded by the run that started it
fresh=0
if [ "$run_panel" = 1 ]; then
	[ -e "$state/panel.db" ] || fresh=1
fi
if [ "$run_sites" = 1 ] && [ -z "${DEV_COMMAND_sites_panel:-}" ]; then
	say "seeding $state/sites_panel.db"
	sites_panel migrate
fi

# ── run ──────────────────────────────────────────────────────────────────────────────────

# Job control: each background job below in a process group of its own, so a Ctrl-C reaches this
# script alone and stop_all stops the rest.
set -m
stop_all() {
	trap - INT TERM EXIT
	set +e # a closed terminal fails the writes below; under -e that would leave every part running
	rm -f "$state/args" "$state/pid" "$state/src"
	echo >&2
	say "stopping"
	# Every descendant, not the jobs' groups: watchexec runs its command in a group of its own, and
	# quitting may stop only that command's shell, leaving the server under it to serve on.
	local pids=() p
	descendants() {
		local c
		for c in $(pgrep -P "$1"); do
			pids+=("$c")
			descendants "$c"
		done
	}
	descendants $$
	kill -TERM ${pids[@]+"${pids[@]}"} 2>/dev/null
	for _ in $(seq 10); do
		local alive=0
		for p in ${pids[@]+"${pids[@]}"}; do kill -0 "$p" 2>/dev/null && alive=1; done
		[ "$alive" = 0 ] && return 0
		sleep 1
	done
	kill -KILL ${pids[@]+"${pids[@]}"} 2>/dev/null
}
trap stop_all EXIT
trap 'stop_all; exit 130' INT TERM

# Polls `url` while `pid` lives, at most `tries` seconds.
wait_for() {
	local name="$1" url="$2" tries="$3" pid="$4"
	local code errors=0
	for _ in $(seq "$tries"); do
		code="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 5 "$url" 2>/dev/null)"
		case "$code" in
		[23]??) return 0 ;;
		500) errors=$((errors + 1)) ;; # next dev answers a compile error with 500, for good
		*) errors=0 ;;
		esac
		[ "$errors" -lt 10 ] || {
			echo "$name answered $url with 500 ten times running — the tail of $state/logs/$name.log:" >&2
			tail -n 40 "$state/logs/$name.log" >&2
			return 1
		}
		kill -0 "$pid" 2>/dev/null || {
			echo "$name stopped before answering $url — the tail of $state/logs/$name.log:" >&2
			tail -n 40 "$state/logs/$name.log" >&2
			return 1
		}
		sleep 1
	done
	echo "$name did not answer $url in ${tries}s — the tail of $state/logs/$name.log:" >&2
	tail -n 40 "$state/logs/$name.log" >&2
	return 1
}

# `watched <checkout> <watchexec's filters…> -- <command>`: <command> (bash, the functions above
# included) in that checkout's dev shell, stopped and run again as the files watched change
# sa_browsers' runner: Orbita headless, the Multilogin app on its own Xvfb, a session per gmail
# under the state dir; the phones it starts, Multilogin's
browsers() {
	BROWSERS_BIND="127.0.0.1:$BROWSERS_PORT" \
		BROWSERS_DIR="$state/browsers" \
		BROWSERS_MAX=4 \
		BROWSERS_ORBITA="$BROWSERS_ORBITA" \
		BROWSERS_FFMPEG="$BROWSERS_FFMPEG" \
		BROWSERS_MLX="$BROWSERS_MLX" \
		BROWSERS_MLX_LAUNCHER=https://launcher.mlx.yt:45001 \
		BROWSERS_MULTILOGIN_API=https://api.multilogin.com \
		BROWSERS_ADB="$BROWSERS_ADB" \
		BROWSERS_BUSYBOX="$BROWSERS_BUSYBOX" \
		BROWSERS_ADB_KEYBOARD="$BROWSERS_ADB_KEYBOARD" \
		PANEL_ASSERTION_KEYS="$assertion_public" \
		"$state/target/debug/sa_browsers" "$@"
}
# sa_sandbox's runner: each run Deno, net to this conductor's /api/run alone
sandbox() {
	SANDBOX_BIND="127.0.0.1:$SANDBOX_PORT" \
		SANDBOX_MAX=2 \
		SANDBOX_DENO="$(command -v deno)" \
		SANDBOX_JS="$src/crates/sandbox/js" \
		SANDBOX_CONDUCTOR_URL="http://127.0.0.1:$PANEL_PORT/api/run" \
		PANEL_ASSERTION_KEYS="$assertion_public" \
		"$state/target/debug/sa_sandbox" "$@"
}
export -f panel sites_panel review_archive browsers sandbox playbook
export state conductor_config spend_flag public_origin role DEV_EMAIL PANEL_WEB_DIR assertion_key assertion_public sites_origin review_archive_origin placement_origin browsers_origin sandbox_origin \
	PANEL_GRAFANA_SERVER PANEL_BIN SITES_BIN SITES_MFE_DIR REVIEW_ARCHIVE_BIN REVIEW_ARCHIVE_MFE_DIR REVIEW_ARCHIVE_DATA_DIR REVIEW_ARCHIVE_PORT \
	PANEL_PORT SITES_PORT BROWSERS_PORT SANDBOX_PORT root src PLAYBOOK_BIN PLAYBOOK_PORT playbook_origin
watched() {
	local dir="$1"
	shift
	in_shell "$dir" watchexec --shell=bash --restart --stop-signal SIGTERM --stop-timeout 10s --debounce 500ms "$@"
}
# A part under --watch: built again and restarted on its checkout's changes; else the build above.
serving() {
	local name="$1" dir="$2" build="$3"
	shift 3
	if [ "$watch" = 1 ]; then
		prefixed "$name" watched "$dir" -w crates -w Cargo.toml -w Cargo.lock "${@:2}" -- "$build && $1"
	else
		prefixed "$name" bash -c "$1"
	fi
}

# The parts whose stop stops the rest.
essential=()
# `custom <name> <checkout> <command>`: a peer served by its user's DEV_COMMAND_<peer> (flake.nix
# devUsers) in place of the stack's own, in its checkout, on the port its dev shell sets
custom() {
	say "starting $1 from $2 with \`$3\`"
	(cd "$2" && prefixed "$1" bash -c "$3") &
	[ "$dev" = 1 ] || essential+=("$!")
}

if [ "$run_sites" = 1 ]; then
	if [ -n "${DEV_COMMAND_sites_panel:-}" ]; then
		custom sites "$SITES_DIR" "$DEV_COMMAND_sites_panel"
		wait_for sites "$sites_origin/health" 600 "$!" || late sites_panel
	else
		say "starting sites_panel on $sites_origin"
		serving sites "$SITES_DIR" "cargo b -q -p panel_server" "sites_panel serve --bind 127.0.0.1:$SITES_PORT" -e rs,toml,sql,lock &
		[ "$dev" = 1 ] || essential+=("$!")
		wait_for sites "$sites_origin/health" 30 "$!" || late sites_panel
		if [ "$watch" = 1 ]; then
			prefixed sites-mfe watched "$SITES_DIR" --postpone -w frontend/src -w frontend/messages -- "cd frontend && npm run -s build" &
		fi
	fi
fi

if [ "$run_review_archive" = 1 ]; then
	if [ -n "${DEV_COMMAND_review_archive:-}" ]; then
		custom review_archive "$REVIEW_ARCHIVE_DIR" "$DEV_COMMAND_review_archive"
		wait_for review_archive "$review_archive_origin/health" 600 "$!" || late review_archive
	else
		say "starting review_archive on $review_archive_origin"
		mkdir -p "$REVIEW_ARCHIVE_DATA_DIR"
		flock "$REVIEW_ARCHIVE_DATA_DIR/serve.lock" sleep infinity &
		serving review_archive "$REVIEW_ARCHIVE_DIR" "cargo b -q -p review_archive_server" "review_archive serve --no-login-check" \
			-i 'crates/review_archive_web/**' -e rs,toml,sql,json,lock &
		[ "$dev" = 1 ] || essential+=("$!")
		wait_for review_archive "$review_archive_origin/health" 60 "$!" || late review_archive
		if [ "$watch" = 1 ]; then
			prefixed review_archive-mfe watched "$REVIEW_ARCHIVE_DIR" --postpone -w crates/review_archive_web -w crates/review_archive_client -w crates/review_archive_core -e rs,css,html,toml -- "$REVIEW_ARCHIVE_WEB_BUILD" &
		fi
	fi
fi

if [ "$run_playbook" = 1 ]; then
	# not watched, and not essential: /mcp lists no playbook tools without it
	say "starting playbook_web on $playbook_origin"
	prefixed playbook playbook &
	wait_for playbook "$playbook_origin/health" 60 "$!" || warn "playbook_web did not come up; /mcp lists no playbook tools"
fi

if [ "$run_placement" = 1 ]; then
	# not watched, and not essential: the map goes on without it; its first build takes minutes
	say "starting gmaps_optimal_placement on $placement_origin (the home map's studies)"
	# its settings' `queries_cache`, through their environment: `serve` takes no settings flag
	(cd "$PLACEMENT_DIR" && PANEL_ASSERTION_KEYS="$assertion_public" GMAPS_OPTIMAL_PLACEMENT_QUERIES_CACHE="$origin/api/queries_cache" \
		prefixed placement bash -c \
		"${DEV_COMMAND_gmaps_optimal_placement:-nix run .#conductor -- --port $PLACEMENT_PORT --countries examples/countries}") &
	placement_pid=$!
fi

if [ -n "$browsers_origin" ]; then
	say "starting the browsers' runner on $browsers_origin (Orbita, the Multilogin app, a phone's tools: nix build .#orbita .#mlx .#phone-tools)"
	BROWSERS_ORBITA="$(timed orbita nix build --no-link --print-out-paths "$root#orbita")/bin/orbita"
	BROWSERS_FFMPEG="$(timed ffmpeg nix build --no-link --print-out-paths nixpkgs#ffmpeg-headless.bin)/bin/ffmpeg"
	BROWSERS_MLX="$(timed mlx nix build --no-link --print-out-paths "$root#mlx")/bin/mlx-headless"
	phone_tools="$(timed phone-tools nix build --no-link --print-out-paths "$root#phone-tools")"
	BROWSERS_ADB="$phone_tools/adb" BROWSERS_BUSYBOX="$phone_tools/busybox" BROWSERS_ADB_KEYBOARD="$phone_tools/ADBKeyboard.apk"
	export BROWSERS_ORBITA BROWSERS_FFMPEG BROWSERS_MLX BROWSERS_ADB BROWSERS_BUSYBOX BROWSERS_ADB_KEYBOARD
	busy "$BROWSERS_PORT" && die "port $BROWSERS_PORT is taken: the browsers' runner's"
	[ "$watch" = 1 ] || [ "$fixed" = 1 ] || timed browsers in_shell "$root" bash -c "$own_cargo b -q -p sa_browsers" # under --watch, the conductor's build
	serving browsers "$root" "$own_cargo b -q $conductor_packages" "browsers serve" -e rs,toml,lock &
	essential+=("$!")
elif [ "$side" = 1 ]; then
	say "--port: no browsers' nor sandbox's runner, whose ports are the stack's on the usual ones"
elif [ -z "${GOLOGIN_TOKEN:-}${MULTILOGIN_TOKEN:-}" ]; then
	say "GOLOGIN_TOKEN and MULTILOGIN_TOKEN unset: no browsers' runner; the gmails' browsers say they are not connected"
fi

if [ -n "$sandbox_origin" ]; then
	say "starting the sandbox's runner on $sandbox_origin"
	busy "$SANDBOX_PORT" && die "port $SANDBOX_PORT is taken: the sandbox's runner's"
	[ "$watch" = 1 ] || [ "$fixed" = 1 ] || timed sandbox in_shell "$root" bash -c "$own_cargo b -q -p sa_sandbox" # under --watch, the conductor's build
	serving sandbox "$root" "$own_cargo b -q $conductor_packages" "sandbox serve" -e rs,toml,lock &
	essential+=("$!")
elif [ "$run_panel" = 1 ] && [ ! -d "$root/crates/sandbox/js/node_modules/@evinvest/api" ]; then
	say "crates/sandbox/js has no @evinvest/api (its _.js-local.json names the checkout until it is published): no sandbox, the agents have no execute"
fi

if [ "$run_panel" = 1 ]; then
	say "starting the conductor on $origin (dev sign-in: $role)"
	# panel.built's mtime: the start of the build serving now, so no source older than it is missing (e2e's stack.ts)
	serving panel "$root" "touch '$state/panel.building' && $own_cargo b -q $conductor_packages && mv '$state/panel.building' '$state/panel.built' || { mv '$state/panel.building' '$state/panel.failed'; false; }" "panel serve --bind 127.0.0.1:$PANEL_PORT" -i 'crates/gen/**' -e rs,toml,sql,lock &
	essential+=("$!")
	timed conductor-ready wait_for panel "$origin/health" 900 "$!" || die "the conductor did not start"
	# Dev sign-in's session, as the admin: the seeds are the conductor's own operations.
	if [ "$role" = admin ]; then
		say "seeding $state/panel.db over HTTP"
		seed_start="$(date +%s%3N)"
		printf 'seed\t%s\t\n' "$seed_start" >>"$STACK_TIMELINE"
		jar="$state/session.jar"
		rm -f "$jar"
		login="$(curl -fsS -o /dev/null -c "$jar" -w '%{redirect_url}' "$origin/auth/login?as=$DEV_EMAIL")"
		curl -fsS -o /dev/null -b "$jar" -c "$jar" "$origin${login#"$public_origin"}"
		csrf="$(awk '$6 == "sa_csrf" { print $7 }' "$jar")"
		[ -n "$csrf" ] || die "dev sign-in as $DEV_EMAIL gave no session"
		# `api <method> <path> [curl's arguments]`
		api() {
			local method="$1" path="$2"
			shift 2
			curl -fsS --fail-with-body -X "$method" -b "$jar" -H "x-sa-csrf: $csrf" -H 'content-type: application/json' "$origin/api/v1$path" "$@"
		}
		# Tokens and GMBs open with the import from review_archive: here, of nothing, into a new panel.db
		# (one copied in has its own).
		if [ "$fresh" = 1 ]; then
			api POST /admin/imports/review-archive -d '{"people": [], "gmails": [], "gmbs": [], "ledger": [], "kickbacks": []}' >/dev/null
		fi
		# The stack's own keys, `<brand>-local`: a copy of prod has prod's `<brand>-site`, whose secrets
		# are not here. A secret is shown once: kept in .local/secrets/<key id> for the next run. Its origin
		# is the landing's; the same origin again is no change, another one is refused.
		source_key() {
			local key_id="$1" brand="$2" landing="$3" file="$state/secrets/$1"
			if api GET /sources | jq -e --arg k "$key_id" '.sources[] | select(.key_id == $k and .revoked_at == null)' >/dev/null; then
				[ -s "$file" ] || die "source $key_id exists but its secret is not in $file — start over with --reset"
				api PUT "/sources/$key_id/origin" -d "$(jq -nc --arg o "$landing" '{origin: $o}')" >/dev/null || die "source $key_id has another origin than $landing — start over with --reset"
			else
				api POST /sources -d "$(jq -nc --arg k "$key_id" --arg b "$brand" --arg o "$landing" '{key_id: $k, kind: "site", brands: [$b], origin: $o}')" | jq -r .secret >"$file"
				[ -s "$file" ] || die "the source $key_id came with no secret"
			fi
		}
		# a landing with no checkout has no port, and no source either
		[ -z "$AQUAFIX_PORT" ] || source_key aquafix-local aquafix "http://localhost:$AQUAFIX_PORT"
		[ -z "$VIFNET_PORT" ] || source_key vifnet-local vifnet "http://localhost:$VIFNET_PORT"
		[ -z "$DEBOUCHAGE_PORT" ] || source_key debouchage-local debouchage "http://localhost:$DEBOUCHAGE_PORT"
		# What gmaps_optimal_placement's own work dir holds, into this conductor's queries cache, each answer
		# as of its file's time (a newer answer cached stays).
		placement_cached="${XDG_CACHE_HOME:-$HOME/.cache}/gmaps_optimal_placement/data/places_cache"
		if [ -d "$placement_cached" ]; then
			mapfile -d '' -t answers_files < <(find "$placement_cached" -maxdepth 1 -name '*.json' -print0)
			for ((i = 0; i < ${#answers_files[@]}; i += 500)); do
				batch=("${answers_files[@]:i:500}")
				jq -n --arg scheme gop --rawfile times <(stat -c '%n	%Y' "${batch[@]}") '
					($times | split("\n") | map(select(length > 0) | split("\t") | {(.[0]): (.[1] | tonumber)}) | add) as $at
					| {scheme: $scheme, answers: (reduce inputs as $answer ({};
						input_filename as $file | .[$file | split("/") | last | sub("\\.json$"; "")] = {answer: $answer, fetched_at: ($at[$file] | todate)}))}
				' "${batch[@]}" | api POST /admin/imports/queries-cache --data-binary @- >/dev/null
			done
		fi
		# sites' places, registered through the conductor's forward; those it knows stay as they are
		if [ "$run_sites" = 1 ] && [ -z "${DEV_COMMAND_sites_panel:-}" ]; then
			sites() {
				local method="$1" path="$2"
				shift 2
				curl -fsS --fail-with-body -X "$method" -b "$jar" -H "x-sa-csrf: $csrf" -H 'content-type: application/json' "$origin/api/sites$path" "$@"
			}
			known="$(sites GET /places | jq -c '[.places[] | [.brand, .location]]')"
			register() {
				jq -e --arg b "$1" --arg s "$2" 'any(. == [$b, $s])' <<<"$known" >/dev/null ||
					sites POST /places -d "$(jq -nc --arg b "$1" --arg s "$2" '{brand: $b, slug: $s}')" >/dev/null
			}
			for slug in "${AQUAFIX_PLACES[@]}"; do register aquafix "$slug"; done
			for slug in "${VIFNET_PLACES[@]}"; do register vifnet "$slug"; done
			for slug in "${DEBOUCHAGE_PLACES[@]}"; do register debouchage "$slug"; done
		fi
		printf 'seed\t%s\t%s\n' "$seed_start" "$(date +%s%3N)" >>"$STACK_TIMELINE"
	elif [ "$fresh" = 1 ]; then
		die "a new state is seeded as the admin: run it once without --role $role"
	fi
fi

if [ "$watch" = 1 ] && [ "$run_panel" = 1 ]; then
	wait "$web_prep_build" || die "the front end's packages or the map's overlay did not build"
	say "starting the front end's next dev on $public_origin"
	(
		cd "$root/frontend"
		# the map's overlay (crates/placement_overlay) again whenever it or the placement core it links changes
		prefixed placement-overlay watched "$root" --postpone -w crates/placement_overlay -w Cargo.lock -w .cargo/config.toml \
			${PLACEMENT_DIR:+-w "$PLACEMENT_DIR/gmaps_optimal_placement_core"} -- placement-overlay &
		PANEL_DEV_BACKEND="$origin" PANEL_DEV_DIST=".next${own_port:+/port-$own_port}" \
			prefixed web npx next dev --port "$WEB_DEV_PORT"
	) &
	web_pid=$!
	timed web-ready wait_for web "$public_origin/" 300 "$web_pid" || die "next dev did not start"
fi

if [ "$run_placement" = 1 ]; then
	wait_for placement "$placement_origin/health" 900 "$placement_pid" || warn "gmaps_optimal_placement did not come up; the map says placement is not connected"
fi

started=()
# A landing's own dev command (`nix run .#dev`, kitstart's next dev) in its checkout: its leads
# to the conductor's ingest, its places from sites_panel; its leads in a file of ours, analytics off.
start_site() {
	local name="$1" dir="$2" port="$3" key_id="$4"
	if [ ! -f "$dir/package.json" ] || [ ! -f "$dir/flake.nix" ]; then
		echo "skipping $name: $dir is not a kitstart checkout (missing, or older than the Next site — git pull)" >&2
		return 0
	fi
	say "starting $name from $dir on :$port"
	(
		cd "$dir"
		prefixed "$name" env -u POSTHOG_KEY -u POSTHOG_HOST -u LEADS_DB_URL \
			PORT="$port" \
			LEAD_WEBHOOK_URL="$origin/api/ingest/v1/events" \
			LEAD_WEBHOOK_KEY_ID="$key_id" \
			LEAD_WEBHOOK_SECRET="$(cat "$state/secrets/$key_id")" \
			SITES_PANEL_INTERNAL_URL="$sites_origin/api/internal" \
			LEADS_DB_PATH="$state/$name-leads.db" \
			nix run .#dev
	) &
	started+=("$name:$port:$!")
}
if [ "$sites" = 1 ]; then
	start_site aquafix "$AQUAFIX_DIR" "$AQUAFIX_PORT" aquafix-local
	start_site vifnet "$VIFNET_DIR" "$VIFNET_PORT" vifnet-local
	start_site debouchage "$DEBOUCHAGE_DIR" "$DEBOUCHAGE_PORT" debouchage-local
	for s in ${started[@]+"${started[@]}"}; do
		IFS=: read -r name port pid <<<"$s"
		# The first `nix run .#dev` may install node_modules: minutes, not seconds.
		wait_for "$name" "http://localhost:$port/health" 600 "$pid" || true
	done
fi

[ "$window" = 0 ] || xdg-open "$public_origin" >"$state/logs/browser.log" 2>&1

{
	echo
	echo "────────────────────────────────────────────────────────────────────────"
	echo " panel    $public_origin   (opens signed in: dev sign-in, $role)"
	echo "          leads $public_origin/sites/leads   places $public_origin/sites/places   connected sites $public_origin/sources"
	answers "$REVIEW_ARCHIVE_PORT" && echo "          review_archive $public_origin/review_archive   Grafana $public_origin/grafana/"
	answers "$SITES_PORT" && echo " sites    $sites_origin   (behind the conductor: its /api/internal for the landings)"
	for s in ${started[@]+"${started[@]}"}; do
		case "${s%%:*}" in
		aquafix)
			for slug in "${AQUAFIX_PLACES[@]}"; do echo " aquafix  http://$slug.localhost:$AQUAFIX_PORT/fr"; done
			;;
		vifnet) echo " vifnet   http://localhost:$VIFNET_PORT/fr" ;;
		debouchage) echo " debouchage http://localhost:$DEBOUCHAGE_PORT/fr" ;;
		esac
	done
	echo
	echo " state    $state   (--reset wipes it)   logs $state/logs/"
	[ "$watch" = 1 ] && echo " watch    every part is rebuilt and restarted as its checkout is edited"
	[ "$fixed" = 1 ] && echo " fixed    the conductor at $fixed_at, from $src: nothing is rebuilt"
	echo " stop     Ctrl-C stops what this run started"
	echo "────────────────────────────────────────────────────────────────────────"
} >&2

# Until Ctrl-C, or until one of the essential stops (then everything does). Polled rather than
# `wait -n`, which macOS's bash 3.2 lacks.
running() {
	local pid
	for pid in "${essential[@]}"; do kill -0 "$pid" 2>/dev/null || return 1; done
}
while running; do sleep 1; done
say "a part stopped — see $state/logs/"
