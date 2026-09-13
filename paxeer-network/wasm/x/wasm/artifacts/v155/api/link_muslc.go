//go:build linux && muslc && !sys_wasmvm

package api

// #cgo LDFLAGS: -Wl,-rpath,${SRCDIR} -L${SRCDIR}
// #cgo amd64 LDFLAGS: -lwasmvm155_muslc
// #cgo arm64 LDFLAGS: -l:libwasmvm155_muslc.aarch64.a
import "C"
