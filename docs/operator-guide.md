# Operator guide: setup, operation and recovery

For the person running an NX-K against grandMA3 onPC with this repository's
surface plugin and service. Qualified scope: [deployments.md](deployments.md)
(macOS, onPC 2.5.1.0, same machine, US layout, beta). Design and protocol:
[surface-protocol.md](surface-protocol.md).

## What you install

| Component | File(s) | Goes where |
| --- | --- | --- |
| Surface plugin entry component | `tools/ma3/mtpnxk_surface.lua` | the onPC user library plugins folder |
| Vendored console modules (three) | `tools/ma3/gma3_mcp_hardkeys.lua` (0.10.0), `tools/ma3/gma3_mcp_feedback.lua` (0.2.0) | same folder; they are components of the same plugin |
| Import wrapper | `tools/ma3/mtpnxk_surface.xml` | same folder |
| Surface service | `service/` → `mtpnxk` binary | the machine the NX-K is plugged into |

The plugin is one onPC plugin with four components. All four `.lua` files and
the XML must be copied together; the plugin checks the modules' versions at
start and refuses to run with a mismatch. Check the files against the hashes in
[tools/ma3/VENDOR.md](../tools/ma3/VENDOR.md) when packaging.

### 1. Build the service

```bash
cd service && cargo build --release
```

The binary is `service/target/release/mtpnxk`. `mtpnxk list` prints the USB
devices it can see; the NX-K is `11be:e102`.

### 2. Install the plugin in onPC

macOS plugins folder: `~/MALightingTechnology/gma3_library/datapools/plugins`.
Copy the four `.lua` files and the XML there, then in the onPC command line,
with a free Plugin pool slot (example: slot 2):

```text
Import Plugin Library "mtpnxk_surface.xml" At Plugin 2
```

Save the show to keep the imported plugin. The plugin must be started again
after every show load.

### Updating from protocol 1

Update the service and `mtpnxk_surface.lua` / XML together. Plugin 0.10.0 uses
protocol 2 (`MTX2`); older keyed clients and plugins are incompatible. Remove
`key=`, `bind=` and `allow=` from plugin start commands and remove `--key`,
`--key-file` or `MTPNXK_KEY` from service launch configuration. Key files are no
longer used; no replacement credential is needed. LAN connections are no longer supported.

Like the MCP bridge, the surface trusts processes on the local machine. The
session handshake establishes a connection and tracks state; it does not authenticate a user.

### 3. Start the plugin (normal case)

```text
Plugin "mtpnxk_surface"
```

The listener is fixed to `127.0.0.1` (port 9810 by default); run the service on the same
machine. No pairing key is needed. Defaults: input on
the keyboard backend, display 1. The start log (System Monitor / command-line
history) lists every NX-K key with the method and backend that would press it
(`key 1: NUM1 via shortcut (keyboard, shortcut-table)`), ending with
`keys: 32 supported, 12 unsupported` on the default US profile, then
`listening on 127.0.0.1:9810 (v0.10.0, protocol 2, gen …)`.

### 3a. Quickey dispatch (KB-15, the preferred path once set up)

```text
Plugin "mtpnxk_surface" "bank=900/1.178-189 input=mixed"
```

`bank=<quickey>/<page>.<first>-<last>` is the operator's decision to create show
objects: one Quickey per hardkey code with KB-10 evidence (nine on onPC 2.5.1:
`MA1 FIXTURE STORE NUM1 NUM5 THRU PLEASE OOPS CLEAR`, names `MCP <CODE>`, from
pool index 900) plus a code-less `MCP RESERVED` placeholder, and the executor
range reserved for holds (12 executors, one per concurrently held key; the
range must be empty, an unowned object anywhere in it refuses the whole setup
and nothing is created). The bank is verified at every start, kept as a record
across `stop`, re-verified by `bank verify`, shown by `bank status`, and removed
only by `bank teardown` (refused while a Quickey is held). The show must be
saved for the objects to survive a reload.

