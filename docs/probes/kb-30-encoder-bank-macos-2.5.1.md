# KB-30 live record: changing the encoder bar's feature group from the panel (macOS, onPC 2.5.1.0)

Run on 2026-10-20 against onPC 2.5.1.0 (Release). The question was which command line switches the encoder bar's
feature group, how to read the switch back, and how the NX-K should issue it.

The feature group the encoder bar shows is its **encoder bank**: an entry of `CurrentProfile().EncoderBarPool[1]`,
which the vendored feedback reader (`encoderBank`, feedback 0.8.0) already reports as `bank 2 "Position" page 1
"PanTilt"`. In `test-fixture-disposable` the pool holds 10 banks: 1 Dimmer, 2 Position, 3 Gobo (pages Gobo, GOBOMISC),
4 Color (RGB, RGB, Color), 5 Beam, 6 Focus, 7 Control, 8 Shapers (no pages), 9 Video (no pages), 10 Phaser.

## 1. Candidate commands (show `test-fixture-disposable`)

Tool: [`tools/probes/kb30_encoder_bank_probe.py`](../../tools/probes/kb30_encoder_bank_probe.py), through the gma3-mcp
bridge that show carries (0.25.1, feedback module 0.8.0, the version this repository vendors). For every candidate:
`Select EncoderBank 1.1`, then wait until `feedback.context` reads bank 1 page 1; run the candidate through the
bridge's `cmd` op; then poll `feedback.context` every 20 ms (at most 1 s) until the bank or page changes. That is the
same `contextSnapshot` the surface plugin builds the service's context from. `SelectedFeature()` and
`SelectionCount()` were read before and after. Raw results:
[run A, nothing selected](kb-30-encoder-bank-onpc-macos-2.5.1-runA-noselection.json),
[run B, Fixtures 101–104 (MegaHex: dimmer, colour, beam) selected](kb-30-encoder-bank-onpc-macos-2.5.1-runB-selection.json).

| Command | Answer, nothing selected | Bar afterwards | Answer, MegaHex selected | Bar afterwards |
| --- | --- | --- | --- | --- |
| `Select EncoderBank 2` | OK | 2 Position / PanTilt, 68 ms | **Not allowed** | unchanged |
| `Select EncoderBank "Color"` | OK | 4 Color / page 3 "Color", 69 ms | OK | 4 Color / page 1 RGB, 84 ms |
| `Select EncoderBank 3.2` | OK | 3 Gobo / page 2 GOBOMISC, 70 ms | OK | **unchanged** (bank 3 is not allowed for the selection) |
| `Select EncoderBank "Color"."Color"` | OK | 4 Color / page 3, 70 ms | OK | 4 Color / page 1 |
| `EncoderBank 5` | OK | 5 Beam, 67 ms | OK | 5 Beam, 67 ms |
| `PresetType 2`, `PresetType "Gobo"` | Object not found | unchanged | Object not found | unchanged |
| `Select PresetType 4` | OK | **unchanged** | OK | unchanged |
| `FeatureGroup 6` | OK | 6 Focus, 66 ms | OK | **unchanged** |
| `Select FeatureGroup "Beam"` | OK | 5 Beam, 66 ms | OK | 5 Beam, 34 ms |
| `Feature 2.1` | OK | 2 Position, 69 ms | OK | **unchanged** |
| `Select EncoderBank 8` (no pages) | Not allowed | unchanged | Not allowed | unchanged |
| `Select EncoderBank 11`, `2.5`, `"Nope"` | Object not found | unchanged | Object not found | unchanged |

Single checks with the MegaHex selection: `Select EncoderBank 3`, `6`, `7` and `10` answered **Not allowed**;
`3.1` answered OK and changed nothing; `4.3` (a page the selection hides) answered OK and landed on **page 1**; `4.2`
reached page 2. A bank keeps its last page: `4.2`, then `1`, then `4` came back on page 2, as clicking the tab does.

Every switch showed on the second read (the first read right after the command still had the old bank), 34–84 ms
after the command. `SelectedFeature()` followed the bank. The selection and its count never changed.

