# `test-timeline [<nextest args>]` (devShell): the tests, and one timeline of their compile units and test runs
cd "$(git rev-parse --show-toplevel)"
t0=$(date +%s%6N)
timings=target/cargo-timings/cargo-timing.html
timeline=target/test-timeline.json
cfg=$(mktemp)
trap 'rm -f "$cfg"' EXIT
printf '[experimental]\nrecord = true\n[record]\nenabled = true\n' >"$cfg"
touch "$cfg" # every artifact this run leaves is newer than this

status=0
NEXTEST_EXPERIMENTAL_RECORD=1 cargo nextest run --workspace --timings --user-config-file "$cfg" "$@" || status=$?

[ "$timings" -nt "$cfg" ] || exit "$status" # cargo never started
build_start=$(date -u -d "$(grep -oP 'Build start:</td><td>\K[^<]+' "$timings")" +%s%6N)
awk '/const UNIT_DATA = \[/ { f = 1; sub(/.*= /, "") } f { print } f && /^\];$/ { exit }' "$timings" | sed 's/^\];$/]/' >"$cfg.units"
trap 'rm -f "$cfg" "$cfg.units" "$cfg.trace"' EXIT

trace='{"traceEvents": []}'
if NEXTEST_EXPERIMENTAL_RECORD=1 cargo nextest store export-chrome-trace latest -o "$cfg.trace" 2>/dev/null \
  && [ "$(jq '[.traceEvents[] | select(.name == "test run" and .ph == "B")][0].ts' "$cfg.trace" | cut -d. -f1)" -gt "$build_start" ]; then
  trace=$(cat "$cfg.trace")
fi # else the build failed and nothing ran: the compile units alone

jq --argjson t0 "$t0" --argjson start "$build_start" --slurpfile units "$cfg.units" '
  ($units[0] | map(select(.duration > 0)) | sort_by(.start)
    | reduce .[] as $u ({lanes: [], ev: []};
        ((.lanes | to_entries | map(select(.value <= $u.start)) | first.key) // (.lanes | length)) as $lane # Chrome traces need a lane without overlaps
        | .lanes[$lane] = $u.start + $u.duration
        | .ev += [{name: ($u.name + $u.target), cat: "compile", ph: "X", ts: ($start + $u.start * 1e6), dur: ($u.duration * 1e6), pid: 1, tid: $lane, args: {version: $u.version, features: $u.features}}])
    | .ev) as $compile
  | .traceEvents += [{name: "process_name", ph: "M", pid: 1, tid: 0, args: {name: "cargo build"}}, {name: "process_sort_index", ph: "M", pid: 1, tid: 0, args: {sort_index: -1}}]
    + [{name: "before the build: resolve, locks", cat: "compile", ph: "X", ts: $t0, dur: ($start - $t0), pid: 1, tid: -1}] + $compile
' <<<"$trace" >"$timeline"

echo
echo "compile, slowest units:"
jq -r 'map(select(.duration > 0)) | sort_by(-.duration)[:10][] | "  \(.duration)s\t\(.name)\(.target)"' "$cfg.units"
echo "tests, slowest:"
jq -r '[.traceEvents[] | select(.cat == "test")] | group_by([.pid, .tid, .name])[] | sort_by(.ts)
  | (.[0].args.binary_id) as $bin | . as $g | range(0; length; 2) | "  \(($g[. + 1].ts - $g[.].ts) / 1e6 * 100 | round / 100)s\t\($bin) \($g[.].name)"' "$timeline" \
  | sort -t$'\t' -k1 -g -r | sed -n 1,10p
echo "$(((build_start - t0) / 1000000))s before the build (resolve, cargo locks), build $(grep -oP 'Total time:</td><td>\K[^<]+' "$timings"), the timeline (open in ui.perfetto.dev): $PWD/$timeline"
exit "$status"
