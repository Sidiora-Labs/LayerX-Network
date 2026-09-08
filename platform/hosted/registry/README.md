# Registry node build boundary

Install `node-provision-build-boundary.sh` at `/usr/libexec/layerx/` and
`layerx-program-registry-boundary.service` at `/etc/systemd/system/`.
Create `/var/lib/layerx-program-registry-builds`, reload systemd, and enable
and start the service before labelling a registry node. The node needs systemd
with `systemd-mount`, util-linux and e2fsprogs, loop devices and cgroup v2.

The provisioner retains `ProtectSystem=strict`, `ProtectHome=yes`,
`PrivateTmp=yes`, `NoNewPrivileges=yes` and its explicit writable paths.
It has only `CAP_CHOWN` and `CAP_DAC_OVERRIDE`; it cannot mount filesystems.
It checks existing images with e2fsck before use, creates bounded ext4 images,
and requests synchronous transient mount units through `systemd-mount`.
PID 1 performs these mounts in the node namespace, making them visible to
kubelet and the registry hostPath. The transient units precede kubelet and are
collected after unmount; each boot recreates them from persistent images.

Mount options remain `loop,nosuid,nodev,noatime`. Execution remains enabled
because the builder executes pinned open-inode supervisor and isolation
binaries from its quota-backed environment; `noexec` would break that contract.
The provisioner validates ext4, mount options, a distinct device, loop backing
identity on reuse, autoclear, byte and inode bounds, and root ownership
`0700 4030:4030` before completing. systemd owns loop setup and autoclear;
no provisioner process performs a mount in its private namespace.

Stopping the provisioner alone does not unmount active build storage. Drain
registry builds before stopping the transient mount units. Do not alter image
size or slot count while builds are active; conflicting existing state fails
closed. A service restart validates and reuses existing mounts.

The registry container starts as root with only CHOWN, SETUID and SETGID,
no privilege escalation, a read-only root filesystem and RuntimeDefault seccomp.
`LAYERX_REGISTRY_HOST_CGROUP_MOUNT` must name the writable hostPath mount of
`/sys/fs/cgroup/kubelet.slice` at `/run/layerx/host-cgroup`. Startup matches the
namespace root `/sys/fs/cgroup` by device and inode in a depth-eight bounded
walk, requires exactly one match and confirms its own PID in `cgroup.procs`.
Discovery descends only into root-owned directories: kubelet and containerd
create the container cgroup and its ancestors as root. Non-root owners are
excluded with one debug message per UID; unreadable root-owned directories
remain fatal. The walk does not descend into the matched container cgroup.
It refuses startup without all four controllers: cpu, memory, pids and io.

Before reading secrets, tokens or the journal, startup moves itself to `C/main`,
enables the four controllers in C and `C/workers`, and delegates the workers
directory, its procs/threads/subtree-control files and C's cgroup.procs to
4030:4030. It disables keepcaps, sets supplementary groups to 4030, sets all
GIDs and UIDs to 4030, verifies zero effective/permitted/inheritable capabilities
and all four identity columns, and requires an attempted root restoration to
fail with EPERM. State and journal creation follows the privilege drop.
Workers and builds remain stopped before attachment beneath
`C/workers/request-<pid>-<ts>/{worker,builds}`, with the existing kill, deadline,
IPC, ownership and controller checks. The node provisioner handles quota slots
only; the deployment requires the node boundary label version v2.

Once controllers are enabled in C, the kernel refuses new processes directly
in C. Consequently `kubectl exec` into the registry container is refused by
design. Readiness remains a `tcpSocket` probe; inspect cgroups and process
identity from the host when diagnosing this boundary.
