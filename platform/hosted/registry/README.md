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
