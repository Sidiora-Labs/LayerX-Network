# LayerX documentation site

MkDocs Material site for LayerX Network. Page sources are under `docs/`.
Normative protocol text remains in `spec/`.

## Preview locally

From this directory:

```sh
python3 -m venv .venv
. .venv/bin/activate
pip install -r requirements.txt
mkdocs serve
```

Then open the URL the command prints (typically `http://127.0.0.1:8000`).

A one-shot build:

```sh
mkdocs build --strict
```

Output lands in `site/`, which is not committed.

`mkdocs serve` and `mkdocs build` must be run from this directory so they
read `mkdocs.yml` here.

## Layout

| Path | Role |
| --- | --- |
| `mkdocs.yml` | Site configuration and navigation |
| `requirements.txt` | MkDocs and the Material theme |
| `docs/` | Markdown pages |

The former wiki pages remain in `../wiki/` and are copied into `docs/` with
rewritten relative links. Prefer this tree for new documentation.

## CI

Pull requests that touch `docs/` run `mkdocs build --strict` through
`.github/workflows/docs-site.yml`. Pushes to `main` that touch `docs/`
deploy the built site to GitHub Pages.

## Wiki HTML export

`build_wiki.py` renders `../wiki/` into static HTML under `out/` (not
committed) and refuses broken relative Markdown links. It is a standalone
Python 3 script and is not part of the MkDocs build:

```sh
python3 docs/site/build_wiki.py
```
