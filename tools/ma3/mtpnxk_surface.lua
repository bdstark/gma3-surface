-- mtpnxk_surface.lua
--
-- Surface plugin for grandMA3 onPC: receives key events from an mtpnxk surface service (the Rust
-- process that owns an NX-K keypad) over UDP, presses console keys through the vendored
-- gma3_mcp_hardkeys module, reads console state through the vendored gma3_mcp_feedback module and
-- sends it back for the surface's LEDs. Protocol, pairing and limits: docs/surface-protocol.md.
--
-- Command line:
--   Plugin "mtpnxk_surface"                 start on 127.0.0.1:9810 (input on the keyboard backend)
--   Plugin "mtpnxk_surface" "port=9811"
--   Plugin "mtpnxk_surface" "input=off"            feedback only, no console key is ever pressed
--   Plugin "mtpnxk_surface" "input=fake"           lifecycle testing: events recorded, no key pressed
--   Plugin "mtpnxk_surface" "bank=900/1.180-191 input=mixed"
--                                                          KB-15: provision the owned Quickey bank (one
--                                                          Quickey per qualified code, KB-10 + KB-28, 12
--                                                          reserved executors) and press every hold-qualified
--                                                          key as an executor press of its Quickey (they reach
--                                                          pop-ups and dialogs); the rest (Undo, Next, Last,
--                                                          Menu) through Keyboard() (explicit per-key table).
--                                                          KB-28: at most one key goes down per tick (the
--                                                          rest wait in a bounded key queue)
--   Plugin "mtpnxk_surface" "bank=... input=quickey"  Quickeys only: unqualified keys are refused
--   Plugin "mtpnxk_surface" "input=mixed route=Undo:quickkey,2:shortcut"
--                                                          per-key override of the dispatch method; an
--                                                          override the backend cannot serve refuses the start
--   Plugin "mtpnxk_surface" "bankcodes=hardkeys"   provision every command-area code (94 Quickeys)
--   Plugin "mtpnxk_surface" "force"                start even though the MCP bridge has input enabled
--   Plugin "mtpnxk_surface" "bench"                report press-to-effect timing for NUM taps
--   Plugin "mtpnxk_surface" "encmacro=950 input=mixed bank=..."
--                                                          KB-30: Swap Prog puts "Select EncoderBank " on the
--                                                          command line (the operator types the bank number
--                                                          and Enter) through Macro 950, which the plugin
--                                                          creates (one line, Execute No) or adopts when it is
--                                                          its own; a foreign Macro 950 is never touched
--   Plugin "mtpnxk_surface" "encmacro status" | "encmacro teardown"
--                                                          (teardown: delete the verified owned macro)
--   Plugin "mtpnxk_surface" "stop" | "status" | "recover"   (recover: re-attempt unresolved key releases)
--   Plugin "mtpnxk_surface" "bank status" | "bank verify" | "bank teardown"
--                                                          (teardown: delete the verified owned Quickeys and
--                                                          clear the reserved executors; refused while a
--                                                          Quickey record is live)
--
-- Rules this plugin keeps (KEYBOARD.md KB-07, docs/surface-protocol.md):
--   * the listener is fixed to 127.0.0.1; remote sources are rejected before parsing. Like the MCP
--     bridge, local processes are trusted; there is no application-level authentication;
--   * sessions have fresh random ids, strictly increasing sequence numbers, event-id deduplication,
--     a 2 s lease renewed by any valid packet and a 10 s forget time;
--   * heartbeats release lost releases and never press anything; nothing is replayed after a restart;
--   * deadline servicing runs before packet processing, packets per iteration are bounded, feedback
--     reads are spread across frames; the loop yields once per console frame;
--   * every state item is tri-state (0, 1, "?") and carries the plugin generation and feedback epoch;
--   * KB-17: the control context (what each encoder slot and executor would operate) is the vendored
--     feedback module's contextSnapshot, assembled from the loop's own observations and sent as a
--     `context` message whenever its binding generation moves and with every full state; the plugin
--     reconstructs no grandMA3 semantics of its own and sends nothing while a part is unobserved;
--   * the MCP bridge and this plugin do not arbitrate the console keyboard: the start refuses to
--     enable input while the bridge reports input enabled (force overrides; it is a courtesy check);
--   * KB-15: the dispatch method of every key is decided before dispatch and reported at start; a
--     Quickey route without a bank or for a code without KB-10 evidence is refused, never replaced by
--     a Keyboard() press, and a PC key next to a held Quickey (or the reverse) is refused as an
--     unqualified mix. The bank is created only by the operator's plugin argument (bank=...), kept as a
--     record across stop/start and removed only by "bank teardown".

local pluginName    = select(1, ...)
local componentName = select(2, ...)
local signalTable   = select(3, ...)
local my_handle     = select(4, ...)

local socket = require("socket")
local json   = require("json")