`input=mixed` presses the keys with hold evidence (`1`, `5`, `Thru`, `Enter`,
`Record`, `Clear`) as executor presses of their Quickeys and every other key
through the shortcut table as before; the start log names the part per key.
`input=quickey` uses Quickeys only (the other keys are reported unsupported).
Rules the module enforces, reported to the service as refusals and never
worked around: a Quickey route without a bank or for a code without evidence is
refused, nothing is typed instead; a shortcut-table key while a Quickey is held,
or the reverse, is refused (`unqualified-mix`); so is a second Quickey next to
one whose code has no chord evidence. `Thru` becomes usable this way on the
default profile (it has no shortcut row).

`route=<key>:<method>,…` overrides single keys (`quickkey` or `shortcut`); an
override the backend cannot serve, or that resolves to no usable route, refuses
the start instead of being accepted inert. `bankcodes=hardkeys` provisions every
command-area code (94 Quickeys) instead of the qualified nine.

Options, appended to the same quoted string:

| Option | Meaning |
| --- | --- |
| `port=9811` | another UDP port |
| `input=mixed`, `input=quickey` | Quickey dispatch through the bank (section 3a); need `bank=` once |
| `bank=900/1.178-189`, `bankcodes=…`, `route=…` | section 3a |
| `input=off` | feedback only: LEDs follow the console, no key is ever pressed |
| `input=fake` | lifecycle testing: events are recorded and acknowledged, no key is pressed |
| `bench` | report press-to-effect timing for digit taps (measurement only) |
| `force` | start even though the MCP bridge reports input enabled (see "Exclusive input") |

### 4. Start the service

With the NX-K plugged in:

```bash
./service/target/release/mtpnxk run
```

`--plugin 127.0.0.1:<port>` selects a different local plugin port (`localhost` is also accepted); remote addresses are refused;
`--verbose` prints pairing, refusals and losses; `--id` names the surface in the
plugin's log (default `nxk-<hostname>`). The service pairs within a frame of
starting, keeps pairing every 2 s while the plugin is down, and reopens the
keypad if it is unplugged and plugged back in.

Without hardware, `mtpnxk sim --script "Record:down,5:tap@50,Record:up@300"`
plays a key script and echoes the LED writes it would make.

Playback banks (KB-21): `--pb-page follow` (default) makes the M-Touch's ten
playback strips and the M-Play's two sections operate executors of the
console's current page; `--pb-page 3` binds them to page 3 whatever page the
console shows (the service never selects or creates a page). `--pb-rows
101,201,301` names the executor rows a strip's upper button, fader and lower
button mean (bank `k` adds `k x 10` for the M-Touch, `k x 12` for the M-Play's
left block); `--pb-ext-row 401` the M-Play's right block. `--mplay off` ignores
an M-Play; `--banks off` leaves the playback controls unmapped. The device's
`PLAYBACK BANK +/-` (M-Touch) and `FADER BANK` / `FADER EXT BANK` (M-Play) keys
move the banks; the page displays show page then bank (`12` = page 1, bank 2).

Playback faders and buttons (KB-22): with the plugin started `control=console`
a strip's buttons press and release the executor (`Press`/`Unpress Page P.E`:
the console runs whatever button function is configured on it; Temp, Flash,
Toggle, Top and Go+ are the qualified ones, anything else is refused with the
function named) and its fader places the executor's configured fader function
(Master and Temp qualified; Rate, Speed and the crossfades refused until
qualified). A fader never jumps: touch it and move it across the level the
executor is at (shown as a blinking yellow span on the strip's bar) and it
picks up; it picks up again after every touch, bank change, reconnect or a
level edited elsewhere (`--pb-pickup 0.02` is the tolerance). Startup reports
of the device never set a level. `--pb-go 201` / `--pb-pause 202` make the
`Next (Go)` and `Pause` keys press those executors' buttons (their configured
function, whatever it is); without them the keys do nothing. If an executor is
reassigned while its button is held, the release is kept as a record instead
of operating the new assignment: the plugin's `status` shows it and `recover`
releases it once the original object is back (or end that playback by hand).

