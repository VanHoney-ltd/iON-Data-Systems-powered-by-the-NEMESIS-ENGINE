#!/usr/bin/env python3
"""Render legal Markdown files with the iON legal-paper page art."""

from __future__ import annotations

import html
import re
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "docs" / "legal"
ASSET_DIR = OUT_DIR / "assets"
PAGE_ART = ROOT / "ion" / "frontend" / "public" / "templates" / "ion-legal-paper"
LEGAL_DOCS = [
    ("EULA.md", "iON End User License Agreement", "EULA"),
    ("LICENSE.txt", "iON Source-Available View-Only License", "LICENSE"),
]


def inline_code(text: str) -> str:
    escaped = html.escape(text)
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", escaped)


def render_markdown(markdown: str) -> str:
    blocks: list[str] = []
    list_items: list[str] = []

    def flush_list() -> None:
        nonlocal list_items
        if list_items:
            blocks.append("<ul>\n" + "\n".join(list_items) + "\n</ul>")
            list_items = []

    for raw_line in markdown.splitlines():
        line = raw_line.strip()

        if not line:
            flush_list()
            continue

        if line.startswith("# "):
            flush_list()
            blocks.append(f"<h1>{inline_code(line[2:].strip())}</h1>")
        elif line.startswith("## "):
            flush_list()
            blocks.append(f"<h2>{inline_code(line[3:].strip())}</h2>")
        elif line.startswith("- "):
            list_items.append(f"<li>{inline_code(line[2:].strip())}</li>")
        else:
            flush_list()
            blocks.append(f"<p>{inline_code(line)}</p>")

    flush_list()
    return "\n".join(blocks)


def copy_assets() -> None:
    ASSET_DIR.mkdir(parents=True, exist_ok=True)
    for asset_name in [
        "iON-Legal-Paper_html_dbaefd42.png",
        "iON-Legal-Paper_html_9c1fd761.png",
    ]:
        shutil.copy2(PAGE_ART / asset_name, ASSET_DIR / asset_name)


def html_document(title: str, body: str) -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{html.escape(title)}</title>
  <style>
    @page {{
      size: letter;
      margin: 1.82in 1.25in 1in 1.25in;
      @bottom-center {{
        content: "iON Data Management Systems";
        color: #69737d;
        font-size: 8pt;
        letter-spacing: 0;
      }}
    }}

    html {{
      color: #111820;
      font-family: "Liberation Serif", "Times New Roman", serif;
      font-size: 11pt;
      line-height: 1.42;
    }}

    body {{
      margin: 0;
    }}

    .header-art {{
      position: fixed;
      top: -1.58in;
      left: 50%;
      width: 3.65in;
      transform: translateX(-50%);
      opacity: 0.95;
      z-index: -1;
    }}

    .page-art {{
      position: fixed;
      right: -0.12in;
      bottom: 0;
      width: 1.45in;
      opacity: 0.32;
      z-index: -1;
    }}

    main {{
      max-width: 6in;
      margin: 0 auto;
    }}

    h1 {{
      font-family: "Liberation Sans", Arial, sans-serif;
      font-size: 18pt;
      margin: 0 0 0.22in;
      color: #101820;
      letter-spacing: 0;
    }}

    h2 {{
      font-family: "Liberation Sans", Arial, sans-serif;
      font-size: 12.5pt;
      margin: 0.22in 0 0.08in;
      color: #1f3445;
      letter-spacing: 0;
    }}

    p {{
      margin: 0 0 0.1in;
    }}

    ul {{
      margin: 0.04in 0 0.12in 0.22in;
      padding: 0;
    }}

    li {{
      margin: 0 0 0.06in;
    }}

    code {{
      font-family: "Liberation Mono", monospace;
      font-size: 9.5pt;
    }}
  </style>
</head>
<body>
  <img class="header-art" src="assets/iON-Legal-Paper_html_dbaefd42.png" alt="">
  <img class="page-art" src="assets/iON-Legal-Paper_html_9c1fd761.png" alt="">
  <main>
{body}
  </main>
</body>
</html>
"""


def render_doc(source_name: str, title: str, output_stem: str) -> Path:
    source = ROOT / source_name
    body = render_markdown(source.read_text(encoding="utf-8").replace("\u00a0", " "))
    output = OUT_DIR / f"{output_stem}.html"
    output.write_text(html_document(title, body), encoding="utf-8")
    return output


def render_pdf(html_path: Path) -> None:
    pdf_path = html_path.with_suffix(".pdf")
    subprocess.run(["weasyprint", str(html_path), str(pdf_path)], check=True)


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    copy_assets()
    for source_name, title, output_stem in LEGAL_DOCS:
        html_path = render_doc(source_name, title, output_stem)
        render_pdf(html_path)
        print(html_path)
        print(html_path.with_suffix(".pdf"))


if __name__ == "__main__":
    main()
