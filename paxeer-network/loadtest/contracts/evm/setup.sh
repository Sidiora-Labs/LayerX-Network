#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")" && pwd)

verify_tree() {
    directory=$1
    expected=$2
    actual=$(cd "$root/$directory" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum | awk '{print $1}')
    test "$actual" = "$expected"
}

verify_tree lib/openzeppelin-contracts 75178fd3042c5dd196588d69f3c5f70b63406c751d5a27a154fc3dd08ff01dba
verify_tree lib/solmate b92432abfecaf534c50837d0868430a72b3fabcc3d548a9c850b5c75650739f1
