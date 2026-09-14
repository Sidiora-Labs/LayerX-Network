package pax_test

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/sidiora-labs/paxeer-network/wasm/staticarchive"
	"github.com/stretchr/testify/require"
)

// linkDirs are the directories whose link_*.go cgo directives must
// point at real on-disk library artifacts vendored next to them.
var linkDirs = []string{
	"wasm/x/wasm/artifacts/v152/api",
	"wasm/x/wasm/artifacts/v155/api",
	"wasm-runtime/internal/api",
}

var reLDFlags = regexp.MustCompile(`#cgo[^\n]*?LDFLAGS:([^\n]*)`)

func linkLibraries(data []byte) []string {
	var libraries []string
	for _, directive := range reLDFlags.FindAllSubmatch(data, -1) {
		for _, flag := range strings.Fields(string(directive[1])) {
			if strings.HasPrefix(flag, "-l") && len(flag) > 2 {
				libraries = append(libraries, flag[2:])
			}
		}
	}
	return libraries
}

func resolveLibrary(dir, library string) ([]string, error) {
	var names []string
	if strings.HasPrefix(library, ":") {
		names = []string{strings.TrimPrefix(library, ":")}
	} else {
		for _, ext := range linkableExts {
			names = append(names, "lib"+library+ext)
		}
	}
	for _, name := range names {
		if filepath.Base(name) != name {
			return nil, fmt.Errorf("library must be local to source directory: %s", name)
		}
		path := filepath.Join(dir, name)
		info, err := os.Stat(path)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, err
		}
		if !info.Mode().IsRegular() {
			return nil, fmt.Errorf("library is not a regular file: %s", path)
		}
		if info.Size() > 1024 {
			return []string{name}, nil
		}
		// Packaging at 72f2f44ac preserves the pinned archive objects in
		// parts selected by a small GNU linker script. Verify their complete
		// provenance before applying the binary-size floor to every part.
		if _, err := staticarchive.Verify(dir, name); err != nil {
			return nil, fmt.Errorf("library is <=1KiB and has no verified archive group: %s: %w", path, err)
		}
		script, err := os.ReadFile(path)
		if err != nil {
			return nil, err
		}
		fields := strings.Fields(string(script))
		if len(fields) < 4 || fields[0] != "GROUP" || fields[1] != "(" || fields[len(fields)-1] != ")" {
			return nil, fmt.Errorf("invalid archive group: %s", path)
		}
		consumed := []string{name}
		for _, flag := range fields[2 : len(fields)-1] {
			if !strings.HasPrefix(flag, "-l:") {
				return nil, fmt.Errorf("invalid archive group flag: %s", flag)
			}
			part := strings.TrimPrefix(flag, "-l:")
			if filepath.Base(part) != part {
				return nil, fmt.Errorf("archive part must be local: %s", part)
			}
			info, err := os.Stat(filepath.Join(dir, part))
			if err != nil {
				return nil, err
			}
			if !info.Mode().IsRegular() || info.Size() <= 1024 {
				return nil, fmt.Errorf("archive part is not a regular file >1KiB: %s", part)
			}
			consumed = append(consumed, part)
		}
		return consumed, nil
	}
	return nil, fmt.Errorf("-l%s has no checked-in library in %s", library, dir)
}

// linkableExts is the set of extensions cgo's linker will accept when
// resolving -l<name> — it searches for lib<name>.{so,a,dylib} (plus a few
// platform variants we don't need to enumerate here).
var linkableExts = []string{".a", ".so", ".dylib"}

// TestLinkDirectivesResolve asserts that every cgo LDFLAGS -l<name>
// directive across the vendored libwasmvm api packages points at a real
// library file checked into the same directory.
//
// This catches two symmetric failure modes:
//
//  1. A link_*.go references -l<name> but no lib<name>.{a,so,dylib}
//     exists in $SRCDIR. This is what surfaced in PLT-41 once the
//     static-build path was first exercised: link_muslc.go declared
//     -lwasmvm155_muslc but no libwasmvm155_muslc.a was checked in,
//     producing `cannot find -lwasmvm155_muslc` at link time.
//
//  2. An artifact is checked in under a name no link directive resolves
//     against — orphaned files that look authoritative but aren't wired
//     to anything. The previous libwasmvm155static.a (Mach-O arm64) was
//     a textbook example: present in the tree, consumed by nothing.
//
// The 1 KiB floor on each binary artifact is a sanity gate against the failure
// mode where a download produced an HTML error page or an LFS pointer
// (~100–200 bytes) instead of a real archive.
func TestLinkDirectivesResolve(t *testing.T) {
	for _, dir := range linkDirs {
		dir := dir
		gofiles, err := filepath.Glob(filepath.Join(dir, "link_*.go"))
		require.NoError(t, err, "glob link_*.go in %s", dir)
		require.NotEmpty(t, gofiles, "expected link_*.go in %s", dir)

		for _, gofile := range gofiles {
			gofile := gofile
			t.Run(gofile, func(t *testing.T) {
				data, err := os.ReadFile(gofile)
				require.NoError(t, err)

				libraries := linkLibraries(data)
				require.NotEmptyf(t, libraries, "no cgo library directive in %s", gofile)
				for _, library := range libraries {
					consumed, err := resolveLibrary(dir, library)
					require.NoErrorf(t, err, "%s declares -l%s", gofile, library)
					t.Logf("ok: %s -l%s -> %v", gofile, library, consumed)
				}
			})
		}
	}
}

// TestArtifactsHaveNoOrphans asserts the inverse direction: every
// library file in a linkDir corresponds to *something* a link_*.go
// might consume. Catches the "checked in but never used" class — files
// like the original libwasmvm155static.a that were never resolved by any
// directive and sat dormant until someone tried to static-link.
//
// Every architecture's directive is checked, including exact -l:filename
// references and archive parts selected by a verified GNU linker group.
func TestArtifactsHaveNoOrphans(t *testing.T) {
	for _, dir := range linkDirs {
		dir := dir
		t.Run(dir, func(t *testing.T) {
			// Resolve every architecture-qualified directive and group part.
			referenced := map[string]bool{}
			gofiles, err := filepath.Glob(filepath.Join(dir, "link_*.go"))
			require.NoError(t, err)
			for _, gofile := range gofiles {
				data, err := os.ReadFile(gofile)
				require.NoError(t, err)
				libraries := linkLibraries(data)
				require.NotEmpty(t, libraries)
				for _, library := range libraries {
					consumed, err := resolveLibrary(dir, library)
					require.NoError(t, err)
					for _, name := range consumed {
						referenced[name] = true
					}
				}
			}
			// Every library on disk must be consumed by an actual directive
			// or by its verified GNU archive group.
			entries, err := os.ReadDir(dir)
			require.NoError(t, err)
			for _, e := range entries {
				name := e.Name()
				ext := filepath.Ext(name)
				isLib := false
				for _, allowed := range linkableExts {
					if ext == allowed {
						isLib = true
						break
					}
				}
				if !isLib {
					continue
				}
				if !referenced[name] {
					t.Errorf("orphan artifact %s/%s — no link_*.go directive or verified archive group consumes it", dir, name)
				}
			}
		})
	}
}
