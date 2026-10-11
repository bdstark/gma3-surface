# Device profiles: the default layouts (KB-24)

Updated 2026-10-10. What every physical control of the three supported surfaces does when the service runs
with its defaults, how the playback banks are changed, which flags move a role, and what is deliberately
unmapped. A "profile" here is the service's fixed mapping plus its command-line flags: there are no profile
files, and nothing on the console is created or changed to make a layout work (the console's own executor
assignments, pages, encoder bank and user profile decide what the controls reach).

The console side of every role is the vendored module set (hardkeys 0.10.0, feedback 0.6.0, control 0.5.0;
[tools/ma3/VENDOR.md](../tools/ma3/VENDOR.md)) behind the `mtpnxk_surface` plugin (0.11.0). A key resolves only
when the user profile's shortcut table has it (the welcome's `unsupported` list names the rest); an executor
control reaches only what the console has assigned at that number on the bound page; a parameter control reaches
only the encoder slots the console shows for the current selection and encoder bank. None of this is grandMA3
hardware equivalence: the controls are translated into console commands and read back from console state, with
the limits listed per device below and in [deployments.md](deployments.md).

Status of the live qualification per control is in [qualification-matrix.md](qualification-matrix.md): what
follows is the mapping, not a claim that each row ran on hardware.

## NX-K (VID `11be`, PID `e102`)

Four rotaries with push, 42 keys, single-colour LEDs under most keys. Roles: `service/src/nxk/mod.rs`
(decoder), `tools/ma3/mtpnxk_surface.lua` `NXK_KEYS` (the hardkey each key means), `service/src/leds.rs`
(what each LED shows), `service/src/main.rs` (rotaries, Bank).

### Keys

| Physical key | Default role (console hardkey) | LED |
| --- | --- | --- |
| `0` … `9` | `NUM0` … `NUM9` | off |
| `.` | `DOT` (keypad row preferred) | off |
| `/` | `SLASH` (keypad row preferred) | off |
| `-` | `MINUS` (keypad row preferred) | off |
| `+` | `PLUS` (keypad row preferred) | off |
| `Enter` | `PLEASE` | off |
| `@` | `AT` | off |
| `Thru` | `THRU` | off |
| `Full` | `FULL` | off |
| `Record` | `STORE` | on while the command line's pending keyword is Store |
| `Update` | `UPDATE` | on while pending is Update |
| `Edit` | `EDIT` | on while pending is Edit |
| `Copy` | `COPY` | on while pending is Copy |
| `Move` | `MOVE` | on while pending is Move |
| `Delete` | `DELETE` | on while pending is Delete |
| `Load` | `LOAD` | on while pending is Load |
| `Cue` | `CUE` | on while pending is Cue |
| `Group` | `GROUP` | on while pending is Group |
| `Macro` | `MACRO` | on while pending is Macro |
| `Preview` | `PREVIEW` | on while the console is in preview |
| `Menu` | `MENU` | on while held (local echo) |
| `HighLight` | `HIGHLIGHT` | blinks while highlight is on |
| `Next` | `NEXT` | on while held (local echo) |
| `Last` | `PREV` | on while held (local echo) |
| `Undo` | `OOPS` | on while held (local echo) |
| `Clear` | `CLEAR` | on while held (local echo) |
| `Fade` | `FADE` is sent, but 2.5.1 has no such hardkey: the welcome reports it unsupported, a press does nothing | on while pending is Fade (never, on 2.5.1) |
| `Delay` | `DELAY`: as Fade, unsupported on 2.5.1 | as Fade |
| `Snap Shot` | **unmapped** (no grandMA3 hardkey; the plugin answers `bad-key`) | on while held (local echo) |
| `Back` | **unmapped** (no grandMA3 hardkey; `bad-key`) | off |
| `Bank` | **local modifier**: held, the rotaries and the M-Touch parameter strips move at the console's Fine resolution (a tenth of a click). Never sent to the console (acked "not a console key") | on while held, with all four encoder LEDs |
| `Swap Prog` | **unmapped** (local, nothing bound) | off |
| `Link` | **unmapped** key; its LED is the link indicator | off while console feedback is fresh, blinking while the link is down, unpaired or the state is stale |

