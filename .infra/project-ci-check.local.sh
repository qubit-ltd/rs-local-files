#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
package_file_list=$(mktemp)
trap 'rm -f "$package_file_list"' EXIT

cd "$project_root"
cargo package --locked --allow-dirty --list > "$package_file_list"
grep -Fxq 'doc/review-rlf5-windows-access-evidence.md' "$package_file_list" || {
    echo 'error: RLF5 Windows access evidence is absent from the Cargo package' >&2
    exit 1
}
RLF_PACKAGE_FILE_LIST="$package_file_list" cargo test --locked --test documentation_links_tests \
    test_packaged_documentation_links -- --exact --ignored
