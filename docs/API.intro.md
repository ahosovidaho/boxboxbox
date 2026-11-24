# BOXBOXBOX – API pro klienty (iOS aplikace)

Server poslouchá na portu z `config.toml` (u tebe `http://192.168.1.156:8099`). Všechno je JSON přes
HTTP, živá data přes WebSocket. Bez přihlášení – je to v domácí síti.

## Verze

`GET /api/version` → `{"server": "0.1.0", "api": 1}`. Klient porovná `api` se svou verzí: když je
vyšší, než zná, upozorní („aktualizuj aplikaci“). `api` se zvyšuje jen při změně nebo odstranění
pole, na které klient spoléhá; nová pole ho nemění. Číslo je `API_VERSION` v `src/web/api.rs`.

## Živá data – `GET /ws` (WebSocket)

Server posílá nejvýš `hz`× za sekundu (výchozí 30) vždy jen poslední stav: jeden JSON objekt
`Snapshot` (textová zpráva). Klient nic neposílá. Při výpadku se znovu připojí; po připojení dostane
hned aktuální stav.

Pro závod je nejdůležitější `strategy` (objekt `Plan`): hlášky `calls`, okno zastávky, palivo,
gumy a plány. Pole `Snapshot` jsou popsaná níž; ta, která klient nepotřebuje, může ignorovat.

## Endpointy

Seznam je v hlavičce `src/web/api.rs` (zdroj pravdy). Nejčastější pro závod:

| Metoda | Cesta | K čemu |
|---|---|---|
| GET | `/api/version` | verze serveru a API |
| GET | `/ws` | živá data (`Snapshot`) |
| GET | `/api/strategy` | uložený stav závodu (`Saved`: závod, stinty, zastávky, kola, kalibrace) |
| GET | `/api/briefing` | brífink vybraného závodu: plány (`options`), vlastní plán (`custom`), podklady |
| POST | `/api/strategy/race` | nastavit závod (`Race`) |
| POST | `/api/strategy/custom` | vlastní plán `{"stints":[{"compound","laps"}], "follow": bool}` → `Custom` |
| POST | `/api/strategy/action` | `{"action": "compound", "compound": "RS"}` (potvrdit/opravit směs), `"new_tyres"`, `"same_tyres"`, `"no_stop"`, `"reset"`, `"forget_wear"`, trénink `"practice"` … |
| GET | `/api/races` | odjeté závody (`Summary[]`, nejnovější první) |
| GET | `/api/races?id=` | jeden závod celý (`Saved`) |
| POST | `/api/races` | `{"id","action":"compounds","compounds":[..]}` / `{"id","action":"remove"}` |
| GET | `/api/practice?track=&car=` | trénink: stav, čísla po směsích, poslední kola |

Chyby: HTTP 404 (neznámé id), 400 (neznámá akce); jinak 200 s JSON.

## Objekty

Tabulky jsou vygenerované ze struktur v Rustu (`scripts/api-doc.sh`, viz konec).
Typy: `Option<T>` = může být `null`, `Vec<T>` = pole, `&'static str` = řetězec.
