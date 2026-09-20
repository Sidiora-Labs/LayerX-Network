package api

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/wasm/staticarchive"
)

func TestStaticArchiveChecksums(t *testing.T) {
	for _, entry := range []struct{ name, digest string }{
		{"libwasmvm_muslc.a", "2c2d90423e5bebba911b0be7963b09062adb555ecd6149f7cc759a494bb2eda6"},
		{"libwasmvm_muslc.aarch64.a", "409cd9da695d4f1989271230094563c38aa2d867b26908b2ca2082a0d71e3b8e"},
		{"libwasmvmstatic_darwin.a", "713d221d712bee043794fc600e093b62e9e878b312f044f548a8852b670451c8"},
	} {
		t.Run(entry.name, func(t *testing.T) {
			got, err := staticarchive.Verify(".", entry.name)
			if err != nil || got != entry.digest {
				t.Fatalf("static archive identity mismatch: %v", err)
			}
		})
	}
}
