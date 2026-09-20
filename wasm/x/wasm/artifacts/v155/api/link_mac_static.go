//go:build darwin && static_wasm && !sys_wasmvm

package api

// #cgo LDFLAGS: -L${SRCDIR}
// #cgo LDFLAGS: -lwasmvm155static_darwin.part00
// #cgo LDFLAGS: -lwasmvm155static_darwin.part01
// #cgo LDFLAGS: -lwasmvm155static_darwin.part02
import "C"
