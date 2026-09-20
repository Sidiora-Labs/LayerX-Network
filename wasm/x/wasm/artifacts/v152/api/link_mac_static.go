//go:build darwin && static_wasm && !sys_wasmvm

package api

// #cgo LDFLAGS: -L${SRCDIR}
// #cgo LDFLAGS: -lwasmvm152static_darwin.part00
// #cgo LDFLAGS: -lwasmvm152static_darwin.part01
// #cgo LDFLAGS: -lwasmvm152static_darwin.part02
import "C"