-- KB-20 (0.6.0): the M-Touch parameter strips reach the console through the same `ctl` events: a strip
-- touch on a slot is served by the vendored console backend (0.3.0) as a hold that moves nothing, the drag
-- inside it as the KB-19 adjustment, a position (`abs`, optional `tk` = takeover) as
-- Attribute "<name>" At <value> over the verified travel; a position on a slot whose fixtures hold
-- different values is refused `mixed-values` unless the surface sent `tk: 1`. The gesture conversion
-- (anchoring, sensitivity, pickup) is the service's; this plugin maps the packet and acknowledges the
-- module's verdict as before.
-- KB-19 (0.5.0): `control=console` attaches the vendored module's console adjustment backend: a rotary
-- detent on slot n becomes the selection-scoped  Attribute "<name>" At +/- <detents x step>  for whatever
-- attribute the bound display's slot n holds (name, layer, resolution, readout, channel function and
-- physical range from this plugin's own context snapshot). One detent is one console encoder click
-- (Percent readout 1, Physical readout range/120 in physical units, Fine a tenth); Bank held (`fine`)
-- is a tenth of that. Rotary pushes, strip touches/positions and executor elements are refused
-- `unsupported` by that backend at admission (nothing pressed; KB-20/21/22). The console semantics are
-- the MCP repository's (docs/modules.md there, "Console adjustment backend"); this plugin only chooses
-- the backend.
-- KB-23 (0.9.0): the context carries what the service lights its controls from (vendored feedback 0.6.0): per
-- attribute slot the feature group / feature / activation group names (`fg`, `fn`, `ag`) and the definition's
-- colour as six hex digits when the show defines one (`col`); per executor the assigned object's appearance
-- background as six hex digits (`col`). A change of what the service shows (values, levels, activity, colours)
-- resends the context after at most `feedbackMinMs`, instead of only with the 1 s cadence; the binding
-- generation is untouched by any of it.
-- KB-24: the heartbeat reply carries the plugin loop's load since the previous reply (`load`: the longest and the
-- mean tick in ms, the most datagrams one tick took, how many ticks hit the per-tick budget, ticks): the
-- measurement harness publishes it next to the service-side latencies. Nothing else changed.
-- KB-30 (0.14.0): Swap Prog enters the encoder bank (the feature group the encoder bar shows) from the panel. A
-- plugin-owned macro (operator argument encmacro=<n>: one line "Select EncoderBank", Execute No) places the keyword
-- text on the operator's command line; Lua cannot write the command line itself (CmdObj().cmdtext and
-- Set("CmdText") are ignored on onPC 2.5.1). Text already on the line is cleared first (CmdObj():ClearCmd(): the text
-- only, never the selection), and the key queue waits until the command line reads back "Select EncoderBank" so the
-- digits typed right after land behind it. The bank switch itself is the operator's Enter; the service follows the
-- encoder bar through the context as before. The state's `ebank` item lights Swap Prog while the keyword is pending.
-- Probe: docs/probes/kb-30-encoder-bank-macos-2.5.1.md.
local VERSION   = "0.14.0"
-- KB-21: the executor-binding bounds (used by the argument parser, the bind handler and the feedback config).
local MAX_BOUND_EXECUTORS = 96  -- the M-Touch (30) and the M-Play (60) windows together, on any bank
-- Review (PR #6, round 2): the watch list budget covers every supported configuration at once: the ordinary state
-- readers (9), two readers per execs= entry (x<n>, f<n>; the argument is bounded to MAX_BOUND_EXECUTORS), the context's
-- own items (data pool, page is shared, encoder bank, slots) and one target per bound executor.
local STATE_READERS = 9
local CONTEXT_ITEMS = 4
local MAX_WATCH_ITEMS = STATE_READERS + 2 * MAX_BOUND_EXECUTORS + CONTEXT_ITEMS + MAX_BOUND_EXECUTORS
local PROTOCOL  = 2
local MAGIC     = "MTX2"
local DEFAULTS = {
  host = "127.0.0.1", port = 9810,
  leaseMs = 2000,        -- hardkeys session lease, renewed by every valid packet
  forgetMs = 10000,      -- a session without packets for this long is closed and forgotten
  hbMs = 250,            -- heartbeat interval the service is told to use
  fullStateMs = 1000,    -- full state to every session at least this often
  stateEveryMs = 40,     -- KB-24: state and context are collected and sent at most this often (the readers refresh
                         -- items on their own cadence; collecting every tick cost half of each tick on onPC 2.5.1)
  feedbackMinMs = 250,   -- KB-23: a changed value/level/activity/colour resends the context after at most this
  maxDatagram = 512,
  maxPacketsPerTick = 32,
  maxKeyQueue = 64,        -- KB-28: key events waiting for their tick (one key-down per tick); beyond it a key is refused
  encBankGateMs = 400,     -- KB-30: how long the key queue waits for "Select EncoderBank" on the command line (live: 50-70 ms)

  maxPacketsPerSecond = 400,  -- per session
  evWindow = 64,         -- event ids remembered per session for deduplication
  nonceMs = 60000,       -- hello nonces remembered this long
  errReplyMs = 500,      -- at most one err reply per source address per this interval
  maxHolds = 12, maxHoldMs = 60000,
  display = 1,
  controlWorkPerTick = 8,  -- KB-18: continuous-control intents applied per loop iteration (KB-24: 8, merged intents)
  maxCtlDelta = 4096,
  benchMs = 500,         -- how long bench mode waits for a NUM tap to show on the command line
  refreshMs = 300,         -- KB-24 hardware pass: how long an executor whose key was pressed is re-read every tick
  readsPerService = 24,    -- KB-21: a bound bank window is up to 96 targets (KB-24: 24 reads per tick; 8 left the state readers behind ~100 watch items, a command-line keyword took 200-600 ms to read back)
}

-- State survives repeated Plugin calls (file-level locals are re-created on ReloadAllPlugins only).
_G.__mtpnxk_surface = _G.__mtpnxk_surface or { running = false, inputEnabled = false }
local state = _G.__mtpnxk_surface
state.startCount = state.startCount or 0

-------------------------------------------------------------------------------
-- Logging (System Monitor and Command Line History; never raises)
-------------------------------------------------------------------------------
local function fmt(f, ...) local ok, s = pcall(string.format, f, ...); return ok and s or tostring(f) end
local function log(f, ...)
  local line = "mtpnxk: " .. fmt(f, ...)
  if type(Echo) == "function" then pcall(Echo, line) end
  if type(Printf) == "function" then pcall(Printf, line) end
  if state.log then state.log[#state.log + 1] = line; if #state.log > 200 then table.remove(state.log, 1) end end
end
local function logerr(f, ...)
  local line = "mtpnxk ERROR: " .. fmt(f, ...)
  if type(ErrEcho) == "function" then pcall(ErrEcho, line) elseif type(Echo) == "function" then pcall(Echo, line) end
  if type(ErrPrintf) == "function" then pcall(ErrPrintf, line) elseif type(Printf) == "function" then pcall(Printf, line) end
  if state.log then state.log[#state.log + 1] = line; if #state.log > 200 then table.remove(state.log, 1) end end
end

-------------------------------------------------------------------------------
-- Randomness (session ids, generation). Seeded once per start from the clock and os.time.
-------------------------------------------------------------------------------
local function seedRandom()
  local t = 0
  pcall(function() t = socket.gettime() end)
  local frac = math.floor((t - math.floor(t)) * 1e6)
  math.randomseed((os.time() * 1000003 + frac * 7919 + math.floor(os.clock() * 1e6)) & 0x7fffffff)
  for _ = 1, 8 do math.random() end
end
local function randomHex(bytes)
  local out = {}
  for _ = 1, bytes do out[#out + 1] = string.format("%02x", math.random(0, 255)) end
  return table.concat(out)
end

-------------------------------------------------------------------------------
-- Surface keys. The service sends the NX-K control names; each maps to a logical key the hardkeys
-- module resolves at start. Names marked unverified are the VirtualKeyCode names we expect; a name the
-- console's enum does not know is reported unsupported at start, never guessed.
-------------------------------------------------------------------------------
local NXK_KEYS = {
  ["0"] = "NUM0", ["1"] = "NUM1", ["2"] = "NUM2", ["3"] = "NUM3", ["4"] = "NUM4",
  ["5"] = "NUM5", ["6"] = "NUM6", ["7"] = "NUM7", ["8"] = "NUM8", ["9"] = "NUM9",
  Enter = "PLEASE", Record = "STORE", Clear = "CLEAR", Undo = "OOPS",
  -- unverified VirtualKeyCode names (resolved through the shortcut table if the profile maps them)
  Update = "UPDATE", Edit = "EDIT", Copy = "COPY", Move = "MOVE", Delete = "DELETE", Load = "LOAD",
  Cue = "CUE", Group = "GROUP", Macro = "MACRO", Fade = "FADE", Delay = "DELAY",
  HighLight = "HIGHLIGHT", Preview = "PREVIEW", Next = "NEXT", Last = "PREV", Menu = "MENU",
  Thru = "THRU", Full = "FULL", ["@"] = "AT", ["+"] = "PLUS", ["-"] = "MINUS", ["."] = "DOT", ["/"] = "SLASH",
  -- Fade, Delay, Snap Shot and Back have no MA3 hardkey (onPC 2.5.1 enum, read live 2026-10-09);
  -- they are reported unsupported rather than mapped to something else.
}
-- The default profile maps two shortcuts with equal modifier count to PLUS, MINUS, DOT and SLASH (main
-- row and keypad). Both rows target the same MA key; the module resolves such a tie only when the consumer
-- names the row (spec.prefer, hardkeys 0.5.0), and the chosen row keeps every safeguard: shortcut
-- enablement, the collision check and the route rechecks before every release.
local NXK_PREFER = { ["+"] = "kpAdd", ["-"] = "kpSubtract", ["."] = "kpDecimal", ["/"] = "kpDivide" }
-- KB-15 dispatch methods. On the mixed backend the default is quickkey (an executor press of the owned
-- Quickey, KB-13) and every surface key whose code has no HOLD evidence in the loaded module's
-- BANK_QUALIFIED (KB-10; KB-28 added the digits, + - . /, At, Full and the keywords, hardkeys 0.11.0) is
-- routed explicitly through the shortcut table instead (the module validates each override against the
-- attached backend; nothing here is inert). Quickeys reach pop-ups and dialogs, the shortcut table does not.
-- The surface sends press/release pairs, so a code qualified for taps only (OOPS) is not a Quickey key.
-- Operators change single keys with route=<key>:<method>; the two methods the surface offers are quickkey
-- and shortcut (the text routes need text the keypad does not carry).
local NXK_QUICKKEY_HOLD_KB10 = { NUM1 = true, NUM5 = true, THRU = true, PLEASE = true, CLEAR = true, STORE = true }
local SURFACE_METHODS = { quickkey = true, shortcut = true }
-- KB-29: a code with direct-tap evidence is a Quickey key as well: the surface taps it on key-down (one
-- Press Quickey, nothing held), so it needs no hold evidence (OOPS: Undo).
local function quickkeyHold(qualified, logical)
  if type(qualified) ~= "table" then return NXK_QUICKKEY_HOLD_KB10[logical] == true end  -- a module older than the table export
  local q = qualified[logical]
  return type(q) == "table" and (q.hold == true or (q.direct == true and state.tapsMode ~= "off"))
end
local function mixedPolicy(overrides, qualified)
  local keys = {}
  for name, logical in pairs(NXK_KEYS) do
    if not quickkeyHold(qualified, logical) then keys[logical] = { method = "shortcut" } end
  end
  for logical, method in pairs(overrides or {}) do keys[logical] = { method = method } end
  return { default = "quickkey", keys = keys }
end
local function quickeyPolicy(overrides)
  local keys = {}
  for logical, method in pairs(overrides or {}) do keys[logical] = { method = method } end
  return { default = "quickkey", keys = keys }
end
-- Keys the service may send that are not console keys (acknowledged unsupported, never an error).
local NXK_LOCAL = { Bank = true, ["Swap Prog"] = true, Link = true, Rotary1 = true, Rotary2 = true, Rotary3 = true, Rotary4 = true }
-- KB-30: the key that enters the encoder bank, the text its macro places, and how the plugin marks the macro it owns.
local ENCBANK_KEY     = "Swap Prog"
local ENCBANK_COMMAND = "Select EncoderBank"
local ENCBANK_MARK    = "mtpnxk_surface encmacro v1"
local ENCBANK_NAME    = "mtpnxk EncoderBank"

-- Preview environment names (previewMode reader) that mean on/off; anything else is unknown.
local PREVIEW_ENV = { Preview = 1, Live = 0, Normal = 0 }

-------------------------------------------------------------------------------
-- Module loading (KB-02 contract: the console runs every component chunk with the same signal table)
-------------------------------------------------------------------------------
local MODULE_REGISTRY_KEY = "__gma3_mcp_modules"
local MODULE_API_VERSION  = 1

local function loadModule(component)
  if type(signalTable) ~= "table" then return nil, "no signal table (the plugin was not loaded by the console)" end
  local reg = rawget(signalTable, MODULE_REGISTRY_KEY)
  if type(reg) ~= "table" then return nil, "no module registered: the module components did not run (import the plugin with every component)" end
  local mod = reg[component]
  if mod == nil then return nil, "module '" .. component .. "' is not registered (check mtpnxk_surface.xml)" end
  if type(mod) ~= "table" or type(mod.new) ~= "function" or mod.API_VERSION == nil then return nil, "registered '" .. component .. "' is not a module table" end
  if mod.API_VERSION ~= MODULE_API_VERSION then return nil, string.format("module API version %s, this plugin expects %d", tostring(mod.API_VERSION), MODULE_API_VERSION) end
  return mod
end

-------------------------------------------------------------------------------
-- Argument parsing
-------------------------------------------------------------------------------
local function parseArgument(argument)
  local opts = { input = "keyboard", control = "off" }
  for tok in tostring(argument or ""):gmatch("%S+") do
    local l = tok:lower()
    local k, v = tok:match("^(%a+)=(.*)$")
    if opts.bankToken and (l == "status" or l == "verify" or l == "teardown") then opts.command = "bank-" .. l; opts.bankToken = nil
    elseif opts.encToken and (l == "status" or l == "teardown") then opts.command = "encmacro-" .. l; opts.encToken = nil
    elseif l == "encmacro" then opts.encToken = true
    elseif l == "stop" or l == "status" or l == "recover" then opts.command = l
    elseif l == "bank" then opts.bankToken = true
    elseif l == "force" then opts.force = true
    elseif l == "static" then opts.bankStatic = true
    elseif l == "bench" then opts.bench = true
    elseif k then
      k = k:lower()
      if k == "key" or k == "bind" or k == "allow" then return nil, k .. "= is no longer supported: the plugin is localhost-only and needs no pairing key"
      elseif k == "port" then opts.port = tonumber(v); if not opts.port or opts.port < 1 or opts.port > 65535 then return nil, "port must be 1..65535" end
      elseif k == "input" then
        local m = v:lower()
        if m == "quickkey" or m == "qk" then m = "quickey" end
        if m ~= "keyboard" and m ~= "fake" and m ~= "off" and m ~= "quickey" and m ~= "mixed" then return nil, "input must be keyboard, quickey, mixed, fake or off" end
        opts.input = m
      elseif k == "bank" then
        local q, page, first, last = v:match("^(%d+)/(%d+)%.(%d+)%-?(%d*)$")
        if not q then return nil, "bank must be <quickey>/<page>.<first>[-<last>], e.g. bank=900/1.180-191" end
        local count = (last ~= "" and (tonumber(last) - tonumber(first) + 1)) or DEFAULTS.maxHolds
        if count < 1 then return nil, "bank executor range is empty" end
        opts.bank = { quickeyFirst = tonumber(q), page = tonumber(page), executorFirst = tonumber(first), executorCount = count }
      elseif k == "encmacro" then
        local n = tonumber(v)
        if not n or n ~= math.floor(n) or n < 1 or n > 9999 then return nil, "encmacro must be a macro number 1..9999, e.g. encmacro=950" end
        opts.encMacro = n
      elseif k == "taps" then
        local m = v:lower()
        if m ~= "on" and m ~= "off" then return nil, "taps must be on or off" end
        opts.taps = m
      elseif k == "bankcodes" then
        local m = v:lower()
        if m ~= "hardkeys" and m ~= "qualified" then return nil, "bankcodes must be hardkeys or qualified" end
        opts.bankCodes = m
      elseif k == "route" then
        opts.routes = opts.routes or {}
        for item in v:gmatch("[^,]+") do
          local name, method = item:match("^([^:]+):(%a+)$")
          if not name then return nil, "route must be <key>:<method>[,<key>:<method>...], e.g. route=Undo:quickkey" end
          local logical = NXK_KEYS[name] or (NXK_KEYS[name:gsub("^%l", string.upper)])
          if not logical then return nil, "route: '" .. name .. "' is not a surface key" end
          if method == "quickey" or method == "qk" then method = "quickkey" end
          if not SURFACE_METHODS[method] then return nil, "route: method must be quickkey or shortcut (got '" .. method .. "')" end
          opts.routes[logical] = method
        end
      elseif k == "control" then
        local m = v:lower()
        if m ~= "fake" and m ~= "console" and m ~= "off" then return nil, "control must be fake, console or off" end
        opts.control = m
      elseif k == "display" then opts.display = tonumber(v); if not opts.display then return nil, "display must be a number" end
      elseif k == "execs" then
        opts.execs = {}
        for n in v:gmatch("[^,]+") do
          local x = tonumber(n)
          if not x or x ~= math.floor(x) or x < 1 or x > 999 then return nil, "execs must be executor numbers 1..999" end
          opts.execs[#opts.execs + 1] = x
        end
        if #opts.execs > MAX_BOUND_EXECUTORS then return nil, string.format("execs: at most %d executors (got %d)", MAX_BOUND_EXECUTORS, #opts.execs) end
      else return nil, "unknown option '" .. tok .. "'" end
    else return nil, "unknown token '" .. tok .. "'" end
  end
  if opts.bankToken then return nil, "bank: expected bank=<quickey>/<page>.<first>[-<last>], \"bank status\", \"bank verify\" or \"bank teardown\"" end
  if opts.encToken then return nil, "encmacro: expected encmacro=<macro number>, \"encmacro status\" or \"encmacro teardown\"" end
  if opts.bankCodes and not opts.bank then return nil, "bankcodes needs a bank=... range in the same argument" end
  if opts.bankStatic and not opts.bank then return nil, "static needs a bank=... range in the same argument (one executor per code)" end
  if opts.routes and (opts.input == "fake" or opts.input == "off") then return nil, "route= needs input=keyboard, quickey or mixed" end
  return opts
end

-------------------------------------------------------------------------------
-- Encoding and sending
-------------------------------------------------------------------------------
local function encodePacket(obj)
  local text = json.encode(obj)
  return MAGIC .. " " .. text
end
state._encodePacket = encodePacket

-- Returns the parsed object or nil, reason. The caller checks the loopback source first.
local function decodePacket(data, maxLen)
  if type(data) ~= "string" then return nil, "not a string" end
  if #data > (maxLen or DEFAULTS.maxDatagram) then return nil, "oversized" end
  if data:sub(1, 5) ~= MAGIC .. " " then return nil, "bad-magic" end
  local text = data:sub(6)
  local ok, obj = pcall(json.decode, text)
  if not ok or type(obj) ~= "table" then return nil, "bad-json" end
  if type(obj.t) ~= "string" then return nil, "no-type" end
  return obj
end
state._decodePacket = decodePacket

local function sendTo(ip, port, obj)
  local data = encodePacket(obj)
  state.counters.sent = state.counters.sent + 1
  local ok, err = pcall(function() return state.sock:sendto(data, ip, port) end)
  if not ok or err == nil then state.counters.sendErrors = state.counters.sendErrors + 1 end
end

local function sendToSession(s, obj)
  s.outSeq = s.outSeq + 1
  obj.sid, obj.seq = s.sid, s.outSeq
  sendTo(s.ip, s.port, obj)
end

-------------------------------------------------------------------------------
-- Sessions
-------------------------------------------------------------------------------
local function hk() local r = state.modules.hardkeys; return r and r.instance end
local function fb() local r = state.modules.feedback; return r and r.instance end
local function ctl() local r = state.modules.control; return r and r.instance end

local function closeSession(s, now, reason)
  local cinst = ctl()
  if cinst and s.controlOpen then
    -- KB-18: every gesture of the session is ended through the backend, queued motion is dropped.
    local ok, r = pcall(cinst.closeSession, cinst, s.sid, now, reason)
    if ok and type(r) == "table" then
      for _, e in ipairs(r.ended or {}) do log("session %s (%s) %s: %s on %s/%s ended (%s)", s.sid, s.id, reason, tostring(e.kind), tostring(e.device), tostring(e.control), tostring(e.outcome)) end
      for _, u in ipairs(r.unresolved or {}) do logerr("session %s (%s) %s: %s release on %s/%s UNRESOLVED: %s", s.sid, s.id, reason, tostring(u.kind), tostring(u.device), tostring(u.control), tostring(u.error)) end
      if (r.dropped or 0) > 0 then log("session %s (%s) %s: %d queued intent(s) dropped, never applied late", s.sid, s.id, reason, r.dropped) end
    elseif not ok then logerr("closing control session %s failed: %s", s.sid, tostring(r)) end
  end
  s.controlOpen = false
  local inst = hk()
  if inst and s.sessionOpen then
    local ok, r = pcall(inst.closeSession, inst, s.sid, now, reason)
    if ok and type(r) == "table" then
      for _, a in ipairs(r.unresolved or {}) do logerr("session %s (%s) %s: release of %s UNRESOLVED: %s", s.sid, s.id, reason, tostring(a.logical or a.tupleKey), tostring(a.error)) end
      if #(r.released or {}) > 0 then log("session %s (%s) %s: released %d key(s)", s.sid, s.id, reason, #r.released) end
    elseif not ok then logerr("closing session %s failed: %s", s.sid, tostring(r)) end
  end
  s.sessionOpen = false
  s.state = "closed"
  state.sessions[s.sid] = nil
  if state.byId[s.id] == s then state.byId[s.id] = nil end
  log("session %s (%s from %s:%d) closed: %s", s.sid, s.id, s.ip, s.port, reason)
end

-------------------------------------------------------------------------------
-- KB-30: the encoder-bank macro (Swap Prog). One line, "Select EncoderBank", Execute No: running it places the text on
-- the operator's command line (onPC 2.5.1 appends whatever AddToCmdline says, so the plugin clears the line first).
-- The macro is the plugin's when its Note carries ENCBANK_MARK; anything else at that number is never changed.
-------------------------------------------------------------------------------
local function objectAt(ref)
  local ok, l = pcall(ObjectList, ref)
  if ok and type(l) == "table" then return l[1] end
  return nil
end
local function propOf(obj, k)
  local ok, v = pcall(function() return obj:Get(k) end)
  if ok then return v end
  return nil
end
local function yesValue(v)
  local t = tostring(v):lower()
  return v == true or t == "yes" or t == "true" or t == "on" or t == "1"
end

-- nil, code, why when Macro n is not the plugin's own one-line macro (missing, foreign or changed); true otherwise.
local function verifyEncMacro(n)
  local m = objectAt("Macro " .. n)
  if not m then return nil, "missing", "Macro " .. n .. " does not exist" end
  local note = tostring(propOf(m, "Note") or "")
  if note ~= ENCBANK_MARK then return nil, "not-owned", string.format("Macro %d '%s' is not this plugin's encoder-bank macro (note '%s')", n, tostring(m.name), note) end
  local okC, count = pcall(function() return m:Count() end)
  if not okC or count ~= 1 then return nil, "changed", string.format("Macro %d has %s line(s), expected 1", n, okC and tostring(count) or "an unreadable number of") end
  local line = objectAt("Macro " .. n .. ".1")
  if not line then return nil, "changed", "Macro " .. n .. ".1 is not readable" end
  local command, execute, enabled = propOf(line, "Command"), propOf(line, "Execute"), propOf(line, "Enabled")
  if tostring(command) ~= ENCBANK_COMMAND then return nil, "changed", string.format("Macro %d.1 reads '%s', expected '%s'", n, tostring(command), ENCBANK_COMMAND) end
  if execute == nil or yesValue(execute) then return nil, "changed", string.format("Macro %d.1 has Execute %s, expected No (it would run the command instead of placing it)", n, tostring(execute)) end
  if enabled ~= nil and not yesValue(enabled) then return nil, "changed", string.format("Macro %d.1 is disabled", n) end
  return true
end

-- The operator's argument encmacro=<n> is the authorization: an empty number gets the plugin's macro, the plugin's own
-- macro is adopted, anything else is reported and left alone (Swap Prog then stays unsupported).
local function provisionEncMacro(n)
  state.encMacroActive = false
  local existing = objectAt("Macro " .. n)
  if existing then
    local ok, code, why = verifyEncMacro(n)
    if not ok then logerr("encmacro: refused [%s]: %s; Swap Prog stays unsupported (choose another number, or fix or delete that macro)", code, why); return false end
    state.encMacroNo, state.encMacroActive = n, true
    log("encmacro: Macro %d adopted (%s, Execute No); Swap Prog enters %s", n, ENCBANK_COMMAND, ENCBANK_COMMAND)
    return true
  end
  local function fail(why)
    logerr("encmacro: creating Macro %d failed: %s; Swap Prog stays unsupported", n, why)
    if objectAt("Macro " .. n) then
      local fb = Cmd("Delete Macro " .. n .. " /NoConfirmation")
      if objectAt("Macro " .. n) then logerr("encmacro: Macro %d (created by this start) could not be removed again (%s); delete it by hand", n, tostring(fb)) end
    end
    return false
  end
  local fb = Cmd("Store Macro " .. n)
  local m = objectAt("Macro " .. n)
  if not m then return fail("Store Macro " .. n .. " answered " .. tostring(fb) .. " and no macro exists") end
  local okS, serr = pcall(function() m:Set("Name", ENCBANK_NAME); m:Set("Note", ENCBANK_MARK) end)
  if not okS then return fail("setting its name and note raised " .. tostring(serr)) end
  fb = Cmd("Store Macro " .. n .. ".1")
  local line = objectAt("Macro " .. n .. ".1")
  if not line then return fail("Store Macro " .. n .. ".1 answered " .. tostring(fb) .. " and no line exists") end
  local okL, lerr = pcall(function() line:Set("Command", ENCBANK_COMMAND); line:Set("Execute", "No") end)
  if not okL then return fail("setting its line raised " .. tostring(lerr)) end
  local ok, code, why = verifyEncMacro(n)
  if not ok then return fail("read back [" .. tostring(code) .. "]: " .. tostring(why)) end
  state.encMacroNo, state.encMacroActive = n, true
  log("encmacro: Macro %d created ('%s': %s, Execute No); Swap Prog enters %s", n, ENCBANK_NAME, ENCBANK_COMMAND, ENCBANK_COMMAND)
  return true
end

-- Deletes the verified owned macro (Plugin "mtpnxk_surface" "encmacro teardown").
local function teardownEncMacro()
  local n = state.encMacroNo
  if not n then log("encmacro teardown: no encoder-bank macro is known (start once with encmacro=<n>)"); return end
  local m = objectAt("Macro " .. n)
  if not m then state.encMacroNo, state.encMacroActive = nil, false; log("encmacro teardown: Macro %d does not exist any more; nothing deleted", n); return end
  if tostring(propOf(m, "Note") or "") ~= ENCBANK_MARK then logerr("encmacro teardown refused: Macro %d '%s' is not this plugin's (note '%s'); nothing deleted", n, tostring(m.name), tostring(propOf(m, "Note"))); return end
  local fb = Cmd("Delete Macro " .. n .. " /NoConfirmation")
  if objectAt("Macro " .. n) then logerr("encmacro teardown: Delete Macro %d answered %s and the macro is still there", n, tostring(fb)); return end
  state.encMacroNo, state.encMacroActive = nil, false
  log("encmacro teardown: Macro %d deleted; Swap Prog is unsupported until a start with encmacro=<n>", n)
end

local function logEncMacro()
  local n = state.encMacroNo
  if not n then log("encmacro: none (start with encmacro=<n>)"); return end
  local ok, code, why = verifyEncMacro(n)
  if ok then log("encmacro: Macro %d verified (%s, Execute No); Swap Prog %s", n, ENCBANK_COMMAND, state.encMacroActive and "enters it" or "is not active in this run")
  else logerr("encmacro: Macro %d [%s]: %s", n, tostring(code), tostring(why)) end
end

-- Resolution of every surface key through the routing policy (KB-15: the method decided per key, the
-- route it selects now and the backend part that would press it), reported in the welcome as two sorted
-- name lists (the datagram stays small) and logged per key at start. A route is usable only when the
-- module reports it supported with no unavailable requirement; the reason is logged, never guessed.
local function describeKeys(logReasons)
  local inst = hk()
  local ok, unsupported, reasons, routes = {}, {}, {}, {}
  for name, logical in pairs(NXK_KEYS) do
    local supported, reason = false, "hardkeys module not loaded"
    if inst then
      local okC, r = pcall(inst.describeRoute, inst, logical, { prefer = NXK_PREFER[name] })
      if okC and type(r) == "table" then
        -- The surface holds every key (press/release pairs), so a Quickey code needs KB-10 hold evidence.
        local qc = r.method == "quickkey" and type(r.quickkeyCapabilities) == "table" and r.quickkeyCapabilities or nil
        -- KB-29: a direct-tap code is tapped on key-down (Press Quickey N), so it needs no hold evidence.
        local direct = qc ~= nil and qc.direct == true and state.tapsMode ~= "off"
        if r.supported and #(r.unavailable or {}) == 0 and qc and qc.hold == false and not direct then
          reason = "Quickey code " .. tostring(r.quickkey) .. " has no KB-10 hold evidence" .. (qc.note and (" (" .. qc.note .. ")") or "") .. "; the surface holds every key, route it with shortcut"
        elseif r.supported and #(r.unavailable or {}) == 0 then supported = true
        elseif r.supported then reason = "unavailable: " .. table.concat(r.unavailable, "; ")
        else reason = tostring(r.reason or r.code) end
        routes[name] = { method = r.method, source = r.methodSource, effective = r.effective, backend = r.dispatchBackend, supported = supported, direct = direct or nil }
        if supported and logReasons then
          local res = r.resolution
          log("key %s: %s via %s (%s%s)%s%s", name, logical, tostring(r.method), tostring(r.dispatchBackend or r.backend or "no backend"),
              r.effective and (", " .. tostring(r.effective)) or "", (res and res.prefer) and (" preferred row " .. tostring(res.prefer) .. " (" .. tostring(res.shortcut) .. ")") or "",
              direct and ", direct tap" or "")
        end
      else reason = tostring(r) end
    end
    if supported then ok[#ok + 1] = name else unsupported[#unsupported + 1] = name; reasons[name] = reason end
  end
  state.routes = routes
  for name in pairs(NXK_LOCAL) do
    if name == ENCBANK_KEY and state.encMacroActive then
      ok[#ok + 1] = name
      routes[name] = { method = "encmacro", supported = true, macro = state.encMacroNo }
      if logReasons then log("key %s: %s via Macro %d (Execute No; the operator types the bank and Enter)", name, ENCBANK_COMMAND, state.encMacroNo) end
    else
      unsupported[#unsupported + 1] = name
      reasons[name] = (name == ENCBANK_KEY) and "not a console key; with encmacro=<n> it enters " .. ENCBANK_COMMAND .. " (KB-30)" or "not a console key"
    end
  end
  table.sort(ok); table.sort(unsupported)
  if logReasons then
    for _, name in ipairs(unsupported) do log("key %s unsupported: %s", name, reasons[name]) end
    log("keys: %d supported, %d unsupported", #ok, #unsupported)
  end
  state.keyReasons = reasons
  return { ok = ok, unsupported = unsupported }
end

local function consoleInfo()
  local info = {}
  pcall(function() info.version = tostring(Version and Version() or nil) end)
  pcall(function() local b = BuildDetails and BuildDetails(); if b then info.build = tostring(b.ProductVersion or b.Version or b.MajorVersion) end end)
  return info
end

local function openSession(obj, ip, port, now)
  if type(obj.id) ~= "string" or obj.id == "" or #obj.id > 64 then return nil, "bad-id" end
  if type(obj.nonce) ~= "string" or #obj.nonce < 8 or #obj.nonce > 32 then return nil, "bad-nonce" end
  if obj.v ~= PROTOCOL then return nil, "bad-version" end
  if state.nonces[obj.nonce] then state.counters.replayedHello = state.counters.replayedHello + 1; return nil, "replayed-hello" end
  state.nonces[obj.nonce] = now
  local held = {}
  if type(obj.held) == "table" then
    if #obj.held > 16 then return nil, "bad-held" end
    for _, k in ipairs(obj.held) do if type(k) ~= "string" then return nil, "bad-held" end; held[#held + 1] = k end
  end
  -- A new hello for a known surface id replaces its session: a restarted service inherits nothing.
  local old = state.byId[obj.id]
  if old then closeSession(old, now, "replaced-by-hello") end
  local sid = randomHex(8)
  while state.sessions[sid] do sid = randomHex(8) end
  local s = { sid = sid, id = obj.id, gen = obj.gen, ip = ip, port = port, openedAt = now, lastSeen = now, lastSeq = 0, outSeq = 0,
              holds = {}, taps = {}, evSeen = {}, evOrder = {}, evAck = {}, lastEvByKey = {}, keyQueued = {}, rate = { since = now, count = 0 }, state = "active",
              unsyncedHeld = held, lastFullAt = nil, lastState = nil, sessionOpen = false, surface = obj.surface, fw = obj.fw }
  local inst = hk()
  if inst and state.inputEnabled then
    local r, err = inst:openSession({ id = sid, leaseMs = DEFAULTS.leaseMs, label = obj.id, binding = ip .. ":" .. tostring(port) }, now)
    if not r then return nil, "session-open-failed: " .. tostring(err and err.message) end
    s.sessionOpen = true
  end
  local cinst = ctl()
  if cinst and state.controlEnabled then
    local r, err = cinst:openSession({ id = sid, leaseMs = DEFAULTS.leaseMs, label = obj.id, binding = ip .. ":" .. tostring(port) }, now)
    if not r then
      if s.sessionOpen then pcall(inst.closeSession, inst, sid, now, "control-session-failed") end
      return nil, "control-session-open-failed: " .. tostring(err and err.message)
    end
    s.controlOpen = true
    s.ctlLastEvByControl = {}
  end
  state.sessions[sid] = s
  state.byId[obj.id] = s
  log("session %s opened for %s (%s fw %s, surface gen %s) from %s:%d; %d key(s) physically down, not pressed", sid, obj.id, tostring(obj.surface), tostring(obj.fw), tostring(obj.gen), ip, port, #held)
  local welcome = { t = "welcome", v = PROTOCOL, nonce = obj.nonce, gen = state.gen, lease = DEFAULTS.leaseMs, hb = DEFAULTS.hbMs,
                    keys = describeKeys(false), modules = state.moduleVersions, console = consoleInfo(), input = state.inputMode, backend = state.backendName,
                    plugin = VERSION, epoch = fb() and fb():epoch() or nil,
                    context = (fb() and type(fb().contextSnapshot) == "function") and 1 or 0,
                    -- KB-18: whether ctl events are admitted, and through which backend
                    control = (ctl() and state.controlEnabled) and 1 or 0, controlBackend = state.controlMode }
  sendToSession(s, welcome)
  s.lastFullAt = nil  -- the next tick sends a full state
  return s
end

-- Per-session packet admission: sequence, rate. Renews the lease on success.
local function admitPacket(s, obj, now)
  if type(obj.seq) ~= "number" or obj.seq ~= math.floor(obj.seq) or obj.seq <= s.lastSeq then
    state.counters.oldSeq = state.counters.oldSeq + 1
    return false, "old-seq"
  end
  if now - s.rate.since >= 1 then s.rate.since, s.rate.count = now, 0 end
  s.rate.count = s.rate.count + 1
  if s.rate.count > DEFAULTS.maxPacketsPerSecond then state.counters.rateDropped = state.counters.rateDropped + 1; return false, "rate" end
  s.lastSeq = obj.seq
  local revived = false
  if s.state == "expired" then s.state = "active"; revived = true end
  s.lastSeen = now
  local inst = hk()
  if inst and s.sessionOpen then
    local r, err = inst:renewSession(s.sid, now, DEFAULTS.leaseMs)
    if not r then
      -- The module forgot the session (closed with nothing unresolved): reopen it. Nothing is re-pressed.
      local r2, err2 = inst:openSession({ id = s.sid, leaseMs = DEFAULTS.leaseMs, label = s.id, binding = s.ip .. ":" .. tostring(s.port) }, now)
      if not r2 then logerr("session %s: lease renewal and reopen failed: %s / %s", s.sid, tostring(err and err.message), tostring(err2 and err2.message)) end
    end
  end
  local cinst = ctl()
  if cinst and state.controlEnabled then
    -- The control lease lapses like the hardkeys one (its gestures were ended, nothing is resumed); a
    -- packet reopens the session so the next gesture is admitted on its own merits.
    local r = s.controlOpen and cinst:renewSession(s.sid, now, DEFAULTS.leaseMs) or nil
    if not r then
      local r2, err2 = cinst:openSession({ id = s.sid, leaseMs = DEFAULTS.leaseMs, label = s.id, binding = s.ip .. ":" .. tostring(s.port) }, now)
      if r2 then s.controlOpen = true; s.ctlLastEvByControl = {}
      else s.controlOpen = false; logerr("session %s: control lease renewal and reopen failed: %s", s.sid, tostring(err2 and err2.message)) end
    end
  end
  return true, revived
end

-- Event-id deduplication: a window of the last evWindow ids per session. A seen event is answered with
-- its ORIGINAL acknowledgment (a refusal stays a refusal) and is never dispatched again.
local function seenEvent(s, ev)
  if s.evSeen[ev] then return true end
  s.evSeen[ev] = true
  s.evOrder[#s.evOrder + 1] = ev
  if #s.evOrder > DEFAULTS.evWindow then
    local old = table.remove(s.evOrder, 1)
    s.evSeen[old] = nil
    s.evAck[old] = nil
  end
  return false
end

local function replayAck(s, ev)
  state.counters.dupEvents = state.counters.dupEvents + 1
  local a = s.evAck[ev]
  if a then
    local copy = {}
    for k, v in pairs(a) do copy[k] = v end
    copy.dup = 1
    sendToSession(s, copy)
  else
    -- Seen but its outcome already left the window: say so rather than invent a success.
    sendToSession(s, { t = "ack", ev = ev, ok = 0, dup = 1, code = "outcome-expired", why = "event already processed; its outcome is no longer remembered" })
  end
end

-- Event ids are monotonic per service start. A key event older than the newest one already processed
-- for the same surface key is superseded: a retransmitted press arriving after its release, or a stale
-- release arriving after a newer press, must not act. Nothing is dispatched for it.
local function superseded(s, name, ev)
  local last = s.lastEvByKey[name]
  if last and ev < last then return true end
  s.lastEvByKey[name] = ev
  return false
end

-- Drops hold bookkeeping for holds the module released on its own (deadlines, lease expiry).
local function pruneReleased(s, releasedIds)
  for name, hid in pairs(s.holds) do
    if releasedIds[hid] then s.holds[name] = nil end
  end
end

local function ackEvent(s, ev, ok, extra)
  local a = { t = "ack", ev = ev, ok = ok and 1 or 0 }
  if extra then for k, v in pairs(extra) do a[k] = v end end
  local keep = {}
  for k, v in pairs(a) do keep[k] = v end
  s.evAck[ev] = keep
  sendToSession(s, a)
end

-------------------------------------------------------------------------------
-- Packet handlers
-------------------------------------------------------------------------------
-- KB-30: Swap Prog. Key-down: re-verify the owned macro (a show load or an edit may have changed what that number is),
-- clear text already on the command line, run the macro, and gate the key queue until the line reads back. The key is
-- acknowledged at dispatch, as every key is; the read-back is the gate's (see serviceKeyGate). Key-up: nothing to do.
local function encoderBankKey(s, ev, down, now)
  if down == 0 then ackEvent(s, ev, true, { noop = 1 }); return nil end
  local n = state.encMacroNo
  if not state.encMacroActive or not n then
    ackEvent(s, ev, false, { code = "unsupported", why = "no encoder-bank macro: start the plugin with encmacro=<n> (KB-30)" }); return nil
  end
  if not state.inputEnabled or state.inputMode == "fake" then
    ackEvent(s, ev, false, { code = "input-disabled", why = "input is " .. tostring(state.inputMode) .. "; the command line is left alone" }); return nil
  end
  local ok, code, why = verifyEncMacro(n)
  if not ok then
    state.counters.encBankRefused = (state.counters.encBankRefused or 0) + 1
    ackEvent(s, ev, false, { code = code, why = why .. "; nothing run (restart with encmacro=<n> after fixing it)" }); return nil
  end
  local okT, text = pcall(state.commandText)
  if not okT then ackEvent(s, ev, false, { code = "unreadable", why = "the command line cannot be read: " .. tostring(text) }); return nil end
  local cleared = nil
  if type(text) == "string" and text:match("%S") then
    local okC, cerr = pcall(function() CmdObj():ClearCmd() end)
    if not okC then ackEvent(s, ev, false, { code = "clear-failed", why = "clearing '" .. text .. "' raised " .. tostring(cerr) .. "; nothing run" }); return nil end
    cleared = text
  end
  local fb = Cmd("Go+ Macro " .. n)
  if tostring(fb) ~= "OK" then
    state.counters.encBankRefused = (state.counters.encBankRefused or 0) + 1
    ackEvent(s, ev, false, { code = "refused", why = "Go+ Macro " .. n .. " answered " .. tostring(fb), cleared = cleared and 1 or nil }); return nil
  end
  state.keyGate = { untilT = now + DEFAULTS.encBankGateMs / 1000, at = now, macro = n }
  state.keyDownThisTick = true
  state.collectNextTick = true
  state.counters.encBank = (state.counters.encBank or 0) + 1
  ackEvent(s, ev, true, { encmacro = n, cleared = cleared and 1 or nil })
  return nil
end

-- KB-30: the gate opens once the command line starts with "Select EncoderBank" (the queued keys then follow in order),
-- or after encBankGateMs (counted and logged: the keys go on to whatever the line holds; the LED shows the truth).
local function encBankPendingText(t)
  return type(t) == "string" and t:lower():match("^%s*select%s+encoderbank") ~= nil
end
local function serviceKeyGate(now)
  local g = state.keyGate
  if not g then return end
  local okT, text = pcall(state.commandText)
  if okT and encBankPendingText(text) then
    state.keyGate = nil
    state.counters.encBankVerified = (state.counters.encBankVerified or 0) + 1
  elseif now >= g.untilT then
    state.keyGate = nil
    state.counters.encBankUnverified = (state.counters.encBankUnverified or 0) + 1
    logerr("Swap Prog: the command line reads '%s' %d ms after Go+ Macro %d, not '%s'; the queued keys go on", okT and tostring(text) or ("unreadable: " .. tostring(text)), math.floor((now - g.at) * 1000 + 0.5), g.macro, ENCBANK_COMMAND)
  end
end

local function handleKey(s, obj, now, fromQueue)
  local ev, name, down = obj.ev, obj.k, obj.d
  if type(ev) ~= "number" or ev ~= math.floor(ev) or ev < 0 then return "bad-ev" end
  if type(name) ~= "string" or (NXK_KEYS[name] == nil and not NXK_LOCAL[name]) then return "bad-key" end
  if down ~= 0 and down ~= 1 then return "bad-d" end
  if s.evSeen[ev] then replayAck(s, ev); return nil end
  -- KB-28: at most one key goes down per tick (Quickey presses in one console frame reorder in a focused text
  -- field). Once a key went down this tick, or while earlier key events wait, a key event joins the queue in
  -- arrival order and is acknowledged when it runs; its retransmissions are ignored meanwhile. Packets are still
  -- read and admitted every tick, so heartbeats and other messages are never held behind a burst.
  s.keyQueued = s.keyQueued or {}
  if s.keyQueued[ev] then return nil end
  -- KB-30: while the command line has not shown Swap Prog's "Select EncoderBank" yet, every key event waits as well.
  if not fromQueue and (state.keyDownThisTick or #state.keyQueue > 0 or state.keyGate) then
    if #state.keyQueue >= DEFAULTS.maxKeyQueue then
      state.counters.keyQueueOverflow = (state.counters.keyQueueOverflow or 0) + 1
      ackEvent(s, ev, false, { code = "overloaded", why = string.format("%d key events are already waiting for their tick; nothing dispatched", DEFAULTS.maxKeyQueue) })
      return nil
    end
    s.keyQueued[ev] = true
    state.keyQueue[#state.keyQueue + 1] = { s = s, obj = obj }
    state.counters.keyQueued = (state.counters.keyQueued or 0) + 1
    return nil
  end
  seenEvent(s, ev)
  if superseded(s, name, ev) then
    state.counters.superseded = state.counters.superseded + 1
    ackEvent(s, ev, false, { code = "superseded", why = "a newer event for this key was already processed; nothing dispatched" })
    return nil
  end
  if name == ENCBANK_KEY then return encoderBankKey(s, ev, down, now) end
  local logical = NXK_KEYS[name]
  if not logical then ackEvent(s, ev, false, { code = "unsupported", why = "not a console key" }); return nil end
  local inst = hk()
  if not inst or not state.inputEnabled then ackEvent(s, ev, false, { code = "input-disabled", why = "input is " .. tostring(state.inputMode) }); return nil end
  -- KB-29: a direct-tap key goes down as one tap (Press Quickey N): nothing is held, its key-up is only acknowledged.
  local route = state.routes and state.routes[name]
  if down == 1 and route and route.direct and state.tapsMode ~= "off" then
    -- bench: the command line before the tap, so the watch measures this tap's effect (review 1 of #13).
    local before = nil
    if state.bench and logical:match("^NUM%d$") then pcall(function() before = state.commandText() end) end
    local r, err = inst:tap(s.sid, now, { key = logical, display = state.display, prefer = NXK_PREFER[name] })
    if not r then
      state.counters.refused = state.counters.refused + 1
      ackEvent(s, ev, false, { code = tostring(err and err.code or "error"), why = tostring(err and err.message or err) })
      return nil
    end
    s.taps = s.taps or {}
    s.taps[name] = r.id
    state.counters.taps = (state.counters.taps or 0) + 1
    state.keyDownThisTick = true
    state.collectNextTick = true
    ackEvent(s, ev, true, { hold = r.id, tap = 1 })
    if state.bench and logical:match("^NUM%d$") then
      state.benchWatch[#state.benchWatch + 1] = { s = s, ev = ev, at = now, before = before, tickAt = state.counters.ticks }
    end
    return nil
  end
  if down == 0 and s.taps and s.taps[name] then
    local hid = s.taps[name]
    s.taps[name] = nil
    ackEvent(s, ev, true, { hold = hid, noop = 1, tap = 1 })
    return nil
  end
  if down == 1 then
    local r, err = inst:press(s.sid, now, { key = logical, display = state.display, prefer = NXK_PREFER[name] })
    if not r then
      state.counters.refused = state.counters.refused + 1
      ackEvent(s, ev, false, { code = tostring(err and err.code or "error"), why = tostring(err and err.message or err) })
      return nil
    end
    s.holds[name] = r.id
    state.counters.presses = state.counters.presses + 1
    if not r.duplicate then state.keyDownThisTick = true end
    state.collectNextTick = true  -- KB-24: read the console state on the next tick (the key's effect), not at the cadence
    if r.duplicate then ackEvent(s, ev, true, { hold = r.id, duplicate = 1 }) else ackEvent(s, ev, true, { hold = r.id }) end
    if state.bench and logical:match("^NUM%d$") then
      local before = nil
      pcall(function() before = state.commandText() end)
      state.benchWatch[#state.benchWatch + 1] = { s = s, ev = ev, at = now, before = before, tickAt = state.counters.ticks }
    end
  else
    local hid = s.holds[name]
    if not hid then ackEvent(s, ev, true, { noop = 1 }); return nil end
    local r, err = inst:release(s.sid, now, { hold = hid })
    s.holds[name] = nil
    if not r then
      if err and (err.code == "no-hold") then ackEvent(s, ev, true, { noop = 1 }); return nil end
      state.counters.refused = state.counters.refused + 1
      ackEvent(s, ev, false, { code = tostring(err and err.code or "error"), why = tostring(err and err.message or err), hold = hid })
      return nil
    end
    state.counters.releases = state.counters.releases + 1
    state.collectNextTick = true
    if r.state == "unresolved" then
      logerr("session %s: release of %s (%s) unresolved: %s", s.sid, name, hid, tostring(r.unresolved and r.unresolved.reason))
      ackEvent(s, ev, false, { code = "unresolved", why = tostring(r.unresolved and r.unresolved.reason), hold = hid })
    else
      ackEvent(s, ev, true, { hold = hid, outcome = r.releaseOutcome, alreadyReleased = r.alreadyReleased and 1 or nil })
    end
  end
  return nil
end

local function handleWheel(s, obj, now)
  local ev = obj.ev
  if type(ev) ~= "number" or ev ~= math.floor(ev) or ev < 0 then return "bad-ev" end
  if type(obj.w) ~= "number" or obj.w < 1 or obj.w > 4 or obj.w ~= math.floor(obj.w) then return "bad-wheel" end
  if type(obj.dx) ~= "number" or obj.dx < -127 or obj.dx > 127 then return "bad-dx" end
  if seenEvent(s, ev) then replayAck(s, ev); return nil end
  state.counters.wheels = state.counters.wheels + 1
  -- No verified Lua route for encoder input exists yet (docs/surface-protocol.md section 7).
  ackEvent(s, ev, false, { code = "unsupported", why = "wheel input has no verified console route yet" })
  return nil
end

-- KB-18: a continuous-control event. {t:"ctl", ev, k: rel|abs|touch|btn, dev, c, es, cg?, gs?, tgt, dx|v|d, fine?, tk?}.
-- Validation first (a malformed packet is rejected, not acknowledged); event-id deduplication as for keys;
-- for touches and buttons the per-control event-id order (a stale press after its release never acts;
-- motion is ordered by the module's per-device sequence `es`); then the vendored module admits it:
-- the binding generation, the target, the per-device order (loss reported, duplicates and reordering
-- refused), rate, coalescing and bounds are its rules, and its outcome is the acknowledgment.
local CTL_KINDS = { rel = "relative", abs = "absolute", touch = "touch", btn = "button" }
local CTL_ELEMENTS = { fader = true, key = true, encoder = true }
local function handleCtl(s, obj, now)
  local ev = obj.ev
  if type(ev) ~= "number" or ev ~= math.floor(ev) or ev < 0 then return "bad-ev" end
  local kind = CTL_KINDS[obj.k]
  if not kind then return "bad-kind" end
  if type(obj.dev) ~= "string" or obj.dev == "" or #obj.dev > 32 then return "bad-dev" end
  if type(obj.c) ~= "string" or obj.c == "" or #obj.c > 32 then return "bad-control" end
  if type(obj.es) ~= "number" or obj.es ~= math.floor(obj.es) or obj.es < 1 then return "bad-es" end
  if obj.cg ~= nil and (type(obj.cg) ~= "number" or obj.cg ~= math.floor(obj.cg)) then return "bad-cg" end
  if obj.gs ~= nil and (type(obj.gs) ~= "number" or obj.gs ~= math.floor(obj.gs) or obj.gs < 0) then return "bad-gs" end
  if obj.fine ~= nil and obj.fine ~= 0 and obj.fine ~= 1 then return "bad-fine" end
  if obj.tk ~= nil and (obj.k ~= "abs" or (obj.tk ~= 0 and obj.tk ~= 1)) then return "bad-tk" end  -- KB-20: takeover is a position's flag
  local t = obj.tgt
  if type(t) ~= "table" then return "bad-target" end
  local target
  if t.slot ~= nil then
    if type(t.slot) ~= "number" or t.slot ~= math.floor(t.slot) or t.slot < 1 or t.slot > 8 then return "bad-target" end
    target = { slot = t.slot }
  elseif t.ex ~= nil then
    if type(t.ex) ~= "number" or t.ex ~= math.floor(t.ex) or t.ex < 1 or not CTL_ELEMENTS[t.el] then return "bad-target" end
    -- KB-21: an executor bound to an explicit page carries it; the module resolves by (page, executor).
    if t.pg ~= nil and (type(t.pg) ~= "number" or t.pg ~= math.floor(t.pg) or t.pg < 1) then return "bad-target" end
    target = { executor = t.ex, element = t.el, page = t.pg }
  else return "bad-target" end
  -- KB-21: the binding revision the event was produced against (the context's bind.br); the module refuses an older one stale-binding.
  if obj.br ~= nil and (type(obj.br) ~= "number" or obj.br ~= math.floor(obj.br) or obj.br < 0) then return "bad-br" end
  local event = { type = kind, device = obj.dev, control = obj.c, seq = obj.es, generation = obj.cg, gesture = obj.gs, target = target, fine = obj.fine == 1 or nil, binding = obj.br }
  if kind == "relative" then
    if type(obj.dx) ~= "number" or obj.dx ~= math.floor(obj.dx) or obj.dx == 0 or math.abs(obj.dx) > DEFAULTS.maxCtlDelta then return "bad-dx" end
    event.delta = obj.dx
  elseif kind == "absolute" then
    if type(obj.v) ~= "number" or obj.v < 0 or obj.v > 1 then return "bad-v" end
    event.value = obj.v
    if obj.tk == 1 then event.takeover = true end
  else
    if obj.d ~= 0 and obj.d ~= 1 then return "bad-d" end
    event.down = obj.d == 1
  end
  if seenEvent(s, ev) then replayAck(s, ev); return nil end
  state.counters.ctl = state.counters.ctl + 1
  if kind == "touch" or kind == "button" then
    if superseded(s, "ctl:" .. obj.dev .. "/" .. obj.c, ev) then
      state.counters.superseded = state.counters.superseded + 1
      ackEvent(s, ev, false, { code = "superseded", why = "a newer touch/button event for this control was already processed; nothing dispatched" })
      return nil
    end
  end
  local inst = ctl()
  if not inst or not state.controlEnabled or not s.controlOpen then
    ackEvent(s, ev, false, { code = "control-disabled", why = "continuous control is " .. tostring(state.controlMode or "off") })
    return nil
  end
  local r, err = inst:submit(s.sid, now, event)
  if not r then
    state.counters.ctlRefused = state.counters.ctlRefused + 1
    local extra = { code = tostring(err and err.code or "error"), why = tostring(err and err.message or err) }
    if err and err.generation ~= nil then extra.cg = err.generation end
    if err and err.owner ~= nil then extra.owner = err.owner end
    ackEvent(s, ev, false, extra)
    return nil
  end
  if (r.lost or 0) > 0 then state.counters.ctlLost = state.counters.ctlLost + r.lost end
  if r.coalesced then state.counters.ctlCoalesced = state.counters.ctlCoalesced + 1 end
  -- KB-24 hardware pass: a playback key on an executor changes what that executor reports (running, stopped); read it
  -- every tick for refreshMs instead of waiting for its turn in the feedback round robin (feedback 0.8.0 refresh).
  if kind == "button" and target.executor ~= nil then
    state.refreshExec = state.refreshExec or {}
    state.refreshExec[tostring(target.executor) .. "|" .. tostring(target.page)] = { executor = target.executor, page = target.page, untilT = now + DEFAULTS.refreshMs / 1000 }
  end
  local extra = { lost = (r.lost or 0) > 0 and r.lost or nil, lu = r.lostUncertain and 1 or nil, coalesced = r.coalesced and 1 or nil, superseded = r.superseded, queued = r.queued, noop = r.noop and 1 or nil, boundary = r.boundary and 1 or nil, evicted = r.evicted }
  ackEvent(s, ev, true, extra)
  return nil
end

-- Heartbeat: the service's view of what is physically held. Lost releases are reconciled here; lost
-- presses are reported and never pressed late.
local function handleHeartbeat(s, obj, now, revived)
  local held = {}
  if obj.held ~= nil then
    if type(obj.held) ~= "table" or #obj.held > 16 then return "bad-held" end
    for _, k in ipairs(obj.held) do if type(k) ~= "string" then return "bad-held" end; held[k] = true end
  end
  local inst = hk()
  local releasedNow = 0
  for name, hid in pairs(s.holds) do
    if not held[name] then
      s.holds[name] = nil
      if inst and state.inputEnabled then
        local r = inst:release(s.sid, now, { hold = hid })
        if r and not r.alreadyReleased then releasedNow = releasedNow + 1; state.counters.lostReleases = state.counters.lostReleases + 1
          log("session %s: %s released by heartbeat reconciliation (its release event never arrived)", s.sid, name) end
      end
    end
  end
  local pluginHeld, unsynced = {}, {}
  for name in pairs(s.holds) do pluginHeld[#pluginHeld + 1] = name end
  for name in pairs(held) do if not s.holds[name] then unsynced[#unsynced + 1] = name end end
  table.sort(pluginHeld); table.sort(unsynced)
  s.unsyncedHeld = unsynced
  local reply = { t = "hb", held = pluginHeld, unsynced = unsynced, gen = state.gen }
  -- KB-24: the loop's load since the last heartbeat reply (any session), then the window starts again.
  -- Review (PR #9): the window is rotated at the END of the tick that answers a heartbeat (so this tick's own
  -- duration, datagrams and budget state are kept), and the reply carries the window completed by the previous
  -- rotation. The first reply of a run therefore has no report; every tick is in exactly one report.
  if state.loadReport then
    reply.load = state.loadReport
    state.loadReport = nil
  end
  state.loadRotate = true
  if revived then reply.resynced = 1 end
  if releasedNow > 0 then reply.reconciled = releasedNow end
  sendToSession(s, reply)
  return nil
end

-- KB-21: the service's executor binding: the numbers its playback banks currently map to and the page they are
-- bound to (pg absent: the executors follow the user's current page; present: that page whatever the console
-- shows). The watch list is rebuilt around the new spec, the vendored control module turns the replaced binding
-- key into a new revision (queued motion dropped, holds rebound) and the ack carries that revision (br). Nothing
-- here changes the console's page or creates one; a page that does not exist is reported pageMissing per target.
local function feedbackHasPages()
  local FB = state.modules.feedback and state.modules.feedback.module
  if not FB or type(FB.contextItems) ~= "function" then return false end
  local ok, items = pcall(FB.contextItems, { executors = { 1 }, executorPage = 1 })
  return ok and type(items) == "table" and items[5] ~= nil and type(items[5].params) == "table" and items[5].params.page == 1
end
local rewatch
local function handleBind(s, obj, now)
  local ev = obj.ev
  if type(ev) ~= "number" or ev ~= math.floor(ev) or ev < 0 then return "bad-ev" end
  if type(obj.ex) ~= "table" then return "bad-bind" end
  local list, seen = {}, {}
  for i, n in ipairs(obj.ex) do
    if type(n) ~= "number" or n ~= math.floor(n) or n < 1 or n > 999 then return "bad-bind" end
    if not seen[n] then seen[n] = true; list[#list + 1] = n end
    if i > MAX_BOUND_EXECUTORS then break end
  end
  if obj.pg ~= nil and (type(obj.pg) ~= "number" or obj.pg ~= math.floor(obj.pg) or obj.pg < 1 or obj.pg > 9999) then return "bad-bind" end
  if seenEvent(s, ev) then replayAck(s, ev); return nil end
  if #obj.ex > MAX_BOUND_EXECUTORS then
    ackEvent(s, ev, false, { code = "bad-bind", why = string.format("%d executors exceed the bound of %d; the binding is unchanged", #obj.ex, MAX_BOUND_EXECUTORS) })
    return nil
  end
  if obj.pg ~= nil and not feedbackHasPages() then
    ackEvent(s, ev, false, { code = "unsupported", why = "the vendored feedback module has no explicit executor pages (0.5.0 or newer); bind without pg to follow the console's page" })
    return nil
  end
  table.sort(list)
  local prevExecs, prevPage = state.execs, state.execPage
  state.execs, state.execPage = list, obj.pg
  local w, werr = rewatch(now)
  if not w then
    -- The new list cannot be watched in full: the previous binding is restored and watched again.
    state.execs, state.execPage = prevExecs, prevPage
    rewatch(now)
    ackEvent(s, ev, false, { code = "bad-bind", why = "the binding cannot be watched in full (" .. tostring(werr) .. "); the previous binding is kept" })
    return nil
  end
  state.counters.binds = (state.counters.binds or 0) + 1
  local c = ctl()
  local info = c and type(c.bindingInfo) == "function" and c:bindingInfo(now) or nil
  log("bind: %d executor(s) %s (revision %s)", #list, obj.pg and ("on page " .. tostring(obj.pg) .. " (independent)") or "following the console's page", tostring(info and info.revision))
  ackEvent(s, ev, true, { br = info and info.revision or nil, cg = info and info.generation or nil, n = #list })
  -- KB-22: the binding revision changed, so the context (which carries it as bind.br, the value events are stamped
  -- with) goes out on the next tick instead of at the next periodic refresh: a press right after a bank change is
  -- not refused stale-binding for up to a second.
  for _, sess in pairs(state.sessions) do if sess.state == "active" then sess.lastContextAt = nil end end
  state.collectNow = true  -- KB-24: the state readers go out on this tick too
  return nil
end

local HANDLERS = {
  key = handleKey, wheel = handleWheel, hb = handleHeartbeat, ctl = handleCtl, bind = handleBind,
  bye = function(s, _, now) closeSession(s, now, "bye"); return nil end,
}

local function sendErr(ip, port, e, sid, now)
  local last = state.errReplies[ip]
  if last and now - last < DEFAULTS.errReplyMs / 1000 then return end
  state.errReplies[ip] = now
  sendTo(ip, port, { t = "err", e = e, sid = sid, gen = state.gen, seq = 0 })
end

local function handleDatagram(data, ip, port, now)
  local c = state.counters
  c.received = c.received + 1
  if type(data) ~= "string" or #data > DEFAULTS.maxDatagram then c.oversized = c.oversized + 1; return "oversized" end
  if ip ~= "127.0.0.1" then c.notAllowed = c.notAllowed + 1; return "not-local" end
  local obj, why = decodePacket(data)
  if not obj then
    c.rejected = c.rejected + 1
    return why
  end
  if obj.t == "hello" then
    local s, err = openSession(obj, ip, port, now)
    if not s then c.rejected = c.rejected + 1; return err end
    return "hello"
  end
  if type(obj.sid) ~= "string" then c.rejected = c.rejected + 1; return "no-sid" end
  local s = state.sessions[obj.sid]
  if not s then c.noSession = c.noSession + 1; sendErr(ip, port, "no-session", obj.sid, now); return "no-session" end
  if s.ip ~= ip or s.port ~= port then
    c.rejected = c.rejected + 1
    return "wrong-peer"
  end
  local handler = HANDLERS[obj.t]
  if not handler then c.rejected = c.rejected + 1; return "unknown-type" end
  local ok, revived = admitPacket(s, obj, now)
  if not ok then return revived end
  local err = handler(s, obj, now, revived)
  if err then c.rejected = c.rejected + 1; return err end
  return obj.t
end
state._handleDatagram = handleDatagram

-------------------------------------------------------------------------------
-- Feedback: watch list, tri-state collection, delta / full sending
-------------------------------------------------------------------------------
local contextSpec
local function watchItems()
  local FB = state.modules.feedback and state.modules.feedback.module
  if not FB then return {} end
  local spec = { readers = { "blind", "highlight", "solo", "previewMode", "previewBar", "maState", "commandText", "page", "shortcutsActive" }, displays = { state.display } }
  -- The KB-06 per-executor state items (x<n>, f<n>) follow the execs= argument only; the executors a service binds
  -- (KB-21, up to 96) are described by the context's ex[] (level, activity) without a second set of readers.
  if #state.execsArg > 0 then spec.executors = state.execsArg end
  local items = FB.itemsFor(spec)
  -- KB-17: the control-context items (data pool, page, encoder bank and slots of the configured display,
  -- one target per configured executor) are watched alongside, so the loop keeps them observed and the
  -- context message is built from the cache without any read of its own.
  -- Review (PR #6): through the INSTANCE, so its configured bound (96 executors) applies, not the module default (32).
  local f = fb()
  if f and type(f.contextItems) == "function" then
    for _, it in ipairs(f:contextItems(contextSpec())) do items[#items + 1] = it end
  end
  return items
end

local function tri(item)
  if not item or item.available ~= true or item.stale then return "?" end
  if item.value == true then return 1 elseif item.value == false then return 0 end
  return "?"
end

local function pendingOf(item)
  if not item or item.available ~= true or item.stale then return "?" end
  local word = tostring(item.value):match("^%s*(%a+)")
  if not word then return "" end
  return word:lower():sub(1, 16)
end

-- KB-30: 1 while the command line starts with "Select EncoderBank" (Swap Prog's LED), 0 otherwise, "?" when unknown.
local function encBankOf(item)
  if not item or item.available ~= true or item.stale then return "?" end
  return encBankPendingText(tostring(item.value)) and 1 or 0
end

-- One tri-state table from the feedback snapshot. "?" for unavailable, stale or identity-uncertain.
local function collectState(now)
  local inst = fb()
  if not inst then return {} end
  local snap = inst:snapshot(now)
  local by = {}
  for _, it in ipairs(snap.items) do by[it.key] = it end
  local uncertain = snap.identityUncertain ~= nil
  local function get(k) local it = by[k]; if uncertain and it then it = { available = it.available, value = it.value, stale = true } end return it end
  local s = {
    blind = tri(get("blind")), highlight = tri(get("highlight")), solo = tri(get("solo")),
    previewBar = tri(get("previewBar[display=" .. state.display .. "]")),
    ma = tri(get("maState")), shortcuts = tri(get("shortcutsActive")),
    pending = pendingOf(get("commandText")),
    ebank = encBankOf(get("commandText")),
    freeze = "?",
  }
  local pm = get("previewMode")
  if pm and pm.available == true and not pm.stale then
    s.previewEnv = tostring(pm.value)
    s.preview = PREVIEW_ENV[tostring(pm.value)] or "?"
  else s.preview = "?" end
  local pg = get("page")
  if pg and pg.available == true and not pg.stale and type(pg.value) == "table" and type(pg.value.no) == "number" then s.page = pg.value.no else s.page = "?" end
  for _, n in ipairs(state.execs) do
    local ex, fd = get("executor[executor=" .. n .. "]"), get("fader[executor=" .. n .. "]")
    if fd and fd.available == true and not fd.stale and type(fd.value) == "table" and type(fd.value.value) == "number" then
      s["f" .. n] = math.floor(fd.value.value + 0.5)
    else s["f" .. n] = "?" end
    -- Activity needs the assigned sequence; the executor reader gives the assignment only. Without a
    -- sequenceActive item per assigned object this stays unknown (M-Touch work).
    s["x" .. n] = (ex and ex.available == true and not ex.stale and ex.value and ex.value.empty == false) and "?" or "?"
  end
  return s
end
state._collectState = collectState

-- KB-17: the snapshot spec this plugin follows (the configured display and executors).
contextSpec = function()
  local spec = { display = state.display }
  if #state.execs > 0 then spec.executors = state.execs; spec.executorPage = state.execPage end
  return spec
end

-- KB-21: the watch list follows the bound executors (replaced on every bind).
-- Returns the watch result; a truncated list (the module's bound was smaller than the list) is an error the caller
-- must act on: an unobserved item means the context never becomes known.
rewatch = function(now)
  local f = fb()
  if not f then return nil, "no feedback instance" end
  local items = watchItems()
  local okW, w = pcall(f.watch, f, items, now)
  if not okW then logerr("feedback watch failed: %s", tostring(w)); return nil, tostring(w) end
  if (w.truncated or 0) > 0 then
    logerr("feedback: watch list of %d item(s) truncated by %d: the context would never be known", #items, w.truncated)
    return nil, string.format("watch list of %d items exceeds the feedback bound by %d", #items, w.truncated)
  end
  log("feedback: watching %d item(s)", w.watched)
  return w
end

-- KB-23: 0..1 components to "rrggbb".
local function hex6(r, g, b)
  local function c(x) x = tonumber(x) or 0; if x < 0 then x = 0 elseif x > 1 then x = 1 end; return math.floor(x * 255 + 0.5) end
  return string.format("%02x%02x%02x", c(r), c(g), c(b))
end

-- KB-23: what the service shows that is not in the generation: values, levels, activity, colours, the known flag.
local function feedbackDigest(msg)
  local parts = { tostring(msg.known), tostring(msg.enc and msg.enc.why) }
  for _, r in ipairs(msg.slots or {}) do parts[#parts + 1] = string.format("%s:%s/%s/%s/%s/%s", tostring(r.n), tostring(r.avail), tostring(r.val), tostring(r.abs), tostring(r.vc), tostring(r.col)) end
  for _, r in ipairs(msg.ex or {}) do parts[#parts + 1] = string.format("%s@%s:%s/%s/%s/%s/%s", tostring(r.n), tostring(r.pg), tostring(r.tgt), tostring(r.lvl), tostring(r.act), tostring(r.col), tostring(r.why)) end
  return table.concat(parts, "|")
end

-- The context message: a compact copy of the cached contextSnapshot. nil while the module is missing or
-- older than 0.3.0. `known` is 0 while any part is unobserved in this epoch (no generation is claimed then).
local function collectContext(now)
  local inst = fb()
  if not inst or type(inst.contextSnapshot) ~= "function" then return nil end
  local ok, snap = pcall(inst.contextSnapshot, inst, contextSpec(), now, { cached = true })
  if not ok then logerr("contextSnapshot raised: %s", tostring(snap)); return nil end
  local known = (snap.generationUnknown ~= true and snap.stale ~= true) and 1 or 0
  local msg = { t = "context", gen = state.gen, epoch = snap.epoch, known = known, cg = snap.generation, display = snap.display,
                -- why the context is not known: the module's reason (a part unobserved, the selection identity
                -- incomplete) or staleness; absent when known
                why = (known == 0) and (snap.generationNote or (snap.stale and "stale observations") or "unknown") or nil,
                pool = snap.identity and snap.identity.dataPool and snap.identity.dataPool.name or "?",
                page = snap.executorPage and snap.executorPage.no or "?" }
  -- KB-21: what this plugin binds (the service compares it with what it asked) and the revision events must carry.
  do
    local c = ctl()
    local info = c and type(c.bindingInfo) == "function" and c:bindingInfo(now) or nil
    msg.bind = { ex = state.execs, pg = state.execPage, br = info and info.revision or nil, mode = snap.executorMode or (state.execPage and "page" or "current") }
  end
  local e = snap.encoder
  if e and e.available and not e.stale then
    msg.enc = { bank = e.value.bank.index, bankName = e.value.bank.name or "?", page = e.value.page.index, pageName = e.value.page.name or "?", ctx = e.value.context or "?", attr = e.value.attributeEditing == true and 1 or 0 }
  else
    msg.enc = { why = e and (e.reason or e.error) or "not observed" }
  end
  local sl = snap.slots
  msg.slots = {}
  if sl and sl.available and not sl.stale then
    for _, x in ipairs(sl.value.slots) do
      local r = { n = x.slot, kind = x.kind }
      if x.kind == "attribute" then
        r.name, r.label, r.unit, r.readout, r.res, r.layer, r.cf = x.name, x.label or "", x.unit or "?", x.readout or "?", x.resolution or "?", x.layer or "?", x.channelFunction or ""
        r.avail, r.val = x.availability, x.valueState
        if x.absolute ~= nil then r.abs = x.absolute end
        -- KB-20 review: vc 1 only when every selected fixture was scanned and read (feedback valueComplete); a
        -- surface picks up a position against the value only then.
        if x.valueComplete ~= nil then r.vc = x.valueComplete and 1 or 0 end
        -- KB-23: the verified metadata a colour scheme rests on (feedback 0.6.0); the label above is textual only.
        if x.featureGroup then r.fg = x.featureGroup end
        if x.featureName then r.fn = x.featureName end
        if x.activationGroup then r.ag = x.activationGroup end
        if x.colorDefined == true then r.col = hex6(x.colorR, x.colorG, x.colorB) end
      elseif x.kind == "other" then r.ref = x.ref end
      msg.slots[#msg.slots + 1] = r
    end
    msg.sel = sl.value.selection and sl.value.selection.count or "?"
    if sl.value.selection and sl.value.selection.identityComplete == false then msg.selIncomplete = 1 end
  else
    msg.slotsWhy = sl and (sl.reason or sl.error) or "not observed"
  end
  msg.ex = {}
  for _, x in ipairs(snap.executors or {}) do
    local r = { n = x.params and x.params.executor }
    if x.params and x.params.page ~= nil then r.pg = x.params.page end
    if x.available and not x.stale then
      local v = x.value
      r.n = v.executor
      r.empty = v.empty and 1 or 0
      r.tgt = v.playbackTarget and 1 or 0
      -- KB-21: the explicit identity and layout: the page read, width, a covering neighbour, a missing page.
      if type(v.page) == "table" and v.page.no then r.pgno = v.page.no elseif type(v.page) == "number" then r.pgno = v.page end
      if v.width then r.w = v.width end
      if v.coveredBy then r.cov = v.coveredBy end
      if v.pageMissing then r.pm = 1 end
      -- Compact (KB-21: up to 96 executors in one datagram): empty fields and the plain "empty" reason are left out.
      if v.assigned then r.cls, r.name = v.assigned.class or "?", v.assigned.name or "?" end
      if v.functions then
        r.kp, r.fd = v.functions.keyPress or "", v.functions.fader or ""
        if v.functions.keyUnpress and v.functions.keyUnpress ~= "" then r.ku = v.functions.keyUnpress end
      end
      if v.level and type(v.level.value) == "number" then r.lvl = math.floor(v.level.value + 0.5); r.tok = v.level.token end
      if v.active ~= nil then r.act = v.active and 1 or 0 elseif not v.empty then r.act = "?" end
      if v.appearance and v.appearance.backRGBA then r.rgba = v.appearance.backRGBA end
      -- KB-23: the assignment colour as six hex digits (feedback 0.6.0 parsed the BackRGBA; absent when it could not)
      if v.appearance and type(v.appearance.r) == "number" then r.col = hex6(v.appearance.r, v.appearance.g, v.appearance.b) end
      if v.reason and v.reason ~= "the executor is empty" then r.why = v.reason end
    else
      r.why = x.reason or x.error or "not observed"
    end
    msg.ex[#msg.ex + 1] = r
  end
  return msg
end
state._collectContext = collectContext

local function sendContexts(now, msg)
  if not msg then return end
  for _, s in pairs(state.sessions) do
    if s.state == "active" then
      local due = s.lastContextAt == nil or (now - s.lastContextAt) >= DEFAULTS.fullStateMs / 1000
      local moved = s.lastContextGen ~= msg.cg or s.lastContextKnown ~= msg.known or s.lastContextEpoch ~= msg.epoch
      -- KB-23: what the service lights (values, levels, activity, colours) changed: resend after at most feedbackMinMs
      -- (coalesced: many changes within the interval are one message); the generation is not involved.
      local digest = feedbackDigest(msg)
      local changed = s.lastContextDigest ~= digest and (s.lastContextAt == nil or (now - s.lastContextAt) >= DEFAULTS.feedbackMinMs / 1000)
      if due or moved or changed then
        sendToSession(s, msg)
        s.lastContextAt, s.lastContextGen, s.lastContextKnown, s.lastContextEpoch, s.lastContextDigest = now, msg.cg, msg.known, msg.epoch, digest
        state.counters.contexts = state.counters.contexts + 1
        if changed and not (due or moved) then state.counters.feedbackContexts = (state.counters.feedbackContexts or 0) + 1 end
      end
    end
  end
end

local function stateChanged(a, b)
  if a == nil or b == nil then return true end
  for k, v in pairs(a) do if b[k] ~= v then return true end end
  for k, v in pairs(b) do if a[k] ~= v then return true end end
  return false
end

local function deltaOf(prev, cur)
  local d = {}
  for k, v in pairs(cur) do if prev[k] ~= v then d[k] = v end end
  for k in pairs(prev) do if cur[k] == nil then d[k] = "?" end end
  return d
end

local function sendStates(now)
  local epoch = fb() and fb():epoch() or 0
  local cur = state.lastState
  for _, s in pairs(state.sessions) do
    if s.state == "active" then
      local full = s.lastFullAt == nil or (now - s.lastFullAt) >= DEFAULTS.fullStateMs / 1000 or s.lastEpoch ~= epoch
      if full then
        sendToSession(s, { t = "state", gen = state.gen, epoch = epoch, full = 1, s = cur })
        s.lastFullAt, s.lastState, s.lastEpoch = now, cur, epoch
        state.counters.fullStates = state.counters.fullStates + 1
      elseif stateChanged(s.lastState, cur) then
        sendToSession(s, { t = "state", gen = state.gen, epoch = epoch, full = 0, s = deltaOf(s.lastState, cur) })
        s.lastState = cur
        state.counters.deltaStates = state.counters.deltaStates + 1
      end
    end
  end
end

-------------------------------------------------------------------------------
-- Bench mode: press-to-effect timing for NUM taps (the command line is the observable)
-------------------------------------------------------------------------------
local function serviceBench(now)
  if #state.benchWatch == 0 then return end
  local keep = {}
  for _, w in ipairs(state.benchWatch) do
    -- frames since the dispatching iteration (0 = the same iteration, 1 = the next frame)
    w.frames = state.counters.ticks - w.tickAt - 1
    local text = nil
    pcall(function() text = state.commandText() end)
    if text ~= nil and text ~= w.before then
      sendToSession(w.s, { t = "effect", ev = w.ev, ms = math.floor((now - w.at) * 1000 + 0.5), frames = w.frames })
    elseif now - w.at > DEFAULTS.benchMs / 1000 then
      sendToSession(w.s, { t = "effect", ev = w.ev, ms = -1, frames = w.frames, why = "no command-line change within " .. DEFAULTS.benchMs .. " ms" })
    else keep[#keep + 1] = w end
  end
  state.benchWatch = keep
end

-------------------------------------------------------------------------------
-- One loop iteration (section 4 of the protocol doc). Exposed for the harness.
-------------------------------------------------------------------------------
-- Takes the hardkeys instance out of service without losing what it owns: input off, a release attempt
-- for every held key, and every record that stays unresolved kept in state.unresolved for adoption by
-- the next start and for the "recover" command. Used on a service() error, at stop and in Cleanup.
-- KB-14: a temporary shortcut-mode change dispose() could not restore (a dependent key still held, the
-- restore delay not elapsed, or the profile/mode changed meanwhile) comes back as a restoration record.
-- It is kept like an unresolved key record: adopted at the next start as an unresolved restoration (every
-- new press refused until it is restored) and restored by "recover" on the original profile. Dropping it
-- would leave the operator's shortcuts changed with nothing that remembers the original state.
local function keepModeRecord(mode, now, reason)
  if type(mode) ~= "table" then return end
  mode.keptAt, mode.keptReason = now, reason
  state.modeRecord = mode
  logerr("%s: keeping the unresolved keyboard-shortcut mode restoration %s (profile '%s', shortcuts %s -> %s): %s; it is adopted at the next start and restored by  Plugin \"mtpnxk_surface\" \"recover\"  on that profile",
    reason, tostring(mode.id), tostring(mode.profile), tostring(mode.original), tostring(mode.target), tostring(mode.unresolved and mode.unresolved.reason))
end

local function detachHardkeys(reason, now)
  local rec = state.modules.hardkeys
  local inst = rec and rec.instance
  if not inst then return end
  local okS, st = pcall(inst.status, inst)
  if okS and type(st) == "table" and st.state == "ready" then
    state.inputEnabled = false
    local okD, dis = pcall(inst.disableInput, inst, now, reason)
    if okD and type(dis) == "table" then
      for _, a in ipairs(dis.unresolved or {}) do logerr("%s: release of %s UNRESOLVED: %s", reason, tostring(a.logical or a.tupleKey), tostring(a.error)) end
    end
  end
  local ok, r = pcall(inst.dispose, inst, now)
  if ok and type(r) == "table" then
    state.unresolved = state.unresolved or {}
    for _, record in ipairs(r.records or {}) do
      record.keptAt, record.keptReason = now, reason
      state.unresolved[#state.unresolved + 1] = record
      logerr("keeping unresolved record %s(%s) of session %s: %s", tostring(record.logical or "raw"), tostring(record.tupleKey), tostring(record.session), tostring(record.unresolved and record.unresolved.reason))
    end
    if #(r.records or {}) > 0 then logerr("%d unresolved release record(s) kept; they are adopted at the next start and released by  Plugin \"mtpnxk_surface\" \"recover\"", #r.records) end
    -- KB-12: the Quickey bank record travels with the consumer like the unresolved records; the next
    -- start re-verifies every owned object (adoptBank) and nothing on the console is touched here.
    if type(r.bank) == "table" then
      state.bankRecord = r.bank
      log("bank record %s kept for the next start (%d code(s)); nothing on the console was changed", tostring(r.bank.id), #(r.bank.codes or {}))
    end
    keepModeRecord(r.mode, now, reason)
  else
    -- dispose() raised (or returned nothing): the instance still owns its records, so it is kept in
    -- quarantine instead of being dropped. New input stays blocked until "recover" exports them.
    state.quarantine = { instance = inst, reason = reason, at = now, error = tostring(r) }
    logerr("dispose failed on %s: %s; the instance is quarantined with its ownership records and input stays blocked until  Plugin \"mtpnxk_surface\" \"recover\"  exports them", reason, tostring(r))
  end
  rec.instance = nil
  state.inputEnabled = false
end

-- Tries to take the records out of a quarantined instance (a release attempt first, then dispose).
-- Returns true when the quarantine is clear.
local function exportQuarantine(t)
  local q = state.quarantine
  if not q then return true end
  local inst = q.instance
  pcall(inst.recover, inst, nil, t)
  local ok, r = pcall(inst.dispose, inst, t)
  if not ok or type(r) ~= "table" then
    logerr("quarantined instance still cannot be disposed (%s); input stays blocked", tostring(r))
    return false
  end
  state.unresolved = state.unresolved or {}
  for _, record in ipairs(r.records or {}) do
    record.keptAt, record.keptReason = t, "quarantine"
    state.unresolved[#state.unresolved + 1] = record
  end
  if type(r.bank) == "table" then state.bankRecord = r.bank end
  keepModeRecord(r.mode, t, "quarantine")
  log("quarantined instance exported %d record(s)%s", #(r.records or {}), type(r.mode) == "table" and " and a mode restoration record" or "")
  state.quarantine = nil
  return true
end

-- KB-18: takes the control instance out of service. Every gesture gets an end attempt through its
-- backend, queued motion is dropped, and the releases that stay unresolved are kept in
-- state.controlUnresolved for "recover" after a restart.
local function detachControl(reason, now)
  local rec = state.modules.control
  local inst = rec and rec.instance
  if not inst then return end
  local ok, r = pcall(inst.dispose, inst, now)
  if ok and type(r) == "table" then
    for _, e in ipairs(r.ended or {}) do log("control: %s on %s/%s ended on %s: %s", tostring(e.kind), tostring(e.device), tostring(e.control), tostring(reason), tostring(e.outcome)) end
    state.controlUnresolved = state.controlUnresolved or {}
    for _, u in ipairs(r.records or {}) do state.controlUnresolved[#state.controlUnresolved + 1] = u; logerr("control: %s release on %s/%s UNRESOLVED on %s; kept for  Plugin \"mtpnxk_surface\" \"recover\": %s", tostring(u.kind), tostring(u.device), tostring(u.control), tostring(reason), tostring(u.error)) end
    if (r.dropped or 0) > 0 then log("control: %d queued intent(s) dropped on %s (never applied late)", r.dropped, tostring(reason)) end
  elseif not ok then logerr("control dispose on %s raised: %s", tostring(reason), tostring(r)) end
  rec.instance = nil
  state.controlEnabled = false
  for _, s in pairs(state.sessions or {}) do s.controlOpen = false end
end

local function serviceModules(now)
  local released = {}
  local inst = hk()
  if inst then
    local ok, res = pcall(inst.service, inst, now)
    if not ok then
      logerr("hardkeys service() raised: %s; input disabled, module detached (its records are kept)", tostring(res))
      detachHardkeys("service-error", now)
    elseif type(res) == "table" then
      for _, a in ipairs(res.released or {}) do released[a.hold] = true end
      for _, a in ipairs(res.unresolved or {}) do released[a.hold] = true; logerr("deadline release of %s UNRESOLVED: %s", tostring(a.logical or a.tupleKey), tostring(a.error)) end
      for _, sid in ipairs(res.expired or {}) do
        local s = state.sessions[sid]
        if s then s.state = "expired"; log("session %s (%s): lease expired, its holds were released; nothing is re-pressed", sid, s.id) end
      end
    end
  end
  local f = fb()
  if f then
    local ok, res = pcall(f.service, f, now)
    if not ok then logerr("feedback service() raised: %s", tostring(res))
    elseif type(res) == "table" and res.invalidated then log("feedback invalidated: %s (epoch %d); the surface sees every item unknown until the next full state", tostring(res.invalidated), f:epoch()) end
  end
  -- KB-18: lease and gesture expiries first, then at most controlWorkPerTick intents through the backend.
  local c = ctl()
  if c then
    local ok, res = pcall(c.service, c, now)
    if not ok then
      logerr("control service() raised: %s; continuous control disabled, module detached (its records are kept)", tostring(res))
      detachControl("service-error", now)
    elseif type(res) == "table" then
      for _, sid in ipairs(res.expired or {}) do
        local s = state.sessions[sid]
        if s then s.controlOpen = false; log("session %s (%s): control lease expired, its gestures were ended and queued motion dropped", sid, s.id) end
      end
      for _, e in ipairs(res.ended or {}) do if e.reason ~= "lease-expired" then log("control: %s on %s/%s ended (%s): %s", tostring(e.kind), tostring(e.device), tostring(e.control), tostring(e.reason), tostring(e.outcome)) end end
      for _, u in ipairs(res.unresolved or {}) do logerr("control: %s release on %s/%s UNRESOLVED: %s", tostring(u.kind), tostring(u.device), tostring(u.control), tostring(u.error)) end
      if res.dropped and (res.dropped.staleGeneration > 0) then state.counters.ctlStaleDropped = state.counters.ctlStaleDropped + res.dropped.staleGeneration end
      if res.dropped and (res.dropped.expired > 0) then state.counters.ctlExpired = state.counters.ctlExpired + res.dropped.expired end
      state.counters.ctlApplied = state.counters.ctlApplied + (res.applied or 0)
    end
  end
  -- KB-24 hardware pass: after the backend applied the keys, re-read the executors they pressed.
  if state.refreshExec and next(state.refreshExec) then
    local f = fb()
    if f and type(f.refresh) == "function" then
      for k, x in pairs(state.refreshExec) do
        local ok = pcall(f.refresh, f, "executorTarget", { executor = x.executor, page = x.page }, now)
        state.counters.execRefreshes = (state.counters.execRefreshes or 0) + 1
        if not ok or now >= x.untilT then state.refreshExec[k] = nil end
      end
    else
      state.refreshExec = nil
    end
  end
  return released
end

-- KB-24: a millisecond wall clock for the tick's phase timing (nil when socket.gettime is unavailable).
local function clockMs()
  local ok, t = pcall(socket.gettime)
  if ok and type(t) == "number" then return t * 1000 end
  return nil
end

local function tick(now)
  state.lastTickAt = now
  -- KB-24: the load window (see the heartbeat reply) and two diagnostics for the console's scheduling: how many
  -- distinct coroutines ticked in the window (more than one means a stale loop is alive) and how often a tick began
  -- while another had not ended (re-entrant resumption).
  local ld = state.load
  -- (the state table survives a plugin restart: a window left by an older plugin without these fields is replaced)
  if not ld or ld.cos == nil then ld = { tmax = 0, tsum = 0, pmax = 0, full = 0, n = 0, smod = 0, spkt = 0, sstate = 0, cos = {}, ovl = 0 }; state.load = ld end
  local co = coroutine.running()
  ld.cos[tostring(co)] = true
  if state.inTick then ld.ovl = ld.ovl + 1 end
  state.inTick = true
  -- 1 + 2: deadlines and feedback before any packet.
  local tA = clockMs()
  local released = serviceModules(now)
  if next(released) then for _, s in pairs(state.sessions) do pruneReleased(s, released) end end
  local tB = clockMs()
  -- Forget sessions that stayed silent past forgetMs.
  for sid, s in pairs(state.sessions) do
    if now - s.lastSeen > DEFAULTS.forgetMs / 1000 then closeSession(s, now, "silent") end
  end
  -- 3: bounded packet processing. At most one key goes down per tick (KB-28): Quickey presses issued in one
  -- console frame reorder in a focused text field (digits land after keywords), presses one frame apart keep
  -- their order. The datagrams behind a key-down stay in the socket for the next tick.
  local n = 0
  state.keyDownThisTick = false
  -- Key events queued by earlier ticks run first, in arrival order: every release, and at most one key-down.
  state.keyQueue = state.keyQueue or {}
  local q = state.keyQueue
  serviceKeyGate(now)
  while #q > 0 and not state.keyGate do
    local item = q[1]
    if item.obj.d == 1 and state.keyDownThisTick then break end
    table.remove(q, 1)
    item.s.keyQueued[item.obj.ev] = nil
    if state.sessions[item.s.sid] == item.s then handleKey(item.s, item.obj, now, true) end
  end
  while n < DEFAULTS.maxPacketsPerTick do
    local ok, data, ip, port = pcall(function() return state.sock:receivefrom() end)
    if not ok or data == nil then break end
    n = n + 1
    handleDatagram(data, ip, port, now)
  end
  state.counters.ticks = state.counters.ticks + 1
  -- KB-24: datagrams this tick, whether the budget was exhausted; where the tick's time went (smod: module servicing
  -- incl. the feedback reads, spkt: datagrams, sstate: state and context collection and sends).
  if n > ld.pmax then ld.pmax = n end
  if n >= DEFAULTS.maxPacketsPerTick then ld.full = ld.full + 1 end
  local tC = clockMs()
  -- 4: state out (at most every stateEveryMs; the bench timing watches every tick).
  if state.collectNextTick and not state.collectNow then state.collectNow, state.collectNextTick = true, nil end
  if state.lastCollectAt == nil or now - state.lastCollectAt >= DEFAULTS.stateEveryMs / 1000 or state.collectNow then
    state.lastCollectAt, state.collectNow = now, nil
    local cur = collectState(now)
    state.lastState = cur
    sendStates(now)
  end
  -- The context is still collected every tick: gating it to the state cadence shifted the binding revision the
  -- KB-21/KB-22 tests expect in the tick of a bind (the module's first snapshot sets revision 1 on this path, and the
  -- bind path and the context path flip the key when they run in a different order); an open KB-24 item.
  sendContexts(now, collectContext(now))
  serviceBench(now)
  -- Housekeeping (cheap, bounded).
  if now - (state.lastSweep or 0) > 5 then
    state.lastSweep = now
    for nonce, t in pairs(state.nonces) do if now - t > DEFAULTS.nonceMs / 1000 then state.nonces[nonce] = nil end end
    for ip, t in pairs(state.errReplies) do if now - t > 5 then state.errReplies[ip] = nil end end
  end
  -- KB-24: how long this tick took (wall clock, ms), for the heartbeat's load report.
  local okT, t1 = pcall(socket.gettime)
  if okT and type(t1) == "number" then
    local ms = (t1 - now) * 1000
    if ms < 0 then ms = 0 end
    if ms > ld.tmax then ld.tmax = ms end
    ld.tsum = ld.tsum + ms
  end
  local tD = clockMs()
  if tA and tB and tC and tD then
    ld.smod = ld.smod + math.max(0, tB - tA); ld.spkt = ld.spkt + math.max(0, tC - tB); ld.sstate = ld.sstate + math.max(0, tD - tC)
  end
  ld.n = ld.n + 1
  state.inTick = false
  if state.loadRotate then
    state.loadRotate = nil
    local r3 = function(v) return math.floor((v or 0) * 1000 + 0.5) / 1000 end
    local cos = 0
    for _ in pairs(ld.cos or {}) do cos = cos + 1 end
    state.loadReport = { tmax = r3(ld.tmax), tavg = r3(ld.tsum / ld.n), pmax = ld.pmax, full = ld.full, n = ld.n, smod = r3(ld.smod), spkt = r3(ld.spkt), sstate = r3(ld.sstate), co = cos, ovl = ld.ovl or 0 }
    state.load = { tmax = 0, tsum = 0, pmax = 0, full = 0, n = 0, smod = 0, spkt = 0, sstate = 0, cos = {}, ovl = 0 }
  end
end
state._tick = tick

-------------------------------------------------------------------------------
-- Start / stop
-------------------------------------------------------------------------------
local function now()
  local ok, t = pcall(socket.gettime)
  if ok and type(t) == "number" then return t end
  return os.clock()
end

local function resetCounters()
  state.keyQueue = {}   -- KB-28: key events waiting for their tick (see handleKey)
  state.counters = { received = 0, sent = 0, sendErrors = 0, rejected = 0, oversized = 0, notAllowed = 0, noSession = 0,
                     oldSeq = 0, rateDropped = 0, dupEvents = 0, superseded = 0, refused = 0, presses = 0, releases = 0, wheels = 0, lostReleases = 0,
                     replayedHello = 0, fullStates = 0, deltaStates = 0, contexts = 0, ticks = 0,
                     ctl = 0, ctlRefused = 0, ctlLost = 0, ctlCoalesced = 0, ctlApplied = 0, ctlStaleDropped = 0, ctlExpired = 0, keyQueued = 0, keyQueueOverflow = 0 }
end

local function loadModules()
  state.modules = {}
  state.moduleVersions = {}
  local summary = {}
  for _, entry in ipairs({ { key = "hardkeys", component = "gma3_mcp_hardkeys" }, { key = "feedback", component = "gma3_mcp_feedback" }, { key = "control", component = "gma3_mcp_control", optional = true } }) do
    local rec = { component = entry.component, loaded = false, optional = entry.optional }
    local mod, err = loadModule(entry.component)
    if mod then
      rec.module, rec.version, rec.loaded = mod, mod.VERSION, true
      state.moduleVersions[entry.component] = mod.VERSION
      local okI, inst = pcall(function()
        local deps = mod.consoleDeps(_G)
        if entry.key == "hardkeys" then
          return mod.new({ owner = pluginName, deps = deps, config = { requireInteraction = false, maxHolds = DEFAULTS.maxHolds, maxHoldMs = DEFAULTS.maxHoldMs, defaultLeaseMs = DEFAULTS.leaseMs } }):init()
        end
        if entry.key == "control" then
          -- KB-18: the binding is this plugin's cached context snapshot (the same one the context message
          -- carries, so the service's cg and the module's generation are one number); the other input
          -- owner is this plugin's hardkeys instance.
          deps.binding = function(t)
            local f = fb()
            if not f or type(f.contextSnapshot) ~= "function" then return nil end
            return f:contextSnapshot(contextSpec(), t, { cached = true })
          end
          deps.busy = function(_, t)
            local h = hk()
            if not h or type(h.admission) ~= "function" then return nil end
            local ok, busy = pcall(h.admission, h, t)
            return ok and busy or nil
          end
          -- KB-22 review: a frozen playback-button release is applied only on a current observation of its executor;
          -- when the bound window moved away from it (a bank change), the module asks for this live read.
          deps.observe = function(pageNo, executor)
            local f = fb()
            if not f or type(f.read) ~= "function" then return nil end
            local r = f:read("executorTarget", { executor = executor, page = pageNo }, now())
            return (type(r) == "table" and r.available) and r.value or nil
          end
          -- This plugin's binding (display= and execs=) is fixed for its run, so events carry no binding
          -- revision; the exemption is declared here, explicitly (the module requires the revision by default).
          return mod.new({ owner = pluginName, deps = deps, config = { defaultLeaseMs = DEFAULTS.leaseMs, maxWorkPerService = DEFAULTS.controlWorkPerTick, requireBindingRevision = false } }):init()
        end
        -- KB-21: a surface binds every bank window at once (M-Touch 30, M-Play 60 executors), above the module's default bound.
        local cfg = { maxReadsPerService = DEFAULTS.readsPerService, priorityStateReads = 1 }  -- KB-24: state items before executors
        -- Review (PR #6): the watch list holds the state items, execs= readers and up to 96 targets: above the module's 64.
        if entry.key == "feedback" then cfg.maxExecutors = MAX_BOUND_EXECUTORS; cfg.maxItems = MAX_WATCH_ITEMS end
        return mod.new({ owner = pluginName, deps = deps, config = cfg }):init()
      end)
      if okI then rec.instance = inst else rec.loaded, rec.error = false, "instance: " .. tostring(inst) end
    else rec.error = err end
    state.modules[entry.key] = rec
    summary[#summary + 1] = entry.key .. (rec.loaded and (" " .. tostring(rec.version)) or (" FAILED (" .. tostring(rec.error) .. ")"))
  end
  log("modules: %s", table.concat(summary, ", "))
end
state._loadModules = loadModules

-- Records a previous run could not release are adopted before any input is admitted: their tuples stay
-- reserved (a new press of the same key is refused with "conflict") until "recover" releases them.
-- Records the module rejects (capacity, a tuple already owned) stay in state.unresolved for a later try.
local function adoptKept(t)
  local inst = hk()
  local kept = state.unresolved or {}
  if not inst or #kept == 0 then return end
  local ok, r = pcall(inst.adopt, inst, kept, t)
  if not ok then logerr("adopting %d kept record(s) failed: %s; they are kept", #kept, tostring(r)); return end
  local remaining = {}
  for _, rej in ipairs(r.rejected or {}) do remaining[#remaining + 1] = rej.record; logerr("kept record not adopted (%s); kept for a later start", tostring(rej.reason)) end
  state.unresolved = remaining
  for _, a in ipairs(r.adopted or {}) do log("adopted unresolved record %s(%s): its key stays reserved until \"recover\" releases it", tostring(a.logical or "raw"), tostring(a.tupleKey)) end
end

-- KB-14: the kept restoration record becomes an unresolved restoration of the new instance (owner
-- "previous-run"); nothing is written until "recover" re-reads the profile and the mode.
local function adoptKeptMode(t)
  local record = state.modeRecord
  if type(record) ~= "table" then return end
  local inst = hk()
  if not inst then return end
  if type(inst.adoptMode) ~= "function" then logerr("a keyboard-shortcut mode restoration record is kept but the loaded module has no adoptMode(); it is kept"); return end
  local ok, r, err = pcall(inst.adoptMode, inst, record, t)
  if not ok then logerr("adopting the kept mode restoration raised: %s; the record is kept", tostring(r)); return end
  if not r then logerr("kept mode restoration not adopted [%s]: %s; the record is dropped", tostring(err and err.code), tostring(err and err.message)); state.modeRecord = nil; return end
  state.modeRecord = nil
  logerr("adopted the unresolved keyboard-shortcut mode restoration %s from a previous run (profile '%s', shortcuts %s -> %s); every new press is refused until  Plugin \"mtpnxk_surface\" \"recover\"  restores it on that profile",
    tostring(r.id), tostring(r.profile), tostring(r.original), tostring(r.target))
end

local enableInput  -- defined below; recover re-enables the requested input after a quarantine clears
local enableControl  -- KB-18: defined below

-------------------------------------------------------------------------------
-- Quickey bank (KB-12) and backend adapters (KB-13/KB-15)
-------------------------------------------------------------------------------
local function logBank(prefix, b)
  if type(b) ~= "table" then return end
  if not b.provisioned then log("%s: no bank (%s)", prefix, tostring(b.note)); return end
  log("%s: bank %s state=%s codes=%d (qualified %d, discovered %d) problems=%d quickeys from %d, executors page %d %d-%d show='%s'",
    prefix, tostring(b.id), tostring(b.state), b.codeCount or 0, b.qualifiedCount or 0, b.discoveredCount or 0, b.problemCount or 0,
    b.spec.quickeys.first, b.spec.executors.page, b.spec.executors.first, b.spec.executors.first + b.spec.executors.count - 1, tostring(b.show))
  for _, p in ipairs(b.problems or {}) do logerr("%s: problem %s %s: %s", prefix, tostring(p.kind), tostring(p.index or p.executor or ""), tostring(p.detail)) end
  -- hardkeys 0.11.0 (KB-28 review): a bank kept from an older module lacks the newly qualified codes; their keys
  -- are refused until the operator re-provisions (Plugin "mtpnxk_surface" "bank teardown", then start with bank=...).
  if b.requalified then log("%s: qualification refreshed for %s", prefix, table.concat(b.requalified, " ")) end
  if b.reprovision then logerr("%s: %s (Plugin \"mtpnxk_surface\" \"bank teardown\", then start again with bank=...)", prefix, tostring(b.reprovision)) end
end

-- At start: a record kept from the previous run is adopted (every object re-verified, nothing created).
local function adoptKeptBank(t)
  local inst, record = hk(), state.bankRecord
  if not inst or type(record) ~= "table" then return end
  if type(inst.adoptBank) ~= "function" then logerr("bank: a record is kept but the loaded module has no adoptBank()"); return end
  local ok, r, err = pcall(inst.adoptBank, inst, record, t)
  if not ok then logerr("bank adopt raised: %s; the record is kept", tostring(r)); return end
  if not r then logerr("bank adopt refused [%s]: %s; the record is dropped (provision again with bank=...)", tostring(err and err.code), tostring(err and err.message)); state.bankRecord = nil; return end
  state.bankRecord = nil
  logBank("bank adopt", r)
end

-- The operator's plugin argument is the authorization (KB-12: never a surface request). A bank this
-- plugin already owns on the same slots is verified and reused by the module; a different live bank
-- is reported, not replaced.
local function provisionBank(opts, t)
  if not opts.bank then return end
  local inst = hk()
  if not inst then return end
  if type(inst.provisionBank) ~= "function" then logerr("bank: the loaded hardkeys module has no Quickey bank (KB-12 needs 0.7.0 or newer)"); return end
  local live = inst:bankStatus(t)
  if live.provisioned then log("bank: %s is already live (state %s); the bank= argument is not applied", tostring(live.id), tostring(live.state)); return end
  local spec = { authorized = true, quickeys = { first = opts.bank.quickeyFirst },
                 executors = { page = opts.bank.page, first = opts.bank.executorFirst, count = opts.bank.executorCount, static = opts.bankStatic or nil },
                 codes = opts.bankCodes or "qualified", label = "mtpnxk NX-K surface" }
  local ok, r, err = pcall(inst.provisionBank, inst, spec, t)
  if not ok then logerr("bank provision raised: %s", tostring(r)); return end
  if not r then
    logerr("bank provision refused [%s]: %s", tostring(err and err.code), tostring(err and err.message))
    for _, rf in ipairs(err and err.refusals or {}) do logerr("bank provision: %s %s: %s", tostring(rf.reason), tostring(rf.index or rf.executor or ""), tostring(rf.detail)) end
    return
  end
  log("bank provision: %d Quickey(s) created, %d reused", r.created or 0, r.reused or 0)
  logBank("bank provision", r)
end

local function adapterFor(kind)
  local rec = state.modules.hardkeys
  local HK, inst = rec and rec.module, rec and rec.instance
  if not inst then return nil, "the hardkeys module is not loaded" end
  rec.adapters = rec.adapters or {}
  local a = rec.adapters
  if kind == "fake" then
    a.fake = a.fake or HK.fakeBackend(); return a.fake
  elseif kind == "keyboard" then
    a.keyboard = a.keyboard or HK.keyboardBackend(HK.consoleDeps(_G), { defaultDisplay = state.display }); return a.keyboard
  elseif kind == "quickey" then
    if type(HK.quickeyBackend) ~= "function" then return nil, "the loaded hardkeys module has no quickeyBackend() (KB-13 needs 0.8.0 or newer)" end
    a.quickey = a.quickey or HK.quickeyBackend(inst); return a.quickey
  elseif kind == "mixed" then
    if type(HK.mixedBackend) ~= "function" then return nil, "the loaded hardkeys module has no mixedBackend() (KB-15 needs 0.10.0 or newer)" end
    if not a.mixed then
      local q, qerr = adapterFor("quickey"); if not q then return nil, qerr end
      local k = adapterFor("keyboard")
      a.mixed = HK.mixedBackend({ quickey = q, keyboard = k })
    end
    return a.mixed
  end
  return nil, "unknown backend '" .. tostring(kind) .. "'"
end

-- The routing policy goes with the backend (KB-11/KB-15). Keyboard and fake keep the module default
-- (shortcut) unless the operator overrides single keys; quickey is quickkey only; mixed is the explicit
-- per-key table above plus the operator's overrides.
local function routingFor(mode, overrides)
  if mode == "mixed" then
    local rec = state.modules.hardkeys
    return mixedPolicy(overrides, rec and rec.module and rec.module.BANK_QUALIFIED)
  end
  if mode == "quickey" then return quickeyPolicy(overrides) end
  if overrides and next(overrides) then
    local keys = {}
    for logical, method in pairs(overrides) do keys[logical] = { method = method } end
    return { default = "shortcut", keys = keys }
  end
  return nil
end

-- Operator recovery: re-attempt every unresolved release (adopted or current). Without input enabled
-- the keyboard backend is attached for cleanup only; input stays as configured.
local function recoverUnresolved()
  local rec = state.modules.hardkeys
  local inst = rec and rec.instance
  if not inst then logerr("recover: the plugin is not running (start it first; kept records are adopted at start)"); return end
  local t = now()
  local wasBlocked = state.quarantine ~= nil
  if not exportQuarantine(t) then return end
  adoptKept(t)
  adoptKeptMode(t)
  if wasBlocked and state.inputRequested and state.inputRequested ~= "off" and not state.inputEnabled then
    log("recover: quarantine cleared; enabling the input mode requested at start (%s)", state.inputRequested)
    enableInput(state.inputRequested, state.forceRequested)
    inst = rec.instance
    -- Surfaces paired while input was blocked have no module session yet.
    if state.inputEnabled then
      for _, s in pairs(state.sessions) do
        if not s.sessionOpen then
          local r = inst:openSession({ id = s.sid, leaseMs = DEFAULTS.leaseMs, label = s.id, binding = s.ip .. ":" .. tostring(s.port) }, t)
          s.sessionOpen = r ~= nil
        end
      end
    end
  end
  local st = inst:status(t)
  if not (st.backend and st.backend.dispatches) then
    -- Records name the part that pressed them (KB-15): keyboard and quickey records together need the
    -- mixed adapter, one kind its own backend; a record is only ever released through the backend that
    -- pressed it.
    local kinds, wanted = {}, nil
    for _, h in ipairs(st.holds or {}) do if h.state ~= "released" and type(h.backend) == "string" then kinds[h.backend] = true end end
    if kinds.keyboard and kinds.quickey then wanted = "mixed"
    elseif kinds.quickey then wanted = "quickey"
    elseif kinds.fake then wanted = "fake"
    else wanted = "keyboard" end
    local adapter, aerr = adapterFor(wanted)
    if not adapter then logerr("recover: cannot attach the %s backend for cleanup: %s; the records stay reserved", wanted, tostring(aerr)); return end
    local a, err = inst:attachBackend(adapter)
    if not a then logerr("recover: could not attach the %s backend for cleanup: %s", wanted, tostring(err and err.message)); return end
    log("recover: %s backend attached for cleanup only (input stays %s)", wanted, tostring(state.inputMode))
  end
  local r = inst:recover(nil, t)
  if type(r.restoration) == "table" then
    local m = r.restoration
    if m.state == "restored" then log("recover: keyboard-shortcut mode restoration %s restored (profile '%s', shortcuts back to %s, by %s%s)", tostring(m.id), tostring(m.profile), tostring(m.original), tostring(m.restoredBy), m.restoreNote and ("; " .. m.restoreNote) or "")
    elseif m.state == "active" then log("recover: keyboard-shortcut mode restoration %s re-validated; the loop restores it %d ms after the last key event", tostring(m.id), tonumber(m.restoreInMs) or 0)
    else logerr("recover: keyboard-shortcut mode restoration %s still unresolved: %s", tostring(m.id), tostring(m.unresolved and m.unresolved.reason)) end
  end
  for _, a in ipairs(r.released or {}) do log("recover: released %s(%s) of session %s", tostring(a.logical or "raw"), tostring(a.tupleKey), tostring(a.session)) end
  for _, a in ipairs(r.unresolved or {}) do logerr("recover: %s(%s) still UNRESOLVED: %s", tostring(a.logical or "raw"), tostring(a.tupleKey), tostring(a.error)) end
  log("recover: %d released, %d still unresolved, %d record(s) not adopted", #(r.released or {}), #(r.unresolved or {}), #(state.unresolved or {}))
end
state._recover = recoverUnresolved

-- KB-18: continuous control is an explicit per-start decision (control=fake); the fake backend records
-- intents and moves nothing on the console (the adjustment backend is KB-19). Records a previous run
-- could not release are adopted so "recover" can re-attempt them.
enableControl = function(mode)
  state.controlMode = mode or "off"
  state.controlEnabled = false
  local rec = state.modules.control
  if mode == nil or mode == "off" then
    if rec and rec.instance then log("control off: ctl events are acknowledged control-disabled") end
    return true
  end
  if not rec or not rec.instance then logerr("control %s requested but the control module is not loaded (%s)", mode, tostring(rec and rec.error)); state.controlMode = "off"; return false end
  local ok, err = pcall(function()
    if mode == "console" then
      if type(rec.module.consoleBackend) ~= "function" then error("the vendored control module " .. tostring(rec.version) .. " has no console backend (0.2.0 or newer is needed; 0.3.0 serves the strips, 0.5.0 the executors)", 0) end
      rec.instance:enableInput(rec.module.consoleBackend(rec.module.consoleDeps(_G)))
    else
      rec.instance:enableInput(rec.module.fakeBackend())
    end
  end)
  if not ok then logerr("control %s: enableInput failed: %s", mode, tostring(err)); state.controlMode = "off"; return false end
  local kept = state.controlUnresolved or {}
  if #kept > 0 then
    local a = rec.instance:adopt(kept, now())
    state.controlUnresolved = {}
    log("control: adopted %d unresolved release(s) from a previous run (recover re-attempts them)", a.adopted)
  end
  state.controlEnabled = true
  if mode == "console" then
    log("control enabled on the console backend: a rotary detent or strip drag on slot n is applied as Attribute \"<name>\" At +/- <detents x step> for the selection (KB-19), a strip touch holds the slot and a strip position is Attribute \"<name>\" At <value> (KB-20); a playback button is Press/Unpress Page <p>.<e> (the executor's configured function) and a playback fader Fader<Function> Page <p>.<e> At <level> for the qualified functions (KB-22, control 0.5.0 or newer); pushes and executor encoders are refused unsupported")
  else
    log("control enabled on the fake backend (intents recorded; nothing moves on the console)")
  end
  return true
end

local function bridgeInputEnabled()
  local b = rawget(_G, "__gma3_mcp_bridge")
  return type(b) == "table" and b.running == true and type(b.input) == "table" and b.input.enabled == true
end

enableInput = function(mode, force)
  state.inputMode = mode
  state.inputEnabled = false
  if mode == "off" then log("input off: no console key will be pressed (feedback only)"); return true end
  local inst = hk()
  if not inst then logerr("input %s requested but the hardkeys module is not loaded", mode); return false end
  if bridgeInputEnabled() and not force then
    logerr("the MCP bridge reports input enabled; refusing to enable surface input (two injectors share one console keyboard). Stop the bridge's input or start with 'force'")
    state.inputMode = "off"
    return false
  end
  if mode == "quickey" or mode == "mixed" then
    -- KB-15: Quickey dispatch needs the explicit bank setup; without it the start reports the
    -- requirement and presses nothing (never a silent fall back to Keyboard()).
    local b = inst:bankStatus(now())
    if not b.provisioned then
      logerr("input %s requested but no Quickey bank is provisioned: start with  bank=<quickey>/<page>.<first>-<last>  (KB-12; 12 reserved executors) or use input=keyboard", mode)
      state.inputMode = "off"
      return false
    end
    if b.state ~= "ready" then
      logerr("input %s requested but the Quickey bank %s is %s (%d problem(s)); run  Plugin \"mtpnxk_surface\" \"bank verify\"  or tear it down and provision again", mode, tostring(b.id), tostring(b.state), b.problemCount or 0)
      state.inputMode = "off"
      return false
    end
  end
  local adapter, aerr = adapterFor(mode)
  if not adapter then logerr("input %s: %s", mode, tostring(aerr)); state.inputMode = "off"; return false end
  local r, err = inst:enableInput(adapter, { routing = routingFor(mode, state.routeOverrides) })
  if not r then
    logerr("enableInput failed [%s]: %s", tostring(err and err.code), tostring(err and err.message))
    state.inputMode = "off"
    return false
  end
  -- Operator overrides must select a usable route now (KB-15: inert configuration is refused).
  for logical, method in pairs(state.routeOverrides or {}) do
    local name
    for n, l in pairs(NXK_KEYS) do if l == logical then name = n; break end end
    local okD, d = pcall(inst.describeRoute, inst, logical, { prefer = NXK_PREFER[name or ""] })
    local usable = okD and type(d) == "table" and d.supported and #(d.unavailable or {}) == 0
    if usable and method == "quickkey" and type(d.quickkeyCapabilities) == "table" and d.quickkeyCapabilities.hold == false then usable = false; d.reason = "no KB-10 hold evidence for " .. tostring(d.quickkey) end
    if not usable then
      logerr("route %s:%s cannot be served now (%s); the override is refused and input stays off", tostring(name or logical), method,
             okD and type(d) == "table" and (d.reason or (d.unavailable and table.concat(d.unavailable, "; ")) or d.code) or tostring(d))
      pcall(inst.disableInput, inst, now(), "route-refused")
      state.inputMode = "off"
      return false
    end
  end
  state.inputEnabled = true
  state.adapter = adapter
  state.backendName = adapter.name
  log("input enabled on the %s backend (routing default %s, %d override(s))", adapter.name, tostring(r.routing and r.routing.default), r.routing and r.routing.overrides or 0)
  return true
end

local function disposeAll(reason)
  local t = now()
  for _, s in pairs(state.sessions or {}) do closeSession(s, t, reason) end
  detachHardkeys(reason, t)
  detachControl(reason, t)
  local f = fb()
  if f then pcall(f.dispose, f) end
  state.modules = {}
  state.inputEnabled = false
end

local function describe()
  local n = 0
  for _ in pairs(state.sessions or {}) do n = n + 1 end
  return fmt("running=%s bind=%s:%d input=%s backend=%s control=%s sessions=%d gen=%s", tostring(state.running), tostring(state.host), tonumber(state.port) or 0, tostring(state.inputMode), tostring(state.backendName), tostring(state.controlMode or "off"), n, tostring(state.gen))
end

local function serverMain()
  local sock, err = socket.udp()
  if not sock then logerr("socket.udp failed: %s", tostring(err)); state.running = false; disposeAll("socket-failed"); return end
  local ok, berr = sock:setsockname(state.host, state.port)
  if not ok then logerr("could not bind %s:%d (%s)", state.host, state.port, tostring(berr)); state.running = false; disposeAll("bind-failed"); return end
  sock:settimeout(0)
  state.sock = sock
  log("listening on %s:%d (v%s, protocol %d, gen %s); %s", state.host, state.port, VERSION, PROTOCOL, state.gen, describe())
  while state.running and not state.stopRequested do
    local okT, terr = xpcall(tick, debug.traceback, now())
    if not okT then logerr("tick failed: %s", tostring(terr)) end
    coroutine.yield()
  end
  disposeAll("shutdown")
  pcall(function() sock:close() end)
  state.sock = nil
  state.running = false
  state.stopRequested = false
  log("stopped")
end

local function start(opts)
  state.host = DEFAULTS.host  -- fixed loopback: no unauthenticated LAN listener
  state.port = opts.port or DEFAULTS.port
  state.display = opts.display or DEFAULTS.display
  state.execs = opts.execs or {}
  state.execsArg = opts.execs or {}  -- the argument's list (its KB-06 state items); state.execs follows the service's bind
  state.execPage = nil  -- KB-21: set by the service's bind (an explicit page), never by an argument
  state.bench = opts.bench == true
  state.tapsMode = opts.taps or "on"  -- KB-29: direct-tap keys go down as one tap; taps=off holds every key (press/release pairs)
  state.benchWatch = {}
  state.sessions, state.byId, state.nonces, state.errReplies = {}, {}, {}, {}
  state.log = state.log or {}
  state.lastState = nil
  resetCounters()
  seedRandom()
  state.startCount = state.startCount + 1
  state.gen = string.format("%d-%s", state.startCount, randomHex(4))
  state.running = true
  state.stopRequested = false
  loadModules()
  local f = fb()
  if f then
    local items = watchItems()
    local w = f:watch(items, now())
    log("feedback: watching %d item(s)", w.watched)
  end
  state.commandText = function() return CmdObj().cmdtext end
  state.keyGate = nil
  state.encMacroActive = false
  if opts.encMacro then provisionEncMacro(opts.encMacro) end
  state.inputRequested, state.forceRequested = opts.input, opts.force
  state.routeOverrides = opts.routes
  state.backendName = nil
  local t = now()
  if exportQuarantine(t) then
    adoptKept(t)
    adoptKeptMode(t)
    adoptKeptBank(t)
    provisionBank(opts, t)
    enableInput(opts.input, opts.force)
    enableControl(opts.control)
  else
    state.inputMode, state.inputEnabled = "off", false
    enableControl("off")
    logerr("input blocked: a previous instance still owns key records it could not export; run  Plugin \"mtpnxk_surface\" \"recover\"")
  end
  describeKeys(true)
  return true
end
state._start = start

local function MainImpl(display_handle, argument)
  local opts, perr = parseArgument(argument)
  if not opts then logerr("refusing argument: %s", perr); return end
  if opts.command == "stop" then
    if state.running then state.stopRequested = true; log("stop requested") else log("not running") end
    return
  end
  if opts.command == "recover" then
    recoverUnresolved()
    local c = ctl()
    if c then
      if not c:backendAvailable() then
        local m = state.modules.control.module
        c:attachBackend((state.controlMode == "console" and type(m.consoleBackend) == "function") and m.consoleBackend(m.consoleDeps(_G)) or m.fakeBackend())
      end
      local kept = state.controlUnresolved or {}
      if #kept > 0 then local a = c:adopt(kept, now()); state.controlUnresolved = {}; log("control recover: adopted %d record(s) from a previous run", a.adopted) end
      local r = c:recover(now())
      log("control recover: %d resolved, %d still unresolved", #r.resolved, #r.unresolved)
      for _, u in ipairs(r.unresolved) do logerr("control recover: still UNRESOLVED %s release on %s/%s: %s", tostring(u.kind), tostring(u.device), tostring(u.control), tostring(u.error)) end
    elseif state.controlUnresolved and #state.controlUnresolved > 0 then log("control recover: not running; %d record(s) kept for the next start with control=fake|console", #state.controlUnresolved) end
    return
  end
  if opts.command == "encmacro-status" then logEncMacro(); return end
  if opts.command == "encmacro-teardown" then teardownEncMacro(); return end
  if opts.command and opts.command:match("^bank%-") then
    local inst = hk()
    if not inst then
      if state.bankRecord then log("bank: not running; a bank record %s is kept and is adopted at the next start", tostring(state.bankRecord.id)) else log("bank: not running, no bank record kept") end
      return
    end
    local t = now()
    if opts.command == "bank-status" then logBank("bank status", inst:bankStatus(t))
    elseif opts.command == "bank-verify" then
      local ok, r, err = pcall(inst.verifyBank, inst, t)
      if not ok then logerr("bank verify raised: %s", tostring(r)) elseif not r then logerr("bank verify refused [%s]: %s", tostring(err and err.code), tostring(err and err.message)) else logBank("bank verify", r) end
    else
      local ok, r, err = pcall(inst.teardownBank, inst, t, { authorized = true })
      if not ok then logerr("bank teardown raised: %s", tostring(r))
      elseif not r then logerr("bank teardown refused [%s]: %s", tostring(err and err.code), tostring(err and err.message))
      else
        state.bankRecord = nil
        log("bank teardown: %d Quickey(s) removed, %d executor(s) cleared, %d skipped; state %s", #(r.removed or {}), #(r.cleared or {}), #(r.skipped or {}), tostring(r.state))
        for _, sk in ipairs(r.skipped or {}) do logerr("bank teardown: skipped %s: %s", tostring(sk.index or sk.executor), tostring(sk.reason)) end
        if state.inputEnabled and (state.inputMode == "quickey" or state.inputMode == "mixed") then
          logerr("bank teardown: input %s has no bank any more; Quickey routes are refused until a bank is provisioned (restart with bank=...)", state.inputMode)
        end
      end
    end
    return
  end
  if opts.command == "status" then
    log("%s", describe())
    logEncMacro()
    local inst0 = hk()
    if inst0 then
      local okR, rr = pcall(inst0.routingReport, inst0)
      if okR and type(rr) == "table" then log("routing: default %s (%s), %d override(s), backend %s", tostring(rr.default), tostring(rr.defaultSource), rr.overrideCount or 0, tostring(rr.backend)) end
      logBank("bank", inst0:bankStatus(now()))
      for _, name in ipairs((function() local l = {} for n in pairs(state.routes or {}) do l[#l + 1] = n end table.sort(l) return l end)()) do
        local r = state.routes[name]
        log("route %s: %s via %s%s%s", name, tostring(NXK_KEYS[name]), tostring(r.method), r.backend and (" (" .. tostring(r.backend) .. ")") or "", r.supported and "" or " UNSUPPORTED: " .. tostring(state.keyReasons and state.keyReasons[name]))
      end
    elseif state.bankRecord then log("bank record %s kept (not running)", tostring(state.bankRecord.id)) end
    if state.unresolved and #state.unresolved > 0 then log("%d unresolved record(s) kept from a previous run (not adopted yet)", #state.unresolved) end
    if type(state.modeRecord) == "table" then logerr("a keyboard-shortcut mode restoration record is kept from a previous run (profile '%s', shortcuts %s -> %s; not adopted yet): run  Plugin \"mtpnxk_surface\" \"recover\"", tostring(state.modeRecord.profile), tostring(state.modeRecord.original), tostring(state.modeRecord.target)) end
    if state.quarantine then logerr("a previous instance is quarantined (%s: %s); input is blocked until  recover  exports its records", tostring(state.quarantine.reason), tostring(state.quarantine.error)) end
    local inst = hk()
    if inst then
      local st = inst:status(now())
      if st.unresolved > 0 then log("%d unresolved hold(s) on the instance; run  Plugin \"mtpnxk_surface\" \"recover\"", st.unresolved) end
      if type(st.modeChange) == "table" then
        local m = st.modeChange
        if m.state == "unresolved" then logerr("keyboard-shortcut mode restoration %s UNRESOLVED (profile '%s', shortcuts %s -> %s): %s; run  Plugin \"mtpnxk_surface\" \"recover\"", tostring(m.id), tostring(m.profile), tostring(m.original), tostring(m.target), tostring(m.unresolved and m.unresolved.reason))
        else log("keyboard-shortcut mode change %s %s (profile '%s', shortcuts %s -> %s)", tostring(m.id), tostring(m.state), tostring(m.profile), tostring(m.original), tostring(m.target)) end
      end
    end
    local c = ctl()
    if c then
      local st = c:status(now())
      local busy = c:admission(now())
      log("control: %s backend=%s unresolved=%d counters admitted=%d applied=%d refused=%d lost=%d coalesced=%d dropped=%d%s", state.controlEnabled and "enabled" or "disabled", tostring(st.backend), #st.unresolved,
          st.counters.admitted, st.counters.applied, st.counters.refused, st.counters.lost, st.counters.coalesced, st.counters.dropped, busy and (" BUSY: " .. tostring(busy.description)) or "")
      -- KB-21: the executor binding (what the service bound, or execs= at start) and its revision.
      local info = type(c.bindingInfo) == "function" and c:bindingInfo(now()) or nil
      log("binding: %d executor(s) [%s] %s revision=%s generation=%s", #state.execs, table.concat(state.execs, ","), state.execPage and ("on page " .. tostring(state.execPage) .. " (independent)") or "following the console's page", tostring(info and info.revision), tostring(info and info.generation or (info and info.unknown and info.unknown.code)))
      for id, sv in pairs(st.sessions) do for _, g in ipairs(sv.gestureList or {}) do log("control gesture %s: %s on %s/%s target=%s generation=%s%s%s", id, g.kind, g.device, g.control, json.encode(g.target), tostring(g.generation), g.frozen and (" frozen=" .. json.encode(g.frozen)) or "", g.rebound and " REBOUND (release and re-touch)" or "") end end
      for _, u in ipairs(st.unresolved) do logerr("control UNRESOLVED: %s release on %s/%s: %s%s", tostring(u.kind), tostring(u.device), tostring(u.control), tostring(u.error), u.reassigned and "  (KB-22: the executor was reassigned or emptied while held; `recover` releases it once the original object is back, or end that playback by hand)" or "") end
    end
    if state.controlUnresolved and #state.controlUnresolved > 0 then log("%d unresolved control release(s) kept from a previous run (not adopted yet)", #state.controlUnresolved) end
    for sid, s in pairs(state.sessions or {}) do
      local held = {}
      for name in pairs(s.holds) do held[#held + 1] = name end
      table.sort(held)
      log("session %s: %s %s:%d state=%s lastSeen=%.1fs ago holds=[%s] unsynced=[%s]", sid, s.id, s.ip, s.port, s.state, now() - s.lastSeen, table.concat(held, ","), table.concat(s.unsyncedHeld or {}, ","))
    end
    if state.counters then
      local parts = {}
      for k, v in pairs(state.counters) do if v ~= 0 then parts[#parts + 1] = k .. "=" .. tostring(v) end end
      table.sort(parts)
      log("counters: %s", table.concat(parts, " "))
    end
    return
  end
  if state.running then
    -- A ReloadAllPlugins (or Delete Plugin) kills the running coroutine without running its shutdown:
    -- the shared state stays "running" with the socket bound and the module instances alive. A loop
    -- that has not ticked for a second is not coming back, so this chunk takes its state over: holds
    -- are released through the old instances, their records and the bank record are kept, the socket is
    -- closed. A loop that still ticks is left alone.
    local t = now()
    local idle = state.lastTickAt and (t - state.lastTickAt) or nil
    if idle ~= nil and idle < 1.0 then log("already running (%s); use \"stop\" first", describe()); return end
    logerr("a previous run is marked running but its loop has not ticked for %s (reloaded plugin?); taking over its state", idle and string.format("%.1f s", idle) or "an unknown time (no tick recorded)")
    if state.sock then pcall(function() state.sock:close() end); state.sock = nil end
    disposeAll("stale-run")
    state.running, state.stopRequested = false, false
  end
  if not start(opts) then state.running = false; return end
  serverMain()
end

local function Main(display_handle, argument)
  local ok, err = xpcall(MainImpl, debug.traceback, display_handle, argument)
  if not ok then logerr("start failed: %s", tostring(err)) end
  -- onPC calls Cleanup after every invocation; a control call ("status") must leave the running loop alone.
  if state.running then state.ignoreNextCleanup = true end
end

local function Cleanup()
  if state.ignoreNextCleanup then state.ignoreNextCleanup = false; return end
  state.stopRequested = true
  if state.sock then pcall(function() state.sock:close() end) end
  state.sock = nil
  disposeAll("cleanup")
  state.running = false
end

-- Exposed for the harness (stock Lua, stubbed socket): the pure functions and the loop pieces.
state._parseArgument, state._NXK_KEYS, state._DEFAULTS, state._VERSION = parseArgument, NXK_KEYS, DEFAULTS, VERSION
state._describe, state._enableInput, state._enableControl = describe, enableInput, enableControl

return Main, Cleanup