Lights (KB-23): the M-Touch and M-Play keys and bars follow the console. A
playback strip's blue buttons are lit while their executor is assigned, blink
slowly while it runs and fast while pressed; the red/green button under a fader
(and every M-Play button) is red for assigned, green for running; the fader's
RGB bar shows the configured fader function's level in the executor's
appearance colour (reduced to red/green/blue/yellow/cyan/magenta/white; green
when the object has no appearance). The four parameter strips are blue-only:
their bar shows the slot's programmer value (so many of ten LEDs), the two end
LEDs blink when the selected fixtures hold different values, the bar is dark
when nothing is in the programmer, and the key is lit while the slot can be
adjusted. The colour a parameter stands for (red component, amber, UV shown as
violet) comes from the show's attribute definitions and is printed in the
`--verbose` `lights:` lines and `sim`'s `light:` lines; the blue strips cannot
show it. When the console state is stale (plugin stopped, link down) every lit
control switches to a blinking "unavailable" pattern instead of keeping a
level. `--lights off` disables all of this but the pickup span;
`--lights-interval 50` is the minimum spacing of writes in milliseconds.

### USB notes

- macOS: nothing to install. The keypad enumerates unconfigured; the service
  selects configuration 1 before claiming the interface.
- Windows: bind the **WinUSB** driver to the NX-K with Zadig before `run`
  (unqualified).
- Linux: a udev rule for `11be:e102` for non-root access (unqualified).

## Supported and unmapped keys (default US profile, onPC 2.5.1.0)

| Resolved (32) | How |
| --- | --- |
| `0`–`9`, `Enter`, `Record`, `Clear`, `Undo`, `Update`, `Edit`, `Copy`, `Move`, `Delete`, `Cue`, `Group`, `HighLight`, `Preview`, `Next`, `Last`, `Menu`, `Full`, `@` | the profile's shortcut table (Enter through the native Please route) |
| `+`, `-`, `.`, `/` | the keypad row of the shortcut table (`kpAdd`, `kpSubtract`, `kpDecimal`, `kpDivide`) |

| Unsupported (12) | Why | What you can do |
| --- | --- | --- |
| `Load`, `Macro`, `Thru` | no keyboard shortcut maps to them in the default profile | add a shortcut for `LOAD`, `MACRO`, `THRU` in your user profile; the plugin resolves them at its next start. KB-09 (MCP repository) plans a guided way to do this |
| `Fade`, `Delay`, `Snap Shot`, `Back` | not grandMA3 hardkeys (not in `Enums.VirtualKeyCode`) | nothing; they stay unsupported |
| `Bank`, `Swap Prog`, `Link` | not console keys: Bank is the rotaries' fine modifier (held), Link is the link LED | nothing; Bank only modifies the rotaries |

The four rotaries are not keys: with the plugin started with `control=console` (KB-19) a detent on rotary n adjusts
encoder slot n of the console's encoder bar for the current selection (`--rotary-slots 2` maps them to slots 2–5),
one detent being one console encoder click, Bank held a tenth of a click; a push does nothing (not qualified) and is
acknowledged `unsupported`. Without `control=console` (or with `control=fake`) nothing moves on the console; the
plugin's `status` and the service's summary line say which backend is active and why an event was refused (no
selection, the slot is unavailable for the selection, an unqualified readout or an editor context).

With an M-Touch attached as well (KB-20, `mtpnxk run` opens it next to the NX-K), its four right-hand base-channel strips
are the same four encoder slots (`--strip-slots` as `--rotary-slots`): touch a strip and drag; the value follows the
movement from where it was (no jump to the touched spot), a full stroke is one attribute range, lift and touch again to
keep going; the strip's own key held (or Bank on the NX-K) is a tenth of a click. `--strips absolute` makes the strips
place values instead, but only after the strip passes the slot's current value (pickup) or when the strip's key is held
as you touch (takeover, which also lets a mixed selection be set to one value); `--strips off` ignores them.

The plugin never guesses a route: a key that does not resolve is reported at
start and every event for it is acknowledged `unsupported`.

## Exclusive input

The surface plugin and the MCP bridge both inject into the same console
keyboard; neither can stop the other or a physical operator. Run only one of
them with input enabled at a time. The plugin refuses to enable input while the
bridge reports input enabled and says so in its log; stop the bridge's input (or
start the bridge without input) rather than reaching for `force`. `force` is for
measurement sessions where the bridge is known to hold no keys, as in the
qualification records.