Which of these resolve depends on the user profile: on the `Default` US profile of the test show 32 keys
resolve ([deployments.md](deployments.md), "Keyboard layout"). Six keys are quickkey-held on the console
(`NUM1`, `NUM5`, `THRU`, `PLEASE`, `CLEAR`, `STORE`): the service holds them for as long as the physical key is
down, up to the plugin's hold limit (12 simultaneous, 60 s).

### Rotaries

| Physical control | Default role | Flag |
| --- | --- | --- |
| Rotary 1 … 4, turned | encoder slot `base + n - 1` of the console's current encoder bank, one report unit = one console click (Coarse; Fine while `Bank` is held). The NX-K encoders are continuous, without physical detents: a "detent" in the service and the console calibration is one report unit of the device (the operator qualified the speed, [hardware record](probes/kb-24-hardware-nxk-macos.md)). Sent as relative motion; nothing is accelerated | `--rotary-slots <base>` (default 1: slots 1–4) |
| Rotary 1 … 4, pushed | a button boundary on the same slot; the console backend refuses encoder presses as `unsupported` (not qualified), the service reports the refusal | — |
| Encoder LEDs | all four on while `Bank` is held | — |

The slots follow the console: the encoder bank (feature group) and the page the console shows for the current
selection are read back (`context`), and the surface's motion is refused `stale-generation` when it was produced
against an older context. Slots that are empty, phaser/editor slots (`other`) and slots of a non-Absolute layer
are refused by the console backend and reported.

## M-Touch (PID `f808`)

Ten playback strips, a playback bank pair, a page display, eleven command keys, ten multi-function keys, four
base-channel strips and their mode key. Roles: `service/src/banks.rs` (playback), `service/src/strips.rs`
(parameter strips), `service/src/lights.rs` (what each light shows). Control ids are the hardware's
(`docs/mtouch-protocol-reuse.md`).

### Playback section (default rows 101 / 201 / 301, bank 1)

| Physical control | Id | Default role | Light |
| --- | --- | --- | --- |
| `PLAYBACK BANK +` / `-` | `0x4111` / `0x4113` | next / previous bank of the ten-strip window (banks 1–9: executors `x01`–`x90` of each row) | — (no LED) |
| Page display | `0x4401` | the console's current executor page then the bank: `12` is page 1, bank 2; blank while the page is unknown | — |
| `PFA n` (top key, strip n) | `0x42n1` | executor key **row A**: executor `101 + 10·(bank-1) + n-1` on the bound page, pressed with its configured button function (Go+, Flash, Temp, Toggle, Top qualified; others refused `unsupported`) | blue: on = assigned, slow blink = running, fast blink = pressed by this surface |
| `LEVEL PLAYBACK n` (fader) | `0x42n3` | executor **fader row**: executor `201 + 10·(bank-1) + n-1`, its configured fader function (Master, Temp qualified; others refused). A touch is a hold; the move waits for pickup within `--pb-pickup` of the executor's level, then follows | RGB bar: the executor's level in its appearance colour; the unavailable pattern while feedback is stale |
| `PFB n` (lower key) | `0x42n2` | executor key **row B**: `301 + 10·(bank-1) + n-1` | blue, as PFA |
| `PFD n` (pressure key) | `0x42n5` | **unmapped** | red/green: lit for the fader-row executor (green while it runs) |

Bank arithmetic (`banks.rs`): a section of width *w* on a row base *r* maps strip *n* at bank index *b* (0-based)
to executor `r + w·b + n - 1`; the highest bank keeps the window inside the row's hundred (`90 / w - 1`: 8 for
the M-Touch, so banks 1–9 reach `x01`–`x90`). A bank change rebinds the window at the plugin (`bind`), and the
lights stay unavailable until the context of that binding arrives; a held key or touched fader keeps the target
it resolved at its down ("frozen") across the change.

