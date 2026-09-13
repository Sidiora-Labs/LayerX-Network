//go:build darwin && static_wasm && !sys_wasmvm

package api

// #cgo LDFLAGS: -L${SRCDIR}
// #cgo amd64 LDFLAGS: -lwasmvmstatic_darwin.amd64
// #cgo arm64 LDFLAGS: -lwasmvmstatic_darwin.arm64
import "C"
