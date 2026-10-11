#!/bin/sh
# End-to-end check without onPC: the real plugin (stock Lua, stubbed console, behind udp_pipe_bridge.py)
# driven by the real Rust service. Needs: lua, python3, a release build of the service.
#   sh tools/ma3/test/e2e.sh
set -e
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
BIN="$ROOT/service/target/release/mtpnxk"
[ -x "$BIN" ] || (cd "$ROOT/service" && cargo build --release -q)
PORT=${PORT:-9817}
LOG="${TMPDIR:-/tmp}/mtpnxk-e2e-$$.log"
python3 "$ROOT/tools/ma3/test/udp_pipe_bridge.py" --port "$PORT" --plugin-arg "input=keyboard control=fake bench" --seconds 26 > "$LOG" 2>&1 &
BRIDGE=$!
sleep 1
fail=0
echo "--- scripted session (press, tap, release, unsupported key) ---"
OUT=$("$BIN" --plugin 127.0.0.1:$PORT sim --script "Record:down,5:tap@50,Record:up@300,Enter:tap,Clear:tap@200,Bank:tap@200" --seconds 3 2>&1 | grep -v '^led ')
echo "$OUT" | grep -q 'paired=true' || { echo "FAIL: not paired"; fail=1; }
echo "$OUT" | grep -q 'lost=0' || { echo "FAIL: events lost"; fail=1; }
echo "$OUT" | grep -q 'refused=2' || { echo "FAIL: expected the two Bank events refused"; fail=1; }
echo "$OUT" | grep -q 'rejected=0' || { echo "FAIL: plugin packets rejected"; fail=1; }
echo "--- control events (KB-18): rotary deltas and a push travel as ctl events; the stub console has no encoder bar, so the plugin refuses the slot targets honestly ---"
CTL=$("$BIN" --plugin 127.0.0.1:$PORT sim --script "rot1:+3@600,rot1:+2@2,btn1:tap@50" --seconds 3 2>&1 | grep -v '^led ')
echo "$CTL" | grep -q 'control: events=4 sent=4 coalesced=0 unbound=0 aged=0 stale=0 unsupported=0 refused=3 lost_reported=0 superseded=0 overflow=0' || { echo "FAIL: control counters: $(echo "$CTL" | grep '^control:')"; fail=1; }
echo "$CTL" | grep -q 'refused ev [0-9]*: \[target-unavailable\] the encoder bar context is unavailable' || { echo "FAIL: expected the slot target refused by the plugin"; fail=1; }
echo "$CTL" | grep -q 'control=fake' || { echo "FAIL: the welcome did not report control=fake"; fail=1; }
echo "$CTL" | grep -q 'lost=0' || { echo "FAIL: control events lost"; fail=1; }
echo "--- strips (KB-20): a simulated M-Touch strip touch, drag and lift travel as ctl events; the stub console has no encoder bar, so the touch and the motion are refused honestly and the lift is a noop ---"
STRIP=$("$BIN" --plugin 127.0.0.1:$PORT sim --script "strip1:t100@600,strip1:m110@30,strip1:m120@30,strip1:lift@50" --seconds 3 2>&1 | grep -v '^led ')
echo "$STRIP" | grep -q 'strips: mode=Relative reports=4 touches=1 lifts=1 detents=9 ' || { echo "FAIL: strip counters: $(echo "$STRIP" | grep '^strips:')"; fail=1; }
echo "$STRIP" | grep -q 'control: events=4 sent=4 coalesced=0 unbound=0 aged=0 stale=0 unsupported=0 refused=3 ' || { echo "FAIL: strip control counters: $(echo "$STRIP" | grep '^control:')"; fail=1; }
echo "$STRIP" | grep -q 'refused ev [0-9]*: \[target-unavailable\] the encoder bar context is unavailable' || { echo "FAIL: expected the strip's slot target refused by the plugin"; fail=1; }
echo "$STRIP" | grep -q 'lost=0' || { echo "FAIL: strip events lost"; fail=1; }
echo "--- playback banks (KB-21): the service binds its bank windows (bank 1, then bank 2 after PLAYBACK BANK +); a PFA press travels as a btn event on the executor's key; the stub console has no executors, so the plugin refuses it empty and the release is a noop ---"
BANK=$("$BIN" --plugin 127.0.0.1:$PORT sim --script "pbank:up@900,pfa1:down@400,pbank:down@50,pfa1:up@400,pb1:t100@50,pb1:m120@40,pb1:lift@50,mpa1:tap@100,go:tap@100" --seconds 4 2>&1 | grep -v '^led ')
echo "$BANK" | grep -q 'banks: follow page=1 mtouch=bank1(101-110) mplay-left=bank1(101-112) mplay-ext=bank1(401-424) held=0 changes=2 edge=0 downs=2 ups=2 frozen=1 ignored_faders=0 unmapped=2 faders: touched=0 touches=1 lifts=1 positions=0 pickups=0 waiting=1 resets=0 rebound=0 forced=0 dedicated=0' || { echo "FAIL: bank summary: $(echo "$BANK" | grep '^banks:')"; fail=1; }
echo "$BANK" | grep -q 'refused ev [0-9]*: \[target-unavailable\] executor 111 is empty' || { echo "FAIL: expected the PFA1 down on bank 2 (executor 111) refused empty by the plugin"; fail=1; }
echo "--- playback faders (KB-22): a touch on playback fader 1 is a hold on executor 201's fader element (refused empty by the stub console), the move waits for pickup (no level known), the lift is a noop; an M-Play pressure button is one down and one up (refused empty); the Go key without a profile is unmapped ---"
echo "$BANK" | grep -q 'refused ev [0-9]*: \[target-unavailable\] executor 201 is empty' || { echo "FAIL: expected the fader touch on executor 201 refused empty by the plugin"; fail=1; }
echo "$BANK" | grep -q 'refused ev [0-9]*: \[target-unavailable\] executor 101 is empty' || { echo "FAIL: expected the M-Play button on executor 101 refused empty by the plugin"; fail=1; }
echo "$BANK" | grep -q 'out MTouch display 4401 <- 1.2' || { echo "FAIL: expected the page display to show page 1 bank 2 (1.2): $(echo "$BANK" | grep '^out ')"; fail=1; }
echo "$BANK" | grep -q 'out MTouch display 4401 <- 1.1' || { echo "FAIL: expected the page display at page 1 bank 1 (1.1)"; fail=1; }
echo "$BANK" | grep -q 'lost=0' || { echo "FAIL: bank events lost"; fail=1; }
echo "--- lights (KB-23): the keys and bars are rendered from the context (every bound executor is empty on the stub console: dark), the textual state names each control, the writes are change-only ---"
echo "$BANK" | grep -q '^lights: enabled=true renders=[0-9]* flushes=[1-9]' || { echo "FAIL: lights summary: $(echo "$BANK" | grep '^lights:')"; fail=1; }
echo "$BANK" | grep -q 'light: M-Touch PFA1 ex101: empty, dark' || { echo "FAIL: expected PFA1's light described as empty and dark: $(echo "$BANK" | grep 'light: M-Touch PFA1')"; fail=1; }
echo "$BANK" | grep -q 'light: M-Touch PB1 ex201: empty, dark' || { echo "FAIL: expected PB1's bar described as empty and dark"; fail=1; }
echo "$BANK" | grep -q 'out MTouch led 4201 <- 0000' || { echo "FAIL: expected the PFA1 key written dark once"; fail=1; }
[ "$(echo "$BANK" | grep -c 'out MTouch led 4201 <- ')" -le 2 ] || { echo "FAIL: PFA1 written more than twice (bank 1 -> 2 -> 1): $(echo "$BANK" | grep -c 'out MTouch led 4201 <- ')"; fail=1; }
grep -q 'bind: 30 executor(s) following the console' "$LOG" || { echo "FAIL: the plugin did not log the service's bind (the simulated M-Touch alone: 30 executors)"; fail=1; }
echo "--- abandoned session (no bye): the plugin's lease must release the held key ---"
"$BIN" --plugin 127.0.0.1:$PORT sim --script "Record:down" --seconds 1 --abandon > /dev/null 2>&1
sleep 3.5
grep -q 'lease expired, its holds were released' "$LOG" || { echo "FAIL: no lease expiry in the plugin log"; fail=1; }
echo "--- bench ---"
BENCH=$("$BIN" --plugin 127.0.0.1:$PORT bench --taps 100 --rate 20 2>&1)
echo "$BENCH" | grep -E 'ack round trip|press-to-effect|^link:'
echo "$BENCH" | grep -q 'lost=0' || { echo "FAIL: bench lost events"; fail=1; }
echo "$BENCH" | grep -q 'press-to-effect (NUM taps, command line): n=90' || { echo "FAIL: expected 90 press-to-effect samples"; fail=1; }
kill $BRIDGE 2>/dev/null || true
wait $BRIDGE 2>/dev/null || true
echo "--- plugin log excerpts ---"
grep -E 'session .* (opened|closed)|lease expired|released by heartbeat|control enabled' "$LOG" | head -14
rm -f "$LOG"
[ $fail -eq 0 ] && echo "E2E PASSED" || { echo "E2E FAILED"; exit 1; }
