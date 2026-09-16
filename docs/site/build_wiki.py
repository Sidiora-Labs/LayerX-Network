#!/usr/bin/env python3
"""Build a static HTML site from docs/wiki and refuse broken relative links."""

from __future__ import annotations

import html
import re
import sys
from pathlib import Path

SITE = Path(__file__).resolve().parent
ROOT = SITE.parents[1]
WIKI = SITE.parent / "wiki"
OUT = SITE / "out"
LINK = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
FENCE = re.compile(r"^```")


def slug(text: str) -> str:
    value = re.sub(r"[^a-zA-Z0-9\s-]", "", text).strip().lower()
    return re.sub(r"[\s]+", "-", value)


def dest_html(target: str) -> str:
    path, _, fragment = target.partition("#")
    if path.endswith(".md"):
        path = path[:-3] + ".html"
    if fragment:
        return f"{path}#{fragment}"
    return path


def render_inline(text: str) -> str:
    pieces: list[str] = []
    cursor = 0
    for match in LINK.finditer(text):
        pieces.append(html.escape(text[cursor : match.start()]))
        label = html.escape(match.group(1))
        target = match.group(2)
        if target.startswith("http://") or target.startswith("https://") or target.startswith("mailto:"):
            href = html.escape(target, quote=True)
        else:
            href = html.escape(dest_html(target), quote=True)
        pieces.append(f'<a href="{href}">{label}</a>')
        cursor = match.end()
    pieces.append(html.escape(text[cursor:]))
    rendered = "".join(pieces)
    rendered = re.sub(r"`([^`]+)`", lambda m: f"<code>{html.escape(m.group(1))}</code>", rendered)
    rendered = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", rendered)
    return rendered


def render_markdown(source: str) -> str:
    lines = source.splitlines()
    out: list[str] = []
    i = 0
    in_fence = False
    fence_lang = ""
    table: list[str] = []

    def flush_table() -> None:
        if not table:
            return
        rows = [row.strip() for row in table if row.strip()]
        table.clear()
        if len(rows) < 2:
            for row in rows:
                out.append(f"<p>{render_inline(row)}</p>")
            return
        out.append("<table>")
        for index, row in enumerate(rows):
            cells = [cell.strip() for cell in row.strip("|").split("|")]
            if index == 1 and all(re.fullmatch(r":?-{3,}:?", cell or "") for cell in cells):
                continue
            tag = "th" if index == 0 else "td"
            out.append("<tr>" + "".join(f"<{tag}>{render_inline(cell)}</{tag}>" for cell in cells) + "</tr>")
        out.append("</table>")

    while i < len(lines):
        line = lines[i]
        if FENCE.match(line):
            flush_table()
            if in_fence:
                out.append("</code></pre>")
                in_fence = False
            else:
                fence_lang = html.escape(line[3:].strip())
                cls = f' class="language-{fence_lang}"' if fence_lang else ""
                out.append(f"<pre><code{cls}>")
                in_fence = True
            i += 1
            continue
        if in_fence:
            out.append(html.escape(line) + "\n")
            i += 1
            continue
        if line.startswith("|"):
            table.append(line)
            i += 1
            continue
        flush_table()
        heading = HEADING.match(line)
        if heading:
            level = len(heading.group(1))
            title = heading.group(2).strip()
            out.append(f'<h{level} id="{html.escape(slug(title), quote=True)}">{render_inline(title)}</h{level}>')
            i += 1
            continue
        if line.startswith("- "):
            out.append("<ul>")
            while i < len(lines) and lines[i].startswith("- "):
                out.append(f"<li>{render_inline(lines[i][2:])}</li>")
                i += 1
            out.append("</ul>")
            continue
        if not line.strip():
            i += 1
            continue
        paragraph = [line]
        i += 1
        while i < len(lines) and lines[i].strip() and not lines[i].startswith("#") and not lines[i].startswith("|") and not lines[i].startswith("- ") and not FENCE.match(lines[i]):
            paragraph.append(lines[i])
            i += 1
        out.append(f"<p>{render_inline(' '.join(paragraph))}</p>")
    flush_table()
    if in_fence:
        out.append("</code></pre>")
    return "".join(out)


def check_links(pages: dict[str, str]) -> list[str]:
    problems: list[str] = []
    for name, source in pages.items():
        for match in LINK.finditer(source):
            target = match.group(2)
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            path, _, _ = target.partition("#")
            if not path:
                continue
            resolved = (WIKI / path).resolve()
            try:
                resolved.relative_to(ROOT.resolve())
            except ValueError:
                problems.append(f"{name}: outside repository {target}")
                continue
            if not resolved.exists():
                problems.append(f"{name}: missing {target}")
    return problems


def page_shell(title: str, body: str, names: list[str]) -> str:
    nav = "".join(
        f'<li><a href="{html.escape(Path(name).stem + ".html", quote=True)}">{html.escape(Path(name).stem)}</a></li>'
        for name in names
    )
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title>
<style>
body {{ font-family: ui-sans-serif, system-ui, sans-serif; margin: 0; display: grid; grid-template-columns: 16rem 1fr; }}
nav {{ background: #111827; color: #e5e7eb; min-height: 100vh; padding: 1rem; }}
nav a {{ color: #93c5fd; }}
main {{ padding: 1.5rem 2rem; max-width: 56rem; }}
pre {{ background: #0f172a; color: #e2e8f0; padding: 1rem; overflow: auto; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ border: 1px solid #d1d5db; padding: 0.4rem 0.6rem; vertical-align: top; }}
code {{ font-family: ui-monospace, SFMono-Regular, monospace; }}
</style>
</head>
<body>
<nav>
<p><a href="Home.html">LayerX wiki</a></p>
<ul>{nav}</ul>
</nav>
<main>{body}</main>
</body>
</html>
"""


def main() -> int:
    pages = {path.name: path.read_text() for path in sorted(WIKI.glob("*.md"))}
    if "Home.md" not in pages:
        print("docs/site/build_wiki.py: Home.md is required", file=sys.stderr)
        return 1
    problems = check_links(pages)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print(f"docs/site/build_wiki.py: {len(problems)} broken relative link(s)", file=sys.stderr)
        return 1
    OUT.mkdir(parents=True, exist_ok=True)
    names = sorted(pages)
    for name, source in pages.items():
        title = source.splitlines()[0].lstrip("# ").strip() if source else name
        (OUT / f"{Path(name).stem}.html").write_text(page_shell(title, render_markdown(source), names))
    (OUT / "index.html").write_text(page_shell("LayerX wiki", render_markdown(pages["Home.md"]), names))
    print(f"wrote {len(pages) + 1} files under {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
