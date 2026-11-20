# `test-timeline [<nextest args>]` (devShell): the tests, and one timeline of their compile units and test runs
cd "$(git rev-parse --show-toplevel)"
t0=$(date +%s%6N)
timings=target/cargo-timings/cargo-timing.html
timeline=target/test-timeline.json
cfg=$(mktemp)
trap 'rm -f "$cfg"' EXIT
printf '[experimental]\nrecord = true\n[record]\nenabled = true\n' >"$cfg"
touch "$cfg" # every artifact this run leaves is newer than this

spans=$PWD/target/spans.jsonl
: >"$spans"
status=0
SA_TRACE=$spans NEXTEST_EXPERIMENTAL_RECORD=1 cargo nextest run --workspace --timings --user-config-file "$cfg" "$@" || status=$?

[ "$timings" -nt "$cfg" ] || exit "$status" # cargo never started
build_start=$(date -u -d "$(grep -oP 'Build start:</td><td>\K[^<]+' "$timings")" +%s%6N)
awk '/const UNIT_DATA = \[/ { f = 1; sub(/.*= /, "") } f { print } f && /^\];$/ { exit }' "$timings" | sed 's/^\];$/]/' >"$cfg.units"
trap 'rm -f "$cfg" "$cfg.units" "$cfg.trace"' EXIT

trace='{"traceEvents": []}'
if NEXTEST_EXPERIMENTAL_RECORD=1 cargo nextest store export-chrome-trace latest -o "$cfg.trace" 2>/dev/null \
  && [ "$(jq '[.traceEvents[] | select(.name == "test run" and .ph == "B")][0].ts' "$cfg.trace" | cut -d. -f1)" -gt "$build_start" ]; then
  trace=$(cat "$cfg.trace")
fi # else the build failed and nothing ran: the compile units alone

jq --argjson t0 "$t0" --argjson start "$build_start" --slurpfile units "$cfg.units" --slurpfile spans "$spans" '
  ($units[0] | map(select(.duration > 0)) | sort_by(.start)
    | reduce .[] as $u ({lanes: [], ev: []};
        ((.lanes | to_entries | map(select(.value <= $u.start)) | first.key) // (.lanes | length)) as $lane # Chrome traces need a lane without overlaps
        | .lanes[$lane] = $u.start + $u.duration
        | .ev += [{name: ($u.name + $u.target), cat: "compile", ph: "X", ts: ($start + $u.start * 1e6), dur: ($u.duration * 1e6), pid: 1, tid: $lane, args: {version: $u.version, features: $u.features}}])
    | .ev) as $compile
  | .traceEvents += [{name: "process_name", ph: "M", pid: 1, tid: 0, args: {name: "cargo build"}}, {name: "process_sort_index", ph: "M", pid: 1, tid: 0, args: {sort_index: -1}}]
    + [{name: "before the build: resolve, locks", cat: "compile", ph: "X", ts: $t0, dur: ($start - $t0), pid: 1, tid: -1}] + $compile
    + ([.traceEvents[] | select(.name == "test run" and .ph == "B")][0].ts as $run | if $run then [{name: "until the first test: locks, build, listing", cat: "compile", ph: "X", ts: $t0, dur: ($run - $t0), pid: 1, tid: -2}] else [] end)
  | ([.traceEvents[] | select(.cat == "test" and .ph == "B")] | group_by([.args.binary_id, .args.test_name]) | map({key: (.[0].args | "\(.binary_id) \(.test_name)"), value: sort_by(.ts)}) | from_entries) as $tests
  | .traceEvents += [{name: "process_name", ph: "M", pid: 0, tid: 0, args: {name: "spans outside a test"}}]
    + ($spans | map(. as $s | ($tests["\($s.args.binary) \($s.args.test)"] // [] | map(select(.ts <= $s.ts)) | last) as $t # under its test, nested; a retry: the attempt it began in
      | if $t then .pid = $t.pid | .tid = $t.tid else .pid = 0 end))
' <<<"$trace" >"$timeline"

echo
echo "compile, slowest units:"
jq -r 'map(select(.duration > 0)) | sort_by(-.duration)[:10][] | "  \(.duration)s\t\(.name)\(.target)"' "$cfg.units"
echo "tests, slowest:"
jq -r '[.traceEvents[] | select(.cat == "test")] | group_by([.pid, .tid, .name])[] | sort_by(.ts)
  | (.[0].args.binary_id) as $bin | . as $g | range(0; length; 2) | "  \(($g[. + 1].ts - $g[.].ts) / 1e6 * 100 | round / 100)s\t\($bin) \($g[.].name)"' "$timeline" \
  | sort -t$'\t' -k1 -g -r | sed -n 1,10p
first=$(jq '[.traceEvents[] | select(.name == "until the first test: locks, build, listing")][0].dur // empty' "$timeline")
[ -z "$first" ] || echo "$((${first%.*} / 1000000))s until the first test ran: the build dir's lock (other checkouts' builds), the build, listing"
echo "spans, by total (SA_TRACE):"
jq -rs 'group_by(.name)[] | {name: .[0].name, n: length, total: (map(.dur) | add), max: (map(.dur) | max)} | "\(.total)\t\(.n)×\t max \(.max / 1e4 | round / 100)s\t\(.name)"' "$spans" \
  | sort -g -r | sed -n 1,10p | awk -F'\t' '{ printf "  %.2fs\t%s\t%s\t%s\n", $1 / 1e6, $2, $3, $4 }'
echo "$(((build_start - t0) / 1000000))s before the build (resolve, cargo locks), build $(grep -oP 'Total time:</td><td>\K[^<]+' "$timings"), the timeline (open in ui.perfetto.dev): $PWD/$timeline"
exit "$status"
