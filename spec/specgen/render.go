package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// renderRequirements builds requirements.md from a feature spec.kvx.
//
// Schema: [req.<n>] title="..." story="..." ac_1="..." ac_2="..." (ordered).
func renderRequirements(f *Doc, banner string) string {
	var b strings.Builder
	fmt.Fprintf(&b, "<!-- %s -->\n\n", banner)
	b.WriteString("# Requirements\n\n")
	if intro := f.Str("meta", "intro"); intro != "" {
		b.WriteString(intro + "\n\n")
	}
	ids := f.SectionsWithPrefix("req")
	SortDottedIDs(ids)
	for _, id := range ids {
		sec := "req." + id
		title := f.Str(sec, "title")
		fmt.Fprintf(&b, "## Requirement %s: %s\n\n", id, title)
		if story := f.Str(sec, "story"); story != "" {
			fmt.Fprintf(&b, "**User Story:** %s\n\n", story)
		}
		b.WriteString("### Acceptance Criteria\n\n")
		n := 0
		for _, kv := range f.OrderedKV(sec, "ac_") {
			n++
			fmt.Fprintf(&b, "%d. %s\n", n, kv[1])
		}
		b.WriteString("\n")
	}
	return b.String()
}

// renderDesign builds design.md from a feature spec.kvx.
//
// Schema: [design] overview="..." include="design.body.md" (inlined verbatim);
// plus optional [design.<name>] sections (title + ordered scalar paragraphs or
// list bullets) for structured outline content.
func renderDesign(f *Doc, featureDir, banner string) (string, error) {
	var b strings.Builder
	fmt.Fprintf(&b, "<!-- %s -->\n\n", banner)
	// When a rich include supplies its own H1, don't emit a duplicate title.
	if f.Str("design", "include") == "" {
		b.WriteString("# Design\n\n")
	}
	if ov := f.Str("design", "overview"); ov != "" {
		b.WriteString(ov + "\n\n")
	}
	// Structured outline sections, in file order.
	for _, name := range f.SectionsWithPrefix("design") {
		sec := "design." + name
		if title := f.Str(sec, "title"); title != "" {
			fmt.Fprintf(&b, "## %s\n\n", title)
		}
		for _, k := range f.Keys(sec) {
			if k == "title" {
				continue
			}
			if f.IsList(sec, k) {
				for _, item := range f.List(sec, k) {
					fmt.Fprintf(&b, "- %s\n", item)
				}
				b.WriteString("\n")
				continue
			}
			b.WriteString(f.Str(sec, k) + "\n\n")
		}
	}
	// Verbatim include of a rich markdown fragment (mermaid/code/long prose).
	if inc := f.Str("design", "include"); inc != "" {
		body, err := os.ReadFile(filepath.Join(featureDir, inc))
		if err != nil {
			return "", fmt.Errorf("design include %q: %w", inc, err)
		}
		b.WriteString(strings.TrimRight(string(body), "\n"))
		b.WriteString("\n")
	}
	return b.String(), nil
}

