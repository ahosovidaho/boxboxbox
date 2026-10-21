# Changelog

## BOXBOXBOX 0.8.0-fork1 – rozpracováno (větev phase6-kouc)

- **Kontrola dalších míst po chybě s podřazením** (motokára 20. 10., F3500 19. 10.):
  - Kouč: „ujíždějící záď“ a „nedotáčivost“ v hodnocení kola jen když je výrazně víc než
    v nejlepším kole ve stejné zatáčce (motokára: dřív 116× za 32 kol, teď 10×).
  - Motokára (poloměr kol pod 0,2 m): trail braking nehodnotí nedotáčivost a záď, živá
    šipka na mapě se nebarví podle vyvážení.
  - Zatáčky session se znovu určí z nejlepšího kola, když byly vzaté z kola se spinem nebo
    výletem (20. 10.: zatáčka „vrchol 0 km/h“, ztráta −12 s v Z4); teoretické kolo jen
    z normálních kol (bylo 0:34,8 místo 0:53,1).
  - Telemetrie: po restartu v GT7 se čísla kol opakují – v seznamu „Kolo 2 · jízda 3“.

- **Řazení dolů v Telemetrii**: pohled Řazení ukazuje i podřazení (↓5→4 se otáčkami, na které
  motor po podřazení vyskočil, a místem v m), dřív jen řazení nahoru – před zatáčkou tak
  vypadalo, že se neřadí. Žluté optimum jen na plném plynu bez brzdy (ne při meziplynu).

- **Nastavení auta u motokáry**: bez čísel nedotáčivosti/přetáčivosti (jen blokování a prokluz),
  bez „Auto je vyvážené“. Férové srovnání u motokáry = průměr čistých kol (gumy ani palivo se
  nemění, nic se nepřepočítává). Setup, který je na své směsi sám, má „jediný setup na SS – není
  s čím srovnat“ místo „nejrychlejší“. Záporné opotřebení (kola rychlejší se stářím gum = jezdec
  se rozjíždí) se už nepoužívá.

- **Řazení v Telemetrii** (čtvrtý pohled „Řazení“): otáčky obou kol se žlutým optimem pro
  zařazený stupeň a značkami řazení (bílé = toto kolo, fialové = nejlepší). Celé kolo: pro každé
  řazení nahoru na plný plyn medián otáček z normálních kol proti optimu („2→3: řadíš při
  14 440, optimum 14 790 – řaď o ~350 ot. později“, „motor stojí na omezovači“, „řadíš
  nepravidelně“) a kolik podřazení skončilo na omezovači. U zatáčky řazení tohoto a nejlepšího
  kola. Optimum = body řazení z jízdy pro auto a setup session, jinak kontrolka hry.
- **Závody bez opotřebení a paliva** (motokáry, sníh, šotolina): v „Závod…“ zaškrtávátka „Gumy se
  neopotřebovávají“ a „Palivo se nespotřebovává“ (u motokáry předvyplněné). Strategie pak plánuje
  jen podle pravidel (povinná zastávka a směs), na závodní stránce místo paliva „závod bez
  spotřeby paliva“; příprava na závod nechce opotřebení, výdrž, spotřebu ani dlouhý stint
  (u motokáry automaticky).

- **Opravy po tréninku 19. 10.** (F3500 Yas Marina, motokára Willow Springs):
  - Ujetí zádi a riziko zatáčky se počítají proti nejlepšímu kolu ve stejné zatáčce (o 6° víc
    než ono) – motokára klouže v každém kole a dřív měla „ujetí zádi“ i v nejrychlejších kolech.
  - Příčina „prokluz zadních kol“ jen při skutečném prokluzu (zadní kola o 15 % rychleji aspoň
    0,2 s); krátké špičky do ~1,1 má i nejlepší kolo.
  - Motokára: nedotáčivost/přetáčivost se nevyhodnocuje (výpočet počítá s rozvorem auta), místo
    toho poznámka. Příprava na závod: u auta bez spotřeby paliva se palivo nevyžaduje.
  - Doporučení setupu se ukážou i pro auto, které se teď nejede na aktuální trati (poslední trať).
  - Záznam nezapisuje stání déle než minutu (19. 10.: soubor 92 MB ze stání v boxu).
  - Převzato z neuložených úprav v pracovní kopii (19. 10. 14:00): převod řízení se pro auto
    naučí jednou a uloží (`perf/<auto>/steer.json`), nová sada gum po přestávce 15+ minut,
    příčina „plyn s natočeným volantem“ hledaná v poslední sekundě.

