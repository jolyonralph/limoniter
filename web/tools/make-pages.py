"""Makes the site's pages from the Markdown kept with the source, so they're pages of the site in its own style:
web/compare.html from COMPARISON.md, and web/features.html from FEATURES.md.

    python web/tools/make-pages.py

The publishing workflow runs it, so the pages are always as the Markdown is. It understands only what those files
use: headings, paragraphs, bullet lists (one level in), tables, **bold**, `code` and [links](...).
"""
import html
import re
from pathlib import Path

APP = Path(__file__).resolve().parents[2]
PAGES = [("COMPARISON.md", "compare-template.html", "compare.html"), ("FEATURES.md", "features-template.html", "features.html")]

MARKS = {"✓": "yes", "◐": "part", "✗": "no", "?": "unknown"}


def inline(text):
    """Bold, code and links, with everything else escaped."""
    out = html.escape(text, quote=False)
    out = re.sub(r"`([^`]+)`", r"<code>\1</code>", out)
    out = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", out)
    out = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", lambda m: f'<a href="{html.escape(m.group(2))}">{m.group(1)}</a>', out)
    return out


def cell(text, header=False, ours=False):
    """A table cell; one that starts with a mark is coloured by it, and keeps any words after it."""
    text = text.strip()
    classes = ["ours"] if ours else []
    if header:
        return f'<th scope="col"{class_of(classes)}>{inline(text)}</th>'
    mark = text[:1]
    if mark in MARKS:
        rest = text[1:].strip()
        words = {"yes": "Yes", "part": "In part", "no": "No", "unknown": "Not checked"}[MARKS[mark]]
        label = f'<span class="mark" aria-label="{words}">{mark}</span>'
        return f'<td{class_of(classes + [MARKS[mark]])}>{label}{" " + inline(rest) if rest else ""}</td>'
    return f"<td{class_of(classes)}>{inline(text)}</td>"


def class_of(classes):
    return f' class="{" ".join(classes)}"' if classes else ""


def table(rows):
    cells = [[c for c in row.strip().strip("|").split("|")] for row in rows]
    head, body = cells[0], cells[2:]
    # Limoniter's own column, or in a table of editors its own row, stands out.
    column = next((n for n, c in enumerate(head) if c.strip() == "Limoniter"), None)
    out = ['<div class="table-scroll" tabindex="0"><table class="compare">', "<thead><tr>"]
    out += [cell(c, header=True, ours=n == column) for n, c in enumerate(head)]
    out.append("</tr></thead><tbody>")
    for row in body:
        ours_row = "Limoniter" in row[0]
        out.append('<tr class="ours">' if ours_row else "<tr>")
        out.append(f'<th scope="row">{inline(row[0].strip())}</th>')
        out += [cell(c, ours=n + 1 == column) for n, c in enumerate(row[1:])]
        out.append("</tr>")
    out.append("</tbody></table></div>")
    return "\n".join(out)


def convert(markdown):
    lines = markdown.splitlines()
    out, i, title, lead, sections = [], 0, "", [], []
    while i < len(lines):
        line = lines[i]
        if line.startswith("# "):
            title = line[2:].strip()
            i += 1
        elif line.startswith("## "):
            name = line[3:].strip()
            slug = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
            out.append(f'<h2 id="{slug}">{inline(name)}</h2>')
            sections.append((slug, name))
            i += 1
        elif line.startswith("### "):
            out.append(f"<h3>{inline(line[4:].strip())}</h3>")
            i += 1
        elif line.startswith("|"):
            rows = []
            while i < len(lines) and lines[i].startswith("|"):
                rows.append(lines[i])
                i += 1
            out.append(table(rows))
        elif line.startswith("- "):
            items = []
            while i < len(lines) and (lines[i].startswith("- ") or lines[i].startswith("  - ")):
                if lines[i].startswith("  - "):
                    # A point under the one before it.
                    inner = []
                    while i < len(lines) and lines[i].startswith("  - "):
                        inner.append(f"<li>{inline(lines[i][4:])}</li>")
                        i += 1
                    items[-1] = items[-1].removesuffix("</li>") + "<ul>" + "".join(inner) + "</ul></li>"
                    continue
                items.append(f"<li>{inline(lines[i][2:])}</li>")
                i += 1
            out.append("<ul>" + "".join(items) + "</ul>")
        elif line.strip():
            para = []
            while i < len(lines) and lines[i].strip() and not lines[i].startswith(("#", "|", "- ")):
                para.append(lines[i].strip())
                i += 1
            joined = " ".join(para)
            # The key is shown as a coloured legend instead.
            if joined.startswith("**Key:**"):
                continue
            text = f"<p>{inline(joined)}</p>"
            # The paragraphs before the first section introduce the page.
            (lead if not any(o.startswith("<h2") for o in out) else out).append(text)
        else:
            i += 1
    toc = "\n".join(f'<li><a href="#{slug}">{inline(name)}</a></li>' for slug, name in sections)
    return title, "\n".join(lead), "\n".join(out), toc


def main():
    for source, template, page in PAGES:
        title, lead, body, toc = convert((APP / source).read_text(encoding="utf-8"))
        text = (APP / "web" / "tools" / template).read_text(encoding="utf-8")
        text = text.replace("{{title}}", html.escape(title)).replace("{{lead}}", lead).replace("{{body}}", body).replace("{{toc}}", toc)
        (APP / "web" / page).write_text(text, encoding="utf-8", newline="\n")
        print(f"Wrote web/{page}")


main()