### Command keys and others

| Physical control | Id | Default role |
| --- | --- | --- |
| `Go` | `0x5513` | presses the executor named by `--pb-go <n>` (its configured key function); **unmapped** without the flag |
| `Pause` (`||` / Back) | `0x5512` | presses the executor named by `--pb-pause <n>`; **unmapped** without the flag |
| `EDIT`, `CLEAR`, `RECORD`, `UPDATE`, `LOAD` | `0x5101`, `0x5103`, `0x5401`, `0x5402`, `0x5411` | **unmapped** (the NX-K carries the command keys) |
| `SELECT`, `RELEASE`, `BEAT`, `SNAP` | `0x5502`–`0x5504`, `0x5511` | **unmapped** |
| `HIGHLIGHT` | `0x6001` | **unmapped** |
| `BROWSE PREVIOUS` / `NEXT` | `0x6401` / `0x6402` | **unmapped** |
| `MF 1` … `MF 10`, mode LEDs | `0x5801`–`0x580A`, `0x5812`–`0x5815` | **unmapped** (pressure keys; the LEDs are not driven) |
| `BASE CHANNEL GROUP Mode` | `0x6107` | **unmapped** |
| Backlight | `0x7110` | not driven |

### Parameter strips (base channel group)

| Physical control | Id | Default role | Flag |
| --- | --- | --- | --- |
| Base channel fader `n` (1–4) | `0x61n2` | encoder slot `base + n - 1` of the console's current encoder bank. **Relative** (default): a touch holds the slot, the drag moves it by detents (`--strip-travel` detents over the full travel, 120 by default: one travel = one console turn range at Coarse), the lift ends the hold. **Absolute**: the position is written over the slot's verified travel once the fader has picked up the console's value | `--strips relative\|absolute\|off`, `--strip-slots <base>` (default 1), `--strip-travel <detents>` |
| Base channel key `n` | `0x61n1` | relative mode: Fine while held (as `Bank`). Absolute mode: takeover (write the position without pickup, including onto a mixed selection) | — |
| Blue bar `n` | (fader LEDs) | the slot's value: *n* of ten LEDs for a single completely read value; a blinking level for an incompletely read value; the two end LEDs blinking for mixed values; dark for empty, none, unavailable or no selection. The bar cannot show the parameter's colour (blue-only hardware); the colour is textual (`sim` / `--verbose` `light:` lines) | — |
| Blue key `n` LED | | on while the slot is adjustable, fast blink while touched | — |

The four strips and the NX-K's four rotaries both follow the console's encoder context: with the defaults they
address the same slots 1–4 (the first four encoders of the bank the console shows); `--rotary-slots 5` or
`--strip-slots 5` moves one set to slots 5–8 where the bank has them. Writes from either are the same
`Attribute "<name>" At …` on the selection, read back into both.

## M-Play (PID `f80c`)

Twelve faders with two button rows, a right block of 24 buttons, two displays with page keys, six command keys.
Roles: `service/src/banks.rs` (sections `mplay-left` and `mplay-ext`).

