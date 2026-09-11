#!/bin/sh
set -eu

root=${1:-.}
schema=$root/agent/schema/lni/v1.kvx
daemon=$root/cmd/layerxd/lxp_daemon_lni.c
client=$root/agent/crates/layerx-client/src/lni/schema.rs
native_program=$root/tests/daemon/lxp_test_program_admission.c
native_admission=$root/tests/test_daemon_lni_admission.c
native_pay=$root/tests/daemon/lxp_test_pay1.c

fail() {
    printf 'LNI version parity: %s\n' "$1" >&2
    exit 1
}

for file in "$schema" "$daemon" "$client" "$native_program" \
        "$native_admission" "$native_pay"; do
    [ -f "$file" ] || fail "$file does not exist"
done

value=
one_value() {
    value=$(sed -n "$2" "$1")
    count=$(printf '%s\n' "$value" | grep -c '[^[:space:]]' || true)
    [ "$count" = 1 ] || fail \
"$3 is declared $count times in $1 where the parity check reads exactly one declaration"
}

one_value "$schema" '/^\[schema\]$/,/^$/{s/^major = \([0-9][0-9]*\)$/\1/p;}' \
    'the interface major version'
schema_major=$value
one_value "$schema" '/^\[schema\]$/,/^$/{s/^minor = \([0-9][0-9]*\)$/\1/p;}' \
    'the interface minor version'
schema_minor=$value

check_pair() {
    file=$1
    major_expression=$2
    minor_expression=$3
    label=$4
    one_value "$file" "$major_expression" "$label major"
    [ "$value" = "$schema_major" ] || fail \
"$label major is $value in $file but the schema declares $schema_major in $schema"
    one_value "$file" "$minor_expression" "$label minor"
    [ "$value" = "$schema_minor" ] || fail \
"$label minor is $value in $file but the schema declares $schema_minor in $schema"
}

check_pair "$daemon" \
    's/^[[:space:]]*LNI_VERSION_MAJOR = \([0-9][0-9]*\)U*,$/\1/p' \
    's/^[[:space:]]*LNI_VERSION_MINOR = \([0-9][0-9]*\)U*,$/\1/p' \
    'the daemon interface version'

check_pair "$native_program" \
    's/^[[:space:]]*LNI_MAJOR = \([0-9][0-9]*\)U*,$/\1/p' \
    's/^[[:space:]]*LNI_MINOR = \([0-9][0-9]*\)U*,$/\1/p' \
    'the native admission client interface version'

check_pair "$native_admission" \
    's/^[[:space:]]*LNI_MAJOR = \([0-9][0-9]*\)U*,$/\1/p' \
    's/^[[:space:]]*LNI_MINOR = \([0-9][0-9]*\)U*,$/\1/p' \
    'the native LNI admission client interface version'

one_value "$native_pay" \
    's/.*response\.minor == \([0-9][0-9]*\)U.*/\1/p' \
    'the native payment client response minor'
[ "$value" = "$schema_minor" ] || fail \
"the native payment client pins response minor $value in $native_pay but the schema declares $schema_minor in $schema"

one_value "$client" \
    's/^[[:space:]]*version: Version::V1_\([0-9][0-9]*\),$/\1/p' \
    'the client crate schema minor'
[ "$value" = "$schema_minor" ] || fail \
"the client crate builds its schema at minor $value in $client but the schema declares $schema_minor in $schema"

one_value "$client" \
    's/^pub const LNI_V1_SOURCE: &str = include_str!("\(.*\)");$/\1/p' \
    'the client crate schema source path'
included=$(cd "$(dirname "$client")" && readlink -f "$value" 2>/dev/null || true)
[ -n "$included" ] && [ "$included" = "$(readlink -f "$schema")" ] || fail \
"the client crate includes $value relative to $(dirname "$client") which is not $schema"

declared=$(sed -n \
    's/^[[:space:]]*pub const V1_\([0-9][0-9]*\): Self = Self { major: \([0-9][0-9]*\), minor: \([0-9][0-9]*\) };$/\1:\2:\3/p' \
    "$client" | sort)
expected=
revision=0
while [ "$revision" -le "$schema_minor" ]; do
    expected=$(printf '%s\n%s' "$expected" "$revision:$schema_major:$revision")
    revision=$((revision + 1))
done
expected=$(printf '%s' "$expected" | grep '[^[:space:]]' | sort)
[ "$declared" = "$expected" ] || fail \
"the client crate declares the revisions $(printf '%s' "$declared" | tr '\n' ' ') but the schema declares $schema_major.$schema_minor, which requires $(printf '%s' "$expected" | tr '\n' ' ')"

printf 'LNI version parity: %s.%s in %s, %s, %s, %s, %s and %s\n' \
    "$schema_major" "$schema_minor" "$schema" "$daemon" "$client" \
    "$native_program" "$native_admission" "$native_pay"
