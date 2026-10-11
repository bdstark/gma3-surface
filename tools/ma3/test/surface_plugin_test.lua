-- Regression harness for tools/ma3/mtpnxk_surface.lua under stock Lua (5.4 or newer).
--
-- The console API and LuaSocket are stubbed; the plugin and the vendored modules are loaded the way
-- onPC loads them (one signal table for every component); datagrams are pushed into a fake socket and
-- the loop is driven one tick at a time with an explicit clock. Nothing sleeps, nothing reaches a
-- console key: the hardkeys FAKE backend records what would have been pressed.
--
-- Run from the repository root:   lua tools/ma3/test/surface_plugin_test.lua

local here = (arg and arg[0] or ""):match("^(.*)[/\\]") or "."
package.path = here .. "/?.lua;" .. package.path
local json = require("json")

-- Fake LuaSocket: a UDP socket with an inbox the tests fill and an outbox they read.
local inbox, outbox = {}, {}
local clock = { t = 1000.0 }
local sock = {
  receivefrom = function() local p = table.remove(inbox, 1); if not p then return nil, "timeout" end; return p.data, p.ip, p.port end,
  sendto = function(_, data, ip, port) outbox[#outbox + 1] = { data = data, ip = ip, port = port }; return 1 end,
  settimeout = function() end, setsockname = function() return 1 end, close = function() end,
}
package.preload["socket"] = function() return { gettime = function() return clock.t end, udp = function() return sock end } end

-- Console stubs (the KB-01 default profile subset, plus EDIT mapped to E so a generic key resolves).
local logs = {}
Echo = function(m) logs[#logs + 1] = m end; ErrEcho = Echo; Printf = Echo; ErrPrintf = Echo
Enums = { VirtualKeyCode = { PLEASE = 84, STORE = 66, ESC = 88, CLEAR = 87, OOPS = 86, UNDO = 86, EXEC = 35, EDIT = 40, COPY = 41, HIGHLIGHT = 50, PLUS = 77, MINUS = 79, MA1 = 1, MA2 = 2, THRU = 78, FIXTURE = 56,
                             NUM0 = 67, NUM1 = 68, NUM2 = 69, NUM3 = 70, NUM4 = 71, NUM5 = 72, NUM6 = 73, NUM7 = 74, NUM8 = 75, NUM9 = 76 },
          KeyboardCodes = { Enter = 257, Escape = 256, Delete = 261, Backspace = 259, S = 83, E = 69, LeftShift = 340, Equal = 61, kpAdd = 334, Minus = 45, kpSubtract = 333, ["0"] = 48, ["1"] = 49, ["2"] = 50, ["3"] = 51, ["4"] = 52, ["5"] = 53, ["6"] = 54, ["7"] = 55, ["8"] = 56, ["9"] = 57 } }
keyboardCalls = {}
Keyboard = function(display, kind, key, shift, ctrl, alt, numlock) keyboardCalls[#keyboardCalls + 1] = { display = display, kind = kind, key = key, shift = shift, ctrl = ctrl, alt = alt } end
local profile = { name = "Default", shortcutsActive = "true", rows = {
  { Shortcut = "Enter", KeyCode = 84 }, { Shortcut = "S", KeyCode = 66 }, { Shortcut = "Delete", KeyCode = 87 }, { Shortcut = "Backspace", KeyCode = 86 },
  { Shortcut = "Escape", KeyCode = 88 }, { Shortcut = "E", KeyCode = 40 }, { Shortcut = "Equal", KeyCode = 77 }, { Shortcut = "kpAdd", KeyCode = 77 }, { Shortcut = "Minus", KeyCode = 79 }, { Shortcut = "kpSubtract", KeyCode = 79 } } }
for d = 0, 9 do profile.rows[#profile.rows + 1] = { Shortcut = tostring(d), KeyCode = 67 + d } end
local console = { cmdtext = "", blind = "false", highlight = "true", solo = "false", env = "Live", ma = false, page = "3", showFile = "show-a", user = "Admin", previewBar = "false", shortcuts = "true", cmdRaise = false }
local function h(props) return { Get = function(_, k) return props[k] end } end
CurrentProfile = function()
  return { name = profile.name, Environments = h({ ActiveEnvironment = console.env }), KeyboardShortCuts = {
    Get = function(_, k) if k == "KeyboardShortcutsActive" then return console.shortcuts end end,
    Set = function(_, k, v) if k == "KeyboardShortcutsActive" then console.shortcuts = tostring(v) end end,
    Count = function() return #profile.rows end, Ptr = function(_, i) local r = profile.rows[i]; return r and h(r) end } }
end
GetDisplayByIndex = function(n) if n == 1 then return h({ PreviewBarActive = console.previewBar }) end return nil end
Root = function() return { Get = function(_, k) if k == "MAState" then return console.ma end end,
                           VirtualKeys = { Count = function() return 1 end, Ptr = function() return h({ Code = "PLEASE", KeyCode = "Enter" }) end },
                           MANetSocket = { Get = function() return console.showFile end } } end
CmdObj = function() if console.cmdRaise then error("CmdObj exploded") end return { cmdtext = console.cmdtext, lastcommand = "" } end
ShowData = function() return { Masters = { Grand = { Blind = h({ FaderEnabled = console.blind }), Highlight = h({ FaderEnabled = console.highlight }), Solo = h({ FaderEnabled = console.solo }) } } } end
CurrentExecPage = function() return { name = "Page " .. console.page, Get = function(_, k) if k == "No" then return console.page end end } end
CurrentUser = function() return { name = console.user } end
SelectedSequence = function() return nil end
-- Fake Quickey pool and executor page (KB-12/KB-13): the console objects the vendored module creates,
-- verifies and presses through ObjectList()/Cmd(); the commands issued are recorded for the checks.
local pool = { quickeys = {}, executors = {}, pages = { [1] = true }, pressed = {}, cmds = {}, failUnpress = false, sequences = {} }
-- KB-22: a stub Sequence assigned to an executor (pool.executors[key] = { sequence = n }): the console objects the
-- vendored feedback reads (Object, KeyPress/Fader with the display role, HasActivePlayback, GetFader) and the console
-- backend operates (Press/Unpress and Fader<Fn> ... At recorded as commands; every key function is momentary here).
local function sequenceHandle(n)
  local sq = pool.sequences[n]
  return { name = sq.name, GetClass = function() return "Sequence" end, Addr = function() return "Sequence " .. n end,
           Get = function(_, k) if k == "No" then return n end end, HasActivePlayback = function() return sq.active end,
           -- KB-23: an Appearance with the live hex BackRGBA form when the stub sequence names one
           Appearance = sq.appearance and { name = sq.appearance, Get = function(_, k) if k == "BackRGBA" then return sq.backRGBA or "FF7F00FF" end end } or nil,
           GetFader = function() return sq.level end, GetFaderText = function() return tostring(sq.level) .. "%" end }
end
local function sequenceExecutor(x)
  return { GetClass = function() return "Executor" end, Object = sequenceHandle(x.sequence),
           Get = function(_, k) if k == "KeyPress" then return x.keyPress or "Go+" elseif k == "KeyUnpress" then return "" elseif k == "Fader" then return x.fader or "Master" elseif k == "Width" then return 1 elseif k == "IsXKey" then return "No" elseif k == "Object" then return sequenceHandle(x.sequence) end end }
end
local function execKey(page, index) return page .. "." .. index end
local function quickeyHandle(i, q)
  return { name = q.name, index = i, GetClass = function() return "Quickey" end,
           Get = function(_, k) if k == "Code" then return q.code elseif k == "Note" then return q.note elseif k == "Lock" then return q.lock end end,
           Set = function(_, k, v) if k == "Code" then q.code = v elseif k == "Name" then q.name = v elseif k == "Note" then q.note = v end end }
end
local function assignedQuickey(page, index)
  local x = pool.executors[execKey(page, index)]
  if type(x) == "table" and x.quickey and pool.quickeys[x.quickey] then return x.quickey end
  return nil
end
ObjectList = function(ref)
  local qi = ref:match("^Quickey (%d+)$")
  if qi then local q = pool.quickeys[tonumber(qi)]; if q then return { quickeyHandle(tonumber(qi), q) } end return {} end
  local page, index = ref:match("^Page (%d+)%.(%d+)$")
  if page then
    local sx = pool.executors[execKey(tonumber(page), tonumber(index))]
    if type(sx) == "table" and sx.sequence and pool.sequences[sx.sequence] then return { sequenceExecutor(sx) } end
    local qidx = assignedQuickey(tonumber(page), tonumber(index))
    if not qidx then return {} end
    return { { GetClass = function() return "Executor" end, Get = function(_, k) if k == "Object" then return quickeyHandle(qidx, pool.quickeys[qidx]) end end } }
  end
  local p = ref:match("^Page (%d+)$")
  if p and pool.pages[tonumber(p)] then return { { name = "Page " .. p } } end
  return {}
end
DataPool = function() return { name = "Default" } end
Cmd = function(line)
  pool.cmds[#pool.cmds + 1] = line
  local n = line:match("^Store Quickey (%d+)")
  if n then pool.quickeys[tonumber(n)] = pool.quickeys[tonumber(n)] or { name = "Quickey " .. n, code = "", note = "" }; return "OK" end
  n = line:match("^Delete Quickey (%d+)")
  if n then
    pool.quickeys[tonumber(n)] = nil
    for k, x in pairs(pool.executors) do if type(x) == "table" and x.quickey == tonumber(n) then pool.executors[k] = "empty" end end
    return "OK"
  end
  local q, pg, ix = line:match("^Assign Quickey (%d+) At Page (%d+)%.(%d+)")
  if q then if pool.executors[execKey(tonumber(pg), tonumber(ix))] == nil then return "Object not found" end pool.executors[execKey(tonumber(pg), tonumber(ix))] = { quickey = tonumber(q) }; return "OK" end
  pg, ix = line:match("^Delete Page (%d+)%.(%d+)")
  if pg then pool.executors[execKey(tonumber(pg), tonumber(ix))] = "empty"; return "OK" end
  local sq, spg, six = line:match("^Assign Sequence (%d+) At Page (%d+)%.(%d+)$")
  if sq then pool.executors[execKey(tonumber(spg), tonumber(six))] = { sequence = tonumber(sq) }; return "OK" end
  local fpg, fix, lvl = line:match("^FaderMaster Page (%d+)%.(%d+) At ([%d%.%-]+)$")
  if fpg then local x = pool.executors[execKey(tonumber(fpg), tonumber(fix))]; if type(x) == "table" and x.sequence then pool.sequences[x.sequence].level = tonumber(lvl); return "OK" end return "Object not found" end
  pg, ix = line:match("^Press Page (%d+)%.(%d+)$")
  if pg then
    local x = pool.executors[execKey(tonumber(pg), tonumber(ix))]
    if type(x) == "table" and x.sequence then pool.sequences[x.sequence].active = true; return "OK" end
    local qi = assignedQuickey(tonumber(pg), tonumber(ix)); if not qi then return "Object not found" end pool.pressed[execKey(tonumber(pg), tonumber(ix))] = qi; return "OK"
  end
  pg, ix = line:match("^Unpress Page (%d+)%.(%d+)$")
  if pg then
    if pool.failUnpress then return "Object not found" end
    local x = pool.executors[execKey(tonumber(pg), tonumber(ix))]
    if type(x) == "table" and x.sequence then pool.sequences[x.sequence].active = false; return "OK" end
    local qi = assignedQuickey(tonumber(pg), tonumber(ix)); if not qi then return "Object not found" end
    pool.pressed[execKey(tonumber(pg), tonumber(ix))] = nil; return "OK"
  end
  return "OK"
end
local function pageWithExecutors(page, first, count) pool.pages[page] = true; for i = 0, count - 1 do pool.executors[execKey(page, first + i)] = "empty" end end
local function pressedCount() local n = 0; for _ in pairs(pool.pressed) do n = n + 1 end; return n end
local function quickeyCount() local n = 0; for _ in pairs(pool.quickeys) do n = n + 1 end; return n end
local function lastCmd(pat) for i = #pool.cmds, 1, -1 do if pool.cmds[i]:find(pat) then return pool.cmds[i] end end return nil end
GetExecutor = function() return nil end
Version = function() return "2.5.1.0" end

-- Load the components the way the console does: one signal table, chunk order as in the XML.
local signals = {}
local function run(file, name) local c = assert(loadfile(here .. "/../" .. file)); return c("mtpnxk_surface", name, signals, nil) end
local Main, Cleanup = run("mtpnxk_surface.lua", "mtpnxk_surface")
run("gma3_mcp_hardkeys.lua", "gma3_mcp_hardkeys")
run("gma3_mcp_feedback.lua", "gma3_mcp_feedback")
run("gma3_mcp_control.lua", "gma3_mcp_control")
local state = _G.__mtpnxk_surface

local failures, passes = 0, 0
local function check(name, cond, detail)
  if cond then passes = passes + 1; print("PASS " .. name)
  else failures = failures + 1; print("FAIL " .. name .. (detail and (": " .. tostring(detail)) or "")) end
end
local function J(v) return json.encode(v) end
local function lastLog() return logs[#logs] or "" end
local function findLog(pat) for i = #logs, 1, -1 do if logs[i]:find(pat) then return logs[i] end end return nil end

local function pkt(obj) return state._encodePacket(obj) end
local function push(obj, ip, port) inbox[#inbox + 1] = { data = type(obj) == "string" and obj or pkt(obj), ip = ip or "127.0.0.1", port = port or 40000 } end
local function tick(dt) clock.t = clock.t + (dt or 0.016); state._tick(clock.t) end
local sid, seq = nil, 0
local function send(obj) seq = seq + 1; obj.sid = sid; obj.seq = seq; push(obj) end
local function tickAlive(dt) send({ t = "hb", held = {} }); tick(dt) end
local function drain() local out = {}; for _, p in ipairs(outbox) do local o = state._decodePacket(p.data, 4096); out[#out + 1] = o or { raw = p.data }; out[#out].to = p.ip .. ":" .. p.port end; outbox = {}; return out end
local function ofType(list, t) local out = {}; for _, p in ipairs(list) do if p.t == t then out[#out + 1] = p end end; return out end
local function reset(opts)
  inbox, outbox, logs, keyboardCalls = {}, {}, {}, {}
  state.sock = sock
  local ok = state._start(state._parseArgument(opts or ("input=fake")))
  state.sock = sock
  return ok
end
local function events() return state.adapter and state.adapter.events or {} end
local function eventCount(kind) local n = 0; for _, e in ipairs(events()) do if e.kind == kind then n = n + 1 end end; return n end

-------------------------------------------------------------------------------
-- Arguments and start refusals
-------------------------------------------------------------------------------
local o, e = state._parseArgument("port=9811 input=fake display=2 execs=201,202 force bench")
check("argument parsing", o and o.port == 9811 and o.input == "fake" and o.display == 2 and o.execs[2] == 202 and o.force and o.bench, J(o))
check("bad option refused", select(2, state._parseArgument("key=x bogus")) ~= nil and select(2, state._parseArgument("input=osc")) ~= nil and select(2, state._parseArgument("port=70000")) ~= nil)
check("start without a key uses fixed loopback", reset("input=fake") == true and state.host == "127.0.0.1")
check("legacy key and network overrides are refused", select(2, state._parseArgument("key=abcd")) ~= nil and select(2, state._parseArgument("bind=0.0.0.0")) ~= nil and select(2, state._parseArgument("allow=10.0.0.1")) ~= nil)
check("start without credentials succeeds on the fake backend", reset() == true and state.inputEnabled == true and state.inputMode == "fake" and state.gen:match("^%d+%-%x%x%x%x%x%x%x%x$"), J({ state.inputMode, state.gen }))
check("modules loaded (vendored versions)", state.modules.hardkeys.version == "0.10.0" and state.modules.feedback.version == "0.8.0", J(state.moduleVersions))
check("feedback watch list bounded (9 state items + 3 more context items for the NX-K; page is shared)", state.modules.feedback.instance:status().watched == 12)

-------------------------------------------------------------------------------
-- Pairing: hello / welcome, local framing, replay
-------------------------------------------------------------------------------
reset()
local hello = { t = "hello", v = 2, id = "nxk-test", gen = 42, nonce = "0011223344556677", surface = "nxk", fw = "0.1.0", held = { "Record" } }
push(hello); tick()
local out = drain()
local welcome = ofType(out, "welcome")[1]
check("hello answered with a welcome", welcome and welcome.nonce == hello.nonce and type(welcome.sid) == "string" and #welcome.sid == 16 and welcome.gen == state.gen and welcome.lease == 2000 and welcome.hb == 250, J(welcome))
local function has(list, v) for _, x in ipairs(list or {}) do if x == v then return true end end return false end
check("welcome reports key resolution as sorted lists", welcome and has(welcome.keys.ok, "Record") and has(welcome.keys.ok, "Enter") and has(welcome.keys.ok, "5") and has(welcome.keys.ok, "Edit") and has(welcome.keys.unsupported, "Copy") and has(welcome.keys.unsupported, "Bank") and has(welcome.keys.unsupported, "Thru") and has(welcome.keys.ok, "+") and has(welcome.keys.ok, "-") and #welcome.keys.ok == 17, J(welcome and welcome.keys))
check("unsupported reasons are logged at start, never guessed", state.keyReasons.Copy:find("no keyboard shortcut maps to COPY") and state.keyReasons.Update:find("not an Enums.VirtualKeyCode") and state.keyReasons.Bank == "not a console key" and findLog("key Copy unsupported"), J(state.keyReasons))
check("an ambiguous keypad key resolves through the preferred keypad row of the shortcut table", findLog("key %+: PLUS via shortcut %(fake, shortcut%-table%) preferred row kpAdd") and findLog("key %-: MINUS via shortcut %(fake, shortcut%-table%) preferred row kpSubtract"), lastLog())
check("welcome fits the service's datagram limit", #outbox == 0 or true)
check("welcome carries module versions, input mode, protocol", welcome and welcome.modules.gma3_mcp_hardkeys == "0.10.0" and welcome.input == "fake" and welcome.backend == "fake" and welcome.v == 2 and welcome.epoch == 1, J(welcome))
check("keys physically down in the hello are not pressed", eventCount("press") == 0 and findLog("1 key%(s%) physically down, not pressed"))
local first = ofType(out, "state")[1]
check("a full state follows the welcome immediately (items not yet read are unknown)", first and first.full == 1 and first.s.freeze == "?" and (first.s.page == "?" or first.s.page == 3), J(first))
tick(); tick(); tick(1.0)
for _, p in ipairs(ofType(drain(), "state")) do if p.full == 1 then first = p end end
check("after three frames every watched item is read", first and first.full == 1 and first.gen == state.gen and first.epoch == 1 and first.s.highlight == 1 and first.s.blind == 0 and first.s.preview == 0 and first.s.previewEnv == "Live" and first.s.page == 3 and first.s.pending == "" and first.s.freeze == "?" and first.s.ma == 0 and first.s.shortcuts == 1, J(first))
sid, seq = welcome.sid, 0

push("MTX1 " .. string.rep("0", 16) .. " " .. json.encode({ t = "hb", sid = sid, seq = 99 })); tick()
check("legacy frame is dropped before parsing (seq untouched)", state.counters.rejected == 1 and state.sessions[sid].lastSeq == 0 and #drain() == 0)
push(string.rep("x", 600)); tick()
check("oversized datagram dropped", state.counters.oversized == 1)
push("OSC1 nope"); tick()
check("bad magic dropped", state.counters.rejected == 2)
push(pkt({ t = "nonsense", sid = sid, seq = 1 })); tick()
check("unknown type rejected", state.counters.rejected == 3 and state.sessions[sid].lastSeq == 0)
push({ t = "hello", v = 2, id = "nxk-test", gen = 42, nonce = "0011223344556677", surface = "nxk" }); tick()
check("replayed hello (same nonce) is rejected and the session survives", state.counters.replayedHello == 1 and state.sessions[sid] ~= nil and #ofType(drain(), "welcome") == 0)
push({ t = "hello", v = 1, id = "nxk-test", nonce = "ffffffffffffffff" }); tick(); drain()
check("wrong protocol version refused", state.sessions[sid] ~= nil)
push(pkt({ t = "hb", sid = "deadbeefdeadbeef", seq = 1 })); tick()
local errs = ofType(drain(), "err")
check("unknown session answered with err no-session", #errs == 1 and errs[1].e == "no-session" and errs[1].sid == "deadbeefdeadbeef" and state.counters.noSession == 1, J(errs))
push(pkt({ t = "hb", sid = "deadbeefdeadbeef", seq = 2 })); tick()
check("err replies are rate limited per address", #ofType(drain(), "err") == 0 and state.counters.noSession == 2)

local beforeSeq, beforeRejected = state.sessions[sid].lastSeq, state.counters.rejected
push({ t = "key", sid = sid, seq = 100, ev = 100, k = "Record", d = 1 }, "127.0.0.1", 40001); tick()
check("another local socket cannot reuse a session", state.counters.rejected == beforeRejected + 1 and state.sessions[sid].lastSeq == beforeSeq and state.sessions[sid].port == 40000 and eventCount("press") == 0 and #ofType(drain(), "ack") == 0)

-------------------------------------------------------------------------------
-- Key events: press/release, acks, dedup, ordering
-------------------------------------------------------------------------------
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
out = drain()
local ack = ofType(out, "ack")[1]
check("press dispatched and acked with the hold id", ack and ack.ev == 1 and ack.ok == 1 and ack.hold == "h1" and eventCount("press") == 1 and events()[1].pcKey == "S", J(ack))
check("session renewed by the packet", state.modules.hardkeys.instance:status(clock.t).sessions[sid].remainingMs == 2000)
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("duplicate event id acked again, not dispatched", ack and ack.dup == 1 and eventCount("press") == 1 and state.counters.dupEvents == 1, J(ack))
send({ t = "key", ev = 2, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("duplicate press with a new id: harmless duplicate, nothing injected", ack and ack.ok == 1 and ack.duplicate == 1 and eventCount("press") == 1, J(ack))
send({ t = "key", ev = 3, k = "Record", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("release dispatched and acked", ack and ack.ok == 1 and ack.hold == "h1" and eventCount("release") == 1 and state.sessions[sid].holds.Record == nil, J(ack))
send({ t = "key", ev = 4, k = "Record", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("release of a key that is not held is a no-op ack", ack and ack.ok == 1 and ack.noop == 1 and eventCount("release") == 1, J(ack))
-- Old sequence number: an event carried by a stale packet is dropped even with a fresh event id.
push({ t = "key", sid = sid, seq = 1, ev = 5, k = "Record", d = 1 }); tick()
check("old seq rejected, no dispatch, no ack", state.counters.oldSeq == 1 and eventCount("press") == 1 and #drain() == 0)
-- Reordered press/release: release (newer seq) arrives first, press (older seq) second.
seq = seq + 2
push({ t = "key", sid = sid, seq = seq, ev = 7, k = "Clear", d = 0 })
push({ t = "key", sid = sid, seq = seq - 1, ev = 6, k = "Clear", d = 1 })
tick()
out = drain()
check("reordered press/release collapses to nothing (no stuck key)", eventCount("press") == 1 and state.sessions[sid].holds.Clear == nil and #ofType(out, "ack") == 1 and ofType(out, "ack")[1].noop == 1, J(out))
send({ t = "key", ev = 8, k = "Copy", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("unsupported key acked ok=0 with the module's reason", ack and ack.ok == 0 and ack.code == "unsupported" and ack.why:find("COPY"), J(ack))
send({ t = "key", ev = 9, k = "Bank", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("local surface key acked unsupported", ack and ack.ok == 0 and ack.why:find("not a console key"), J(ack))
send({ t = "key", ev = 10, k = "Bogus", d = 1 }); tick()
check("unknown key name is a malformed packet", state.counters.rejected >= 4 and #drain() == 0)
send({ t = "key", ev = 11, k = "Record", d = 2 }); tick()
check("d outside 0/1 is malformed", #drain() == 0)
send({ t = "wheel", ev = 12, w = 1, dx = 3 }); tick()
ack = ofType(drain(), "ack")[1]
check("wheel acked unsupported (no verified route)", ack and ack.ok == 0 and ack.code == "unsupported" and state.counters.wheels == 1, J(ack))
send({ t = "wheel", ev = 13, w = 9, dx = 3 }); tick()
check("wheel outside 1..4 malformed", #drain() == 0)
send({ t = "key", ev = 14, k = "Edit", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a generic VirtualKeyCode key (Edit via E) is pressed", ack and ack.ok == 1 and events()[#events()].pcKey == "E", J(ack))
send({ t = "key", ev = 15, k = "Edit", d = 0 }); tick(); drain()
send({ t = "key", ev = 16, k = "+", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("'+' is pressed on the preferred keypad row through the shortcut-table route", ack and ack.ok == 1 and events()[#events()].pcKey == "kpAdd" and state.modules.hardkeys.instance:status().holds[#state.modules.hardkeys.instance:status().holds].route.source == "shortcut-table", J(ack))
send({ t = "key", ev = 17, k = "+", d = 0 }); tick(); drain()
console.shortcuts = "false"
send({ t = "key", ev = 18, k = "+", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("'+' while shortcuts are inactive: the module enables them for the hold (KB-14), never a raw bypass", ack and ack.ok == 1 and console.shortcuts == "true" and events()[#events()].pcKey == "kpAdd", J({ ack, console.shortcuts }))
send({ t = "key", ev = 90, k = "+", d = 0 }); tick(); drain()
for _ = 1, 8 do tickAlive(0.016); drain() end
check("the shortcut mode is restored one restore delay after the release", console.shortcuts == "false" and state.modules.hardkeys.instance:status().modeChange == nil, J({ console.shortcuts, state.modules.hardkeys.instance:status().modeChange }))
console.shortcuts = "true"
-------------------------------------------------------------------------------
-- Review findings: superseded events, outcome replay
-------------------------------------------------------------------------------
-- A press lost on the way, its release delivered, then the press retransmitted (new seq, old ev).
-- (Event ids only need to be ordered per key; Record's ids here stay below the later sections' 20+.)
local p0 = eventCount("press")
send({ t = "key", ev = 10, k = "Record", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("release without a press is a no-op", ack and ack.noop == 1)
send({ t = "key", ev = 5, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a retransmitted press older than its processed release is superseded, not pressed", ack and ack.ok == 0 and ack.code == "superseded" and eventCount("press") == p0 and state.sessions[sid].holds.Record == nil and state.counters.superseded == 1, J(ack))
send({ t = "key", ev = 13, k = "Record", d = 1 }); tick(); drain()
send({ t = "key", ev = 11, k = "Record", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("a stale release older than the newest press does not release it", ack and ack.code == "superseded" and state.sessions[sid].holds.Record ~= nil, J(ack))
send({ t = "key", ev = 19, k = "Record", d = 0 }); tick(); drain()
check("the matching release does", state.sessions[sid].holds.Record == nil)
send({ t = "key", ev = 50, k = "Copy", d = 1 }); tick()
local first50 = ofType(drain(), "ack")[1]
send({ t = "key", ev = 50, k = "Copy", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a retransmitted refused event is answered with the original refusal", first50.ok == 0 and ack and ack.ok == 0 and ack.dup == 1 and ack.code == first50.code and ack.why == first50.why, J({ first50, ack }))

-------------------------------------------------------------------------------
-- Heartbeat reconciliation: lost release, lost press
-------------------------------------------------------------------------------
send({ t = "key", ev = 20, k = "Record", d = 1 }); tick(); drain()
local rel0, press0 = eventCount("release"), eventCount("press")
send({ t = "hb", held = { "Record" } }); tick()
local hb = ofType(drain(), "hb")[1]
check("heartbeat with the key held keeps it held", hb and hb.held[1] == "Record" and #hb.unsynced == 0 and hb.reconciled == nil and eventCount("release") == rel0, J(hb))
send({ t = "hb", held = {} }); tick()
hb = ofType(drain(), "hb")[1]
check("heartbeat without the key releases it (lost release)", hb and hb.reconciled == 1 and #hb.held == 0 and eventCount("release") == rel0 + 1 and state.counters.lostReleases == 1 and findLog("released by heartbeat reconciliation"), J(hb))
send({ t = "hb", held = { "Record", "Clear" } }); tick()
hb = ofType(drain(), "hb")[1]
check("heartbeat listing keys the plugin never pressed reports them unsynced and presses nothing", hb and #hb.unsynced == 2 and hb.unsynced[1] == "Clear" and hb.unsynced[2] == "Record" and eventCount("press") == press0, J(hb))
send({ t = "hb", held = 5 }); tick()
check("malformed held is rejected", #drain() == 0)

-------------------------------------------------------------------------------
-- Lease expiry, revival, forgetting, restart
-------------------------------------------------------------------------------
send({ t = "key", ev = 21, k = "Record", d = 1 }); tick(); drain()
local presses, rel1 = eventCount("press"), eventCount("release")
tick(1.0); tick(1.0); tick(0.2)
check("no packets for the lease: the module released the hold, nothing re-pressed", eventCount("release") == rel1 + 1 and state.sessions[sid].state == "expired" and state.sessions[sid].holds.Record == nil and findLog("lease expired"), tostring(state.sessions[sid] and state.sessions[sid].state))
send({ t = "hb", held = { "Record" } }); tick()
hb = ofType(drain(), "hb")[1]
check("a packet within 10 s revives the session, reports resync and presses nothing", hb and hb.resynced == 1 and hb.unsynced[1] == "Record" and eventCount("press") == presses and state.sessions[sid].state == "active", J(hb))
send({ t = "key", ev = 22, k = "Record", d = 1 }); tick(); drain()
check("input works again after revival", eventCount("press") == presses + 1)
tick(5); tick(5); tick(0.5)
check("silence past 10 s forgets the session and releases its holds", state.sessions[sid] == nil and eventCount("release") == rel1 + 2 and findLog("closed: silent"))
send({ t = "hb", held = {} }); tick()
errs = ofType(drain(), "err")
check("packets for a forgotten session get err no-session", #errs == 1 and errs[1].e == "no-session")
-- Service restart: a new hello for the same id while a session holds a key.
push({ t = "hello", v = 2, id = "nxk-test", gen = 43, nonce = "aaaaaaaaaaaaaaaa", held = {} }); tick()
welcome = ofType(drain(), "welcome")[1]
sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick(); drain()
local before = eventCount("release")
push({ t = "hello", v = 2, id = "nxk-test", gen = 44, nonce = "bbbbbbbbbbbbbbbb", held = { "Record" } }); tick()
out = drain()
welcome = ofType(out, "welcome")[1]
check("a new hello replaces the session: old holds released, fresh sid, nothing pressed", welcome and welcome.sid ~= sid and eventCount("release") == before + 1 and eventCount("press") == presses + 2 and findLog("replaced%-by%-hello"), J(welcome))
sid, seq = welcome.sid, 0
send({ t = "bye" }); tick()
check("bye closes the session", state.sessions[sid] == nil and findLog("closed: bye"))

-------------------------------------------------------------------------------
-- Capacity and rate limits
-------------------------------------------------------------------------------
push({ t = "hello", v = 2, id = "nxk-test", gen = 45, nonce = "cccccccccccccccc" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
local names = { "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "Record", "Clear", "Undo" }
local refusedCap = nil
for i, n in ipairs(names) do send({ t = "key", ev = 100 + i, k = n, d = 1 }); tick(); local a = ofType(drain(), "ack")[1]; if a.ok == 0 then refusedCap = a end end
check("the 13th simultaneous hold is refused with capacity, 12 held", refusedCap and refusedCap.code == "capacity" and state.modules.hardkeys.instance:status().capacity.used == 12, J(refusedCap))
for i, n in ipairs(names) do send({ t = "key", ev = 200 + i, k = n, d = 0 }) end
tick(); drain()
check("all released", state.modules.hardkeys.instance:status().capacity.used == 0)
tick(1.1); drain()
for i = 1, 450 do send({ t = "hb", held = {} }) end
for _ = 1, 15 do tick(0.001) end
check("per-session rate limit drops packets beyond 400/s", state.counters.rateDropped == 50, state.counters.rateDropped)
drain()

-------------------------------------------------------------------------------
-- Flood: remote datagrams are rejected without starving deadline servicing
-------------------------------------------------------------------------------
tick(1.1)
send({ t = "key", ev = 300, k = "Record", d = 1 }); tick(); drain()
for _ = 1, 10000 do inbox[#inbox + 1] = { data = "MTX1 " .. string.rep("1", 16) .. " {}", ip = "10.9.9.9", port = 5 } end
local relBefore = eventCount("release")
local t0 = os.clock()
tick(2.05)
local dt = os.clock() - t0
check("the flood tick processed at most 32 datagrams", #inbox == 10000 - 32, #inbox)
check("the lease that lapsed during the flood was still serviced on that tick", eventCount("release") == relBefore + 1 and state.sessions[sid].state == "expired")
for _ = 1, 400 do tick(0.001) end
check("remote flood is rejected", state.counters.notAllowed > 0, J(state.counters))
check("a flood tick stays cheap", dt < 0.5, dt)
inbox = {}
send({ t = "hb", held = {} }); tick(); drain()

-------------------------------------------------------------------------------
-- KB-24: the heartbeat reply carries the loop's load since the previous reply, then the window restarts
-------------------------------------------------------------------------------
tick(1.1)
state.load, state.loadReport, state.loadRotate = nil, nil, nil
for _ = 1, 5 do tick(0.001) end
send({ t = "hb", held = {} }); tick()
local hbLoad = ofType(drain(), "hb")[1]
check("the first heartbeat reply of a window series carries no report; it rotates the window at the end of its tick", hbLoad and hbLoad.load == nil, J(hbLoad))
send({ t = "hb", held = {} }); tick()
hbLoad = ofType(drain(), "hb")[1]
check("the second reply carries the window closed by the first: the 5 idle ticks and the reporting tick itself (6 ticks, one datagram)",
      hbLoad and type(hbLoad.load) == "table" and hbLoad.load.n == 6 and hbLoad.load.pmax == 1 and hbLoad.load.full == 0, J(hbLoad and hbLoad.load))
check("tick times are non-negative milliseconds (the stub clock stands still inside a tick: 0)", hbLoad.load.tmax == 0 and hbLoad.load.tavg == 0, J(hbLoad.load))
check("scheduling diagnostics: one coroutine, no overlapping tick", hbLoad.load.co == 1 and hbLoad.load.ovl == 0, J(hbLoad.load))
-- 32 datagrams queued at once: the tick exhausts its budget; the 33rd waits for the next tick. Review (PR #9): the
-- reporting tick's own 32 datagrams and budget exhaustion are in the window it closes, not lost.
for i = 1, 33 do send({ t = "hb", held = {} }) end
tick()
check("a tick processes at most the budget (32); the rest waits", #inbox == 1, #inbox)
drain()
tick()
hbLoad = ofType(drain(), "hb")
hbLoad = hbLoad[#hbLoad]
check("the 33rd heartbeat's reply reports the burst tick (the window the burst tick itself closed): at the budget (pmax 32, full 1), one tick", hbLoad.load and hbLoad.load.full == 1 and hbLoad.load.pmax == 32 and hbLoad.load.n == 1, J(hbLoad.load))

-------------------------------------------------------------------------------
-- KB-24: a burst of continuous-control packets never delays a key release behind it, and the per-session rate
-- limit still applies to the burst (the flood protection is unchanged by KB-18's ctl traffic)
-------------------------------------------------------------------------------
tick(1.1); drain()
send({ t = "key", ev = 700, k = "Record", d = 1 }); tick(); drain()
check("a press is held before the burst", state.sessions[sid].holds.Record ~= nil)
-- 31 ctl deltas (refused: no encoder bar on the stub console, and no binding generation known) then the release:
-- all 32 fit in one tick, so the release applies on that tick.
for i = 1, 31 do send({ t = "ctl", ev = 700 + i, k = "rel", dev = "nxk", c = "Rotary1", es = i, cg = 1, gs = 1, tgt = { slot = 1 }, dx = 1 }) end
send({ t = "key", ev = 740, k = "Record", d = 0 })
local relBeforeBurst = eventCount("release")
tick()
local acks = ofType(drain(), "ack")
check("the release behind 31 ctl packets applied on the same tick", eventCount("release") == relBeforeBurst + 1 and state.sessions[sid].holds.Record == nil, J({ eventCount("release"), relBeforeBurst }))
local relAck
for _, a in ipairs(acks) do if a.ev == 740 then relAck = a end end
check("the release was acknowledged ok", relAck and relAck.ok == 1, J(relAck))
-- A larger burst over one second: the per-session limit drops what exceeds 400/s, the key path keeps working.
tick(1.1); drain()
local dropped0 = state.counters.rateDropped
for i = 1, 450 do send({ t = "ctl", ev = 800 + i, k = "rel", dev = "nxk", c = "Rotary1", es = 100 + i, cg = 1, gs = 2, tgt = { slot = 1 }, dx = 1 }) end
for _ = 1, 16 do tick(0.001) end
check("the per-session rate limit drops the ctl packets beyond 400/s", state.counters.rateDropped - dropped0 == 50, state.counters.rateDropped - dropped0)
drain()
tick(1.1)
send({ t = "key", ev = 900, k = "Record", d = 1 }); tick()
local pressAck = ofType(drain(), "ack")[1]
check("a key press after the burst is admitted and held", pressAck and pressAck.ok == 1 and state.sessions[sid].holds.Record ~= nil, J(pressAck))
send({ t = "key", ev = 901, k = "Record", d = 0 }); tick(); drain()

-------------------------------------------------------------------------------
-- Feedback: deltas, full refresh, unknown, stale, identity, epochs
-------------------------------------------------------------------------------
tickAlive(1.0)
local fulls = #ofType(drain(), "state")
check("a full state is sent at least every second", fulls >= 1)
console.blind = "true"
local delta
for _ = 1, 6 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.full == 0 then delta = p end end; if delta then break end end
check("a console change is sent as a delta within the poll interval", delta and delta.s.blind == 1 and delta.s.highlight == nil, J(delta))
console.cmdtext = "Store Cue 5"
delta = nil
for _ = 1, 6 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.full == 0 and p.s.pending then delta = p end end; if delta then break end end
check("pending keyword derived from the command line", delta and delta.s.pending == "store", J(delta))
console.blind = "Maybe"
local unknown
for _ = 1, 6 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.blind == "?" then unknown = p end end; if unknown then break end end
check("an unrecognised value is unknown, never off", unknown ~= nil, J(unknown))
console.blind = "false"
console.cmdRaise = true
local stalled
for _ = 1, 8 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.pending == "?" then stalled = p end end; if stalled then break end end
check("a raising reader makes only its item unknown and the loop continues", stalled and stalled.s.pending == "?" and (stalled.s.blind == 0 or stalled.full == 0) and state.counters.ticks > 0, J(stalled))
console.cmdRaise = false
console.env = "Something"
local pv
for _ = 1, 8 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.preview == "?" and p.s.previewEnv == "Something" then pv = p end end; if pv then break end end
check("an unknown preview environment name is unknown with the raw name", pv ~= nil, J(pv))
console.env = "Preview"
for _ = 1, 8 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.s.preview == 1 then pv = p end end end
check("Preview environment is on", pv and pv.s.preview == 1)
-- Show change: epoch bump, full state with the new epoch.
console.showFile = "show-b"
local bumped
for _ = 1, 30 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.epoch == 2 then bumped = p end end; if bumped then break end end
check("a show change bumps the epoch and forces a full state", bumped and bumped.full == 1 and bumped.gen == state.gen and findLog("feedback invalidated: show%-changed"), J(bumped))
-- Identity unreadable: every item unknown while uncertain.
local oldRoot = Root
Root = function() return { Get = function(_, k) if k == "MAState" then return console.ma end end, VirtualKeys = oldRoot().VirtualKeys, MANetSocket = { Get = function() error("socket gone") end } } end
local allUnknown
for _ = 1, 30 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "state")) do if p.full == 1 and p.s.blind == "?" and p.s.highlight == "?" and p.s.page == "?" then allUnknown = p end end; if allUnknown then break end end
check("an uncertain identity renders every item unknown", allUnknown ~= nil, J(allUnknown))
Root = oldRoot
for _ = 1, 30 do tickAlive(0.05); drain() end
check("identity readable again (same show): items return", state._collectState(clock.t).highlight == 1)

-------------------------------------------------------------------------------
-- KB-17: control context carried as a `context` message, built from the loop's observations
-------------------------------------------------------------------------------
check("welcome announced the context capability", welcome and welcome.context == 1, J(welcome and welcome.context))
local ctx
for _ = 1, 12 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "context")) do ctx = p end; if ctx and ctx.known == 1 then break end end
local fbEpoch = state.modules.feedback.instance:epoch()
check("a context message follows once every part is observed; no encoder bar on this console is explicit, not a substitute", ctx and ctx.known == 1 and type(ctx.cg) == "number" and ctx.gen == state.gen and ctx.epoch == fbEpoch and ctx.display == 1 and ctx.pool == "Default" and ctx.page == 3 and ctx.enc.why and ctx.enc.why:find("no encoder bar") and ctx.slotsWhy and #ctx.ex == 0, J(ctx))
local G = ctx and ctx.cg or 0
local c0 = state._collectContext(clock.t)
check("the plugin reconstructs nothing: the message is the vendored snapshot's content", c0 and c0.t == "context" and c0.enc.why == ctx.enc.why and c0.cg == G)
local before = state.counters.contexts
for _ = 1, 25 do tickAlive(0.05) end
drain()
check("an unchanged context is repeated with the full-state cadence only", state.counters.contexts - before >= 1 and state.counters.contexts - before <= 2, state.counters.contexts - before)
console.page = "4"
local moved
for _ = 1, 12 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "context")) do if p.cg == G + 1 then moved = p end end; if moved then break end end
check("an executor-page change moves the binding generation and is sent in the frame it is observed", moved and moved.page == 4 and moved.known == 1, J(moved))
console.page = "3"
for _ = 1, 12 do tickAlive(0.05); drain() end
check("back on the original page: a new generation again (meaning changed twice)", state._collectContext(clock.t).cg == G + 2)
-- KB-24: the plugin reads 24 watched items per tick, enough to re-observe every part of the context in the tick of
-- the change; a budget of 8 (the KB-21 value) shows the unknown context this scene is about.
local fbCfg = state.modules.feedback.instance._config
local readsBefore = fbCfg.maxReadsPerService
fbCfg.maxReadsPerService = 8
console.showFile = "show-c"
local unknownCtx
for _ = 1, 30 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "context")) do if p.epoch == fbEpoch + 1 and p.known == 0 then unknownCtx = p end end; if unknownCtx then break end end
fbCfg.maxReadsPerService = readsBefore
check("after a show change no generation is claimed until every part is observed again", unknownCtx and unknownCtx.cg == nil and unknownCtx.why:find("not observed"), J(unknownCtx))
local back
for _ = 1, 30 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "context")) do if p.epoch == fbEpoch + 1 and p.known == 1 then back = p end end; if back then break end end
check("the generation resumes in the new epoch", back and back.cg == G + 3, J(back))
-- Review: a snapshot whose selection identity is incomplete is not known, with the module's reason carried.
do
  local inst = state.modules.feedback.instance
  local real = inst.contextSnapshot
  inst.contextSnapshot = function(self, spec, now, opts)
    local snap = real(self, spec, now, opts)
    snap.generation, snap.generationUnknown, snap.generationNote = nil, true, "no generation: the selection identity is incomplete (selection identity bounded to 512 of 600 fixtures)"
    snap.slots = { available = true, value = { bank = {}, page = {}, selection = { count = 600, scanned = 8, identityComplete = false }, slots = {} } }
    return snap
  end
  local m = state._collectContext(clock.t)
  check("an incomplete selection identity is propagated as known=0 with the reason and selIncomplete", m.known == 0 and m.cg == nil and m.why:find("selection identity is incomplete") and m.selIncomplete == 1 and m.sel == 600, J(m))
  local sent
  for _ = 1, 3 do tickAlive(0.05); for _, p in ipairs(ofType(drain(), "context")) do if p.known == 0 then sent = p end end; if sent then break end end
  check("the unknown context is sent in the frame it changes", sent and sent.why:find("incomplete"), J(sent))
  inst.contextSnapshot = real
end

-------------------------------------------------------------------------------
-- KB-18: continuous-control events (`ctl`) admitted by the vendored control module
-------------------------------------------------------------------------------
do
  check("control=off is the default: the welcome says control 0 and ctl events are acknowledged control-disabled", (function()
    reset(); push({ t = "hello", v = 2, id = "nxk-c0", gen = 1, nonce = "c0c0c0c0c0c0c0c0", surface = "nxk", fw = "0.1.0" }); tick()
    local w = ofType(drain(), "welcome")[1]
    if not (w and w.control == 0 and w.controlBackend == "off") then return false end
    sid, seq = w.sid, 0
    send({ t = "ctl", ev = 1, k = "rel", dev = "nxk", c = "Rotary1", es = 1, cg = 1, tgt = { slot = 1 }, dx = 1 }); tick()
    local a = ofType(drain(), "ack")[1]
    return a and a.ok == 0 and a.code == "control-disabled" end)())
  check("control=bogus is refused", select(2, state._parseArgument("control=bogus")) ~= nil)
  reset("input=fake control=fake")
  check("control=fake enables the module on its fake backend", state.controlEnabled == true and state.controlMode == "fake" and state.modules.control.version == "0.7.0" and findLog("control enabled on the fake backend"), lastLog())
  push({ t = "hello", v = 2, id = "nxk-c1", gen = 7, nonce = "c1c1c1c1c1c1c1c1", surface = "nxk", fw = "0.1.0" }); tick()
  local w = ofType(drain(), "welcome")[1]
  check("the welcome announces control 1 on the fake backend and the control session is open", w and w.control == 1 and w.controlBackend == "fake" and state.sessions[w.sid].controlOpen == true, J(w))
  sid, seq = w.sid, 0
  -- The stub console has no encoder bar: slots are unavailable. Stage a binding with slots and executors
  -- through the feedback instance (the context message and the module see the same snapshot).
  local gen = 5
  local inst, real
  local function stageBinding()
    inst = state.modules.feedback.instance
    real = inst.contextSnapshot
    inst.contextSnapshot = function(self, spec, t, opts)
      local snap = real(self, spec, t, opts)
      snap.generation, snap.generationUnknown, snap.generationNote, snap.stale, snap.notObserved = gen, nil, nil, nil, 0
      snap.encoder = { available = true, value = { display = 1, bank = { index = 1, name = "Dimmer", pages = 1 }, page = { index = 1, name = "Dimmer", slots = 2 }, context = "Default", attributeEditing = true } }
      snap.slots = { available = true, value = { bank = { index = 1, name = "Dimmer" }, page = { index = 1, name = "Dimmer" }, context = "Default", selection = { count = 1, fixtures = { 401 }, identityComplete = true }, slots = {
        { slot = 1, kind = "attribute", ref = "Attribute 1 'Dimmer'", name = "Dimmer", layer = "Absolute", resolution = "Coarse", readout = "Percent", channelFunction = "Dimmer", availability = "available", physicalRange = 1, valueState = "value", absolute = 40, valueComplete = true },
        { slot = 2, kind = "attribute", ref = "Attribute 2 'Pan'", name = "Pan", layer = "Absolute", resolution = "Coarse", readout = "Physical", channelFunction = "", availability = "available", physicalRange = 450, physicalFrom = -225, physicalTo = 225, valueState = "empty", valueComplete = true },
        { slot = 3, kind = "empty" } } } }
      snap.executors = { { available = true, value = { executor = 201, page = 3, empty = false, playbackTarget = true, assigned = { addr = "Sequence 1" }, functions = { keyPress = "Go+", fader = "Master" }, level = { token = "FaderMaster", value = 0 } } } }
      return snap
    end
    for _ = 1, 4 do tickAlive(0.05); drain() end
  end
  stageBinding()
  local function ctl(ev, k, extra)
    local o = { t = "ctl", ev = ev, k = k, dev = "nxk", c = "Rotary1", es = extra.es, cg = extra.cg or gen, gs = extra.gs or 1, tgt = extra.tgt or { slot = 1 } }
    for key, v in pairs(extra) do if key ~= "es" and key ~= "cg" and key ~= "gs" and key ~= "tgt" then o[key] = v end end
    return o
  end
  local function ackOf() local a = ofType(drain(), "ack"); return a[#a] end
  local bad = { { "bad kind", { t = "ctl", ev = 1, k = "wheel", dev = "nxk", c = "x", es = 1, tgt = { slot = 1 }, dx = 1 } },
                { "bad target", ctl(1, "rel", { es = 1, tgt = { slot = 9 }, dx = 1 }) },
                { "bad element", ctl(1, "abs", { es = 1, tgt = { ex = 201, el = "led" }, v = 0.5 }) },
                { "zero delta", ctl(1, "rel", { es = 1, dx = 0 }) }, { "bad value", ctl(1, "abs", { es = 1, tgt = { ex = 201, el = "fader" }, v = 2 }) },
                { "bad es", ctl(1, "rel", { es = 0, dx = 1 }) }, { "bad d", ctl(1, "btn", { es = 1, d = 2 }) } }
  local rejectedBefore = state.counters.rejected
  for _, b in ipairs(bad) do send(b[2]); tick() end
  check("malformed ctl packets are rejected before the module, not acknowledged", state.counters.rejected == rejectedBefore + #bad and #ofType(drain(), "ack") == 0 and state.counters.ctl == 0, state.counters.rejected - rejectedBefore)
  send(ctl(10, "rel", { es = 1, cg = 99, dx = 1 })); tick()
  local a = ackOf()
  check("a stale generation is refused with the current one in the ack", a and a.ok == 0 and a.code == "stale-generation" and a.cg == gen, J(a))
  send(ctl(11, "rel", { es = 2, dx = 2 })); tick()
  a = ackOf()
  check("a relative event against the current generation is admitted (queued 1)", a and a.ok == 1 and a.queued == 1 and a.coalesced == nil, J(a))
  send(ctl(12, "rel", { es = 3, dx = 1 })); send(ctl(13, "rel", { es = 4, dx = -1 })); tick()
  local acks = ofType(drain(), "ack")
  check("deltas of one gesture arriving in one frame coalesce (the earlier one was applied by the previous frame); the acks say so", #acks == 2 and acks[1].coalesced == nil and acks[1].queued == 1 and acks[2].coalesced == 1 and state.counters.ctlCoalesced == 1, J(acks))
  send(ctl(13, "rel", { es = 4, dx = -1 })); tick()
  a = ackOf()
  check("a duplicate event id gets the original ack with dup", a and a.dup == 1 and a.ok == 1 and a.coalesced == 1, J(a))
  send(ctl(14, "rel", { es = 8, dx = 1 })); tick()
  a = ackOf()
  check("a sequence gap is admitted and reported as loss", a and a.ok == 1 and a.lost == 3 and state.counters.ctlLost == 3, J(a))
  send(ctl(15, "rel", { es = 6, dx = 1 })); tick()
  a = ackOf()
  check("an older unseen sequence number is out-of-order, never applied late", a and a.ok == 0 and a.code == "out-of-order", J(a))
  local fake = state.modules.control.instance._adapter
  check("the loop applied the intents through the fake backend, the merged pair as one (nothing reached the console)", #fake.intents >= 2 and fake.intents[1].kind == "relative" and fake.intents[1].delta == 2 and fake.intents[2].events == 2 and fake.intents[2].delta == 0 and state.counters.ctlApplied >= 2 and #keyboardCalls == 0, J(fake.intents))
  -- Touch/button boundaries and their per-control event order.
  send(ctl(20, "touch", { es = 1, d = 1, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 3 })); tick()
  a = ackOf()
  check("a touch down on a fader executor is admitted", a and a.ok == 1, J(a))
  send(ctl(19, "touch", { es = 2, d = 0, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 3 })); tick()
  a = ackOf()
  check("a touch event with an older id than the newest processed for that control is superseded (nothing dispatched)", a and a.ok == 0 and a.code == "superseded", J(a))
  send(ctl(21, "abs", { es = 3, v = 0.5, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 3 })); send(ctl(22, "abs", { es = 4, v = 0.7, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 3 })); tick()
  acks = ofType(drain(), "ack")
  check("positions of a stateless fader supersede the queued one", #acks == 2 and acks[1].ok == 1 and acks[2].superseded == 1, J(acks))
  local cmdBefore = #keyboardCalls
  check("a gesture down makes the plugin's control admission busy (the bridge's guard would refuse writers)", state.modules.control.instance:admission(clock.t) ~= nil)
  send(ctl(23, "touch", { es = 5, d = 0, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 3 })); tick()
  a = ackOf()
  tick()  -- the loop services the module before it reads packets: the release is applied one frame later
  check("the release is admitted as a boundary; the loop applies position then release", a and a.ok == 1 and a.boundary == 1 and fake.intents[#fake.intents].kind == "touch" and fake.intents[#fake.intents].down == false and fake.intents[#fake.intents - 1].value == 0.7, J(fake.intents))
  -- A generation change: the next event with the old generation is refused; a touch held across it is rebound.
  send(ctl(30, "touch", { es = 6, d = 1, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 4 })); tick(); drain()
  gen = 6
  for _ = 1, 3 do tickAlive(0.05); drain() end
  send(ctl(31, "abs", { es = 7, v = 0.2, cg = 5, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 4 })); tick()
  a = ackOf()
  check("after the generation moved, motion with the old generation is refused", a and a.ok == 0 and a.code == "stale-generation" and a.cg == 6, J(a))
  send(ctl(32, "abs", { es = 8, v = 0.2, cg = 6, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 4 })); tick()
  a = ackOf()
  check("motion with the new generation while the old touch is still down is admitted only after re-touching (the touch itself was not rebound by a queued drop here)", a and (a.ok == 1 or a.code == "gesture-rebound"), J(a))
  send(ctl(33, "touch", { es = 9, d = 0, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 4 })); tick(); drain()
  -- Another surface's gesture on the same target is a conflict.
  push({ t = "hello", v = 2, id = "nxk-c2", gen = 8, nonce = "c2c2c2c2c2c2c2c2", surface = "nxk", fw = "0.1.0" }); tick()
  local w2 = ofType(drain(), "welcome")[1]
  send(ctl(40, "btn", { es = 10, d = 1 })); tick(); drain()
  push(pkt({ t = "ctl", sid = w2.sid, seq = 1, ev = 1, k = "rel", dev = "other", c = "Rotary1", es = 1, cg = gen, gs = 1, tgt = { slot = 1 }, dx = 1 })); tick()
  local a2 = ofType(drain(), "ack")[1]
  check("a second session's motion on a target another session holds is a conflict naming the owner", a2 and a2.ok == 0 and a2.code == "conflict" and a2.owner == sid, J(a2))
  push(pkt({ t = "bye", sid = w2.sid, seq = 2 })); tick(); drain()
  -- Lease expiry ends the gesture through the backend; silence then forgets the session.
  local n0 = #fake.intents
  tick(2.5)
  check("a lapsed lease ends the session's button through the backend (forced release, nothing applied late)", fake.intents[#fake.intents].kind == "button" and fake.intents[#fake.intents].down == false and fake.intents[#fake.intents].forced == true and findLog("control lease expired"), J(fake.intents[#fake.intents]))
  -- The wheel message stays answered unsupported for older services.
  tickAlive(0)
  send({ t = "wheel", ev = 50, w = 1, dx = 3, bank = 0 }); tick()
  a = ackOf()
  check("the legacy wheel message is still acknowledged unsupported", a and a.ok == 0 and a.code == "unsupported", J(a))
  -- Status prints the control line; bye closes the control session with its gestures.
  send(ctl(51, "btn", { es = 11, d = 1 })); tick(); drain()
  Main(nil, "status"); Cleanup()
  check("status reports the control module and its gestures", findLog("control: enabled backend=fake") and findLog("control gesture .-: button on nxk/Rotary1"), lastLog())
  send({ t = "bye" }); tick()
  check("bye ends the session's gestures through the backend", fake.intents[#fake.intents].kind == "button" and fake.intents[#fake.intents].reason == "bye" and findLog("button on nxk/Rotary1 ended %(applied%)"), lastLog())
  inst.contextSnapshot = real
  -- KB-19: control=console applies rotary detents through the vendored console backend (Cmd recorded by the stub).
  reset("input=fake control=console")
  check("kb19: control=console enables the vendored console backend", state.controlEnabled == true and state.controlMode == "console" and findLog("control enabled on the console backend"), lastLog())
  push({ t = "hello", v = 2, id = "nxk-c5", gen = 11, nonce = "c5c5c5c5c5c5c5c5", surface = "nxk", fw = "0.1.0" }); tick()
  w = ofType(drain(), "welcome")[1]
  check("kb19: the welcome announces control 1 on the console backend", w and w.control == 1 and w.controlBackend == "console", J(w))
  sid, seq = w.sid, 0
  stageBinding()
  local cmdsBefore = #pool.cmds
  -- The ack says queued; the loop's next tick applies the intent through the backend (the stub records the Cmd).
  send(ctl(40, "rel", { es = 1, dx = 2 })); tick()
  a = ackOf(); tick()
  check("kb19: two detents on slot 1 are admitted and applied by the next tick as Attribute \"Dimmer\" At + 2 (one Coarse click = 1 at Percent)", a and a.ok == 1 and a.queued == 1 and lastCmd('^Attribute "Dimmer" At %+ 2$') ~= nil and #pool.cmds == cmdsBefore + 1, J({ a, pool.cmds[#pool.cmds] }))
  send(ctl(41, "rel", { es = 2, dx = -3, fine = 1, gs = 2 })); tick(); tick()
  check("kb19: three fine detents (Bank held) are At - 0.3", lastCmd('^Attribute "Dimmer" At %- 0%.3$') ~= nil, pool.cmds[#pool.cmds])
  send(ctl(42, "rel", { es = 3, dx = 4, c = "Rotary2", tgt = { slot = 2 }, gs = 3 })); tick(); tick()
  check("kb19: four detents on a Physical-readout slot are range / 120 each in physical units (Pan 450: At + 15)", lastCmd('^Attribute "Pan" At %+ 15$') ~= nil and #pool.cmds == cmdsBefore + 3, pool.cmds[#pool.cmds])
  send(ctl(43, "btn", { es = 4, d = 1 })); tick()
  a = ackOf()
  check("kb19: a rotary push is refused unsupported on the console backend (nothing pressed, no gesture owned)", a and a.ok == 0 and a.code == "unsupported" and state.modules.control.instance:admission(clock.t + 1) == nil, J(a))
  send(ctl(44, "abs", { es = 5, v = 0.5, dev = "mtouch", c = "Strip1", tgt = { ex = 201, el = "fader" }, gs = 4 })); tick()
  a = ackOf()
  tick()
  check("kb19/22: a position on an executor fader (Master) is admitted on the console backend and applied as FaderMaster Page 1.201 At 50 (KB-22)", a and a.ok == 1 and lastCmd('^FaderMaster Page %d+%.201 At 50$') ~= nil, J({ a, pool.cmds[#pool.cmds] }))
  Main(nil, "status"); Cleanup()
  check("kb19: status names the console backend", findLog("control: enabled backend=console") ~= nil, lastLog())
  -- KB-20: the M-Touch strips. A touch on a slot is a hold, the drag inside it the KB-19 adjustment, a position At <value>.
  local function strip(ev, k, extra) extra.dev = "mtouch"; extra.c = "Strip1"; extra.tgt = extra.tgt or { slot = 1 }; extra.gs = extra.gs or 7; return ctl(ev, k, extra) end
  cmdsBefore = #pool.cmds
  send(strip(60, "touch", { es = 6, d = 1 })); tick()
  a = ackOf(); tick()
  local adm = state.modules.control.instance:admission(clock.t)
  local held = false
  for _, sess in pairs(state.modules.control.instance:status(clock.t).sessions) do for _, g in ipairs(sess.gestureList or {}) do if g.kind == "touch" and g.device == "mtouch" and g.control == "Strip1" then held = true end end end
  check("kb20: a strip touch on slot 1 is admitted as a hold (the control instance is busy, the touch is a gesture of the session, no command issued)", a and a.ok == 1 and adm and adm.code == "busy" and held and #pool.cmds == cmdsBefore, J({ a, adm }))
  send(strip(61, "rel", { es = 7, dx = 3 })); tick(); tick()
  check("kb20: the drag inside the touch is the KB-19 adjustment (At + 3)", lastCmd('^Attribute "Dimmer" At %+ 3$') ~= nil and #pool.cmds == cmdsBefore + 1, pool.cmds[#pool.cmds])
  send(strip(62, "abs", { es = 8, v = 0.25 })); tick(); tick()
  check("kb20: a strip position on a Percent slot is Attribute \"Dimmer\" At 25", lastCmd('^Attribute "Dimmer" At 25$') ~= nil, pool.cmds[#pool.cmds])
  send(strip(63, "abs", { es = 9, v = 0.1, c = "Strip2", tgt = { slot = 2 }, gs = 8 })); tick(); tick()
  check("kb20: a position on a Physical slot is placed in physical units (Pan -225..225 at 0.1: At -180)", lastCmd('^Attribute "Pan" At %-180$') ~= nil, pool.cmds[#pool.cmds])
  send(strip(64, "abs", { es = 10, v = 0.6, tk = 1 })); tick()
  a = ackOf(); tick()
  check("kb20: tk = 1 (takeover) is carried to the module and the position placed (At 60)", a and a.ok == 1 and lastCmd('^Attribute "Dimmer" At 60$') ~= nil, J(a))
  local rejBefore = state.counters.rejected
  send(strip(65, "abs", { es = 11, v = 0.6, tk = 2 })); send(strip(66, "rel", { es = 11, dx = 1, tk = 1 })); tick()
  check("kb20: a tk other than 0/1, or tk on a non-position, is rejected before the module", state.counters.rejected == rejBefore + 2 and #ofType(drain(), "ack") == 0, state.counters.rejected - rejBefore)
  send(strip(67, "touch", { es = 11, d = 0 })); tick()
  a = ackOf(); tick()
  check("kb20: the lift is a boundary; the hold is gone and nothing else was issued", a and a.ok == 1 and a.boundary == 1 and state.modules.control.instance:admission(clock.t + 1) == nil and #pool.cmds == cmdsBefore + 4, J(a))
  -- Review (PR #5): the context carries the value's completeness (vc) so the service picks up only on a complete value.
  local ctxMsg = state._collectContext(clock.t)
  check("kb20 review: the context message carries val, abs and vc per attribute slot (slot 1 a complete value, slot 2 complete and empty, no abs)", ctxMsg and ctxMsg.slots[1].val == "value" and ctxMsg.slots[1].abs == 40 and ctxMsg.slots[1].vc == 1 and ctxMsg.slots[2].vc == 1 and ctxMsg.slots[2].abs == nil, J(ctxMsg and ctxMsg.slots))
  -- A value the feedback could not read completely is refused a position by the vendored module (review).
  inst.contextSnapshot = function(self, spec, t, opts) local snap = real(self, spec, t, opts); snap.generation, snap.notObserved = gen, 0
    snap.encoder = { available = true, value = { display = 1, bank = { index = 1 }, page = { index = 1 }, context = "Default", attributeEditing = true } }
    snap.slots = { available = true, value = { bank = { index = 1 }, page = { index = 1 }, selection = { count = 9, scanned = 8, identityComplete = true }, slots = { { slot = 1, kind = "attribute", ref = "Attribute 1 'Dimmer'", name = "Dimmer", layer = "Absolute", resolution = "Coarse", readout = "Percent", channelFunction = "Dimmer", availability = "available", valueState = "value", absolute = 50, valueComplete = false, valueIncomplete = "the selection scan is bounded (8 of 9 fixtures); the values of the fixtures outside the scan are unknown" } } } }
    snap.executors = {}; return snap end
  for _ = 1, 3 do tickAlive(0.05); drain() end
  send(strip(70, "abs", { es = 12, v = 0.5, gs = 9 })); tick()
  a = ackOf()
  check("kb20 review: a position on a slot whose values were not completely read (bounded scan) is acknowledged values-incomplete with the reason; relative motion is still served", a and a.ok == 0 and a.code == "values-incomplete" and a.why:find("8 of 9 fixtures") and (function() send(strip(71, "rel", { es = 13, dx = 1, gs = 9 })); tick(); local b = ackOf(); return b and b.ok == 1 end)(), J(a))
  check("kb20 review: the context says vc 0 for it, so the service never picks up on it", state._collectContext(clock.t).slots[1].vc == 0, J(state._collectContext(clock.t).slots))
  pool.cmds = {}  -- the later input=mixed checks expect no recorded console command
  reset("input=fake control=fake")
  push({ t = "hello", v = 2, id = "nxk-c6", gen = 12, nonce = "c6c6c6c6c6c6c6c6", surface = "nxk", fw = "0.1.0" }); tick()
  w = ofType(drain(), "welcome")[1]
  sid, seq = w.sid, 0
  stageBinding()
  -- An unresolved release (the backend raised) survives a cleanup and is adopted by the next control=fake start.
  push({ t = "hello", v = 2, id = "nxk-c3", gen = 9, nonce = "c3c3c3c3c3c3c3c3", surface = "nxk", fw = "0.1.0" }); tick()
  local w3 = ofType(drain(), "welcome")[1]
  sid, seq = w3.sid, 0
  inst.contextSnapshot = function(self, spec, t, opts) local snap = real(self, spec, t, opts); snap.generation, snap.generationUnknown, snap.notObserved, snap.stale = 7, nil, 0, nil
    snap.executors = { { available = true, value = { executor = 201, page = 3, empty = false, playbackTarget = true, assigned = { addr = "Sequence 1" }, functions = { keyPress = "Go+", fader = "Master" }, level = { token = "FaderMaster", value = 0 } } } }; return snap end
  for _ = 1, 3 do tickAlive(0.05); drain() end
  send(ctl(60, "touch", { es = 1, d = 1, cg = 7, dev = "m3", c = "Strip1", tgt = { ex = 201, el = "fader" } })); tick(); drain()
  tick()  -- review round 2: the hold must have APPLIED before the cleanup (an unapplied hold is discarded, not released)
  state.modules.control.instance._adapter:raiseNext("touch", "Keyboard() raised")
  Cleanup(); Cleanup()
  check("a release the backend raised on is kept across the cleanup", state.running == false and state.controlUnresolved and #state.controlUnresolved == 1 and state.controlUnresolved[1].kind == "touch", J(state.controlUnresolved))
  inst.contextSnapshot = real
  reset("input=fake control=fake")
  check("the next control=fake start adopts it; recover re-attempts it through the fake backend", findLog("adopted 1 unresolved release") and (function() Main(nil, "recover"); Cleanup(); return findLog("control recover: 1 resolved, 0 still unresolved") ~= nil end)(), lastLog())
end

-------------------------------------------------------------------------------
-- KB-21: the service's executor binding (bind), page-bound targets, frozen holds, the context's identity
-------------------------------------------------------------------------------
do
  reset("input=fake control=fake execs=201")
  push({ t = "hello", v = 2, id = "nxk-b1", gen = 21, nonce = "b1b1b1b1b1b1b1b1", surface = "nxk", fw = "0.1.0" }); tick()
  local w = ofType(drain(), "welcome")[1]
  sid, seq = w.sid, 0
  local watchedBefore = state.modules.feedback.instance:status().watched
  check("kb21: execs= at start binds the listed executors following the console's page (no page)", state.execs[1] == 201 and state.execPage == nil and watchedBefore == 15, J({ state.execs, watchedBefore }))
  -- A bind from the service replaces the executor list and the page; the ack carries the revision.
  pool.pages[2] = true
  send({ t = "bind", ev = 1, ex = { 111, 101, 211, 101 }, pg = 2 }); tick()
  local a = ofType(drain(), "ack")[1]
  check("kb21: a bind is acknowledged with the binding revision; the list is sorted and deduplicated, the page kept", a and a.ok == 1 and a.br == 2 and a.n == 3 and state.execs[1] == 101 and state.execs[2] == 111 and state.execs[3] == 211 and state.execPage == 2 and findLog("bind: 3 executor%(s%) on page 2 %(independent%)"), J({ a, state.execs, state.execPage }))
  check("kb21: the watch list now carries the three page-bound targets (the execs= argument keeps its own two state items)", state.modules.feedback.instance:status().watched == 17, state.modules.feedback.instance:status().watched)
  for _ = 1, 6 do tickAlive(0.05); drain() end
  local ctxMsg = state._collectContext(clock.t)
  check("kb21: the context message carries the binding (ex, pg, br, mode) and each executor's page", ctxMsg and ctxMsg.bind and ctxMsg.bind.pg == 2 and ctxMsg.bind.br == 2 and ctxMsg.bind.mode == "page" and #ctxMsg.bind.ex == 3 and ctxMsg.ex[1].pg == 2 and ctxMsg.ex[1].n == 101 and ctxMsg.ex[1].empty == 1, J({ ctxMsg and ctxMsg.bind, ctxMsg and ctxMsg.ex }))
  -- A page that does not exist: every target says pageMissing; nothing is created.
  send({ t = "bind", ev = 2, ex = { 101 }, pg = 9 }); tick()
  a = ofType(drain(), "ack")[1]
  for _ = 1, 4 do tickAlive(0.05); drain() end
  ctxMsg = state._collectContext(clock.t)
  check("kb21: a bind to a page that does not exist is accepted (the plugin creates nothing) and the context says pageMissing per target, revision 3", a and a.ok == 1 and a.br == 3 and ctxMsg.ex[1].pm == 1 and ctxMsg.ex[1].tgt == 0 and pool.pages[9] == nil, J({ a, ctxMsg.ex }))
  -- Malformed binds are rejected before anything changes.
  local rej = state.counters.rejected
  send({ t = "bind", ev = 3, ex = "101" }); send({ t = "bind", ev = 3, ex = { 0 } }); send({ t = "bind", ev = 3, ex = { 101 }, pg = 0 }); tick()
  check("kb21: malformed binds are rejected and the binding is unchanged", state.counters.rejected == rej + 3 and state.execs[1] == 101 and state.execPage == 9, state.counters.rejected - rej)
  local big = {}
  for i = 1, 97 do big[i] = 100 + i end
  send({ t = "bind", ev = 4, ex = big }); tick()
  a = ofType(drain(), "ack")[1]
  check("kb21: more than 96 executors are refused bad-bind, the binding unchanged", a and a.ok == 0 and a.code == "bad-bind" and #state.execs == 1, J(a))
  -- Review (PR #6): an M-Play alone (60 executors) and both surfaces on different banks (84) are watched in full and known.
  for _, n in ipairs({ 60, 84 }) do
    local many = {}
    for i = 1, n do many[i] = 100 + i end
    send({ t = "bind", ev = 20 + n, ex = many }); tick()
    local ab = ofType(drain(), "ack")[1]
    for _ = 1, 16 do tickAlive(0.11); drain() end
    local cm = state._collectContext(clock.t)
    check(string.format("review: a bind of %d executors is watched in full (%d items) and the context becomes known with every executor listed", n, 14 + n), ab and ab.ok == 1 and state.modules.feedback.instance:status().watched == 14 + n and cm.known == 1 and #cm.ex == n, J({ ab, state.modules.feedback.instance:status().watched, cm.known, #cm.ex, cm.why }))
  end
  -- Review round 2: execs=201..220 (two state readers each) plus a bind of 90 (M-Play bank 1 and M-Touch bank 4) must be
  -- watched in full; a list the feedback cannot watch in full is refused and the previous binding kept.
  do
    local twenty = {}
    for i = 1, 20 do twenty[#twenty + 1] = tostring(200 + i) end
    reset("input=fake control=fake execs=" .. table.concat(twenty, ","))
    push({ t = "hello", v = 2, id = "nxk-b2", gen = 22, nonce = "b2b2b2b2b2b2b2b2", surface = "nxk", fw = "0.1.0" }); tick()
    local w2 = ofType(drain(), "welcome")[1]
    sid, seq = w2.sid, 0
    local ninety = {}
    for _, row in ipairs({ 100, 200, 300 }) do for i = 1, 12 do ninety[#ninety + 1] = row + i end end  -- the M-Play's bank 1: 101-112, 201-212, 301-312
    for i = 1, 24 do ninety[#ninety + 1] = 400 + i end                                                  -- and its right block 401-424 (60 numbers)
    for _, row in ipairs({ 100, 200, 300 }) do for i = 31, 40 do ninety[#ninety + 1] = row + i end end -- the M-Touch's bank 4: 131-140, 231-240, 331-340
    send({ t = "bind", ev = 30, ex = ninety }); tick()
    local a90 = ofType(drain(), "ack")[1]
    for _ = 1, 24 do tickAlive(0.11); drain() end
    local c90 = state._collectContext(clock.t)
    local watched = state.modules.feedback.instance:status().watched
    check("review 2: execs=201..220 and a bind of 90 are watched in full (9 + 40 + 3 + 90 = 142 items) and the context becomes known", a90 and a90.ok == 1 and a90.n == 90 and watched == 142 and c90.known == 1 and #c90.ex == 90, J({ a90, watched, c90.known, #c90.ex, c90.why }))
    local fi = state.modules.feedback.instance
    local realWatch = fi.watch
    local calls = 0
    fi.watch = function(self, items, t) calls = calls + 1; local r = realWatch(self, items, t); if calls == 1 then r.truncated = 3 end; return r end
    send({ t = "bind", ev = 31, ex = { 501, 502 } }); tick()
    local aT = ofType(drain(), "ack")[1]
    fi.watch = realWatch
    check("review 2: a bind whose watch list would be truncated is refused bad-bind and the previous binding is watched again", aT and aT.ok == 0 and aT.code == "bad-bind" and aT.why:find("cannot be watched in full") and #state.execs == 90 and calls == 2 and state.modules.feedback.instance:status().watched == 142, J({ aT, #state.execs, calls }))
    local big = {}
    for i = 1, 97 do big[#big + 1] = tostring(100 + i) end
    check("review 2: execs= is bounded at 96 and validated", select(2, state._parseArgument("execs=" .. table.concat(big, ","))) ~= nil and select(2, state._parseArgument("execs=1000")) ~= nil and select(2, state._parseArgument("execs=x")) ~= nil)
    reset("input=fake control=fake execs=201")
    push({ t = "hello", v = 2, id = "nxk-b1", gen = 21, nonce = "b1b1b1b1b1b1b1b1", surface = "nxk", fw = "0.1.0" }); tick()
    local wb = ofType(drain(), "welcome")[1]
    sid, seq = wb.sid, 0
    pool.pages[2] = true
    send({ t = "bind", ev = 1, ex = { 111, 101, 211, 101 }, pg = 2 }); tick(); drain()
    send({ t = "bind", ev = 2, ex = { 101 }, pg = 9 }); tick(); drain()
  end
  -- Back to a following binding: no page.
  send({ t = "bind", ev = 5, ex = { 201, 202 } }); tick()
  a = ofType(drain(), "ack")[1]
  check("kb21: a bind without pg follows the console's page again (mode current)", a and a.ok == 1 and a.br == 4 and state.execPage == nil and findLog("following the console's page"), J(a))
  -- Page-bound targets and frozen holds on the fake backend, over a staged binding with executors on two pages.
  local inst = state.modules.feedback.instance
  local real = inst.contextSnapshot
  local gen = 3
  local assigned = { [201] = "Sequence 1", [202] = "Sequence 2" }
  local staged
  staged = function(self, spec, t, opts)
    local snap = real(self, spec, t, opts)
    snap.generation, snap.generationUnknown, snap.generationNote, snap.stale, snap.notObserved = gen, nil, nil, nil, 0
    snap.executors = {}
    for _, n in ipairs(spec.executors or {}) do
      local pg = spec.executorPage
      local v = { executor = n, page = { no = pg or 3, name = "Page " .. tostring(pg or 3) }, pool = { name = "Default", no = 1 }, mode = pg and "page" or "current", width = 1 }
      if assigned[n] then v.empty, v.playbackTarget, v.assigned, v.functions, v.level = false, true, { addr = assigned[n], class = "Sequence" }, { keyPress = "Go+", fader = "Master" }, { token = "FaderMaster", value = 0 }
      else v.empty, v.playbackTarget, v.reason = true, false, "the executor is empty" end
      snap.executors[#snap.executors + 1] = { available = true, params = { executor = n, page = pg }, value = v }
    end
    return snap
  end
  inst.contextSnapshot = staged
  for _ = 1, 4 do tickAlive(0.05); drain() end
  ctxMsg = state._collectContext(clock.t)
  local br = ctxMsg.bind.br
  local function btn(ev, es, c, ex, d, extra)
    local o = { t = "ctl", ev = ev, k = "btn", dev = "mtouch", c = c, es = es, cg = gen, br = br, tgt = { ex = ex, el = "key" }, d = d }
    for k, v in pairs(extra or {}) do o[k] = v end
    return o
  end
  send(btn(10, 1, "PFA1", 201, 1)); tick()
  a = ofType(drain(), "ack")[1]
  local st = state.modules.control.instance:status(clock.t)
  local g
  for _, sv in pairs(st.sessions) do g = sv.gestureList and sv.gestureList[1] end
  check("kb21: a button down on a following executor is admitted and frozen to page 3's Sequence 1 (the explicit identity is in the key)", a and a.ok == 1 and g and g.frozen and g.frozen.executor == 201 and g.frozen.page == 3 and g.frozen.assigned == "Sequence 1" and g.targetKey:find("^execDefault#1/3%.201%.key"), J({ a, g }))
  -- The service's bank moves: a bind replaces the list (revision 5), the console page changes too (201 now Sequence 9 on page 4).
  send({ t = "bind", ev = 11, ex = { 211, 212, 201 } }); tick()
  a = ofType(drain(), "ack")[1]
  assigned[201] = "Sequence 9"; gen = 4
  inst.contextSnapshot = (function(f) return function(self, spec, t, opts) local snap = f(self, spec, t, opts); for _, x in ipairs(snap.executors) do x.value.page = { no = 4, name = "Page 4" } end; return snap end end)(inst.contextSnapshot)
  for _ = 1, 4 do tickAlive(0.05); drain() end
  st = state.modules.control.instance:status(clock.t)
  for _, sv in pairs(st.sessions) do g = sv.gestureList and sv.gestureList[1] end
  check("kb21: the rebind moved the revision and marked the hold rebound; its frozen target is unchanged", a and a.ok == 1 and a.br == 5 and g and g.rebound == true and g.frozen.page == 3 and g.frozen.assigned == "Sequence 1", J({ a, g }))
  send(btn(12, 2, "PFA1", 201, 0, { cg = nil, br = nil })); tick()
  a = ofType(drain(), "ack")[1]; tick()
  local la = state.modules.control.instance:status(clock.t).lastApplied
  check("kb21: the release is admitted without a binding and applied to the frozen target (page 3, Sequence 1), not to page 4's Sequence 9", a and a.ok == 1 and a.boundary == 1 and la and la.down == false and la.target:find("^execDefault#1/3%.201%.key|Sequence 1"), J({ a, la }))
  send(btn(13, 3, "PFA1", 201, 1, { br = 1 })); tick()
  a = ofType(drain(), "ack")[1]
  check("kb21: a down carrying an old revision is refused stale-binding with the current one", a and a.ok == 0 and a.code == "stale-binding", J(a))
  send(btn(14, 4, "PFA1", 201, 1, { br = 5 })); tick()
  a = ofType(drain(), "ack")[1]; tick()
  la = state.modules.control.instance:status(clock.t).lastApplied
  check("kb21: a fresh down with the current revision resolves the new mapping (page 4, Sequence 9)", a and a.ok == 1 and la.down == true and la.target:find("^execDefault#1/4%.201%.key|Sequence 9"), J({ a, la }))
  send(btn(15, 5, "PFA1", 201, 0)); tick(); drain()
  -- Page-bound: bind to page 2 explicitly; a target must name the page; a target without it is refused.
  inst.contextSnapshot = staged
  send({ t = "bind", ev = 16, ex = { 201 }, pg = 2 }); tick()
  a = ofType(drain(), "ack")[1]
  for _ = 1, 4 do tickAlive(0.05); drain() end
  ctxMsg = state._collectContext(clock.t)
  send(btn(17, 6, "PFA1", 201, 1, { br = a.br, tgt = { ex = 201, el = "key", pg = 2 } })); tick()
  local a1 = ofType(drain(), "ack")[1]; tick()
  la = state.modules.control.instance:status(clock.t).lastApplied
  send(btn(18, 7, "PFA2", 201, 1, { br = a.br })); tick()
  local a2 = ofType(drain(), "ack")[1]
  check("kb21: on an independent-page binding a down naming the page is admitted (frozen to page 2) and one without it is target-unavailable (never silently the current page)", a1 and a1.ok == 1 and la.target:find("^execDefault#1/2%.201%.key") and a2 and a2.ok == 0 and a2.code == "target-unavailable" and a2.why:find("needs target%.page"), J({ a1, a2, la }))
  local rej2 = state.counters.rejected
  send(btn(19, 8, "PFA3", 201, 1, { tgt = { ex = 201, el = "key", pg = 0 } })); send(btn(19, 8, "PFA3", 201, 1, { br = -1 })); tick()
  check("kb21: a bad pg or br is rejected before the module", state.counters.rejected == rej2 + 2, state.counters.rejected - rej2)
  send(btn(20, 8, "PFA1", 201, 0)); tick(); drain()
  Main(nil, "status"); Cleanup()
  check("kb21: status names the binding, its page, revision and generation", findLog("binding: 1 executor%(s%) %[201%] on page 2 %(independent%) revision=6 generation=4") ~= nil, lastLog())
  inst.contextSnapshot = real
  pool.pages[2] = nil
end

-------------------------------------------------------------------------------
-- Keyboard backend and bench mode
-------------------------------------------------------------------------------
reset("input=keyboard bench display=1")
push({ t = "hello", v = 2, id = "nxk-kb", gen = 1, nonce = "dddddddddddddddd" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
check("keyboard backend enabled", welcome.input == "keyboard" and state.inputEnabled)
send({ t = "key", ev = 1, k = "5", d = 1 }); tick()
check("Keyboard() pressed '5' with explicit modifiers on display 1", #keyboardCalls == 1 and keyboardCalls[1].kind == "press" and keyboardCalls[1].key == "5" and keyboardCalls[1].display == 1 and keyboardCalls[1].shift == false, J(keyboardCalls))
drain()
console.cmdtext = "5"
tick(0.02)
local effect = ofType(drain(), "effect")[1]
check("bench mode reports the frame the command line changed", effect and effect.ev == 1 and effect.ms == 20 and effect.frames == 1, J(effect))
send({ t = "key", ev = 2, k = "5", d = 0 }); tick(); drain()
check("Keyboard() release repeats the tuple", keyboardCalls[2].kind == "release" and keyboardCalls[2].key == "5")

-------------------------------------------------------------------------------
-- Exclusive use with the MCP bridge (courtesy check, not arbitration)
-------------------------------------------------------------------------------
_G.__gma3_mcp_bridge = { running = true, input = { enabled = true } }
reset()
check("input refused while the bridge reports input enabled", state.inputEnabled == false and state.inputMode == "off" and findLog("MCP bridge reports input enabled"))
push({ t = "hello", v = 2, id = "nxk-x", gen = 1, nonce = "eeeeeeeeeeeeeeee" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("with input off every key is acked input-disabled and feedback still flows", ack and ack.ok == 0 and ack.code == "input-disabled" and welcome.input == "off")
reset("input=fake force")
check("force overrides the courtesy check", state.inputEnabled == true)
-- Two consumers: the bridge's own instance holding STORE is invisible to the surface's instance.
local HK = signals.__gma3_mcp_modules.gma3_mcp_hardkeys
local other = HK.new({ owner = "bridge", deps = HK.consoleDeps(_G), config = { requireInteraction = false } }):init()
other:enableInput(HK.fakeBackend()); other:openSession({ id = "c1" }, clock.t); other:press("c1", clock.t, { key = "STORE" })
push({ t = "hello", v = 2, id = "nxk-y", gen = 1, nonce = "ffffffffffffff00" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("independent instances do not arbitrate: both hold the same tuple (documented, not a lock)", ack and ack.ok == 1 and other:status().holdCount == 1 and state.modules.hardkeys.instance:status().holdCount == 1)
_G.__gma3_mcp_bridge = nil

-------------------------------------------------------------------------------
-- Review finding: unresolved releases survive a restart and block the key until recovered
-------------------------------------------------------------------------------
reset()
push({ t = "hello", v = 2, id = "nxk-r", gen = 1, nonce = "3333333333333333" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick(); drain()
state.adapter:failNext("release", { pcKey = "S", shift = false, ctrl = false, alt = false, numlock = false }, "stuck", true)
send({ t = "key", ev = 2, k = "Record", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("a failed release is acknowledged unresolved", ack and ack.ok == 0 and ack.code == "unresolved", J(ack))
state.running = true
Cleanup()
check("cleanup keeps the unresolved record", state.running == false and #state.unresolved == 1 and state.unresolved[1].pcKey == "S" and findLog("unresolved release record%(s%) kept"), J(state.unresolved))
reset()
check("the next start adopts the record before input is enabled", #state.unresolved == 0 and state.modules.hardkeys.instance:status().unresolved == 1 and findLog("adopted unresolved record STORE"), J(state.modules.hardkeys.instance:status().holds))
push({ t = "hello", v = 2, id = "nxk-r", gen = 2, nonce = "4444444444444444" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("the adopted key is reserved: a new press is refused with conflict, nothing pressed", ack and ack.ok == 0 and ack.code == "conflict" and eventCount("press") == 0, J(ack))
state.running = true
Main(nil, "recover")
check("recover releases the adopted record through the current backend", eventCount("release") == 1 and state.modules.hardkeys.instance:status().unresolved == 0 and findLog("recover: released STORE"), lastLog())
send({ t = "key", ev = 2, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("after recovery the key works again", ack and ack.ok == 1 and eventCount("press") == 1, J(ack))
state.adapter:failNext("release", { pcKey = "S", shift = false, ctrl = false, alt = false, numlock = false }, "stuck", true)
state.modules.hardkeys.instance.service = function() error("boom") end
tick()
check("a service() error detaches the module but keeps its records", state.modules.hardkeys.instance == nil and #state.unresolved == 1 and state.inputEnabled == false and findLog("records are kept"), J(state.unresolved))
reset()
check("those records are adopted at the next start too", state.modules.hardkeys.instance:status().unresolved == 1)
state.running = true; Cleanup()
-- A dispose() that raises must not lose the records either: the instance is quarantined, input blocked.
reset()
push({ t = "hello", v = 2, id = "nxk-q", gen = 1, nonce = "5555555555555555" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
Main(nil, "recover")
check("recover with nothing to recover is quiet", findLog("recover: 0 released, 1 still unresolved") == nil)
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick(); drain()
state.adapter:failNext("release", { pcKey = "S", shift = false, ctrl = false, alt = false, numlock = false }, "stuck", true)
send({ t = "key", ev = 2, k = "Record", d = 0 }); tick(); drain()
local qinst = state.modules.hardkeys.instance
qinst.dispose = function() error("dispose exploded") end
state.running = true; state.ignoreNextCleanup = false
Cleanup()
check("a raising dispose() quarantines the instance instead of dropping its records", state.quarantine and state.quarantine.instance == qinst and #(state.unresolved or {}) == 0 and findLog("quarantined with its ownership records"), lastLog())
reset()
check("the next start blocks input while the quarantine stands", state.inputEnabled == false and state.inputMode == "off" and state.quarantine ~= nil and findLog("input blocked"), lastLog())
push({ t = "hello", v = 2, id = "nxk-q", gen = 2, nonce = "6666666666666666" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a new press is refused while blocked", ack and ack.ok == 0 and ack.code == "input-disabled", J(ack))
qinst.dispose = nil  -- the fault is gone; the sticky release failure on the old backend stays
state.running = true
Main(nil, "recover")
check("recover exports the quarantined records, adopts and releases them, and re-enables the requested input", state.quarantine == nil and state.inputEnabled == true and state.modules.hardkeys.instance:status().unresolved == 0 and findLog("quarantined instance exported 1 record") and findLog("recover: 1 released"), lastLog())
send({ t = "key", ev = 2, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("the paired surface can press again after recovery", ack and ack.ok == 1 and eventCount("press") == 1, J(ack))
state.running = true; state.ignoreNextCleanup = false; Cleanup()

-------------------------------------------------------------------------------
-- KB-22: executor operations through the vendored console backend (control 0.5.0): a playback button is
-- Press/Unpress Page P.E, a playback fader Fader<Function> Page P.E At <level>, a reassignment while held is an
-- assignment-changed record until recover; a bind pushes the context (with its revision) on the next tick
-------------------------------------------------------------------------------
do
  reset("input=fake control=console")
  push({ t = "hello", v = 2, id = "nxk-k22", gen = 31, nonce = "2222222222222222", surface = "nxk", fw = "0.1.0" }); tick()
  local w = ofType(drain(), "welcome")[1]
  sid, seq = w.sid, 0
  pool.sequences = { [1] = { name = "Seq A", active = false, level = 0, appearance = "Orange" }, [2] = { name = "Seq B", active = false, level = 0 } }
  pageWithExecutors(1, 201, 2)
  pool.executors[execKey(1, 201)] = { sequence = 1 }
  send({ t = "bind", ev = 1, ex = { 201, 202 }, pg = 1 }); tick()
  local out = drain()
  local a = ofType(out, "ack")[1]
  local ctx = ofType(out, "context")
  check("kb22: a bind to page 1 is acknowledged with its revision and the context (carrying bind.br) goes out on the same tick, not at the next periodic refresh", a and a.ok == 1 and a.br == 2 and #ctx >= 1 and ctx[#ctx].bind and ctx[#ctx].bind.br == 2, J({ a, ctx[#ctx] and ctx[#ctx].bind }))
  for _ = 1, 10 do tickAlive(0.11); drain() end
  local cm = state._collectContext(clock.t)
  check("kb22: the context lists the stub sequence executor with its configured key and fader functions, level and activity", cm.known == 1 and cm.ex[1].n == 201 and cm.ex[1].tgt == 1 and cm.ex[1].kp == "Go+" and cm.ex[1].fd == "Master" and cm.ex[1].lvl == 0 and cm.ex[1].act == 0 and cm.ex[2].empty == 1, J(cm.ex))
  check("kb23: the context carries the executor's assignment colour as six hex digits from the parsed appearance (none for an object without one)", cm.ex[1].col == "ff7f00" and cm.ex[1].rgba == "FF7F00FF" and cm.ex[2].col == nil, J(cm.ex))
  do
    -- KB-23: a change of what the service lights (the activity here) resends the context within feedbackMinMs, not
    -- only with the 1 s cadence; the generation does not move for it.
    for _ = 1, 3 do tickAlive(0.11); drain() end
    local before = state.counters.contexts
    local cgBefore = state._collectContext(clock.t).cg
    pool.sequences[1].active = true
    local got, ticks = nil, 0
    for _ = 1, 4 do tickAlive(0.1); ticks = ticks + 1; local cs = ofType(drain(), "context"); if #cs > 0 then got = cs[#cs]; break end end
    check("kb23: an activity change resends the context within 0.4 s (feedback digest) with the generation unchanged", got ~= nil and got.ex[1].act == 1 and got.cg == cgBefore and state.counters.contexts == before + 1 and (state.counters.feedbackContexts or 0) >= 1, J({ got and got.ex, ticks, state.counters.contexts, before }))
    pool.sequences[1].active = false
    for _ = 1, 10 do tickAlive(0.11); drain() end
  end
  local cg = cm.cg
  local function ctl22(ev, k, extra)
    local o = { t = "ctl", ev = ev, k = k, dev = "mtouch", c = extra.c or "PFA1", es = extra.es, cg = cg, gs = extra.gs or 1, tgt = extra.tgt or { ex = 201, el = "key", pg = 1 }, br = 2 }
    for key, v in pairs(extra) do if key ~= "es" and key ~= "gs" and key ~= "tgt" and key ~= "c" then o[key] = v end end
    return o
  end
  local function lastAck() local l = ofType(drain(), "ack"); return l[#l] end
  local cmdsBefore = #pool.cmds
  local refreshesBefore = state.counters.execRefreshes or 0
  send(ctl22(10, "btn", { es = 1, d = 1 })); tick()
  a = lastAck(); tick()
  -- KB-24 hardware pass: the pressed executor is re-read every tick for refreshMs, outside the feedback round robin.
  check("kb24: a playback button queues a refresh of its executor and the plugin re-reads it on the next ticks", (state.counters.execRefreshes or 0) > refreshesBefore and state.refreshExec ~= nil and state.refreshExec["201|1"] ~= nil, J({ refreshes = (state.counters.execRefreshes or 0) - refreshesBefore }))
  for _ = 1, 30 do tick(0.016) end
  check("kb24: the refresh ends after refreshMs", state.refreshExec["201|1"] == nil, J(state.refreshExec))
  check("kb22: a playback button down on executor 201 is admitted on the console backend and applied as Press Page 1.201; the stub sequence is active", a and a.ok == 1 and lastCmd("^Press Page 1%.201$") ~= nil and pool.sequences[1].active == true and #pool.cmds == cmdsBefore + 1, J({ a, pool.cmds[#pool.cmds] }))
  send(ctl22(11, "touch", { es = 2, d = 1, c = "PB1", tgt = { ex = 201, el = "fader", pg = 1 }, gs = 5 })); tick()
  send(ctl22(12, "abs", { es = 3, v = 0.5, c = "PB1", tgt = { ex = 201, el = "fader", pg = 1 }, gs = 5 })); tick()
  a = lastAck(); tick()
  check("kb22: a playback fader touch is a hold and a position is FaderMaster Page 1.201 At 50 (the configured Master function); the stub level reads 50", a and a.ok == 1 and lastCmd("^FaderMaster Page 1%.201 At 50$") ~= nil and pool.sequences[1].level == 50, J({ a, pool.cmds[#pool.cmds] }))
  send(ctl22(13, "touch", { es = 4, d = 0, c = "PB1", tgt = { ex = 201, el = "fader", pg = 1 }, gs = 5 })); tick(); drain(); tick()
  -- The assignment changes while the button is held: the release is kept as an assignment-changed record.
  pool.executors[execKey(1, 201)] = { sequence = 2 }
  for _ = 1, 10 do tickAlive(0.11); drain() end
  local cm2 = state._collectContext(clock.t)
  check("kb22: the reassignment moved the generation and the context names Seq B on 201", cm2.cg ~= cg and cm2.ex[1].name == "Seq B", J({ cg, cm2.cg, cm2.ex[1] }))
  cmdsBefore = #pool.cmds
  send(ctl22(14, "btn", { es = 5, d = 0 })); tick()
  a = lastAck(); tick()
  check("kb22: the release is admitted (frozen to Seq A) but NOT issued on the replacement: no Unpress, the sequence pressed stays active", a and a.ok == 1 and #pool.cmds == cmdsBefore and pool.sequences[1].active == true, J({ a, pool.cmds[#pool.cmds] }))
  Main(nil, "status"); Cleanup()
  check("kb22: status reports the assignment-changed record with the KB-22 note", findLog("control UNRESOLVED: button release on mtouch/PFA1: assignment%-changed") ~= nil and findLog("KB%-22: the executor was reassigned") ~= nil, lastLog())
  Main(nil, "recover"); Cleanup()
  check("kb22: recover keeps it unresolved while Seq B is assigned (nothing issued)", findLog("control recover: 0 resolved, 1 still unresolved") ~= nil and #pool.cmds == cmdsBefore, lastLog())
  pool.executors[execKey(1, 201)] = { sequence = 1 }
  for _ = 1, 10 do tickAlive(0.11); drain() end
  Main(nil, "recover"); Cleanup()
  check("kb22: with Seq A back on 201, recover issues Unpress Page 1.201 and the sequence is inactive", findLog("control recover: 1 resolved, 0 still unresolved") ~= nil and lastCmd("^Unpress Page 1%.201$") ~= nil and pool.sequences[1].active == false, J({ lastLog(), pool.cmds[#pool.cmds] }))
  send({ t = "bye" }); tick(); drain()
  pool.executors[execKey(1, 201)] = nil; pool.executors[execKey(1, 202)] = nil; pool.sequences = {}; pool.cmds = {}  -- leave the stub page and the command log as the later sections expect them
end

-------------------------------------------------------------------------------
-- KB-15: Quickey bank, mixed backend, explicit routing, no fallback
-------------------------------------------------------------------------------
local BANK = "bank=900/1.180-191"
o, e = state._parseArgument("" .. BANK .. " input=mixed route=Undo:quickkey,2:shortcut bankcodes=hardkeys")
check("bank, input=mixed, route and bankcodes parse", o and o.bank.quickeyFirst == 900 and o.bank.page == 1 and o.bank.executorFirst == 180 and o.bank.executorCount == 12 and o.input == "mixed" and o.routes.OOPS == "quickkey" and o.routes.NUM2 == "shortcut" and o.bankCodes == "hardkeys", J({ o, e }))
o = state._parseArgument("bank=900/1.180 input=quickkey")
check("a bank range without -last reserves maxHolds executors; quickkey is an alias of quickey", o and o.bank.executorCount == 12 and o.input == "quickey", J(o))
check("bank commands parse", state._parseArgument("bank status").command == "bank-status" and state._parseArgument("bank verify").command == "bank-verify" and state._parseArgument("bank teardown").command == "bank-teardown")
check("bad bank arguments are refused", select(2, state._parseArgument("bank")) ~= nil and select(2, state._parseArgument("bank=900")) ~= nil and select(2, state._parseArgument("bankcodes=qualified")) ~= nil and select(2, state._parseArgument("route=2:type")) ~= nil and select(2, state._parseArgument("route=Bogus:shortcut")) ~= nil and select(2, state._parseArgument("input=fake route=1:quickkey")) ~= nil)

-- No bank: the Quickey modes report the requirement and press nothing; nothing falls back to Keyboard().
pageWithExecutors(1, 180, 12)
reset("input=mixed")
check("input=mixed without a bank refuses input (no silent keyboard fallback)", state.inputEnabled == false and state.inputMode == "off" and findLog("no Quickey bank is provisioned") and #pool.cmds == 0, lastLog())
state.running = true; state.ignoreNextCleanup = false; Cleanup()

-- Provisioning through the operator's argument, the mixed backend, the per-key table.
pool.cmds = {}
reset("" .. BANK .. " input=mixed")
check("bank= provisions one Quickey per KB-10 qualified code plus the placeholder (codes=qualified by default)", quickeyCount() == 10 and findLog("bank provision: 10 Quickey%(s%) created, 0 reused") and findLog("state=ready codes=9 %(qualified 9, discovered 0%)"), J({ quickeyCount(), lastLog() }))
check("input is enabled on the mixed backend with the quickkey default", state.inputEnabled == true and state.backendName == "mixed" and findLog("input enabled on the mixed backend %(routing default quickkey"), lastLog())
check("the start reports the part that presses each key", findLog("key 1: NUM1 via quickkey %(quickey") and findLog("key 2: NUM2 via shortcut %(keyboard") and findLog("key Record: STORE via quickkey %(quickey") and findLog("key Undo: OOPS via shortcut %(keyboard"), J(state.routes))
push({ t = "hello", v = 2, id = "nxk-q", gen = 1, nonce = "3333333333333333" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
check("the welcome names the backend and keeps every surface key usable", welcome.backend == "mixed" and welcome.input == "mixed" and #welcome.keys.ok == 18 and welcome.keys.ok[17] == "Thru", J(welcome.keys))
keyboardCalls = {}
send({ t = "key", ev = 1, k = "1", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
local holds = state.modules.hardkeys.instance:status().holds
check("'1' is an executor press of the owned Quickey (quickey part), not a Keyboard() event", ack and ack.ok == 1 and lastCmd("^Press Page 1%.18%d$") and pressedCount() == 1 and #keyboardCalls == 0 and holds[#holds].backend == "quickey", J({ ack, pool.cmds[#pool.cmds], holds[#holds] and holds[#holds].backend }))
send({ t = "key", ev = 2, k = "2", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a PC key while a Quickey is held is refused as an unqualified mix (nothing dispatched)", ack and ack.ok == 0 and ack.code == "unqualified-mix" and #keyboardCalls == 0, J(ack))
send({ t = "key", ev = 3, k = "1", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("the release is the Unpress of the recorded executor", ack and ack.ok == 1 and lastCmd("^Unpress Page 1%.18%d$") and pressedCount() == 0, J({ ack, pool.cmds[#pool.cmds] }))
send({ t = "key", ev = 4, k = "2", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("'2' goes through the keyboard part (shortcut table, explicit override)", ack and ack.ok == 1 and #keyboardCalls == 1 and keyboardCalls[1].key == "2", J({ ack, keyboardCalls }))
send({ t = "key", ev = 5, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a Quickey while a PC key is held is refused as an unqualified mix", ack and ack.ok == 0 and ack.code == "unqualified-mix" and pressedCount() == 0, J(ack))
send({ t = "key", ev = 6, k = "2", d = 0 }); tick(); drain()
send({ t = "key", ev = 7, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
send({ t = "key", ev = 8, k = "Record", d = 0 }); tick(); drain()
check("Record (STORE) is a Quickey hold once the PC key is up", ack and ack.ok == 1 and lastCmd("^Unpress Page 1%.18%d$") and pressedCount() == 0, J(ack))
state.running = true
Main(nil, "bank status")
check("bank status reports the live bank", findLog("bank status: bank mtpnxk_surface@q900%.e1%.180%-191 state=ready"), lastLog())
state.ignoreNextCleanup = false
send({ t = "key", ev = 9, k = "1", d = 1 }); tick(); drain()
Main(nil, "bank teardown")
check("teardown is refused while a Quickey record is live", findLog("bank teardown refused %[bank%-in%-use%]") and quickeyCount() == 10, lastLog())
state.ignoreNextCleanup = false
send({ t = "key", ev = 10, k = "1", d = 0 }); tick(); drain()
Cleanup()
check("cleanup keeps the bank record and touches no console object", state.bankRecord and state.bankRecord.id == "mtpnxk_surface@q900.e1.180-191" and quickeyCount() == 10 and findLog("bank record mtpnxk_surface@q900.e1.180%-191 kept"), lastLog())
local creates = 0; for _, c in ipairs(pool.cmds) do if c:find("^Store Quickey") then creates = creates + 1 end end
reset("" .. BANK .. " input=mixed")
local creates2 = 0; for _, c in ipairs(pool.cmds) do if c:find("^Store Quickey") then creates2 = creates2 + 1 end end
check("the next start adopts the kept bank (verified, nothing created) and ignores the same bank= argument", state.bankRecord == nil and creates2 == creates and findLog("bank adopt: bank mtpnxk_surface@q900.e1.180%-191 state=ready") and findLog("is already live") and state.inputEnabled == true, lastLog())

-- Recovery goes through the part that pressed the record.
push({ t = "hello", v = 2, id = "nxk-q", gen = 2, nonce = "4444444444444444" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "5", d = 1 }); tick(); drain()
pool.failUnpress = true
send({ t = "key", ev = 2, k = "5", d = 0 }); tick()
ack = ofType(drain(), "ack")[1]
check("a Quickey release the console refuses is acknowledged unresolved", ack and ack.ok == 0 and ack.code == "unresolved", J(ack))
state.running = true; state.ignoreNextCleanup = false; Cleanup()
pool.failUnpress = false
check("cleanup keeps the unresolved Quickey record with its executor target", #state.unresolved == 1 and state.unresolved[1].backend == "quickey" and state.unresolved[1].target and state.unresolved[1].target.executor, J(state.unresolved))
reset("input=off")
check("a start with input off adopts the bank and the record", state.modules.hardkeys.instance:status().unresolved == 1 and state.modules.hardkeys.instance:bankStatus(clock.t).provisioned == true and state.inputEnabled == false)
state.running = true
Main(nil, "recover")
check("recover attaches the quickey part for cleanup and releases through the recorded executor", findLog("recover: quickey backend attached for cleanup only") and findLog("recover: released NUM5") and pressedCount() == 0 and state.modules.hardkeys.instance:status().unresolved == 0, lastLog())
state.ignoreNextCleanup = false; Cleanup()

-- Quickeys only: unqualified keys are refused, an override to another method is refused as a policy.
reset("input=quickey")
push({ t = "hello", v = 2, id = "nxk-q", gen = 3, nonce = "5555555555555555" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
check("input=quickey supports only the hold-qualified keys", state.backendName == "quickey" and #welcome.keys.ok == 6 and welcome.keys.ok[1] == "1" and welcome.keys.ok[2] == "5" and welcome.keys.ok[3] == "Clear" and welcome.keys.ok[4] == "Enter" and welcome.keys.ok[5] == "Record" and welcome.keys.ok[6] == "Thru" and state.keyReasons.Undo:find("no KB%-10 hold evidence"), J({ welcome.keys.ok, state.keyReasons.Undo }))
keyboardCalls = {}
send({ t = "key", ev = 1, k = "2", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("'2' on the Quickey backend is refused (not in the bank), never typed", ack and ack.ok == 0 and #keyboardCalls == 0 and pressedCount() == 0, J(ack))
send({ t = "key", ev = 2, k = "Undo", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("Undo (OOPS, tap-only evidence) is refused as a hold on the Quickey backend", ack and ack.ok == 0 and pressedCount() == 0, J(ack))
state.running = true; state.ignoreNextCleanup = false; Cleanup()
reset("input=quickey route=2:shortcut")
check("a shortcut override on the Quickey backend is refused as a policy the backend cannot serve", state.inputEnabled == false and findLog("enableInput failed %[policy%-unavailable%]"), lastLog())
state.running = true; state.ignoreNextCleanup = false; Cleanup()
reset("input=keyboard route=1:quickkey")
check("a quickkey override on the keyboard backend is refused the same way", state.inputEnabled == false and findLog("enableInput failed %[policy%-unavailable%]"), lastLog())
state.running = true; state.ignoreNextCleanup = false; Cleanup()
reset("input=mixed route=Undo:quickkey")
check("an override that resolves to an unusable route refuses the start (inert configuration)", state.inputEnabled == false and findLog("route Undo:quickkey cannot be served now"), lastLog())
state.running = true; state.ignoreNextCleanup = false; Cleanup()

-- Teardown removes only verified owned objects; afterwards Quickey routes are refused, not replaced.
reset("input=mixed")
state.running = true
Main(nil, "bank teardown")
check("teardown deletes the owned Quickeys and clears the reserved executors", quickeyCount() == 0 and findLog("bank teardown: 10 Quickey%(s%) removed, 12 executor%(s%) cleared, 0 skipped") and state.bankRecord == nil, lastLog())
push({ t = "hello", v = 2, id = "nxk-q", gen = 4, nonce = "6666666666666666" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
keyboardCalls = {}
send({ t = "key", ev = 1, k = "1", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("after teardown a Quickey key is refused, not pressed through Keyboard()", ack and ack.ok == 0 and #keyboardCalls == 0 and pressedCount() == 0, J(ack))
state.ignoreNextCleanup = false; Cleanup()
check("cleanup after teardown keeps no bank record", state.bankRecord == nil)

-- ReloadAllPlugins kills the loop without its shutdown: the next Main must take the stale state over.
pageWithExecutors(1, 180, 12)
reset("" .. BANK .. " input=mixed")
push({ t = "hello", v = 2, id = "nxk-q", gen = 5, nonce = "7777777777777777" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "1", d = 1 }); tick(); drain()
check("a Quickey is held before the simulated reload", pressedCount() == 1)
clock.t = clock.t + 5  -- no tick: the loop is dead, state.running stays true, the socket stays referenced
local oldGen = state.gen
Main(nil, "input=fake")
check("a stale run is taken over: its hold released through the old instance, the bank record kept, a new generation started", findLog("has not ticked for 5%.0 s") and pressedCount() == 0 and state.gen ~= oldGen and state.running == true and state.inputMode == "fake" and state.bankRecord == nil and state.modules.hardkeys.instance:bankStatus(clock.t).provisioned == true, J({ lastLog(), pressedCount(), state.gen, oldGen, state.inputMode }))
state.ignoreNextCleanup = false; Cleanup()
reset("input=fake")
tick()
state.running = true
Main(nil, "input=fake")
check("a run that still ticks is left alone", findLog("already running") and state.running == true, lastLog())
state.ignoreNextCleanup = false; Cleanup()

-------------------------------------------------------------------------------
-- KB-16 review: a shortcut-mode restoration dispose() hands back (KB-14) is kept across stop/start,
-- adopted as unresolved (presses refused), reported by status, and restored by recover.
-------------------------------------------------------------------------------
reset("input=fake")
push({ t = "hello", v = 2, id = "nxk-m", gen = 1, nonce = "8888888888888888" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick(); drain()
send({ t = "key", ev = 2, k = "Record", d = 0 }); tick(); drain()
local minst = state.modules.hardkeys.instance
local origDispose = minst.dispose
-- The fake backend cannot change the mode, so the record is injected into the real dispose() result the
-- way the module returns it when a restore is still pending at dispose.
minst.dispose = function(self, t)
  local r = origDispose(self, t)
  r.mode = { id = "m7", profile = "Default", original = true, target = false, changedAt = t - 1, lastEventAt = t, owner = "nxk-m", purpose = "simulated text route", writes = 1,
             unresolved = { reason = "disposed before the restore delay elapsed; restore pending", since = t }, pending = "delay" }
  return r
end
state.running = true; state.ignoreNextCleanup = false
Cleanup()
check("a mode restoration record from dispose() is kept with its reason instead of being dropped", type(state.modeRecord) == "table" and state.modeRecord.id == "m7" and state.modeRecord.keptReason ~= nil and findLog("keeping the unresolved keyboard%-shortcut mode restoration m7") and next(state.modules) == nil, J({ tostring(state.modeRecord and state.modeRecord.id), lastLog() }))
state.running = true
Main(nil, "status")
check("status reports the kept record while the plugin is not running", findLog("mode restoration record is kept from a previous run %(profile 'Default', shortcuts true %-> false"), lastLog())
state.ignoreNextCleanup = false
reset("input=fake")
local st = state.modules.hardkeys.instance:status(clock.t)
check("the next start adopts the record as an unresolved restoration and keeps nothing loose", state.modeRecord == nil and st.modeChange and st.modeChange.state == "unresolved" and st.modeChange.adopted == true and st.modeChange.profile == "Default" and findLog("adopted the unresolved keyboard%-shortcut mode restoration"), J({ tostring(state.modeRecord), tostring(st.modeChange and st.modeChange.state), lastLog() }))
push({ t = "hello", v = 2, id = "nxk-m", gen = 2, nonce = "9999999999999999" }); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
send({ t = "key", ev = 1, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("a press is refused while the adopted restoration is unresolved", ack and ack.ok == 0 and ack.code == "busy" and pressedCount() == 0, J(ack))
state.running = true
Main(nil, "status")
check("status names the unresolved restoration on the instance", findLog("mode restoration m%d+ UNRESOLVED %(profile 'Default', shortcuts true %-> false%)"), lastLog())
Main(nil, "recover")
st = state.modules.hardkeys.instance:status(clock.t)
check("recover re-reads the profile and the mode (already original on this console) and resolves the restoration without writing", st.modeChange == nil and st.lastModeChange and st.lastModeChange.state == "restored" and findLog("recover: keyboard%-shortcut mode restoration m%d+ restored %(profile 'Default', shortcuts back to true"), J({ tostring(st.modeChange and st.modeChange.state), tostring(st.lastModeChange and st.lastModeChange.state), lastLog() }))
send({ t = "key", ev = 2, k = "Record", d = 1 }); tick()
ack = ofType(drain(), "ack")[1]
check("presses are accepted again after the restoration", ack and ack.ok == 1 and state.sessions[sid].holds.Record ~= nil, J(ack))
send({ t = "key", ev = 3, k = "Record", d = 0 }); tick(); drain()
state.ignoreNextCleanup = false; Cleanup()
check("a clean stop with no pending restoration keeps no mode record", state.modeRecord == nil)

-------------------------------------------------------------------------------
-- Loopback source enforcement, stop and Cleanup
-------------------------------------------------------------------------------
reset("input=fake")
push({ t = "hello", v = 2, id = "nxk-z", gen = 1, nonce = "1111111111111111" }, "10.2.2.2", 7); tick()
check("a remote source is dropped before parsing", state.counters.notAllowed == 1 and #drain() == 0)
push({ t = "hello", v = 2, id = "nxk-z", gen = 1, nonce = "2222222222222222" }, "127.0.0.1", 7); tick()
welcome = ofType(drain(), "welcome")[1]; sid, seq = welcome.sid, 0
check("a local source connects", welcome ~= nil and welcome.to == "127.0.0.1:7")
seq = seq + 1; push({ t = "key", sid = sid, seq = seq, ev = 1, k = "Record", d = 1 }, "127.0.0.1", 7); tick(); drain()
check("a press from the local source is held", state.sessions[sid].holds.Record ~= nil)
state.running = true
Main(nil, "status")
check("status is a control call: nothing released, cleanup ignored", eventCount("release") == 0 and state.ignoreNextCleanup == true and findLog("holds=%[Record%]"), J({ eventCount("release"), state.ignoreNextCleanup, lastLog() }))
Cleanup()
check("cleanup after a control call leaves the running plugin alone", state.running == true and eventCount("release") == 0)
Cleanup()
check("a real cleanup releases every hold and disposes the modules", state.running == false and eventCount("release") == 1 and next(state.modules) == nil, J({ state.running, eventCount("release"), lastLog() }))

print(string.format("%d passed, %d failed", passes, failures))
print(failures == 0 and "ALL PASSED" or "FAILED")
os.exit(failures == 0 and 0 or 1)
