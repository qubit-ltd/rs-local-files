#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
infra_tool="$project_root/.infra/bin/infra-tool.sh"

"$infra_tool" rs-infra-verify --project "$project_root" lock check
"$infra_tool" rs-infra-verify --project "$project_root" run --suite build
"$infra_tool" rs-infra-verify --project "$project_root" run --suite test