## Status and stop

```text
Plugin "mtpnxk_surface" "status"
Plugin "mtpnxk_surface" "stop"
```

`status` prints running state, binding, input mode, generation, every session
with its held keys, counters, and any unresolved release records; since KB-21
also `binding: N executor(s) [...] following the console's page | on page P
(independent) revision=R generation=G` (what the service bound) and each held
control gesture's frozen target. `stop`
releases every key the plugin holds, closes the sessions and keeps any release
it could not confirm as a record for the next start. The service notices a
stopped plugin within 1.5 s: the Link LED blinks and every other LED goes off
(unknown), and it pairs again on its own when the plugin comes back.

## LEDs

The M-Touch / M-Play lights are described above (KB-23): there, "unknown" is a
distinct blinking pattern on anything that was lit, and dark means empty or
unassigned. On the NX-K:

Off means "not active" **or** "unknown": the NX-K has single-colour LEDs. The
Link LED carries the difference: off while paired with fresh console state,
slow blink while unpaired, link down or state stale. Keyword keys (Record,
Update, Edit, …) follow the pending command-line keyword, HighLight blinks while
highlight mode is on, Clear/Undo/Next/Last/Menu light while held, Bank lights
itself and the four encoder LEDs while held.

## Recovery

A release the console could not confirm (the module refused it, raised, or the
key's route changed while it was held) is kept as a **record**. Records survive
`stop`, a loop error and the console's Cleanup; the next start **adopts** them
before enabling input and keeps the affected key reserved, so a new press of it
is refused with `conflict` until you act.

1. Read the start log or `status`: adopted records are listed
   (`adopted unresolved record …: its key stays reserved until "recover"
   releases it`).
2. Check the console: if the key is visibly stuck, press and release it on the
   physical keyboard once.
3. Run

   ```text
   Plugin "mtpnxk_surface" "recover"
   ```

   It re-attempts every unresolved release through the current backend (attached
   for cleanup only when input is off) and logs
   `recover: N released, M still unresolved, K record(s) not adopted`.

4. **If something stays unresolved**: the log names the key, the session and the
   module's error. Common causes are shortcuts turned off (`shortcutsActive=0`
   on the Link LED pattern; turn ShCuts back on and run `recover` again), a
   profile or user change that removed the key's shortcut (restore it, or press
   the key physically), or the module raising. A record the module rejects at
   adoption is kept for the next start and listed by `status`; it is not
   dropped. Nothing is ever retried automatically and no press is replayed.
5. If `stop` itself reported `dispose failed … the instance is quarantined`,
   input stays blocked at the next start until `recover` exports the
   quarantined records; the same command then re-enables the input mode you
   asked for at start.

After a show load the plugin is not running; start it again and the kept
records (if any) are adopted as above.

## Updating

1. Copy the new `.lua` files and the XML into the plugins folder (all three
   `.lua` files, even if only one changed: the versions are checked together).
2. In onPC, with the plugin in slot 2:

   ```text
   Plugin "mtpnxk_surface" "stop"
   Delete Plugin 2 /NoConfirmation
   Import Plugin Library "mtpnxk_surface.xml" At Plugin 2
   ReloadAllPlugins
   Plugin "mtpnxk_surface"
   ```

   `ReloadAllPlugins` is needed on 2.5.1: delete and re-import alone can keep
   the cached Lua running (MCP KB-02). It reloads every plugin, so pick the
   moment. The reload kills the running loop without its shutdown; since 0.2.0
   the next start notices the stale run (`has not ticked for …; taking over its
   state`), releases its holds through the old module instances, keeps their
   records and the bank record, and continues. Check the version in the
   `listening on …` line, then save the show.
3. Rebuild the service (`cargo build --release`) and restart `run`. The service
   logs the plugin's module versions in `paired: …` and in `welcome.modules`.
4. Re-vendoring the console modules is described in
   [tools/ma3/VENDOR.md](../tools/ma3/VENDOR.md); never edit them in place.
