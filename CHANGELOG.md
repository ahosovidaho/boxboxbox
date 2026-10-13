# Changelog

## BOXBOXBOX 0.5.1-fork1 – 2026-10-13

- **Oprava: zkrácené 1. kolo závodu** (Nürburgring GP a další tratě, kde je start jinde než cíl):
  1. kolo začíná na roštu ~350 m za cílovou čarou, a bylo proto o ~5 s „rychlejší“. Kouč ho bral
  jako nejlepší kolo (všechna další kola +5 s, nesmyslná ztráta v Z1), živá delta ukazovala +5 s
  a barevné úseky se řezaly z neúplného kola. Kolo, které nekončí tam, kde začalo (> 80 m),
  už není referencí pro kouče, živou deltu, úseky ani průměrnou spotřebu na stránce 1.

## BOXBOXBOX 0.5.0-fork1 – 2026-10-13

- **Čeština**: celý dashboard (stránky 1–4, start, nastavení, stav spojení) i texty serveru
  (kouč, souhrny session, analýzy, konzole a log) jsou česky. Jazyk `cs` v nastavení
  a v `config.toml`; „Automaticky“ = jazyk prohlížeče (dashboard) a `LANG` (server).
  Docker má `LANG=cs_CZ.UTF-8`. Čeština píše desetinnou čárku v textech kouče, zatáčky
  značí Z1, Z2 …, kola LP/PP/LZ/PZ. Strategie zůstává jen česky.
- `tr!` má teď tři texty (DE, EN, CS) – chybějící překlad neprojde překladem.

## BOXBOXBOX 0.4.0-fork1 – 2026-10-13

- **Barevné úseky na mapě** (stránka 1): trať se po prvním celém kole rozdělí na 20 stejně
  dlouhých úseků. Úsek projetý v tomto kole: fialová = nejlepší čas v session, zelená = do 0,1 s
  od nejlepšího, červená = pomalejší. Úseky, které v tomto kole ještě nebyly, ukazují tlumeně
  výsledek minulého kola. Snapshot posílá `seg_now`, `seg_last`, `seg_delta`.

## BOXBOXBOX 0.3.0-fork1 – 2026-10-12

- **Plánovač strategie**: v „Závod…“ se zadá, kolik kol vydrží každá směs (a volitelně tempo),
  aplikace spočítá doporučenou a alternativní strategii (směsi, délky stintů, kola zastávek,
  kolik dotankovat). Během závodu se plán přepočítává a „BOX ZA N KOL“ se řídí jím.
- Ztráta zastávkou se měří pro každou trať (in-lap + out-lap − 2 × běžné kolo − tankování);
  do první zastávky na trati se počítá s 25 s nebo s ručně zadanou hodnotou.
- Při stání v boxu ukazuje první stránka „DOTANKUJ +X“ (přesně na zbytek závodu / další stint).
- Oprava: zastávka přes přelom kola (GT7 přičte kolo při stání v boxu) se počítala dvakrát.
- Průběh závodu se při novém závodu nepřepisuje, ukládá se do `strategy-history/`.

## BOXBOXBOX 0.2.0-fork1 – 2026-10-11

- **Strategie** (stránka 5, přejetím doprava): palivo do cíle, okno zastávky („Box v kole A–B“),
  kolik dotankovat při boxu na konci kola a doba tankování, povinná zastávka a povinná směs,
  opotřebení gum z ručně zadaných procent ze hry (kalibrace auto + trať + směs + multiplikátor).
- Výběr závodu z kalendáře šampionátu (Firestore tamayo.cz, jen čtení) nebo ruční zadání.
- Automatická detekce tankování v boxech a spotřeby po kolech (bez 1. kola z roštu a kol s tankováním).
- Na stránce 1 ve spodní liště nejdůležitější pokyn (BOX / varování).
- Stav strategie v `strategy.json` v datové složce (vydrží restart).

## BOXBOXBOX 0.1.0-fork1 – 2026-10-11

- Fork Apexline pod názvem BOXBOXBOX (program `boxboxbox`, titulek dashboardu). Datová složka
  nativní instalace zůstává `apexline`, aby byly vidět dřívější záznamy.
- Docker: `Dockerfile`, `docker-compose.yml`, `.env.example` (PS5 IP povinná, data v `telemetry-data/`).
- CSV kol nově ukládá `fuel_capacity` (poslední sloupec; starší soubory ho nemají).
- `tools/stint_analyza.py`: oficiální čas kola z `last_lap_ms`, rozjeté kolo se nepočítá,
  průměrná spotřeba a dojezd na plnou nádrž.

## 0.1.0 – 2026-10-08

First public release.

- **Live dashboard** for the phone (built for iPhone, works in any browser): tyre temperatures, gear,
  speed, rev lights, pedals, track map, understeer/oversteer balance, lap times with live delta, fuel;
  a page with all telemetry channels; burn-in protection for OLED screens.
- **Finds the PS5 by itself** on the local network (broadcast, remembers the address); a fixed IP can be
  set on the dashboard, in `config.toml` or with `--ps5`. A hint shows when no data arrives.
- **Recording** of every lap (CSV, 60 samples/s) grouped into sessions (one car on one track).
- **Coach** (rule based, no AI): corner-by-corner comparison with your best lap, trail-braking rating,
  a focus for the next laps and a session summary; `apexline analyze` for the console.
- **Measurements** detected automatically: 0–100 km/h / 0–60 mph and more, rolling intervals, 1/4 mile,
  braking distances, best shift points per gear and the power band – no car weight needed;
  `apexline perf` finds runs in older recordings.
- **German and English**, metric and imperial units.
- One program for Linux, Raspberry Pi, Windows and macOS; settings in `config.toml`, data in the
  OS data folder (`apexline config` shows where).

### Known limitations

- No track recognition yet: corners are numbered per session (T1, T2 …), no corner names.
- The live delta compares with the best lap of the current session only.
- Raw lap data is kept forever (about 4 MB per lap).
- Coach thresholds are tuned on a few cars.
- Shift points need full-throttle runs through several gears and vary between runs.
- The Windows, macOS and Raspberry Pi programs are new and so far only tested by CI – please report
  any problems.
