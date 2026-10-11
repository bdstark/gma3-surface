# Vendored console modules

`gma3_mcp_hardkeys.lua`, `gma3_mcp_feedback.lua` and `gma3_mcp_control.lua` are copied **unchanged**
from [bdstark/GrandMA3MCP](https://github.com/bdstark/GrandMA3MCP) (MIT, see
[LICENSE.gma3-mcp](LICENSE.gma3-mcp)). They are the console-semantics layer:
key routes, ownership and leases, read-only state readers, continuous-control admission. The surface plugin
(`mtpnxk_surface.lua`) only consumes them.

| Component | Version | Module API | Copied from |
| --- | --- | --- | --- |
| `gma3_mcp_hardkeys` | 0.10.0 | 1 | `d60bb06` (branch `feat/kb21-playback-banks`; unchanged since `3960334`: KB-12 Quickey bank, KB-13 owned-Quickey backend, KB-14 scoped shortcut-mode changes and text routes, KB-15 mixed backend); sha256 `a57ebd29af3b2c7e9e06ef3dcd8b7059c775db83b0dc61760f49e75973cc7dc3` |
| `gma3_mcp_feedback` | 0.5.0 | 1 | `d60bb06` (KB-21: `executorTarget { executor, page? }` reads the user's page (`GetExecutor`, mode current) or an explicit page (`ObjectList("Page P.E")`, mode page; a missing page is `pageMissing`, nothing is created), every target carries pool/page/mode/width, a wider neighbour of the same row covers the following numbers (`coveredBy`), `contextSnapshot({ executors, executorPage? })`, the user's page in the digest only while some executor follows it; KB-20 review 1: `valueComplete`/`valueIncomplete` per attribute slot, true only when every selected fixture was scanned, discovered and read; KB-19 incl. the PR #23 review: every attribute slot carries the physical range of the channel function that names the attribute, `physicalFrom/To/Range/Function/...`, the smallest over the selection, only when complete and verified (a failed channel discovery, including a nil or unreadable attribute mapping, is incomplete coverage, reviews 2 and 3), and in the binding digest; KB-17: control-context readers, `contextSnapshot()`/`watchContext()` with the binding generation); sha256 `7108a8854523ee1078ef51c7c5db79a360b770d595ea35d0cd16f2739d89bf4d` |
| `gma3_mcp_control` | 0.5.0 | 1 | `0315e18` (branch `feat/kb22-executor-operations`; KB-22: the console backend serves executor keys as `Press`/`Unpress Page <p>.<e>` (the configured button function; Temp, Flash, Toggle, Top, Go+ qualified, other key functions refused `unsupported`) and executor faders as `Fader<Function> Page <p>.<e> At <level>` for the configured fader function (Master, Temp qualified; Rate, Speed, X, CrossFade, Time refused until qualified), a fader touch is a hold, `executorOperation()`, `KEY_FUNCTIONS`, `FADER_FUNCTIONS` exported, a frozen executor-key release whose executor was reassigned or emptied while held is an `assignment-changed` record until `recover()` finds the original object back; KB-21: executor targets `{ executor, element, page? }` resolve by (page, executor) with the explicit identity in the key (`exec<pool>/<page>.<n>.<element>|...`), covered / missing-page / empty / reserved / Quickey targets refused `target-unavailable` with the reason, a hold's release, forced end and unresolved record carry the target its down resolved (`frozen`); KB-20 review 1: a position on a slot whose values were not completely read is refused `values-incomplete` unless `takeover`; KB-20: strip touches on attribute slots are holds served by the console backend, positions are `Attribute "<name>" At <value>` over the verified travel (Percent 0..100, Physical From..To, never a mixed physical range), the `mixed-values` refusal with `takeover`, `position()` exported, `valueState`/`absolute` on the resolved slot; KB-19: the console adjustment backend `consoleBackend(consoleDeps(_G))`, calibrated per readout/resolution/physical range, fine a tenth, slots resolve only while the encoder bar is in attribute editing; KB-18: admission, ordering, coalescing, bounds, recovery); sha256 `c0716917d457a88fbe74a6a38f80c16794950f2088ed097be5dfc7b0af048968` |

Previous pins: hardkeys 0.10.0 / feedback 0.5.0 / control 0.4.0 at `d60bb06` (KB-21 after review 1; control sha256 `aa6e1964f6cd3ea03a101f12063edbe90dc298268a0e78aaa33b99c475aa18e2`); hardkeys 0.10.0 / feedback 0.4.0 / control 0.3.0 at `597ba47` (KB-20 after review 1) and `01e1561` (KB-20 before review); hardkeys 0.10.0 / feedback 0.4.0 / control 0.2.0 at `107f548` (KB-19); hardkeys 0.10.0 / feedback 0.3.0 / control 0.1.0 at `9f08f87` (KB-18); hardkeys 0.10.0 / feedback 0.3.0 at `c8dbb3a` (KB-17); hardkeys 0.10.0 / feedback 0.2.0 at `3960334` (KB-15); hardkeys 0.5.0 `d73a8e10…` (PR #12, `6e0d9c1`), used by the KB-07/KB-08 records.

Upstream's `plugin/modules.lock.json` carries the same hashes. Check them before
packaging (`shasum -a 256 tools/ma3/gma3_mcp_*.lua`): the version strings alone do
not identify a revision.

Rules (from that repository's `docs/modules.md`, "Vendoring into another plugin"):

1. Never edit these files here. A defect found through the surface is fixed in
   GrandMA3MCP with a regression test, then re-vendored, so console semantics
   never fork.
2. Keep them as `ComponentLua` entries after the entry component in
   `mtpnxk_surface.xml`; the console runs every component chunk at import and
   show load with the same signal table, and the entry component looks the
   modules up there in `Main` (never through `require` or globals).
3. Re-vendor with:

   ```bash
   cp "$GMA3MCP"/plugin/gma3_mcp_hardkeys.lua "$GMA3MCP"/plugin/gma3_mcp_feedback.lua "$GMA3MCP"/plugin/gma3_mcp_control.lua tools/ma3/
   ```

   where `GMA3MCP` is the GrandMA3MCP checkout (on this machine `~/Development/GrandMA3`);

   ```bash
   ```

   then update the table above (version, commit, sha256) and run
   `lua tools/ma3/test/surface_plugin_test.lua` and `sh tools/ma3/test/e2e.sh`.
