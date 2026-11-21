#!/usr/bin/env bash
# queries-cache-sync <state>: gives prod's queries cache every version of an answer the conductor's files in <state>
# (panel.db, queries_history.db) hold and prod lacks, by `panel queries-cache` at both ends: prod's over ssh (PROD_SSH)
# and kubectl exec in its pod, as `panel migrate` is run there; here PANEL_BIN's (docs/LOCAL.md, Prod's data).
# What prod held at the last sync is kept in <state>, so a state holding nothing newer asks prod nothing.
set -euo pipefail
: "${PROD_SSH:?PROD_SSH: the host prod runs on, as in ~/.ssh/config (the flake sets it)}"
state="${1:?queries-cache-sync <state>: the dir holding panel.db and queries_history.db}"
panel="${PANEL_BIN:?PANEL_BIN: a conductor binary that opens <state>/panel.db}"
[ -e "$state/panel.db" ] || { echo "queries-cache-sync: no $state/panel.db" >&2; exit 66; }
known="$state/queries_cache.prod-index"
here() {
	# the caller's environment is not this conductor's: a production one would ask for the deploy's variables
	env -i PATH="$PATH" PANEL_DB_PATH="$state/panel.db" PANEL_HISTORY_DB_PATH="$state/queries_history.db" RUST_LOG=warn "$panel" queries-cache "$@"
}
prod() {
	ssh -o BatchMode=yes "$PROD_SSH" "sudo -n k3s kubectl -n service-arb exec -i deploy/panel -c panel -- /bin/panel queries-cache $1"
}
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
[ -e "$known" ] || : >"$known"
here index | LC_ALL=C sort >"$work/held"
if [ -z "$(LC_ALL=C sort "$known" | LC_ALL=C comm -23 "$work/held" -)" ]; then
	echo "queries-cache-sync: prod holds every answer $state does" >&2
	exit 0
fi
prod index >"$work/index"
here export --except "$work/index" >"$work/lacking"
if [ -s "$work/lacking" ]; then
	prod import <"$work/lacking"
fi
echo "queries-cache-sync: prod took $(wc -l <"$work/lacking") versions of answers from $state" >&2
here index >"$known" # prod held its index and took the rest: it holds every one of these now
