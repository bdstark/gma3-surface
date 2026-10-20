#!/usr/bin/env python3
"""KB-30 probe: which command line switches the encoder bar's feature group (encoder bank) on onPC, read back.

Talks to the gma3-mcp bridge (127.0.0.1:9800, one JSON document per line). For each candidate it puts the bar
back on a known bank (Select EncoderBank 1.1), runs the candidate through the bridge's `cmd` op, then reads the
encoder bar through `feedback.context` (the vendored feedback module's contextSnapshot, the same reader the
surface plugin builds the service's context from) twice: on the next request and after a short pause.

Run it only on a disposable show: it changes the encoder bank and, with --select, the fixture selection.

  python3 -I tools/probes/kb30_encoder_bank_probe.py [--select "Fixture 101"] [--out probe.json]
"""
import argparse, json, socket, sys, time

class Bridge:
    def __init__(self, host="127.0.0.1", port=9800):
        self.s = socket.create_connection((host, port), timeout=10)
        self.f = self.s.makefile("rwb")
        self.n = 0

    def op(self, name, args=None):
        self.n += 1
        self.f.write((json.dumps({"id": self.n, "op": name, "args": args or {}}) + "\n").encode())
        self.f.flush()
        while True:
            r = json.loads(self.f.readline())
            if r.get("id") == self.n:
                if not r.get("ok"):
                    return {"error": r.get("error")}
                return r["result"]

    def cmd(self, c):
        return self.op("cmd", {"command": c}).get("feedback")

def bar(b):
    snap = b.op("feedback.context", {"display": 1})
    if "error" in snap:
        return {"error": snap["error"]}
    out = {"generation": snap.get("generation")}
    enc, sl = snap.get("encoder") or {}, snap.get("slots") or {}
    v = enc.get("value") or {}
    out["available"], out["reason"] = enc.get("available"), enc.get("reason") or enc.get("error")
    out["bank"], out["page"], out["context"] = v.get("bank"), v.get("page"), v.get("context")
    out["banks"], out["poolUnavailable"] = v.get("banks"), v.get("poolUnavailable")
    sv = sl.get("value") or {}
    out["slotsAvailable"], out["slotsReason"] = sl.get("available"), sl.get("reason") or sl.get("error")
    out["slots"] = [x.get("name") for x in (sv.get("slots") or [])]
    out["selection"] = (sv.get("selection") or {}).get("count")
    if "bank" not in out:
        out["raw"] = snap
    return out

def brief(r):
    if "error" in r:
        return "error: " + str(r["error"])
    bk, pg = r.get("bank") or {}, r.get("page") or {}
    return "bank %s %s page %s %s ctx %s gen %s slots %s sel %s%s" % (
        bk.get("index"), bk.get("name"), pg.get("index"), pg.get("name"), r.get("context"), r.get("generation"),
        ",".join(str(s) for s in r.get("slots") or []), r.get("selection"),
        ((" UNAVAILABLE: " + str(r.get("reason"))) if r.get("available") is False else "") + ((" slots unavailable: " + str(r.get("slotsReason"))) if r.get("slotsAvailable") is False else "") + ((" pool: " + str(r.get("poolUnavailable"))) if r.get("poolUnavailable") else ""))

SIDE = ("local f = SelectedFeature(); local fg = f and f:Parent(); "
        "local n = 0; for _ in ipairs(ObjectList('Programmer') or {}) do n = n + 1 end; "
        "return { feature = f and f.name or nil, featureGroup = fg and fg.name or nil, selection = SelectionCount() }")

def side(b):
    r = b.op("lua", {"code": SIDE})
    if "error" in r:
        return {"error": r["error"]}
    return (r.get("values") or [None])[0]

def key(r):
    return ((r.get("bank") or {}).get("index"), (r.get("page") or {}).get("index"))

def settle(b, want=None, since=None, limit=1.0, step=0.02):
    """Read until the bank/page is `want` (or differs from `since`), at most `limit` s. (reads, ms, last)."""
    t0, n = time.time(), 0
    while True:
        r = bar(b); n += 1
        k = key(r)
        if (want is not None and k == want) or (since is not None and k != since):
            return n, round((time.time() - t0) * 1000), r
        if time.time() - t0 > limit:
            return n, None, r
        time.sleep(step)

CANDIDATES = [
    # documented: EncoderBank keyword (grandMA3 2.5 help, keyword_encoderbank)
    "Select EncoderBank 2",
    "Select EncoderBank \"Color\"",
    "Select EncoderBank 3.2",
    "Select EncoderBank \"Color\".\"Color\"",
    "EncoderBank 5",
    # the brief's candidates
    "PresetType 2",
    "PresetType \"Gobo\"",
    "Select PresetType 4",
    "FeatureGroup 6",
    "Select FeatureGroup \"Beam\"",
    "Feature 2.1",
    # edges: an empty bank (no pages), a bank and a page that do not exist, a name that does not exist
    "Select EncoderBank 8",
    "Select EncoderBank 11",
    "Select EncoderBank 2.5",
    "Select EncoderBank \"Nope\"",
]

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--select", help="a selection command run first (e.g. 'Fixture 101')")
    ap.add_argument("--pause", type=float, default=0.3)
    ap.add_argument("--out")
    a = ap.parse_args()
    b = Bridge()
    ping = b.op("ping")
    rec = {"show": ping.get("showfile"), "build": ping.get("build"), "bridge": ping.get("bridgeVersion"),
           "feedback": (ping.get("modules") or {}).get("feedback"), "runs": []}
    print("show %s build %s bridge %s feedback %s" % (rec["show"], rec["build"], rec["bridge"], rec["feedback"]))
    if rec["show"] == "Shehds":
        sys.exit("refusing: Shehds is the live show")
    if a.select:
        print("select: %s -> %s" % (a.select, b.cmd(a.select)))
    rec["before"] = bar(b)
    print("before: " + brief(rec["before"]))
    for c in CANDIDATES:
        reset = b.cmd("Select EncoderBank 1.1")
        _, _, base = settle(b, want=(1, 1))
        sb = side(b)
        fb = b.cmd(c)
        n, ms, r = settle(b, since=(1, 1))
        time.sleep(a.pause)
        r2 = bar(b)
        sa = side(b)
        run = {"command": c, "reset": reset, "base": base, "feedback": fb, "reads": n, "changedMs": ms, "first": r,
               "after": r2, "sideBefore": sb, "sideAfter": sa}
        rec["runs"].append(run)
        print("%-36s %-16s changed after %s ms (%d reads): %s\n%38s side %s -> %s" % (
            c, fb, ms, n, brief(r2), "", sb, sa))
    print("restore: Select EncoderBank 1.1 -> %s" % b.cmd("Select EncoderBank 1.1"))
    rec["end"] = bar(b)
    print("end: " + brief(rec["end"]))
    if a.out:
        with open(a.out, "w") as f:
            json.dump(rec, f, indent=1)

if __name__ == "__main__":
    main()
