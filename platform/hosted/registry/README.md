# Registry node build boundary

Deployment ingest, the sealed journal envelope, and the Human `pairs/`
export are documented in [RegistryDeploymentJournal.md](../../../docs/wiki/RegistryDeploymentJournal.md)
and [HostedRegistry.md](../../../docs/wiki/HostedRegistry.md).

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

The listener verifies the immutable builder environment at startup and retains
that builder and its configured digest. Isolated workers receive it through the
bounded parent-owned stdin pipe, then bind their own delegated build cgroup.
Each build still copies and fully verifies its environment against the pinned
digest before executing. Health workers replay durable registry evidence and
check the real node without acquiring the build request serialization lock or
updating the protocol cursor.

A background monitor walks the complete rootfs metadata tree every 250 ms,
including device, inode, mode, size, mtime and ctime with nanoseconds. Recursive
nonblocking inotify watches additionally invalidate the fingerprint on writes,
attribute changes, creation, removal, moves and queue overflow, including rapid
same-size writes that retain identical filesystem timestamps. Watch installation
and bounded event reads fail closed. This also
detects nested writes, additions, removals and same-size replacements; it does
not rely on an operator updating a manifest. A changed tree immediately clears
readiness and triggers full digest verification off the request path. Readiness
returns only if the bytes still match the startup pin and metadata stayed stable
during verification. A failed or stalled monitor fails closed: readiness accepts
only a successful check started less than two seconds ago. Detection is bounded
by that freshness window; build-time byte verification remains independent.

Run `tests/readiness.py` inside a disposable container with its own delegated
cgroup, quota slots and registry state. It launches the real registry binary
using an inherited real
node, TLS and delegated quota/cgroup configuration and a temporary copy of the
supplied rootfs. Supply the binary, rootfs, HTTPS URL, CA/client certificate/key,
request token file, real registered program build route and source request body,
and a process log path using its required arguments. It requires successful real
build work overlapping health probes, enforces the existing one-second probe
bound, mutates the private copy, and requires health and build refusal. It never
changes the supplied rootfs or relaxes TLS or isolation requirements.
