#!/bin/bash
set -euo pipefail

PROJECT_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
EXCEPTIONAL_COVERAGE_REGEX="/src/local/internal/(copy_dir/(namespace_race|statistics_overflow)|io_result_context|rooted_io_result|rooted_staging_retry)\\.rs$"
if [ -n "${COVERAGE_EXTRA_EXCLUDE_REGEX:-}" ]; then
    EXCEPTIONAL_COVERAGE_REGEX="${COVERAGE_EXTRA_EXCLUDE_REGEX}|${EXCEPTIONAL_COVERAGE_REGEX}"
fi

exec env \
    RS_CI_PROJECT_ROOT="$PROJECT_ROOT" \
    COVERAGE_EXTRA_EXCLUDE_REGEX="$EXCEPTIONAL_COVERAGE_REGEX" \
    "$PROJECT_ROOT/.rs-ci/align-ci.sh" "$@"
