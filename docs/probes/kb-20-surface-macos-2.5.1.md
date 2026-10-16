# KB-20 live check: the surface plugin serves M-Touch strip gestures on a console encoder slot (macOS, onPC 2.5.1.0)

Evidence for [KEYBOARD.md](../../KEYBOARD.md) "KB-20". Run on 2026-10-10 with `mtpnxk_surface` 0.6.0 (imported as
`mtpnxk.xml` into Plugin 2 of show `mcp-test-disposable` and started from the command line with
`Plugin 2 "key=... input=fake control=console force"`; `force` because the MCP bridge 0.16.0 ran alongside with
`input=keyboard` and held no keys, the KB-08 rule), the vendored set 0.10.0 / 0.4.0 / 0.3.0 from gma3-mcp `01e1561`,
the release service at this commit. The bridge was used only to select the fixture, set the starting value and read
the programmer back (`cmd` and `lua` ops); its own control instance stayed idle. Input stayed on the fake backend; only
the control path was exercised. The show as loaded had none of the earlier sessions' macros, so no Macro 121/123.
Re-run unchanged after the PR #5 / MCP PR #24 review round 1 (vendored set from `597ba47`, the context's `vc` required
for pickup, the NX-K and M-Touch opened independently in `run`): the plugin re-imported and restarted the same way,
same scripts, same values (40.900003910065 relative; `waiting_pickup=1`, two positions, 54.901999235153 absolute; the
one-fixture selection is a complete read, `vc: 1`, so the pickup engaged as before).

## Procedure

1. `Fixture 401`, `Select EncoderBank 1`, `Attribute "Dimmer" At 40` through the bridge; Dimmer read back 40.
2. Relative mode (the default), the real service driven by its simulated M-Touch strips (the same code path as the
   USB decoder's events onward):

   ```
   mtpnxk --verbose sim --script "strip1:t100@1500,strip1:m110@40,strip1:m121@40,strip1:lift@300,strip1:t200@500,strip1:m179@40,strip1:lift@300,skey1:down@300,strip1:t50@50,strip1:m71@40,strip1:lift@300,skey1:up" --seconds 6
   ```

3. Absolute mode, with the programmer at 40.9 from step 2:

   ```
   mtpnxk --strips absolute --verbose sim --script "strip1:t0@1500,strip1:m60@40,strip1:m110@40,strip1:m140@40,strip1:lift@300" --seconds 4
   ```

4. The programmer value read back after each run, then `ClearAll`, `ClearSelection`.

## What happened

- The service paired (`control console` in the welcome) and received the plugin's context: generation 2, bank 1
  `Dimmer` page 1, 5 slots.
- **Relative:** three touches, three lifts, 27 detents in ten control events (touch, two motion packets, lift; the
  same again; the strip key held, then the same with `fine`): the programmer read **40.900003910065** afterwards.
  Touch at 100 and 21 counts up = `At + 9` (4 + 5, the fraction carried) -> 49; lift; retouch at 200 (no jump) and 21
  counts down = `At - 9` -> 40; the strip's key held: 21 counts = 9 fine detents = `At + 0.9` -> 40.9. Summary:
  `control: events=10 sent=10 ... refused=0 lost_reported=0 max_detent=9`, `strips: reports=10 touches=3 lifts=3
  detents=27`, link `lost=0 rejected=0`.
- **Absolute:** touch at 0 and the move to 60 (0.235 of the travel) waited for pickup (`waiting_pickup=1`, nothing
  placed); 110 (0.431) crossed the context's last known value (`abs` 40.9) and was placed as
  `Attribute "Dimmer" At 43.1373`, then 140 as `At 54.902`; the programmer read **54.901999235153** after the lift.
  Summary: `control: events=4 sent=4 ... refused=0`, `strips: positions=2 waiting_pickup=1 takeovers=0`.
- No event was refused by the plugin in either run (the console backend 0.3.0 serves touches and positions on slots).

## Not covered here

The physical M-Touch through `run` (the `sim` path enters the service after the USB decoder; the device qualification
with the operator at the surface is KB-24's), an explicit takeover on a mixed selection (the MCP repository's probe
covers `takeover` through the bridge, 28/28), a binding change while touched (harness evidence; the hold keeps the
console busy), two surfaces on one slot, Windows/Linux hosts.
