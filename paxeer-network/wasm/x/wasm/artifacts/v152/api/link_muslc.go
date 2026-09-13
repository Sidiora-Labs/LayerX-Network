//go:build linux && muslc && !sys_wasmvm

package api

// #cgo LDFLAGS: -Wl,-rpath,${SRCDIR} -L${SRCDIR}
// #cgo amd64 LDFLAGS: -lwasmvm152_muslc
// #cgo arm64 LDFLAGS: -l:libwasmvm152_muslc.aarch64.a
import "C"
