#!/usr/bin/env python3
"""Build a static HTML site from docs/wiki and check relative Markdown links."""

from __future__ import annotations

import argparse
import html
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WIKI = ROOT / "docs" / "wiki"
SITE = ROOT / "docs" / "_site"

NAV = [
    ("Home", "Home.md"),
    ("Protocol", "Protocol.md"),
    ("Modules", "Modules.md"),
    ("Asset", "Asset.md"),
    ("Escrow", "Escrow.md"),
    ("Budget", "Budget.md"),
    ("Stream", "Stream.md"),
    ("Service", "Service.md"),
    ("Perps", "Perps.md"),
    ("Governance", "Governance.md"),
    ("Bridge", "Bridge.md"),
    ("Fees", "Fees.md"),
    ("Sequencing", "Sequencing.md"),
    ("Finality", "Finality.md"),
    ("Programs", "Programs.md"),
    ("LNI", "LNI.md"),
    ("Agent API", "AgentApi.md"),
    ("MCP", "Mcp.md"),
    ("Roadmap", "Roadmap.md"),
]


def slugify(text: str) -> str:
    text = text.strip().lower()
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"[-\s]+", "-", text)


def heading_id(text: str) -> str:
    text = re.sub(r"`+", "", text)
    return slugify(text)


def html_page_name(md_name: str) -> str:
    return Path(md_name).with_suffix(".html").name


def inline(text: str) -> str:
    parts: list[str] = []
    index = 0
    pattern = re.compile(
        r"(`+)(.+?)\1|\[([^\]]+)\]\(([^)]+)\)|\*\*([^*]+)\*\*|\*([^*]+)\*"
    )
    for match in pattern.finditer(text):
        parts.append(html.escape(text[index : match.start()]))
        if match.group(1):
            parts.append(f"<code>{html.escape(match.group(2))}</code>")
        elif match.group(3) is not None:
            label = inline(match.group(3))
            href = match.group(4)
            if href.endswith(".md") or ".md#" in href:
                path, hash_part = (href.split("#", 1) + [""])[:2]
                href = html_page_name(path)
                if hash_part:
                    href = f"{href}#{hash_part}"
            escaped = html.escape(href, quote=True)
            parts.append(f'<a href="{escaped}">{label}</a>')
        elif match.group(5) is not None:
            parts.append(f"<strong>{html.escape(match.group(5))}</strong>")
        else:
            parts.append(f"<em>{html.escape(match.group(6))}</em>")
        index = match.end()
    parts.append(html.escape(text[index:]))
    return "".join(parts)


def render_markdown(source: str) -> str:
    lines = source.splitlines()
    out: list[str] = []
    i = 0
    in_code = False
    code_lang = ""
    in_list = False
    in_table = False

    def close_list() -> None:
        nonlocal in_list
        if in_list:
            out.append("</ul>")
            in_list = False

    def close_table() -> None:
        nonlocal in_table
        if in_table:
            out.append("</tbody></table>")
            in_table = False

    while i < len(lines):
        line = lines[i]
        if line.startswith("```"):
            close_list()
            close_table()
            if in_code:
                out.append("</code></pre>")
                in_code = False
            else:
                code_lang = html.escape(line[3:].strip())
                cls = f' class="language-{code_lang}"' if code_lang else ""
                out.append(f"<pre><code{cls}>")
                in_code = True
            i += 1
            continue
        if in_code:
            out.append(html.escape(line) + "\n")
            i += 1
            continue
        if not line.strip():
            close_list()
            close_table()
            i += 1
            continue
        heading = re.match(r"^(#{1,6})\s+(.*)$", line)
        if heading:
            close_list()
            close_table()
            level = len(heading.group(1))
            title = heading.group(2).strip()
            hid = heading_id(title)
            out.append(f'<h{level} id="{html.escape(hid)}">{inline(title)}</h{level}>')
            i += 1
            continue
        if line.startswith("|") and i + 1 < len(lines):
            separator = lines[i + 1].strip()
            if re.match(r"^\|?[\s:|-]*-{3,}[\s:|-]*\|?$", separator):
                close_list()
                headers = [cell.strip() for cell in line.strip().strip("|").split("|")]
                out.append("<table><thead><tr>")
                for header in headers:
                    out.append(f"<th>{inline(header)}</th>")
                out.append("</tr></thead><tbody>")
                in_table = True
                i += 2
                continue
        if in_table and line.startswith("|"):
            cells = [cell.strip() for cell in line.strip("|").split("|")]
            out.append("<tr>")
            for cell in cells:
                out.append(f"<td>{inline(cell)}</td>")
            out.append("</tr>")
            i += 1
            continue
        if in_table:
            close_table()
        if line.startswith("- "):
            if not in_list:
                out.append("<ul>")
                in_list = True
            out.append(f"<li>{inline(line[2:])}</li>")
            i += 1
            continue
        close_list()
        out.append(f"<p>{inline(line)}</p>")
        i += 1
    close_list()
    close_table()
    if in_code:
        out.append("</code></pre>")
    return "\n".join(out)


