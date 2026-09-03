#!/bin/sh
set -eu

CGROUP_PARENT=${LAYERX_REGISTRY_NODE_CGROUP_PARENT:-/sys/fs/cgroup}
CGROUP_ROOT=${LAYERX_REGISTRY_NODE_CGROUP_ROOT:-/sys/fs/cgroup/layerx-program-registry}
QUOTA_ROOT=${LAYERX_REGISTRY_NODE_QUOTA_ROOT:-/var/lib/layerx-program-registry-builds}
SLOTS=${LAYERX_REGISTRY_MAX_BUILDS:?missing build slot count}
BYTES=${LAYERX_REGISTRY_BUILD_QUOTA_BYTES:?missing build byte quota}
INODES=${LAYERX_REGISTRY_BUILD_QUOTA_INODES:?missing build inode quota}

case "$SLOTS:$BYTES:$INODES" in *[!0-9:]*|0:*|*:0:*|*:0) exit 64 ;; esac
exec 9>"${LAYERX_REGISTRY_NODE_LOCK:-/run/lock/layerx-program-registry-boundary.lock}"
flock -x 9

controllers="$(cat "$CGROUP_PARENT/cgroup.controllers")"
for controller in cpu memory pids io; do
    case " $controllers " in *" $controller "*) ;; *) exit 65 ;; esac
done
printf '+cpu +memory +pids +io' > "$CGROUP_PARENT/cgroup.subtree_control"
mkdir -p "$CGROUP_ROOT"
test -z "$(cat "$CGROUP_ROOT/cgroup.procs")"
test "$(stat -c %a "$CGROUP_ROOT")" = 700 || chmod 0700 "$CGROUP_ROOT"
chown 4030:4030 "$CGROUP_ROOT" "$CGROUP_ROOT/cgroup.procs" "$CGROUP_ROOT/cgroup.threads" "$CGROUP_ROOT/cgroup.subtree_control"
printf '+cpu +memory +pids +io' > "$CGROUP_ROOT/cgroup.subtree_control"
test "$(stat -c %u:%g "$CGROUP_ROOT")" = 4030:4030
for controller in cpu memory pids io; do
    case " $(cat "$CGROUP_ROOT/cgroup.subtree_control") " in *" $controller "*) ;; *) exit 66 ;; esac
done

mkdir -p "$QUOTA_ROOT"
groups=$(( (BYTES + 32768 * 4096 - 1) / (32768 * 4096) ))
inodes_per_group=$(( INODES / groups / 16 * 16 ))
test "$inodes_per_group" -ge 16 || exit 64
slot=0
while [ "$slot" -lt "$SLOTS" ]; do
    image="$QUOTA_ROOT/slot-$slot.ext4"
    mountpoint="$QUOTA_ROOT/slot-$slot"
    mkdir -p "$mountpoint"
    if mountpoint -q "$mountpoint"; then
        loop="$(findmnt -n -o SOURCE --target "$mountpoint")"
        case "$loop" in /dev/loop[0-9]*) ;; *) exit 67 ;; esac
        test "$(losetup -j "$image" | wc -l)" = 1
        backing="$(losetup -n -O BACK-FILE "$loop" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
        test "$(readlink -f "$backing")" = "$(readlink -f "$image")"
    else
        if [ -e "$image" ]; then
            test -f "$image"
            test -z "$(losetup -j "$image")"
            test "$(stat -c %s "$image")" = "$BYTES"
            e2fsck -p "$image" || test "$?" = 1
        else
            temporary="$image.new.$$"
            trap 'rm -f "$temporary"' EXIT HUP INT TERM
            truncate -s "$BYTES" "$temporary"
            mkfs.ext4 -q -F -b 4096 -g 32768 -I 256 -N "$((inodes_per_group * groups))" "$temporary"
            mv -T "$temporary" "$image"
            trap - EXIT HUP INT TERM
        fi
        loop="$(losetup --find --show "$image")"
        mount -t ext4 -o nosuid,nodev,noatime "$loop" "$mountpoint"
        losetup -d "$loop"
        test "$(losetup -n -O AUTOCLEAR "$loop" | tr -d '[:space:]')" = 1
    fi
    tune="$(dumpe2fs -h "$image" 2>/dev/null)"
    blocks="$(printf '%s\n' "$tune" | awk -F: '/^Block count:/{gsub(/ /,"",$2);print $2}')"
    block_size="$(printf '%s\n' "$tune" | awk -F: '/^Block size:/{gsub(/ /,"",$2);print $2}')"
    inode_count="$(printf '%s\n' "$tune" | awk -F: '/^Inode count:/{gsub(/ /,"",$2);print $2}')"
    test "$((blocks * block_size))" -le "$BYTES"
    test "$inode_count" -le "$INODES"
    test "$(stat -c %a "$mountpoint")" = 700 || chmod 0700 "$mountpoint"
    chown 4030:4030 "$mountpoint"
    test "$(stat -c %u:%g "$mountpoint")" = 4030:4030
    slot=$((slot + 1))
done

for candidate in "$QUOTA_ROOT"/slot-*; do
    [ -e "$candidate" ] || continue
    base="$(basename "$candidate")"
    index="${base#slot-}"
    index="${index%.ext4}"
    case "$index" in ''|*[!0-9]*) exit 68 ;; esac
    test "$index" -lt "$SLOTS" || exit 68
    test "$base" = "slot-$index" || test "$base" = "slot-$index.ext4" || exit 68
done