// renderTasks builds tasks.md from a feature spec.kvx.
//
// Schema:
//
//	[tasks]  heading="Implementation Plan: X"
//	         overview_include="tasks.overview.md"  notes_include="tasks.notes.md"
//	[task.<id>]  title  status=pending|in_progress|done  wave=<n>
//	             section="MVP First Slice ..."    (heading emitted before a top-level task)
//	             do_1, do_2, ...                  (ordered implementation detail bullets)
//	             note                             (a plain detail bullet, e.g. for checkpoints)
//	             property                         (-> "**Property ...**")
//	             validates                        (-> "**Validates: ...**")
//	             reqs=["1.1","16.2"]              (-> "_Requirements: ..._")
//
// IDs are dotted (1, 1.1, 1.2). A task uses EITHER property+validates (a
// property test) OR reqs (an implementation task) — emitted in that order
// after the detail bullets, mirroring the hand-authored convention.
func renderTasks(f *Doc, featureDir, banner string) (string, error) {
	var b strings.Builder
	fmt.Fprintf(&b, "<!-- %s -->\n\n", banner)
	heading := f.Str("tasks", "heading")
	if heading == "" {
		heading = "Implementation Plan"
	}
	fmt.Fprintf(&b, "# %s\n\n", heading)

	if inc := f.Str("tasks", "overview_include"); inc != "" {
		body, err := os.ReadFile(filepath.Join(featureDir, inc))
		if err != nil {
			return "", fmt.Errorf("tasks overview include %q: %w", inc, err)
		}
		b.WriteString(strings.TrimRight(string(body), "\n"))
		b.WriteString("\n\n")
	} else {
		b.WriteString("## Tasks\n\n")
	}

	ids := f.SectionsWithPrefix("task")
	SortDottedIDs(ids)
	for i, id := range ids {
		sec := "task." + id
		depth := strings.Count(id, ".")
		indent := strings.Repeat("  ", depth)
		if depth == 0 {
			// Blank line before each top-level group; a section emits a heading.
			if section := f.Str(sec, "section"); section != "" {
				if i != 0 {
					b.WriteString("\n")
				}
				fmt.Fprintf(&b, "## %s\n\n", section)
			} else if i != 0 {
				b.WriteString("\n")
			}
		}
		idLabel := id
		if depth == 0 {
			idLabel = id + "." // top-level groups render as "1.", subtasks as "1.1"
		}
		title := f.Str(sec, "title")
		if f.Str(sec, "status") == "implemented" {
			title += " — **Implemented - qualification pending**"
		}
		fmt.Fprintf(&b, "%s- %s %s %s\n", indent, checkbox(f.Str(sec, "status")), idLabel, title)
		// Implementation detail bullets, in file order.
		for _, kv := range f.OrderedKV(sec, "do_") {
			fmt.Fprintf(&b, "%s  - %s\n", indent, kv[1])
		}
		if note := f.Str(sec, "note"); note != "" {
			fmt.Fprintf(&b, "%s  - %s\n", indent, note)
		}
		if prop := f.Str(sec, "property"); prop != "" {
			fmt.Fprintf(&b, "%s  - **%s**\n", indent, prop)
		}
		if val := f.Str(sec, "validates"); val != "" {
			fmt.Fprintf(&b, "%s  - **Validates: %s**\n", indent, val)
		}
		if reqs := f.List(sec, "reqs"); len(reqs) > 0 {
			fmt.Fprintf(&b, "%s  - _Requirements: %s_\n", indent, strings.Join(reqs, ", "))
		}
	}

	if inc := f.Str("tasks", "notes_include"); inc != "" {
		body, err := os.ReadFile(filepath.Join(featureDir, inc))
		if err != nil {
			return "", fmt.Errorf("tasks notes include %q: %w", inc, err)
		}
		b.WriteString("\n")
		b.WriteString(strings.TrimRight(string(body), "\n"))
		b.WriteString("\n")
	}

	b.WriteString(renderWaveGraph(f, ids))
	return b.String(), nil
}

// renderWaveGraph emits the "## Task Dependency Graph" JSON block, grouping
// leaf tasks by their `wave` value (ids sorted within each wave).
func renderWaveGraph(f *Doc, ids []string) string {
	waves := map[uint64][]string{}
	maxWave := uint64(0)
	any := false
	for _, id := range ids {
		sec := "task." + id
		if f.Raw(sec, "wave") == "" {
			continue
		}
		any = true
		w := f.UintOr(sec, "wave", 0)
		waves[w] = append(waves[w], id)
		if w > maxWave {
			maxWave = w
		}
	}
	if !any {
		return ""
	}
	var present []uint64
	for w := uint64(0); w <= maxWave; w++ {
		if _, ok := waves[w]; ok {
			present = append(present, w)
		}
	}
	var b strings.Builder
	b.WriteString("\n## Task Dependency Graph\n\n")
	b.WriteString("```json\n{\n  \"waves\": [\n")
	for i, w := range present {
		ts := waves[w]
		SortDottedIDs(ts)
		quoted := make([]string, len(ts))
		for j, t := range ts {
			quoted[j] = "\"" + t + "\""
		}
		comma := ","
		if i == len(present)-1 {
			comma = ""
		}
		idField := fmt.Sprintf("%d,", w)
		fmt.Fprintf(&b, "    { \"id\": %-4s\"tasks\": [%s] }%s\n", idField, strings.Join(quoted, ", "), comma)
	}
	b.WriteString("  ]\n}\n```\n")
	return b.String()
}

func checkbox(status string) string {
	switch status {
	case "done":
		return "[x]"
	case "in_progress":
		return "[-]"
	default:
		return "[ ]"
	}
}