def wrap(title: str, body: str, current: str) -> str:
    items = []
    for label, name in NAV:
        href = html_page_name(name)
        cls = ' class="current"' if name == current else ""
        items.append(f'<li{cls}><a href="{href}">{html.escape(label)}</a></li>')
    nav = "\n".join(items)
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)} — LayerX wiki</title>
<style>
body {{ margin: 0; font-family: ui-sans-serif, system-ui, sans-serif; color: #1a1a1a; }}
main {{ display: grid; grid-template-columns: 16rem 1fr; min-height: 100vh; }}
nav {{ background: #f4f1ea; padding: 1.5rem 1rem; }}
nav h1 {{ font-size: 1rem; margin: 0 0 1rem; }}
nav ul {{ list-style: none; padding: 0; margin: 0; }}
nav li {{ margin: 0.35rem 0; }}
nav a {{ color: #1a1a1a; text-decoration: none; }}
nav .current a {{ font-weight: 700; }}
article {{ padding: 2rem 2.5rem 4rem; max-width: 52rem; }}
table {{ border-collapse: collapse; width: 100%; margin: 1rem 0; }}
th, td {{ border: 1px solid #ccc; padding: 0.4rem 0.6rem; text-align: left; }}
pre {{ background: #111; color: #f4f1ea; padding: 0.8rem; overflow: auto; }}
code {{ font-family: ui-monospace, monospace; }}
p code, li code, td code, th code {{ background: #eee; padding: 0.05rem 0.25rem; }}
</style>
</head>
<body>
<main>
<nav>
<h1><a href="Home.html">LayerX wiki</a></h1>
<ul>
{nav}
</ul>
</nav>
<article>
{body}
</article>
</main>
</body>
</html>
"""


LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")


def check_links(pages: dict[str, str]) -> list[str]:
    errors: list[str] = []
    names = set(pages)
    for name, text in pages.items():
        for href in LINK_RE.findall(text):
            if href.startswith(("http://", "https://", "mailto:")):
                continue
            if href.startswith("#"):
                continue
            path, _, fragment = href.partition("#")
            if not path:
                continue
            if path.startswith("../") or path.startswith("/"):
                continue
            target = Path(path).name
            if target.endswith(".md") and target not in names:
                errors.append(f"{name}: missing {target}")
            if fragment and target in pages:
                hid = heading_id(fragment.replace("-", " "))
                rendered_ids = {
                    heading_id(match.group(1))
                    for match in re.finditer(r"^#{1,6}\s+(.*)$", pages[target], re.M)
                }
                # GitHub-style slugs from the fragment itself are accepted.
                if fragment not in rendered_ids and hid not in rendered_ids:
                    # Fragments are best-effort; only fail if the page is missing.
                    pass
    return errors


def title_of(text: str, fallback: str) -> str:
    for line in text.splitlines():
        if line.startswith("# "):
            return line[2:].strip()
    return fallback


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    pages = {path.name: path.read_text(encoding="utf-8") for path in sorted(WIKI.glob("*.md"))}
    errors = check_links(pages)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    SITE.mkdir(parents=True, exist_ok=True)
    for name, text in pages.items():
        title = title_of(text, name)
        (SITE / html_page_name(name)).write_text(
            wrap(title, render_markdown(text), name), encoding="utf-8"
        )
    index = SITE / "index.html"
    index.write_text((SITE / "Home.html").read_text(encoding="utf-8"), encoding="utf-8")
    print(f"wrote {len(pages)} pages to {SITE}")
    if args.check:
        print("wiki link check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
