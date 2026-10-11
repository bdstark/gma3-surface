# KB-24 hardware record: M-Touch through `run` on macOS, 2026-10-11

The physical M-Touch next to the NX-K against onPC 2.5.1, with the operator at the surface and the console read back
over the MCP bridge. Second part of the hardware pass ([NX-K record](kb-24-hardware-nxk-macos.md)).

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Apple silicon, `bdsmbpm401`), onPC 2.5.1.0, show `mcp-test-disposable`, user Admin |
| Devices | M-Touch `11be:f808` and NX-K `11be:e102`, both attached (two surfaces in one service) |
| Console | `mtpnxk_surface` 0.11.0 (Plugin 2) and bridge 0.18.0 (Plugin 1) from main; modules hardkeys 0.10.0, feedback 0.7.0, control 0.7.0 |
| Service | `fix/kb24-hardware-findings` (`0dbd9d3`), `mtpnxk --verbose run` in a Terminal panel tab |
| Setup | fixtures 401 + 402 on encoder bank 4.1 (R, G, B, Amber); page 1 fader row 201–208 StartShow 1–8 (Flash/Master), 210 Rotate (Temp); temporary `Assign Sequence 2 At Page 1.101` and `Sequence 3 At Page 1.301` for the key test, removed afterwards |

## Playback strips (left section)

| # | Check | Observed | Verdict |
| --- | --- | --- | --- |
| P1 | strip 1 fader on executor 201 (Master 0) | picked up at 0, raised Master to 37 % and started StartShow 1 (the console's own autostart); RGB bar green (the appearance), PFD key green (running) | PASS (restored to 0 and Off afterwards) |
| P2 | strips 2, 3 on 202, 203 (Master 0) with the fader above 0 | the bar showed the pickup state following the finger (yellow), nothing changed on the console until the fader reached the executor's level | PASS |
| P3 | PFD keys (pressure keys under the faders) | unmapped: holding one changes nothing; lit red for every assigned fader-row executor, dark for the empty 209 | PASS (as specified) |
| P4 | PFA 1 → 101 (Go+), PFB 1 → 301 (Go+) | both sequences ran (`HasActivePlayback` true); the keys went from solid (assigned) to slow blink (running) | PASS |
| P5 | PLAYBACK BANK + / − | page display `11` ↔ `12` ↔ `13`, keys followed the bound window (bank 2 dark: 111, 311 empty) | PASS |
| P6 | hold PFA 1, BANK +, release | `released executor 101 key (pressed before the bank moved; now executor 111 key)`: the frozen target | PASS |

## Parameter strips (right section, base channel group)

| # | Check | Observed | Verdict |
| --- | --- | --- | --- |
| S1 | strip 1 on R (slot 1), relative | Red followed; the blue bar followed the finger, fairly responsive, lagging when moved fast | PASS, see finding 4 |
| S2 | strip 2 on G | as strip 1 | PASS |
| S3 | strip 3 with its key held (Fine) | fine control; a full stroke moved Blue about 7 % (Fine is a tenth of a click per detent) | PASS |
| S4 | strip 1 against NX-K rotary 1 on the same slot | a touched strip holds the slot (the rotary is refused `conflict`); a slowly turning rotary holds it only 0.3 s after each step, so a strip touch can take it in between | PASS (as designed) |
| S5 | strip keys at rest | lit blue while the slot is adjustable | PASS |

## Findings

1. **Colour-picker encoders are unsupported (feature gap, to fix).** With onPC's Color Picker window open, the encoder
   bar shows the picker's encoders (x, y, brightness, colour space; `ColorEditBar`) and the console lets you edit
   them. The control module writes attribute values only, so the surface refuses every strip and rotary event while
   the picker is open. The operator wants this supported; it needs its own backend path (separate work item).
2. **The refusal reason named the wrong cause.** With the picker open the context said `display 1 has an encoder bar
   without EncoderBankSelector/PresetBar`; the bank selector was there, the attribute bar was not. The reason should
   say the encoder bar shows the colour picker's encoders. (The same reason appeared with onPC's Command Section
   open, where it was accurate.)
3. **Parameter bars dark at rest.** The blue bars show the programmer's value only; a fresh selection has none, so they
   stayed dark while the console's encoder bar showed 100 %. Touching R put the colour group into the programmer and
   all four bars jumped to 100 %. The bars should show the value the encoder bar shows.