**Qualified:** `Select EncoderBank <n>`, with the bank number alone. It addresses exactly what the context reader
reports (the profile bar's bank index), it is refused with `Not allowed` when the selection has nothing for the bank
(instead of silently doing nothing, as `FeatureGroup`, `Feature` and the `bank.page` form do), and it reaches banks that
are not feature groups (Phaser). `PresetType` does not switch the bar on 2.5.1.

## 2. How the panel issues it (Shehds, by mistake; cleaned up)

The user asked for a one-handed flow: Swap Prog puts `Select EncoderBank` on the command line, and they type the
number and Enter. These checks were meant for the disposable show, but by then another session working on the same
console (gma3-surface#19) had loaded **Shehds** again. The feedback generation restarting at 1 showed it, and I
noticed too late. What ran in Shehds, and what was undone:

- With `1 Thru ` typed by the operator, two `Select EncoderBank` commands through the bridge's `Cmd()` switched the bar
  (4 Color, then 1 Dimmer) and left the typed text exactly as it was: `cmdtext` read `1 Thru ` before and after. The
  bar started on 2 Position.
- Lua cannot write the command line: `CmdObj().cmdtext = …` and `CmdObj():Set("CmdText", …)` return without error and
  change nothing. `CmdObj():ClearCmd()` (an invokable `CLEARCMD` property of the `Cmdline` object) clears the text,
  and only the text: the selection count was unchanged.
- A macro line with **Execute No** places its command on the operator's command line when the macro runs, also when
  `Go+ Macro n` comes from a plugin's `Cmd()`. The text appeared in the next frame (the same chunk still read the old
  text, the next bridge request the new one). `AddToCmdline No` still **appended** to text already on the line
  (`1 Thru Select EncoderBank Select EncoderBank `), so the plugin clears the line first. For this I created Macro 951
  and 952 (one line each, `Select EncoderBank`, Execute No, AddToCmdline No / Yes).
- With `Select EncoderBank ` placed by Macro 951, the operator pressed `5` and Enter on the panel (`Press Quickey 928`
  in the history): the bar went to 5 Beam.
- `ClearCmd()`, then `Go+ Macro 951` in one Lua chunk: empty right after the clear, `Select EncoderBank ` from the next
  request on.

Cleanup, with the user's approval: `Delete Macro 951 Thru 952 /NoConfirmation` (the macro pool read back empty) and
`Select EncoderBank 2` (Position, as before). Shehds.show on disk was not written by any of this. It had been saved at
13:38 UTC, probably by the first `LoadShow "test-fixture-disposable" /NoConfirmation`, which seems to save the current
show by default. For the second load the user chose `LoadShow … /Save /NoConfirmation`.

## 3. Plugin 0.14.0 end to end (show `test-fixture-disposable`)

Plugin 0.14.0 from this branch, imported at Plugin 2 next to the bridge (Plugin 1). The plugin library's 0.13.1 files
(gma3-surface#19) were backed up first and put back afterwards, and Plugin 2 was deleted again.

- `bank=900/1.178-189 input=mixed control=console encmacro=950`: `encmacro: Macro 950 created ('mtpnxk EncoderBank':
  Select EncoderBank, Execute No)`. It read back as one line, Command `Select EncoderBank`, Execute false, Enabled true,
  Note `mtpnxk_surface encmacro v1`. The kept Quickey bank record belonged to Shehds and was adopted **stale** (the
  problem gma3-surface#19 fixes), so input fell back to off and Swap Prog would have been refused. Restarted with
  `input=keyboard control=console encmacro=950`: `Macro 950 adopted`, `key Swap Prog: Select EncoderBank via Macro 950`,
  `keys: 34 supported, 10 unsupported`.
- `mtpnxk --id kb30-sim sim --script "1:tap@2000,Swap Prog:tap@300,4:tap@20,Enter:tap@20"` (the release build of this
  branch, its own surface id next to the operator's running service): the bar went from 1 Dimmer to **4 Color**, and the
  last command read `OK: Select EncoderBank 4`, so the typed `1` had been cleared. Plugin counters: `encBank=1
  encBankVerified=1 keyQueued=5`: the keys behind Swap Prog waited for the read-back. The sim's NX-K LED writes:
  `Swap Prog <- on`, then `off` after Enter. Link: `lost=0 refused=0`.
- MegaHex 101–104 selected: `Swap Prog 2 Enter` gave `Not allowed: Select EncoderBank 2`, and the bar stayed on Dimmer.
  `Swap Prog 5 Enter` reached 5 Beam. The selection stayed 4 throughout.
- **Physical NX-K**, through the operator's running service (the main-branch build, so no Swap Prog LED): Swap Prog,
  `4`, Enter took the bar from Dimmer to **4 Color** (`OK: Select EncoderBank 4`; counters `encBank` and
  `encBankVerified` up by one, two more key presses). The selection was kept.
- `encmacro teardown`: `Macro 950 deleted; Swap Prog is unsupported until a start with encmacro=<n>`, and the macro
  pool read back empty.

## Not covered

- Quickey dispatch (`input=mixed`) in the same run: the stale Shehds bank record blocked it until gma3-surface#19
  merges. The digits after Swap Prog went through the keyboard backend here, and through Quickeys in the Shehds check
  of section 2.
- The new service's Swap Prog LED on the physical panel (the sim's LED writes only).
- The gate timeout (the keyword never arriving) is covered by the harness only.
