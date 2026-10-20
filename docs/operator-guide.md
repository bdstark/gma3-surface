# Operator guide: setup, operation and recovery

For the person running an NX-K against grandMA3 onPC with this repository's
surface plugin and service. Qualified scope: [deployments.md](deployments.md)
(macOS, onPC 2.5.1.0, same machine, US layout, beta). Design and protocol:
[surface-protocol.md](surface-protocol.md).

## What you install

| Component | File(s) | Goes where |
| --- | --- | --- |
| Surface plugin entry component | `tools/ma3/mtpnxk_surface.lua` | the onPC user library plugins folder |
| Vendored console modules (three) | `tools/ma3/gma3_mcp_hardkeys.lua` (0.12.0), `tools/ma3/gma3_mcp_feedback.lua` (0.2.0) | same folder; they are components of the same plugin |
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
`listening on 127.0.0.1:9810 (v0.14.0, protocol 2, gen …)`.

### 3a. Quickey dispatch (KB-15, the preferred path once set up)

```text
Plugin "mtpnxk_surface" "bank=900/1.178-189 input=mixed"
```

`bank=<quickey>/<page>.<first>-<last>` is the operator's decision to create show
objects: one Quickey per hardkey code with KB-10/KB-28 evidence (33 on onPC 2.5.1 with
hardkeys 0.11.0: `MA1 FIXTURE STORE PLEASE OOPS CLEAR THRU`, the digits `NUM0`–`NUM9`,
`PLUS MINUS DOT SLASH AT FULL UPDATE EDIT COPY MOVE DELETE CUE GROUP MACRO PREVIEW
HIGHLIGHT`, names `MCP <CODE>`, from pool index 900, so 900–932) plus a code-less
`MCP RESERVED` placeholder (933), and the executor
range reserved for holds (12 executors, one per concurrently held key; the
range must be empty, an unowned object anywhere in it refuses the whole setup
and nothing is created). The bank is verified at every start, kept as a record
across `stop`, re-verified by `bank verify`, shown by `bank status`, and removed
only by `bank teardown` (refused while a Quickey is held). The show must be
saved for the objects to survive a reload.

`input=mixed` presses every key with hold evidence (plugin 0.12.0: the digits,
`+ - . /`, `@`, `Thru`, `Full`, `Enter`, `Record`, `Clear`, `Update`, `Edit`,
`Copy`, `Move`, `Delete`, `Cue`, `Group`, `Macro`, `Preview`, `HighLight`) as
executor presses of their Quickeys, and `Undo`, `Next`, `Last` and `Menu` through
the shortcut table; the start log names the part per key. Quickeys reach pop-ups
and dialogs (the Edit Command pop-up, an encoder's calculator), the shortcut table
does not. At most one key goes down per plugin tick: Quickey keys pressed in one
console frame reorder in a focused text field (KB-28), so a burst is spread over
a few ticks (a fast burst can make the service retransmit an unacknowledged
press; the duplicate is recognised and dropped). `@` inserts nothing in the Edit
Command pop-up (a console behaviour, KB-28).

Direct taps (plugin 0.13.0, KB-29): a key whose code has direct-tap evidence (every
Quickey key of the NX-K, `Undo` included) goes down as one `Press Quickey N`: no
executor is used, nothing is held, and the key-up is only acknowledged. The start
log ends those lines with `, direct tap`, and the command history shows one line
per key. `taps=off` holds every key instead (press/release through an executor,
as in 0.12.0; `Undo` then goes back to the shortcut table). With held keys, a
Quickey key that goes down while another held Quickey key is still down is refused
when either is one of the KB-28 codes (digits, `+ - . /`, `@`, `Thru` and the
keywords): the module qualified those pairs only inside a combo, so let the previous
key come up first. Direct taps hold nothing, so the limit does not apply to them.

Static executors (`static` next to `bank=...`, KB-29): every code of the bank gets
its own executor, assigned once when the bank is created, so a held key needs no
Assign. The range must cover the codes (33 with hardkeys 0.12.0), for example
`Plugin "mtpnxk_surface" "bank=940/1.301-333 static input=mixed"`. Keep it clear of
show executors (an executor in the range that holds anything else refuses the whole
setup). With direct taps the NX-K holds nothing, so the executors only matter for
`taps=off` and for other consumers of the module.
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
command-area code (94 Quickeys) instead of the qualified 33.

Upgrading from plugin 0.11.0 (hardkeys 0.10.0): the bank kept from the older
version has only the nine KB-10 codes. It is adopted and verified as before, the
start log names the qualified codes it lacks, and their keys are refused until the
bank is provisioned again: `Plugin "mtpnxk_surface" "bank teardown"`, then start
with `bank=...` (the Quickey range must have room for 34 objects).

Options, appended to the same quoted string:

| Option | Meaning |
| --- | --- |
| `port=9811` | another UDP port |
| `input=mixed`, `input=quickey` | Quickey dispatch through the bank (section 3a); need `bank=` once |
| `bank=900/1.178-189`, `bankcodes=…`, `route=…` | section 3a |
| `input=off` | feedback only: LEDs follow the console, no key is ever pressed |
| `input=fake` | lifecycle testing: events are recorded and acknowledged, no key is pressed |
| `encmacro=950` | Swap Prog enters the encoder bank (feature group) on the command line, through Macro 950 (section 3b) |
| `bench` | report press-to-effect timing for digit taps (measurement only) |
| `force` | start even though the MCP bridge reports input enabled (see "Exclusive input") |

### 3b. Changing the feature group from the panel (Swap Prog, KB-30)

```text
Plugin "mtpnxk_surface" "bank=900/1.178-189 input=mixed control=console encmacro=950"
```

The feature group the encoder bar shows is its encoder bank: `Dimmer`, `Position`, `Gobo`, `Color`, `Beam`, `Focus`,
`Control`, … numbered in the order of your user profile's encoder bar (1 Dimmer, 2 Position, 3 Gobo, 4 Color, 5 Beam,
6 Focus in the default bar). With `encmacro=<n>`, **Swap Prog** puts `Select EncoderBank ` on the command line; type
the bank's number on the keypad and press Enter. `Swap Prog 4 Enter` shows Color, one-handed. The rotaries follow on
their own: the service reads the encoder bar back, and motion produced against the old bank is refused, not applied
to the new one. Swap Prog's LED is lit while `Select EncoderBank` waits on the command line.

