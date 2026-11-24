#!/bin/sh
# Rebuilds docs/API.md: the hand-written docs/API.intro.md + the object tables generated from the
# Rust structs. Run from the repo root after changing a struct a client reads.
set -e
{
  cat docs/API.intro.md
  echo
  python3 scripts/structdoc.py src/session/tracker.rs:Snapshot src/strategy/mod.rs:Plan src/strategy/mod.rs:Call \
    src/strategy/planner.rs:Option_ src/strategy/planner.rs:Stint src/strategy/planner.rs:PlannedStop \
    src/strategy/mod.rs:Custom src/strategy/mod.rs:Saved src/strategy/mod.rs:Race src/strategy/mod.rs:CustomStint \
    src/strategy/mod.rs:Stint src/strategy/mod.rs:Stop src/strategy/mod.rs:FuelLap src/strategy/history.rs:Summary \
    src/strategy/mod.rs:PracticeState
  printf '## Jak dokument obnovit\n\n```\nscripts/api-doc.sh\n```\n\nÚvod je v `docs/API.intro.md`, tabulky se generují z `///` komentářů struktur.\n'
} > docs/API.md
