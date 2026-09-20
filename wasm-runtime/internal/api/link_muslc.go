//go:build linux && muslc && !sys_wasmvm

package api

// #cgo LDFLAGS: -Wl,-rpath,${SRCDIR} -L${SRCDIR}
// #cgo amd64 LDFLAGS: -lwasmvm_muslc
// #cgo arm64 LDFLAGS: -l:libwasmvm_muslc.aarch64.a
import "C"