- **Příprava na závod** (Strategie → Trénink, nahoře): pro vybraný závod co je hotové (✓),
  částečně (!) a co chybí (○) – nastavení závodu, čistá kola na trati se stejnými násobiči,
  každá směs (kola, ztráta za kolo, výdrž), spotřeba, ztráta zastávkou, dlouhý stint, karta
  setupu, body řazení a vyrovnanost (cíl 10 kol v rozmezí 0,3 s) – a co udělat dál. API
  `/api/prep`.

- **Palivo živě v závodě** (závodní stránka): „TOTO KOLO 3,1/3,2“ = palivo spotřebované v tomto
  kole proti tomu, co povoluje cíl v tomto místě kola (zeleně pod, červeně nad). Když šetření
  ušetří zastávku (jedna zastávka a není povinná → žádná; dvě → jedna, do ~12 % úspory),
  objeví se žlutý štítek „ŠETŘI −0,21 → BEZ ZASTÁVKY“ a cílová spotřeba „CÍL“; v pruhu rad
  „Bez zastávky při spotřebě … / kolo“.

- **Férové srovnání setupů** (stránka Nastavení auta): čistá tréninková kola se proloží
  modelem čas = základ setupu + opotřebení × stáří gum + vliv paliva × (palivo − referenční);
  „Férově“ = čas setupu na nových gumách se stejným palivem ± nejistota a verdikt („o 0,3 s
  pomalejší“, nebo „zatím nerozhodnuto“, dokud rozdíl není větší než dvojnásobek nejistoty).
  Kola s chybou (o 2 s / 2 % pomalejší než typické) se vynechají. Opotřebení a vliv paliva se
  učí z dat, při málo kolech drží typické hodnoty (0,2 s/kolo, 0,03 s/l). Tréninková kola nově
  ukládají palivo na začátku kola.
- **Setupy po zatáčkách**: záznam si u každého kola pamatuje aktivní setup (`session.json`,
  `lap_setups`); když se v jedné session jezdilo s víc setupy, tabulka ukáže, ve kterých
  zatáčkách který setup získává.

- **Vyrovnanost** (stránka Telemetrie, třetí pohled): rozptyl ztráty v každé zatáčce mezi
  normálními koly (do 107 % nejlepšího) – zeleně do ±0,12 s, oranžově do ±0,25 s, jinak červeně,
  i na mapě. Celé kolo: počet normálních kol, průměr ± rozptyl, teoretické kolo a tři
  nejrozkolísanější zatáčky. U zatáčky ztráta v každém kole zvlášť.

- **Mapa rizika a rozbor spinů** (stránka Telemetrie): přepínač Ztráta / Riziko. Riziko u každé
  zatáčky = spiny, ujetí zádi a kolik kol mělo záď přes 8° (červeně / oranžově i na mapě).
  Spin (smyk karoserie přes 30° nebo zastavení na trati) a ujetí zádi (záď přes 12° aspoň 0,25 s)
  se hledají v každém kole; z 1,5 s před nimi se určí pravděpodobná příčina (podřazení na
  brzdách, brzdění v zatáčce, prudké ubrání nebo přidání plynu, plyn s natočeným volantem,
  rychlejší vjezd) a tip. Seznam vlevo – ťuknutím se otevře to kolo a zatáčka s vyznačeným
  místem v grafech. Kouč je píše i do hodnocení kola.

- **Telemetrie po zatáčkách** (Trénink, nová stránka za Koučem): libovolné kolo session proti
  nejlepšímu na stejné vzdálenosti – rozdíl času, rychlost, plyn nahoru / brzda dolů, volant
  a stupeň. Celé kolo nebo jedna zatáčka s brzdnou zónou (výchozí: zatáčka s největší ztrátou),
  ťuknutím do grafu hodnoty obou kol v tom místě a bod na mapě. Nahoře souhrn zatáčky: brzdný
  bod, vjezd, vrchol, plný plyn, výjezd a stupeň. Plyn a brzda každý ve vlastním grafu (0 % dole). API `/api/traces?session=&lap=`.

- **Kouč – cíl bez spinů**: „Cíl: Zx“ a souhrn session počítají průměrnou ztrátu jen z normálních
  kol (do 107 % nejlepšího). Kolo se spinem, výletem mimo trať nebo zastávkou dřív přetáhlo
  průměr (F3500 Yas Marina: Z14 „10,39 s“ kvůli spinu se ztrátou 41 s).

- **Doporučení nastavení** (stránka Nastavení auta): z posledních 10 celých kol aktivního setupu
  na trati (min. 3) se počítá vyvážení ve třech fázích zatáčky (brzdy při vjezdu, střed, plyn
  při výjezdu – nedotáčivost/přetáčivost nad ±2°), blokování kol, prokluz a omezovač v nejvyšším
  stupni (`handling.json`). Z jasných nálezů je jedna změna v nastavení – s aktuální hodnotou,
  když je zadaná („přední stabilizátor 6 → 5“) – a tip pro jízdu.
