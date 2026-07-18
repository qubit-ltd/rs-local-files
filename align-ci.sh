#!/bin/bash
set -euo pipefail

PROJECT_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
EXCEPTIONAL_COVERAGE_REGEX="/src/local/internal/(atomic_file_install|atomic_metadata/[^/]+|atomic_namespace_race|copy_dir/(namespace_race|staging_io|statistics_overflow)|io_result_context|opened_atomic_destination|rooted_atomic_install|rooted_atomic_namespace_race|rooted_io_result|rooted_staging_retry|unix_nonblocking)\\.rs$"
if [ -n "${COVERAGE_EXTRA_EXCLUDE_REGEX:-}" ]; then
    EXCEPTIONAL_COVERAGE_REGEX="${COVERAGE_EXTRA_EXCLUDE_REGEX}|${EXCEPTIONAL_COVERAGE_REGEX}"
fi

exec env \
    RS_CI_PROJECT_ROOT="$PROJECT_ROOT" \
    COVERAGE_EXTRA_EXCLUDE_REGEX="$EXCEPTIONAL_COVERAGE_REGEX" \
    "$PROJECT_ROOT/.rs-ci/align-ci.sh" "$@"
