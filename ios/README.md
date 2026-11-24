# BOXBOXBOX pro iOS

Nativní aplikace pro iPhone (a iPad): **závod na jedné obrazovce** – hláška inženýra, potvrzení
směsi po zastávce, palivo, zastávka, gumy, časy, plán. Všechno počítá server BOXBOXBOX na Mac mini;
aplikace je jeho displej a ovladač (`docs/API.md`).

- `BoxCore/` – Swift balíček: data ze serveru, připojení (`/ws`), API, ukázková data ze závodu
  Spa 23. 11. 2026 a testy.
- `App/` – SwiftUI aplikace (obrazovka Závod, Nastavení, hlas a haptika).
- `project.yml` – popis projektu pro XcodeGen (z něj vznikne `Boxboxbox.xcodeproj`).

## Co je potřeba

- Mac s **Xcode** (App Store) – Mac mini M4 stačí.
- iPhone s **iOS 17** nebo novějším, kabel.
- Apple ID. Zdarma („Personal Team“) běží aplikace 7 dní, pak ji z Xcode nahraješ znovu. Placený
  Apple Developer Program to omezení nemá a umožní TestFlight.
- **XcodeGen**: `brew install xcodegen`

## Postup (poprvé)

```
cd ~/src/boxboxbox && git pull
cd ios && xcodegen
open Boxboxbox.xcodeproj
```

1. V Xcode vlevo klikni na projekt **Boxboxbox** → target **Boxboxbox** → **Signing & Capabilities**
   → **Team**: tvoje Apple ID (Personal Team). Když Xcode hlásí obsazený Bundle Identifier, změň
   `cz.tamayo.boxboxbox.viktor` na jiný (např. `cz.tamayo.boxboxbox.viktor2`).
2. Připoj iPhone kabelem, nahoře vyber jako cíl svůj iPhone.
3. Na iPhonu zapni **Nastavení → Soukromí a zabezpečení → Režim pro vývojáře** (iPhone se restartuje).
4. V Xcode **▶ Run** (⌘R).
5. Na iPhonu poprvé: **Nastavení → Obecné → Správa VPN a zařízení** → důvěřovat svému vývojářskému
   profilu. Po spuštění aplikace povol **Místní síť**.

**PASS:** aplikace se otevře, vpravo nahoře zelená tečka, s běžící GT7 se hýbe rychlost a otáčky.
Bez PS5: ozubené kolo → **Ukázková data (závod Spa)** – každé 3 s další okamžik závodu (start,
box, „Nasazeno?“, cíl).
**FAIL:** červená tečka → v Nastavení zkontroluj adresu (`192.168.1.156:8099`, stejná jako
dashboard v prohlížeči) a že je povolená Místní síť (Nastavení iPhonu → BOXBOXBOX).

## Po změnách

Po `git pull` znovu `xcodegen` (jen když přibyl soubor) a ▶ Run. Testy balíčku na Macu:

```
cd ios/BoxCore && swift test
```

## Stav

Fáze 1 (tato verze): připojení, obrazovka Závod, potvrzení směsi, hlas a haptika, ukázková data.
Přejetím doleva se otevře druhá stránka: celý webový dashboard serveru (Strategie, Trénink, Kouč,
Měření, Rekap…), dokud nebudou nativní. Tečky dole ukazují stránku.
Další fáze: brífink a vlastní plán (2), Rekap a historie (3), Live Activity / Dynamic Island a
Apple Watch (4).
