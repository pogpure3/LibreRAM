#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Release helper: bump version, push, tag (CI builds), wait, mirror to releases/, prune.
# Usage: ./release.sh 0.3.4 "Release notes in Russian"
set -euo pipefail

VERSION="${1:?usage: release.sh <version> \"notes\"}"
NOTES="${2:?usage: release.sh <version> \"notes\"}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
REPO=pogpure3/LibreRAM
RELEASES="$ROOT/../releases"
LOCAL="$ROOT/releases"   # always duplicate inside the project too

cd "$ROOT"
python - <<PY
import json
conf = json.load(open('src-tauri/tauri.conf.json', encoding='utf-8'))
conf['version'] = '$VERSION'
json.dump(conf, open('src-tauri/tauri.conf.json', 'w', encoding='utf-8'), indent=2, ensure_ascii=False)
cargo = open('src-tauri/Cargo.toml', encoding='utf-8').read()
import re
cargo = re.sub(r'version = "0\.\d+\.\d+"', 'version = "$VERSION"', cargo, count=1)
open('src-tauri/Cargo.toml', 'w', encoding='utf-8').write(cargo)
js = open('ui/app.js', encoding='utf-8').read()
js = re.sub(r'LibreRAM \d+\.\d+\.\d+ \(Tauri\)', 'LibreRAM $VERSION (Tauri)', js)
open('ui/app.js', 'w', encoding='utf-8').write(js)
print('bumped $VERSION')
PY
(cd src-tauri && cargo +stable check -q)   # refresh Cargo.lock; CI uses --locked

git add -A
git commit -q -m "Release $VERSION"
git push origin tauri
git checkout main -q && git merge tauri --ff-only -q && git push origin main -q && git checkout tauri -q
git tag "v$VERSION"
git push origin "v$VERSION"

echo "Waiting for CI…"
sleep 20
RUN=$(gh run list --repo "$REPO" --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')
gh run watch "$RUN" --repo "$REPO" --exit-status > /dev/null

gh release edit "v$VERSION" -R "$REPO" --notes "$NOTES" > /dev/null
gh release download "v$VERSION" -R "$REPO" -D "$RELEASES" --clobber
mkdir -p "$LOCAL"
cp -f "$RELEASES/LibreRAM-$VERSION-preview-x64-tauri.msi" "$RELEASES/LibreRAM-$VERSION-preview-x64-tauri.msi.sha256" "$LOCAL/"

python - "$RELEASES" <<'PY'
import re, sys
from pathlib import Path
versions = {}
for path in Path(sys.argv[1]).iterdir():
    m = re.match(r'LibreRAM-(\d+)\.(\d+)\.(\d+)', path.name)
    if m:
        versions.setdefault(tuple(int(g) for g in m.groups()), []).append(path)
for stale in sorted(versions)[:-5]:
    for p in versions[stale]:
        print('Removed', p.name)
        p.unlink()
PY
echo "Released v$VERSION into $RELEASES"
