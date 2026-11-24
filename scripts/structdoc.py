"""Markdown table of the pub fields (with their /// docs) of named Rust structs."""
import re, sys
def struct(path, name):
    src = open(path).read()
    m = re.search(r'pub struct ' + name + r' \{\n(.*?)\n\}', src, re.S)
    if not m: return None
    rows, doc = [], []
    for line in m.group(1).split('\n'):
        t = line.strip()
        if t.startswith('///'): doc.append(t[3:].strip()); continue
        if t.startswith('#['): continue
        f = re.match(r'pub (\w+): (.+),$', t)
        if f:
            rows.append((f.group(1), f.group(2).replace('|', '\\|'), ' '.join(doc).replace('|', '\\|')))
        doc = []
    out = ['| Pole | Typ | Význam |', '|---|---|---|']
    out += [f'| `{n}` | `{ty}` | {d} |' for n, ty, d in rows]
    return '\n'.join(out)
for spec in sys.argv[1:]:
    path, name = spec.split(':')
    print(f'### `{name}`\n')
    print(struct(path, name) or '_nenalezeno_')
    print()