4. **Bar lag while moving.** On both strip types the bar follows the console's read-back while the fader controls
   the target, so it trails a fast finger (the pickup indication, drawn locally, felt instant). Echo the fader's
   position locally while touched; settle on the read-back after the lift.
5. **A red flash on some bank changes.** 11→12 blinked every red PFD once: the stale/unavailable pattern rendered in
   the few hundred milliseconds between the bind and the new binding's context. A gap that short should go dark,
   not show the pattern that means "feedback is stale".
6. **The page display reads `11` for page 1, bank 1** while the log says "bank 1"; the page-then-bank format is
   documented but not self-explanatory.
7. **One log line per identical refusal.** Every refused strip event printed its own line (hundreds while the encoder
   bar was unavailable); identical consecutive refusals should collapse into one line with a count.

## Status of the findings (2026-10-11)

| # | Finding | Status |
| --- | --- | --- |
| 1 | colour-picker encoders unsupported | open feature: [work item](../work-item-colour-picker-encoders.md) with a handoff note |
| 2 | misleading reason with the picker open | fixed: feedback 0.7.1 (GrandMA3MCP `1061735`) names the colour picker's encoders, a covering view (bank selector missing) or missing attribute encoders |
| 3 | parameter bars dark at rest | open, no source found: the encoder bar's value is not readable from Lua on onPC 2.5.1. `GetDMXValue`/`GetDMXUniverse` return nil (DMX output not granted without output hardware); the encoder widgets carry no value (`SHOWVALUE=false`, hidden band faders at 0.0); the encoder bar's fixture-sheet grid returns no cell data; the API list has no output-value function. Defaults would match an idle stage but invent a value under a running playback, so nothing is shown rather than a guess |
| 4 | bar lag while moving | fixed and re-tested: local echo while touched (strips: value at the touch plus detents x one click, Percent readouts; playback faders: the fader position once picked up). The first re-test snapped back to the older read-back at the lift; the bar now holds the echo until the read-back agrees within one LED or 1 s, and the operator confirmed it holds, then settles |
| 5 | red flash on a bank change | fixed and re-tested: the first fix missed the plugin's interim "context unknown" after each bind; any missing context from a live, answering plugin is now a transition, dark for up to 2 s, and only an unresolved one (or a silent plugin) shows the unavailable pattern. The operator confirmed no flash |
| 6 | page display `11` | fixed and re-tested: `page.bank` with the display's decimal point (`1.1`); the operator confirmed the point lights |
| 7 | one log line per identical refusal | fixed: logged once, the repeats summarised when anything else is logged or every 2 s |

Fixed during this pass (branch `fix/kb24-hardware-findings`): consistent device log prefixes and one "not attached"
line per device and absence; slow device opens and slow loop steps are logged; unit tests no longer reach a live
plugin (they had replaced the running service's session twice).

Not tested yet: the dedicated Go/Pause keys (`--pb-go`, `--pb-pause`), the M-Play.

Re-test note: switching the encoder bank (Dimmer ↔ Color) lights the four strip bars with the new slots' programmer
values after the brief dark transition, as designed; attributes with nothing in the programmer stay dark (finding 3).

## Removal while held (2026-10-11)

Setup: `Assign Sequence 2 At Page 1.101` with its key set to Flash (`Set("Key", "Flash")`), surface on bank 1,
fixtures 401 + 402 on bank 4.1.

| # | Check | Observed | Verdict |
| --- | --- | --- | --- |
| U1 | hold PFA 1 (Flash on 101) | StartShow 1 ran while held (PFD green), off at the release (PFD red); a noticeable but acceptable delay between the press and the light | PASS; delay noted below |
| U2 | hold PFA 1 and touch parameter strip 1, unplug the M-Touch | `mtouch: M-Touch disconnected; its strip touches are ended and its playback buttons released`, `mtouch/PFA1: mtouch lost while held; released executor 101 key`, the bank window unbound; on the console StartShow 1 off, no control gestures, nothing unresolved, admission not busy | PASS |
| U3 | replug | `mtouch: M-Touch (no serial) open`, the window bound again, the lights repainted as before (PFA 1 solid: 101 assigned) | PASS |

Observation: a playback key's press-to-light delay is noticeable (key event, plugin tick, the console's Flash, the
read-back to the light). Key acks measure about 45 ms median on this onPC; the light also waits for the read-back.
Not measured yet; the harness can time it.

Cleanup: `Off Page 1.101`, `Delete Page 1.101 /NoConfirmation`, `ClearAll`.
