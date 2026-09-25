#!/bin/sh
# Explorer frontend lint gate: the two checks the explorer-lint job runs, in the
# order it runs them, so a developer runs exactly what continuous integration runs.
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "${script_dir}/../.." && pwd)
frontend_dir="${repo_root}/explorer/frontend"

if [ ! -f "${frontend_dir}/package.json" ]; then
    echo "explorer frontend not found at ${frontend_dir}" >&2
    exit 1
fi

cd "${frontend_dir}"

# Husky installs git hooks from the package prepare script; a lint run must not
# rewrite the checkout's hooks, and continuous integration has none to install.
HUSKY=0
export HUSKY

echo "==> yarn install --frozen-lockfile"
if yarn install --frozen-lockfile; then
    :
else
    install_status=$?
    echo "explorer frontend dependency installation failed with exit code ${install_status}" >&2
    exit "${install_status}"
fi

status=0

echo "==> yarn lint:eslint"
if yarn lint:eslint; then
    :
else
    eslint_status=$?
    echo "explorer frontend eslint failed with exit code ${eslint_status}" >&2
    status=${eslint_status}
fi

echo "==> yarn lint:tsc"
if yarn lint:tsc; then
    :
else
    tsc_status=$?
    echo "explorer frontend typecheck failed with exit code ${tsc_status}" >&2
    if [ "${status}" -eq 0 ]; then
        status=${tsc_status}
    fi
fi

exit "${status}"
