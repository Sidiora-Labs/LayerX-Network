#!/bin/sh
#
# test-ratio-test.sh
#
# The test of the explorer test-ratio gate. It builds a throwaway git
# repository in a temporary directory, commits a satisfied ratio, an
# unsatisfied ratio, a test that references nothing the range changed, a
# change made only of shell, configuration and documentation, and a Rust pair,
# then runs tools/explorer/test-ratio.sh over each range and asserts its exit
# code and the files it names.
#
# Run from anywhere:
#
#   tools/explorer/tests/test-ratio-test.sh
#
# Exit codes: 0 when every case holds, 1 when one does not.

set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
ratio_script=$script_dir/../test-ratio.sh

[ -x "$ratio_script" ] || {
    printf 'test-ratio-test: %s is not an executable script\n' "$ratio_script" >&2
    exit 1
}

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM

output_file=$work_dir/output
failures=0

pass() {
    printf 'ok   %s\n' "$*"
}

fail() {
    printf 'FAIL %s\n' "$*"
    failures=$((failures + 1))
}

run_case() {
    case_name=$1
    case_expected=$2
    shift 2
    set +e
    "$ratio_script" "$@" >"$output_file" 2>&1
    case_status=$?
    set -e
    if [ "$case_status" -eq "$case_expected" ]; then
        pass "$case_name exits $case_expected"
    else
        fail "$case_name: expected exit $case_expected, got $case_status"
        sed -e 's/^/     /' "$output_file"
    fi
}

assert_says() {
    if grep -qF -- "$1" "$output_file"; then
        pass "the output says: $1"
    else
        fail "the output does not say: $1"
        sed -e 's/^/     /' "$output_file"
    fi
}

refute_says() {
    if grep -qF -- "$1" "$output_file"; then
        fail "the output says what it should not: $1"
        sed -e 's/^/     /' "$output_file"
    else
        pass "the output does not say: $1"
    fi
}

commit_all() {
    git add -A
    git commit -q -m "$1"
    git rev-parse HEAD
}

repository=$work_dir/repository
mkdir -p "$repository"
cd "$repository"

git init -q -b main .
mkdir -p "$work_dir/hooks"
git config core.hooksPath "$work_dir/hooks"
git config commit.gpgsign false
git config user.email "test-ratio@paxeer.invalid"
git config user.name "Test Ratio"

mkdir -p explorer
cat >explorer/README.md <<'EOF'
# Explorer

The throwaway repository the test-ratio test measures ranges in.
EOF
base_rev=$(commit_all "Add the explorer directory")

# ---------------------------------------------------------------------------
# A satisfied ratio: an Elixir module with its mirror test and a frontend
# component with the co-located test that imports it.
# ---------------------------------------------------------------------------
mkdir -p explorer/backend/apps/explorer/lib/explorer/chain/paxeer_x
cat >explorer/backend/apps/explorer/lib/explorer/chain/paxeer_x/identity.ex <<'EOF'
defmodule Explorer.Chain.PaxeerX.Identity do
  @moduledoc "Resolves a kernel identity to the address bound to it."

  def resolve(term) when is_binary(term), do: {:ok, term}
end
EOF
mkdir -p explorer/backend/apps/explorer/test/explorer/chain/paxeer_x
cat >explorer/backend/apps/explorer/test/explorer/chain/paxeer_x/identity_test.exs <<'EOF'
defmodule Explorer.Chain.PaxeerX.IdentityTest do
  use ExUnit.Case, async: true

  alias Explorer.Chain.PaxeerX.Identity

  test "resolves a bound identity" do
    assert Identity.resolve("pax1") == {:ok, "pax1"}
  end
end
EOF
mkdir -p explorer/frontend/ui/paxeerX/receipts
cat >explorer/frontend/ui/paxeerX/receipts/ReceiptDetails.tsx <<'EOF'
export default function ReceiptDetails() {
  return null;
}
EOF
cat >explorer/frontend/ui/paxeerX/receipts/ReceiptDetails.spec.tsx <<'EOF'
import ReceiptDetails from './ReceiptDetails';

it('renders the receipt fields', () => {
  expect(ReceiptDetails()).toBe(null);
});
EOF
satisfied_rev=$(commit_all "Add a module and a component, each with its test")

run_case "a satisfied ratio" 0 "$base_rev..$satisfied_rev"
assert_says "source files changed: 2"
assert_says "test files changed: 2 (2 referencing a changed source file)"
assert_says "the test ratio holds"

run_case "a satisfied ratio measured from the merge base" 0 "$base_rev...$satisfied_rev"
assert_says "the test ratio holds"

run_case "a satisfied ratio given as two revisions" 0 "$base_rev" "$satisfied_rev"
assert_says "the test ratio holds"

# ---------------------------------------------------------------------------
# An unsatisfied ratio: two modules, one test.
# ---------------------------------------------------------------------------
cat >explorer/backend/apps/explorer/lib/explorer/chain/paxeer_x/anchor.ex <<'EOF'
defmodule Explorer.Chain.PaxeerX.Anchor do
  @moduledoc "An anchor the kernel wrote to the chain."

  def settled?(anchor), do: anchor != nil
end
EOF
cat >explorer/backend/apps/explorer/lib/explorer/chain/paxeer_x/capability.ex <<'EOF'
defmodule Explorer.Chain.PaxeerX.Capability do
  @moduledoc "A capability an account holds."

  def active?(capability), do: capability != nil
