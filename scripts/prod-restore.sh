#!/usr/bin/env bash
# prod-restore <replica>=<path>...: each of prod's SQLite files, named as the cluster's litestream has them under /pvc
# (panel-data/panel.db, panel-data/queries_history.db, …), restored from its replica in R2 to <path>, as gitops main
# configures the replicas (GITOPS_DIR's checkout, fetched), with the R2 keys from DEVOPS_DIR's sops. A file already at
# <path> is replaced only once its restore is whole (docs/LOCAL.md, Prod's data).
set -euo pipefail
gitops="${GITOPS_DIR:?GITOPS_DIR: the gitops checkout, e.g. ~/s/ev/gitops}"
devops="${DEVOPS_DIR:?DEVOPS_DIR: the devops checkout, e.g. ~/s/ev/devops}"
[ "$#" -gt 0 ] || { echo "prod-restore <replica>=<path>..., e.g. panel-data/panel.db=.local/panel.db" >&2; exit 64; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# the replicas as gitops main configures them, not as an old checkout of it says
git -C "$gitops" fetch -q origin main
git -C "$gitops" show FETCH_HEAD:clusters/rpi5/tenants/service-arb/manifests.yaml |
	yq eval-all '[select(.kind == "ConfigMap" and .data["litestream.yml"])][0].data["litestream.yml"]' >"$work/litestream.yml"
unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY # the caller's would win over litestream's own variables
LITESTREAM_ACCESS_KEY_ID="$(sops -d --extract '["r2_access_key_id"]' "$devops/nix/secrets/host.json")"
LITESTREAM_SECRET_ACCESS_KEY="$(sops -d --extract '["r2_secret_access_key"]' "$devops/nix/secrets/host.json")"
export LITESTREAM_ACCESS_KEY_ID LITESTREAM_SECRET_ACCESS_KEY
pids=()
for pair in "$@"; do
	replica="${pair%%=*}"
	path="${pair#*=}"
	[ "$replica" != "$pair" ] && [ -n "$path" ] || { echo "prod-restore: $pair is not <replica>=<path>" >&2; exit 64; }
	db="/pvc/$replica" yq -e '.dbs[] | select(.path == strenv(db))' "$work/litestream.yml" >/dev/null ||
		{ echo "prod-restore: gitops main replicates no /pvc/$replica" >&2; exit 1; }
	mkdir -p "$(dirname "$path")"
	(
		part="$path.restoring"
		rm -f "$part" "$part-wal" "$part-shm"
		litestream restore -config "$work/litestream.yml" -o "$part" "/pvc/$replica"
		rm -f "$path-wal" "$path-shm" # the old copy's, which would be read as the new one's
		mv "$part" "$path"
		echo "prod-restore: /pvc/$replica → $path" >&2
	) &
	pids+=("$!")
done
failed=0
for p in "${pids[@]}"; do wait "$p" || failed=1; done
exit "$failed"