- **Porovnání setupů**: čistá tréninková kola na trati podle karty setupu – počet, nejlepší
  a medián; nejrychlejší medián (aspoň 3 kola) zeleně.
- **BoP**: karta setupu může být označená jako BoP (nastavení v závodě zamčené) – bez polí
  nastavení, doporučení jen pro jízdu.

- **Trénink na vybraný závod**: na stránce Strategie je tlačítko „Závod…“ i v Tréninku (výběr
  z kalendáře šampionátu) a pod tratí tlačítko „Trénovat na závod: <trať> · opotř. ×… · spotř. ×…“,
  které přenese trať a násobiče závodu do tréninku. Nabídka tratí obsahuje i trať závodu.
  Když v Tréninku vybereš a uložíš jiný závod, trať a násobiče se přenesou samy.

- **Karty nastavení auta** (Trénink, nová stránka „Nastavení auta“ před stránkou dat): hodnoty
  z menu nastavení GT7 – pneumatiky, podvozek (světlá výška, frekvence, stabilizátory, tlumiče,
  odklon, sbíhavost), přítlak, diferenciál, brzdy a asistenty, převodovka, výkon a zátěž –
  předek/zadek vedle sebe, plus poznámka. Více karet na auto (Nový / Duplikovat / Uložit /
  Smazat), jedna je aktivní (`setups.json`, API `/api/setups`).
- Tréninková kola a body řazení z jízdy se ukládají **pro aktivní setup**, takže se nemíchají
  data s jinou převodovkou nebo přítlakem (`track_s2.json`). Starší kola patří pod „bez setupu“.
  Tréninková karta ukazuje, se kterým setupem jedeš; strategie závodu bere data aktivního setupu.

- **Body řazení z jízdy**: každý úsek na plný plyn v rovině (bez brzdy, malá rychlost stáčení)
  se ukládá pro auto (`perf/<auto>/track.json`, max. ~11 min plného plynu) a z nich se počítá
  výkonová křivka a body řazení pro všechny použité stupně (`track_dyno.json`, přepočet každých
  ~25 s plného plynu). Stoupání a klesání se započítá (gravitace po sklonu), prokluz se vyřadí.
- Otáčkoměr bere bod řazení z jízdy, pak z měření z místa, pak kontrolku hry; „pozdě“ se bere
  jen ze zdroje, který došel k omezovači. Pod LED: „ŘADIT 7 440 · Z JÍZDY / Z MĚŘENÍ / HRA“.
- Starší nahrávky jde naimportovat: `boxboxbox perf --save <session>` (každá session jen jednou).

- **Živý kouč v Tréninku** (stránka 1): dole na mapě rada pro příští zatáčku podle toho, jak
  minulé vyhodnocené kolo dopadlo proti nejlepšímu: „brzdi o 12 m dřív / později“, „víc
  rychlosti do vrcholu“, „dřív na plyn“ nebo „✓ drž to“, a ztráta v sekundách. Rámeček zežloutne
  250 m před brzdným bodem. Na mapě jsou kroužky, kde nejlepší kolo začíná brzdit.
- Kouč posílá dashboardu pro každou zatáčku brzdný bod nejlepšího kola a rozdíly posledního
  kola (čas, brzdění, rychlost ve vrcholu, plný plyn).

## BOXBOXBOX 0.7.3-fork1 – 2026-10-16 (opravy po závodě na Nürburgringu)

- **Tankování se počítalo jen asi ze 70 %**: GT7 přidává palivo po krocích ~0,05 a část kroků je
  těsně pod 0,05, které se nepočítaly (závod 15. 10.: skutečně +42,4, zapsáno +29,4). Ve stání
  se teď počítá každé zvýšení. Tím byla zkreslená i změřená ztráta zastávkou.
- **Otáčkoměr**: 15 LED se plní rovnoměrně až k bodu řazení (5 zelených, 5 červených, 5 modrých),
  v bodě řazení všechny modře blikají (blikání řízené časem, ne CSS animací, která se při
  přepínání restartovala a vypadala jako trvalé světlo). Příznak „omezovač“ z GT7 se už
  nepoužívá: zapíná se už na začátku rozsahu řadicí kontrolky (u Lanceru 7 000 ot/min, omezovač
  je kolem 8 000).
- **Bod řazení z měření**: použije se jízda, která došla nejvýš v otáčkách; „pozdě“ z jízdy,
  která omezovač nikdy nedosáhla, se nebere (kontrolka hry).
- Směsi s českými názvy (SS = sportovní měkké, SM = střední, SH = tvrdé) v Tréninku, ve Strategii
  i v nastavení závodu.