end
EOF
cat >explorer/backend/apps/explorer/test/explorer/chain/paxeer_x/anchor_test.exs <<'EOF'
defmodule Explorer.Chain.PaxeerX.AnchorTest do
  use ExUnit.Case, async: true

  alias Explorer.Chain.PaxeerX.Anchor

  test "an anchor with a value has settled" do
    assert Anchor.settled?(%{})
  end
end
EOF
unsatisfied_rev=$(commit_all "Add two modules with one test between them")

run_case "an unsatisfied ratio" 1 "$satisfied_rev..$unsatisfied_rev"
assert_says "source files changed: 2"
assert_says "test files changed: 1 (1 referencing a changed source file)"
assert_says "no changed test references explorer/backend/apps/explorer/lib/explorer/chain/paxeer_x/capability.ex"
refute_says "no changed test references explorer/backend/apps/explorer/lib/explorer/chain/paxeer_x/anchor.ex"
assert_says "FAILED: 1 referencing test file(s) for 2 changed source file(s)"

# ---------------------------------------------------------------------------
# A test that references nothing the range changed.
# ---------------------------------------------------------------------------
mkdir -p explorer/frontend/lib/paxeerX
cat >explorer/frontend/lib/paxeerX/formatRung.ts <<'EOF'
export function formatRung(rung: number): string {
  return `rung ${ rung }`;
}
EOF
cat >explorer/frontend/lib/paxeerX/unrelated.spec.ts <<'EOF'
import { shortenHash } from 'toolkit/utils/shortenHash';

it('shortens a hash', () => {
  expect(shortenHash('0xabcdef')).toBe('0xab…');
});
EOF
unreferencing_rev=$(commit_all "Add a helper and a test that measures something else")

run_case "a test that references nothing changed" 1 "$unsatisfied_rev..$unreferencing_rev"
assert_says "test files changed: 1 (0 referencing a changed source file)"
assert_says "no changed test references explorer/frontend/lib/paxeerX/formatRung.ts"
assert_says "explorer/frontend/lib/paxeerX/unrelated.spec.ts references no changed source file"

# ---------------------------------------------------------------------------
# Shell, configuration and documentation carry no test requirement.
# ---------------------------------------------------------------------------
mkdir -p explorer/deploy/tools explorer/deploy/railway explorer/frontend/configs/envs explorer/frontend/public/static/paxeer-x
cat >explorer/deploy/tools/report.sh <<'EOF'
#!/bin/sh
set -eu
printf 'the explorer deployment definitions\n'
EOF
chmod +x explorer/deploy/tools/report.sh
cat >explorer/deploy/railway/backend.json <<'EOF'
{ "build": { "builder": "DOCKERFILE" } }
EOF
cat >explorer/deploy/compose.yml <<'EOF'
services:
  backend:
    image: explorer-backend
EOF
cat >explorer/frontend/configs/envs/paxeer-x.env <<'EOF'
NEXT_PUBLIC_NETWORK_NAME=Paxeer X Network
EOF
cat >explorer/frontend/public/static/paxeer-x/logo.svg <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"></svg>
EOF
cat >>explorer/README.md <<'EOF'

The deployment definitions live under `deploy/`.
EOF
documentation_rev=$(commit_all "Describe the deployment definitions")

run_case "a shell and documentation change" 0 "$unreferencing_rev..$documentation_rev"
assert_says "source files changed: 0"
assert_says "the range changes no explorer source file"

# ---------------------------------------------------------------------------
# An empty range.
# ---------------------------------------------------------------------------
run_case "an empty range" 0 "$documentation_rev..$documentation_rev"
assert_says "source files changed: 0"
assert_says "the range changes no explorer source file"

# ---------------------------------------------------------------------------
# Rust: a module under src/ is a source file, a file under tests/ is a test,
# and a module carrying its own #[cfg(test)] block is a test.
# ---------------------------------------------------------------------------
mkdir -p explorer/services/sig-provider/src explorer/services/sig-provider/tests
cat >explorer/services/sig-provider/src/rung.rs <<'EOF'
pub fn settlement_rung(height: u64) -> u64 {
    height / 64
}
EOF
cat >explorer/services/sig-provider/tests/settlement.rs <<'EOF'
use sig_provider::rung::settlement_rung;

#[test]
fn a_height_maps_to_its_rung() {
    assert_eq!(settlement_rung(128), 2);
}
EOF
rust_rev=$(commit_all "Add the settlement rung helper with its integration test")

run_case "a Rust module with a test under tests/" 0 "$documentation_rev..$rust_rev"
assert_says "source files changed: 1"
assert_says "test files changed: 1 (1 referencing a changed source file)"

mkdir -p explorer/services/smart-contract-verifier/src
cat >explorer/services/smart-contract-verifier/src/payload.rs <<'EOF'
pub fn payload_hash(bytes: &[u8]) -> usize {
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::payload_hash;

    #[test]
    fn the_hash_covers_every_byte() {
        assert_eq!(payload_hash(&[1, 2, 3]), 3);
    }
}
EOF
inline_rev=$(commit_all "Add the payload helper with its own test module")

run_case "a Rust module carrying its own test module" 0 "$rust_rev..$inline_rev"
assert_says "source files changed: 0"

# ---------------------------------------------------------------------------
# Arguments that do not resolve.
# ---------------------------------------------------------------------------
run_case "no argument" 2
assert_says "a git range is required"

run_case "an argument that is not a range" 2 "$documentation_rev"
assert_says "is not a git range"

if [ "$failures" -ne 0 ]; then
    printf '\ntest-ratio-test: %s assertion(s) failed\n' "$failures" >&2
    exit 1
fi

printf '\ntest-ratio-test: every case holds\n'
