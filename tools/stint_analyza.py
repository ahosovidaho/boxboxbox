#!/usr/bin/env python3
"""
stint_analyza.py – souhrn jedné session (jedno CSV = jedno kolo).

Použití:
  python3 tools/stint_analyza.py                     # nejnovější session
  python3 tools/stint_analyza.py "/cesta/k/session"  # konkrétní session
  BOXBOX_DATA=./telemetry-data python3 tools/stint_analyza.py   # data z Dockeru

Oficiální čas kola se bere z `last_lap_ms` následujícího kola (shoduje se se hrou).
Kolo bez následujícího souboru je rozjeté a do průměrů se nepočítá.

Jen čte data. Jediné, co zapíše, je stint_souhrn.csv do složky session.
"""
import csv
import glob
import os
import sys
from statistics import mean

DEFAULT_DIRS = [
    os.environ.get("BOXBOX_DATA", ""),
    "./telemetry-data",
    "~/Library/Application Support/apexline",
    "~/.local/share/apexline",
]
WHEELS = ["fl", "fr", "rl", "rr"]


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def recordings_dir():
    for d in DEFAULT_DIRS:
        p = os.path.join(os.path.expanduser(d), "recordings") if d else ""
        if p and os.path.isdir(p):
            return p
    sys.exit("FAIL: nenašel jsem složku recordings (nastav BOXBOX_DATA nebo zadej cestu k session)")


def pick_session():
    if len(sys.argv) > 1:
        return sys.argv[1]
    rec = recordings_dir()
    sessions = [d for d in glob.glob(os.path.join(rec, "*")) if os.path.isdir(d)]
    if not sessions:
        sys.exit(f"FAIL: v {rec} není žádná session")
    return max(sessions, key=os.path.getmtime)


def load(path):
    cols = {}
    with open(path, newline="") as f:
        for row in csv.DictReader(f):
            for k, v in row.items():
                x = num(v)
                if x is not None:
                    cols.setdefault(k, []).append(x)
    return cols


def summarize(cols, name):
    if not cols.get("lap_ms"):
        return None
    r = {"soubor": name, "kolo": int(cols["lap"][0]) if cols.get("lap") else None, "vzorku": len(cols["lap_ms"])}
    fuel = cols.get("fuel", [])
    if fuel:
        r["palivo_start"] = round(fuel[0], 2)
        r["palivo_konec"] = round(fuel[-1], 2)
        rises = [b - a for a, b in zip(fuel, fuel[1:])]
        r["tankovano"] = round(max([0.0] + rises), 2)
        # spotřeba bez tankování: součet poklesů
        r["spotreba"] = round(sum(-d for d in rises if d < 0), 3)
    cap = cols.get("fuel_capacity", [])
    if cap:
        r["nadrz"] = round(max(cap), 1)
    for w in WHEELS:
        t = cols.get(f"tire_temp_{w}", [])
        if t:
            r[f"t_{w}_avg"] = round(mean(t), 1)
            r[f"t_{w}_max"] = round(max(t), 1)
    r["_first_last_lap_ms"] = cols.get("last_lap_ms", [0])[0]
    r["_max_lap_ms"] = max(cols["lap_ms"])
    return r


def main():
    session = pick_session()
    files = sorted(f for f in glob.glob(os.path.join(session, "*.csv")) if os.path.basename(f) != "stint_souhrn.csv")
    if not files:
        sys.exit(f"FAIL: v {session} nejsou CSV kola")

    laps = [r for r in (summarize(load(f), os.path.basename(f)) for f in files) if r]
    for i, r in enumerate(laps):
        nxt = laps[i + 1] if i + 1 < len(laps) else None
        official = nxt["_first_last_lap_ms"] if nxt else 0
        r["dokonceno"] = bool(official and official > 0)
        r["cas_s"] = round((official if r["dokonceno"] else r["_max_lap_ms"]) / 1000, 3)
    for r in laps:
        r.pop("_first_last_lap_ms")
        r.pop("_max_lap_ms")

    print(f"Session: {session}")
    print(f"Kol: {len(laps)} (dokončených {sum(r['dokonceno'] for r in laps)})\n")
    hdr = f"{'kolo':>4} {'čas':>9} {'spotř.':>7} {'tank':>5} " + " ".join(f"{'T' + w.upper():>6}" for w in WHEELS)
    print(hdr)
    print("-" * len(hdr))
    for r in laps:
        temps = " ".join(f"{r.get(f't_{w}_avg', 0):6.1f}" for w in WHEELS)
        mark = "" if r["dokonceno"] else "  (rozjeté)"
        print(f"{r['kolo'] or 0:>4} {r['cas_s']:>9.3f} {r.get('spotreba', 0):>7.2f} {r.get('tankovano', 0):>5.1f} {temps}{mark}")

    done = [r for r in laps if r["dokonceno"] and not r.get("tankovano")]
    if len(done) >= 2:
        # první kolo závodu bývá kratší (rošt za cílovou čarou), proto průměr od 2. dokončeného kola
        base = done[1:] if len(done) >= 3 else done
        avg_fuel = mean(r["spotreba"] for r in base)
        avg_time = mean(r["cas_s"] for r in base)
        cap = next((r["nadrz"] for r in laps if r.get("nadrz")), None)
        print(f"\nPrůměr (bez 1. kola a kol s tankováním): {avg_time:.3f} s, spotřeba {avg_fuel:.2f} / kolo")
        if cap and avg_fuel > 0:
            print(f"Plná nádrž ({cap:g}) vystačí na {cap / avg_fuel:.1f} kola")

    out = os.path.join(session, "stint_souhrn.csv")
    fields = list(dict.fromkeys(k for r in laps for k in r))
    with open(out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(laps)
    print(f"\nPASS: souhrn uložen do {out}")


if __name__ == "__main__":
    main()