- Text already on the command line is cleared first. Only the text goes: the selection and the programmer are not
  touched. `Clear`, or `Enter` without a number, leaves the bar as it is.
- A bank comes back on the page it was last on, as clicking its tab does.
- A bank the current selection has no attributes for is refused by the console (`Not allowed`, for example
  Position with only LED washes selected). With nothing selected, every bank that has pages is offered. A bank
  without pages (Shapers and Video in the default bar) is always `Not allowed`.
- `bank.page` (`Swap Prog 4 . 2 Enter`) also works, but the console does not check it. With a selection that has
  nothing for that bank it answers OK and changes nothing, and a page the selection hides is replaced by the first
  page. Prefer the bank number alone.
- `encmacro=<n>` is the operator's decision to create one show object: Macro n, named `mtpnxk EncoderBank`, with one
  line `Select EncoderBank` (Execute No), marked as the plugin's in its note. An empty number gets the macro, later
  starts adopt the plugin's own, and any other macro at that number is reported and never changed (Swap Prog then
  stays unsupported). Save the show for the macro to survive a reload. Every press checks the macro again, because a
  show load can put another show's Macro n there: an edited, foreign or missing macro is refused and nothing runs.
- `Plugin "mtpnxk_surface" "encmacro status"` checks the macro, and `"encmacro teardown"` deletes it, but only while it
  is still exactly what the plugin created. A marked macro you edited (its line, Execute, another line) is yours: it is
  refused and kept. Without `encmacro=`, Swap Prog stays a local key and is acknowledged `unsupported`.
- Lua cannot write the command line, so the macro is how the keyword gets there: the command history shows one
  `Go+ Macro n` line per press. Keys pressed right after Swap Prog wait until the command line shows the keyword (at
  most 0.4 s; 50–70 ms on onPC 2.5.1), so a fast `Swap Prog 4 Enter` still arrives in order. If the keyword has not
  appeared by then, the log says so and the keys go on to whatever the command line holds.
- With `input=off` or `input=fake`, Swap Prog is refused and the command line is left alone.

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
`Go` and `Pause` keys press those executors' buttons (their configured
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
| `Bank`, `Swap Prog`, `Link` | not console keys: Bank is the rotaries' fine modifier (held), Link is the link LED | nothing for Bank and Link; Swap Prog enters the encoder bank with `encmacro=<n>` (section 3b, then it is a supported key) |

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
itself and the four encoder LEDs while held. Swap Prog lights while
`Select EncoderBank` is pending on the command line (KB-30, section 3b).

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