| Physical control | Id | Default role | Light |
| --- | --- | --- | --- |
| Left display | `0x9F01` | page then bank of the left (fader) section | — |
| Left `PageUp` / `PageDown` | `0x9F12` / `0x9F13` | next / previous bank of the left section (twelve wide: banks 1–7, executors `x01`–`x84`) | — |
| `FADER LEVEL n` (1–12) | `0x92n3`* | executor **fader row**: `201 + 12·(bank-1) + n-1`, as the M-Touch's playback faders (pickup, hold, configured function) | RGB bar: level in the appearance colour |
| `FADER PFD n`, top button (1–12) | `0x9205 + 0x10·(n-1)` | executor key **row A**: `101 + 12·(bank-1) + n-1` (pressure key: one down at the first touched report, one up at the release) | red/green: assigned / running |
| `FADER PFD n`, bottom button (13–24) | `0x92C5 + 0x10·(n-13)` | executor key **row B**: `301 + 12·(bank-1) + n-13` | red/green |
| Right display | `0x9E01` | page then bank of the right (ext) section | — |
| Right `PageUp` / `PageDown` | `0x9E12` / `0x9E13` | next / previous bank of the right section (24 wide: banks 1–3, executors `x01`–`x72`) | — |
| `FADER EXT PF 1` … `12` | `0x9805 + 0x10·(n-1)` | executor keys of the **ext row**: `401 + 24·(bank-1) + n-1` | red/green |
| `FADER EXT PF 13` … `24` | `0x98C5 + 0x10·(n-13)` | `401 + 24·(bank-1) + n-1` (continues the same window) | red/green |
| `Go` / `Pause` | `0x5513` / `0x5512` | `--pb-go` / `--pb-pause` executors, as on the M-Touch; **unmapped** without the flags | — |
| `SELECT`, `RELEASE`, `BEAT`, `SNAP` | `0x5502`–`0x5504`, `0x5511` | **unmapped** | — |
| Backlight | `0x7110` | not driven | — |

\* `0x9203 + 0x10·(n-1)`.

**Optional parameter strips on the M-Play: not implemented.** The acceptance of KB-24 allows explicitly selected
M-Play faders to act as parameter strips; the service has no flag for it yet, every M-Play fader is a playback
fader, and the RGB bars that would show the parameter colour scheme are used for levels. Until a flag exists
this is an explicit gap, not a hidden default (`lights.rs` carries the colour scheme ready for it).

## Flags that move a role

| Flag | Default | Moves |
| --- | --- | --- |
| `--rotary-slots <1..5>` | 1 | the NX-K rotaries' first encoder slot |
| `--strips relative\|absolute\|off`, `--strip-slots <1..5>`, `--strip-travel <n>` | relative, 1, 120 | the M-Touch base-channel strips' mode, first slot and detents per travel |
| `--pb-rows <A>,<F>,<B>` | `101,201,301` | the executor row bases of the playback keys A, faders, keys B (M-Touch strips and M-Play left block; each must end in `01`) |
| `--pb-ext-row <r>` | 401 | the M-Play right block's row base |
| `--pb-page follow\|<n>` | follow | the executor page the banks mean: the user's current page, or a fixed page (a missing page binds nothing; nothing is created) |
| `--banks on\|off` | on | whether the playback banks are bound at all |
| `--pb-pickup <0..0.5>` | 0.02 | how close a fader must come to the executor's level before it follows |
| `--pb-go <n>`, `--pb-pause <n>` | unmapped | the executors behind the dedicated Go and Pause keys |
| `--mtouch auto\|off`, `--mplay auto\|off` | auto | whether `run` opens the device at all |
| `--lights on\|off`, `--lights-interval <ms>` | on, 50 | the M-Touch / M-Play lights and their coalescing bound |

## Devices together, hot-plug and restart

`run` opens the NX-K, the M-Touch and the M-Play independently and keeps running with whichever is present
(absent ones are retried every 2 s). One device of each kind is supported: two M-Touches cannot be told apart
(the first match wins), and a second NX-K is not supported. A device that goes away has its holds released at
the console (keys, rotary pushes, strip touches, playback holds: `frozen` releases on the targets they resolved),
its bank window is unbound, and a device that returns gets every light rewritten and its window bound again. A
plugin restart ends every session at the console (the lease releases what the surface held) and the service
pairs again on its own, with nothing re-pressed.

## What is not claimed

- Executor functions outside Go+, Flash, Temp, Toggle, Top (keys) and Master, Temp (faders) are refused, not
  approximated. Rate, Speed, X, CrossFade and Time faders are unqualified.
- Encoder presses, executor encoders, phaser/editor slots and non-Absolute layers are refused `unsupported`.
- The M-Touch's parameter bars show value, never colour; the parameter colour scheme is textual there.
- Fade, Delay, Snap Shot and Back on the NX-K have no console hardkey on 2.5.1.
- Nothing here emulates the grandMA3 hardware's own key layout, its encoders' acceleration or its displays.
