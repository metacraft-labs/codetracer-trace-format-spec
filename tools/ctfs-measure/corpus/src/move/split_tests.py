#!/usr/bin/env python3
"""For a Move source file, emit one copy per #[test] function in which every
other test's #[test]/#[expected_failure] attributes are stripped, so that
`sui move test --trace` produces exactly one trace per package."""
import re, sys, os
src, outroot, pkgname = sys.argv[1], sys.argv[2], sys.argv[3]
lines = open(src).read().split('\n')
tests = []  # (attr_line_indices, fun_name)
i = 0
while i < len(lines):
    if re.match(r'\s*#\[test(\s*[\],(])', lines[i]) or lines[i].strip() == '#[test]':
        attrs = [i]; j = i + 1
        while j < len(lines) and lines[j].strip().startswith('#['):
            attrs.append(j); j += 1
        m = re.search(r'fun\s+(\w+)', lines[j])
        tests.append((attrs, m.group(1) if m else f'l{i}'))
        i = j
    else:
        i += 1
stem = os.path.splitext(os.path.basename(src))[0]
for attrs, name in tests:
    out = list(lines)
    for a2, n2 in tests:
        if n2 != name:
            for k in a2: out[k] = ''
    d = os.path.join(outroot, f'{stem}__{name}')
    os.makedirs(os.path.join(d, 'sources'), exist_ok=True)
    open(os.path.join(d, 'sources', stem + '.move'), 'w').write('\n'.join(out))
    open(os.path.join(d, 'Move.toml'), 'w').write(
        f'[package]\nname = "{pkgname}"\nedition = "2024"\n\n[addresses]\n{pkgname} = "0x0"\n')
    print(d)
