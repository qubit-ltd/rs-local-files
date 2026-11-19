#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
siblings="$(dirname "$root")"
local_files="$root"
fs_local="$siblings/rs-fs-local"
mime="$siblings/rs-mime"

for path in "$fs_local" "$mime"; do
  test -f "$path/Cargo.toml" || { echo "missing downstream checkout: $path" >&2; exit 2; }
  git -C "$path" diff --quiet HEAD -- || { echo "downstream has pre-existing tracked changes: $path" >&2; exit 2; }
done

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
backup_repo() {
  local repo="$1" name doc
  name="$(basename "$repo")"
  cp "$repo/Cargo.toml" "$tmp/$name-Cargo.toml"
  cp "$repo/Cargo.lock" "$tmp/$name-Cargo.lock"
  for doc in README.md README.zh_CN.md doc/user_guide.md doc/user_guide.zh_CN.md doc/local_file_system_adapter_design.md doc/local_file_system_adapter_design.zh_CN.md; do
    if [[ -f "$repo/$doc" ]]; then cp "$repo/$doc" "$tmp/$name-${doc//\//_}"; fi
  done
}
restore_repo() {
  local repo="$1" name doc
  name="$(basename "$repo")"
  cp "$tmp/$name-Cargo.toml" "$repo/Cargo.toml"
  cp "$tmp/$name-Cargo.lock" "$repo/Cargo.lock"
  for doc in README.md README.zh_CN.md doc/user_guide.md doc/user_guide.zh_CN.md doc/local_file_system_adapter_design.md doc/local_file_system_adapter_design.zh_CN.md; do
    if [[ -f "$tmp/$name-${doc//\//_}" ]]; then cp "$tmp/$name-${doc//\//_}" "$repo/$doc"; fi
  done
}
restore_all() {
  restore_repo "$fs_local"
  restore_repo "$mime"
  cp "$tmp/fs-local-shared-Cargo.toml" "$fs_local/Cargo.toml"
  rm -rf "$tmp"
}
backup_repo "$fs_local"
backup_repo "$mime"
cp "$fs_local/Cargo.toml" "$tmp/fs-local-shared-Cargo.toml"
trap restore_all EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for repo in "$fs_local" "$mime"; do
  restore_repo "$repo"
  cp "$tmp/fs-local-shared-Cargo.toml" "$fs_local/Cargo.toml"
  if [[ "$repo" == "$mime" ]]; then
    python3 - "$fs_local/Cargo.toml" "$local_files" <<'PY'
from pathlib import Path
import sys
manifest, local_files = map(Path, sys.argv[1:])
text = manifest.read_text().replace('qubit-local-files = { version = "^0.5"', 'qubit-local-files = { version = "^0.6"')
text += f'\n[patch.crates-io]\nqubit-local-files = {{ path = "{local_files}" }}\n'
manifest.write_text(text)
PY
  fi
  python3 - "$repo/Cargo.toml" "$repo" "$local_files" <<'PY'
from pathlib import Path
import sys
manifest, repo, local_files = map(Path, sys.argv[1:])
text = manifest.read_text().replace('qubit-local-files = { version = "^0.5"', 'qubit-local-files = { version = "^0.6"')
if 'qubit-local-files = { version = "^0.6"' not in text:
    raise SystemExit(f"no ^0.5 local-files dependency found in {manifest}")
if '[patch.crates-io]' in text:
    raise SystemExit(f"unexpected pre-existing crates.io patch table in {manifest}")
text += f'\n[patch.crates-io]\nqubit-local-files = {{ path = "{local_files}" }}\n'
manifest.write_text(text)
for relative in ('README.md', 'README.zh_CN.md', 'doc/user_guide.md', 'doc/user_guide.zh_CN.md', 'doc/local_file_system_adapter_design.md', 'doc/local_file_system_adapter_design.zh_CN.md'):
    path = repo / relative
    if not path.exists():
        continue
    text = path.read_text()
    path.write_text(text.replace('`qubit-local-files` 0.5.3', '`qubit-local-files` 0.6.0')
                     .replace('`qubit-local-files` 0.5.1', '`qubit-local-files` 0.6.0')
                     .replace('`qubit-local-files` 0.5', '`qubit-local-files` 0.6.0'))
PY
  cargo update --manifest-path "$repo/Cargo.toml" -p qubit-local-files
  cargo metadata --manifest-path "$repo/Cargo.toml" --locked --all-features --format-version 1 |
    python3 -c '
import hashlib
import json
from pathlib import Path
import sys

consumer_manifest, candidate_manifest = (Path(arg).resolve() for arg in sys.argv[1:])
metadata = json.load(sys.stdin)
packages = metadata["packages"]
consumers = [p for p in packages if Path(p["manifest_path"]).resolve() == consumer_manifest]
assert len(consumers) == 1, consumer_manifest
consumer = consumers[0]
assert consumer["id"] in metadata["workspace_members"], consumer["id"]

candidates = [p for p in packages if p["name"] == "qubit-local-files"]
assert len(candidates) == 1, [(p["version"], p["manifest_path"]) for p in candidates]
candidate = candidates[0]
assert candidate["version"] == "0.6.0" and candidate["source"] is None, candidate
assert Path(candidate["manifest_path"]).resolve() == candidate_manifest, candidate

nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
edges = [
    dep for dep in nodes[consumer["id"]]["deps"]
    if dep["name"].replace("_", "-") == "qubit-local-files" and dep["pkg"] == candidate["id"]
]
assert edges, (consumer["name"], candidate["id"])
declared_kinds = {dep["kind"] for dep in consumer["dependencies"] if dep["name"] == "qubit-local-files"}
resolved_kinds = {kind["kind"] for edge in edges for kind in edge["dep_kinds"]}
assert None in resolved_kinds and declared_kinds == resolved_kinds, (declared_kinds, resolved_kinds)

lock_sha256 = hashlib.sha256(consumer_manifest.with_name("Cargo.lock").read_bytes()).hexdigest()
consumer_name = consumer["name"]
version = candidate["version"]
features = nodes[candidate["id"]]["features"]
print(
    f"candidate {consumer_name}: version={version} manifest={candidate_manifest} "
    f"dependency_kinds={sorted(map(str, resolved_kinds))} features={features} "
    f"lock_sha256={lock_sha256}"
)
' "$repo/Cargo.toml" "$local_files/Cargo.toml"
  if [[ "$repo" == "$fs_local" ]]; then
    cargo test --manifest-path "$repo/Cargo.toml" --locked --all-features \
      --test local_error_mapping_tests --test local_temp_resource_spi_tests --test documentation_examples_tests
  else
    cargo test --manifest-path "$repo/Cargo.toml" --locked --all-features --lib managed_staging_tests
  fi
  restore_repo "$repo"
  cp "$tmp/fs-local-shared-Cargo.toml" "$fs_local/Cargo.toml"
done