- Bez nastavené startovní směsi se v závodě bere směs naposledy označená v Tréninku na stejné trati.

## BOXBOXBOX 0.7.2-fork1 – 2026-10-14

- **Řazení podle naměřeného optima**: modré LED závodního dashboardu se rozsvítí a blikají
  v otáčkách nejlepšího přeřazení pro aktuální stupeň, spočítaných z výkonové křivky
  (stránka Měření, totéž auto). Když je optimum „pozdě“, svítí 100 ot/min před nejvyššími
  naměřenými otáčkami. Bez měření a na nejvyšší stupeň platí kontrolka hry. Pod LED je
  „ŘADIT 7 190 · OPTIMUM“ nebo „· HRA“.

## BOXBOXBOX 0.7.1-fork1 – 2026-10-13

- **Závodní dashboard „Rosso“** podle vybraného návrhu: 15 LED otáčkoměru (5 zelených,
  5 červených, 5 modrých, u omezovače bliká modře), otáčky a bod řazení; vlevo teploty gum se
  směsí a stářím sady a tempo (kolo, živá delta, poslední, nejlepší); uprostřed pokyn k boxu
  (žlutý příští kolo, červený toto kolo) a velký stupeň s rychlostí, doporučeným stupněm
  a pruhy plynu a brzdy; vpravo mapa s barevnými úseky; dole palivo jako 30 segmentů
  (bílá = potřeba do cíle, zelená = rezerva, červená = nevystačí, červená čára = potřeba do cíle).
  Palivo: 20 čtverců přes celou šířku jako LED otáčkoměru, údaje v řádku nad nimi.
  Písma Saira Condensed a JetBrains Mono z Google Fonts (bez internetu systémová).

## BOXBOXBOX 0.7.0-fork1 – 2026-10-13

- **Dva módy aplikace: ZÁVOD a TRÉNINK** (přepínač u teček stránek a na stránce Strategie; mód
  je uložený na serveru, takže iPhone i Mac ukazují totéž).
  - Trénink: jízdní pohled, kouč, měření, strategie (trénink), všechna data.
  - Závod: závodní dashboard, rekapitulace závodu, strategie a nastavení závodu.
- **Závodní dashboard**: velký otáčkoměr s číslem, rychlostní stupeň, rychlost, teploty gum;
  tempo (kolo X/Y, živá delta, poslední, průměr 3 kol, nejlepší, trend, pozice); palivo
  (množství, rezerva v kolech, značka potřeby do cíle, vystačí na, spotřeba) a velký pokyn
  BOX ZA N KOL / DOTANKUJ +X se směsí a kolem zastávky.
- **Rekapitulace závodu**: celkový čas, nejlepší a průměrné kolo, vyrovnanost, graf časů kol
  (nejlepší / rychlejší / pomalejší / zastávka), stinty se ztrátou za kolo, zastávky
  s dotankováním a ztrátou času, splnění pravidel, spotřeba, největší ztráty a silná místa
  podle kouče a jeho souhrn.

## BOXBOXBOX 0.6.0-fork1 – 2026-10-13

- **Mód Trénink** (stránka Strategie, přepínač Závod / Trénink): označíš směs, aplikace ukládá
  čistá kola (celé kolo od čáry, bez boxu, bez out-lapu, bez chyby) zvlášť pro auto, trať,
  směs a násobiče opotřebení a spotřeby. Trať se pozná podle tvaru mapy (napoprvé ji napíšeš).
  Výměnu gum v boxu pozná sama. Název trati = trať a varianta („Nürburgring GP“); kolo, které
  tvarem neodpovídá uložené variantě, se nepočítá a aplikace upozorní – varianty se nemíchají. Tlačítko „Počítat kola“ vypne ukládání pro jízdu automatem,
  jednotlivá kola jde vyřadit. Data: `practice.json` vedle `strategy.json`.
- **Výsledky tréninku**: pro každou směs nejlepší čas, ztráta času na každé kolo stáří gum
  (přímka přes kola na sadě, od 4 kol a rozpětí 3 kol), tempo proti nejrychlejší směsi,
  spotřeba, nejdelší run. „Vydrží kol“ zadáváš ručně podle opotřebení ve hře.
- **Závod z tréninku**: pro závod na stejné trati se stejným autem a násobiči se doplní
  životnost, tempo a ztráta stářím každé směsi a spotřeba (co zadáš v nastavení závodu, má
  přednost). Plánovač počítá s degradací změřenou pro každou směs. V nastavení závodu jsou
  hodnoty z tréninku vidět jako šedá čísla, v plánu je řádek „Z tréninku …“.
- Tabulka gum v nastavení závodu má nový sloupec „Ztráta s/kolo stáří“.

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
