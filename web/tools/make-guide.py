"""Makes the user guide, web/guide/*.html, from the Markdown in web/guide/src/, inside tools/guide-template.html.

    python web/tools/make-guide.py

The publishing workflow runs it, after make-pages.py. It borrows make-pages.py's inline formatting and tables,
and adds what a guide needs: numbered steps, notes, code blocks, screenshots, keys and a contents list for the
pages. Each Markdown file starts with a `description:` line, then a blank line, then its `# Title`.

On top of make-pages.py's Markdown it understands:
    1. A numbered step                  (later lines of the step indented by three spaces)
    > A note                            (one paragraph, shown set apart)
    ```                                 (a block of code, kept as it is)
    ![What it shows](NAME "Caption")    (images/NAME-close.webp, enlarging to images/NAME.webp)
    [[Ctrl+S]]                          (a key, as <kbd>)
"""
import ast
import html
import re
from pathlib import Path

APP = Path(__file__).resolve().parents[2]
WEB = APP / "web"
SOURCE = WEB / "guide" / "src"

# The pages in the order they're read, after the guide's own front page (index.md).
PAGES = [
    "getting-started",
    "editing",
    "character-sets",
    "disks-and-tapes",
    "basic",
    "assembling-and-running",
    "debugging",
    "git",
    "remote-files",
    "settings",
]


def borrow_from_make_pages():
    """make-pages.py's functions, without running it: it writes its pages as soon as it's loaded."""
    path = Path(__file__).with_name("make-pages.py")
    tree = ast.parse(path.read_text(encoding="utf-8"))
    tree.body = [node for node in tree.body if not isinstance(node, ast.Expr) or isinstance(node.value, ast.Constant)]
    space = {"__file__": str(path), "__name__": "make_pages"}
    exec(compile(tree, str(path), "exec"), space)
    return space


PAGES_MODULE = borrow_from_make_pages()
table = PAGES_MODULE["table"]


def inline(text):
    out = PAGES_MODULE["inline"](text)
    return re.sub(r"\[\[([^\]]+)\]\]", r"<kbd>\1</kbd>", out)


def slug_of(name):
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


def figure(alt, name, caption):
    alt, caption = html.escape(alt), inline(caption) if caption else ""
    shot = (
        f'<button class="shot" data-full="../images/{name}.webp" aria-label="Enlarge: {alt}">'
        f'<img src="../images/{name}-close.webp" alt="{alt}" loading="lazy" width="1280" height="720"></button>'
    )
    return f'<figure class="guide-shot">{shot}' + (f"<figcaption>{caption}</figcaption>" if caption else "") + "</figure>"


def convert(markdown):
    """The page's description, title, lead, body and its sections."""
    lines = markdown.splitlines()
    description = ""
    if lines and lines[0].startswith("description:"):
        description = lines[0].split(":", 1)[1].strip()
        lines = lines[1:]
    out, i, title, lead, sections = [], 0, "", [], []

    def started():
        return any(o.startswith("<h2") for o in out)

    while i < len(lines):
        line = lines[i]
        if line.startswith("# "):
            title = line[2:].strip()
            i += 1
        elif line.startswith("## "):
            name = line[3:].strip()
            slug = slug_of(name)
            out.append(f'<h2 id="{slug}">{inline(name)}</h2>')
            sections.append((slug, name))
            i += 1
        elif line.startswith("### "):
            out.append(f'<h3 id="{slug_of(line[4:])}">{inline(line[4:].strip())}</h3>')
            i += 1
        elif line.startswith("```"):
            i += 1
            code = []
            while i < len(lines) and not lines[i].startswith("```"):
                code.append(lines[i])
                i += 1
            i += 1
            out.append(f'<div class="code-block"><pre><code>{html.escape(chr(10).join(code), quote=False)}</code></pre></div>')
        elif m := re.match(r'!\[([^\]]*)\]\(([\w-]+)(?: "([^"]*)")?\)\s*$', line):
            out.append(figure(m.group(1), m.group(2), m.group(3)))
            i += 1
        elif line.startswith("|"):
            rows = []
            while i < len(lines) and lines[i].startswith("|"):
                rows.append(lines[i])
                i += 1
            out.append(table(rows))
        elif line.startswith("> "):
            note = []
            while i < len(lines) and lines[i].startswith(">"):
                note.append(lines[i][1:].strip())
                i += 1
            out.append(f'<div class="note"><p>{inline(" ".join(note))}</p></div>')
        elif re.match(r"\d+\. ", line):
            steps = []
            first = int(line.split(".", 1)[0])
            while i < len(lines) and (re.match(r"\d+\. ", lines[i]) or lines[i].startswith("   ")):
                if re.match(r"\d+\. ", lines[i]):
                    steps.append(lines[i].split(". ", 1)[1].strip())
                else:
                    steps[-1] += " " + lines[i].strip()
                i += 1
            # A list carried on after a block of code goes on counting from where it was.
            start = f' start="{first}"' if first != 1 else ""
            out.append(f'<ol class="steps"{start}>' +"".join(f"<li>{inline(s)}</li>" for s in steps) + "</ol>")
        elif line.startswith("- "):
            items = []
            while i < len(lines) and (lines[i].startswith("- ") or lines[i].startswith("  ")):
                if lines[i].startswith("  - "):
                    inner = []
                    while i < len(lines) and lines[i].startswith("  - "):
                        inner.append(f"<li>{inline(lines[i][4:])}</li>")
                        i += 1
                    items[-1] = items[-1].removesuffix("</li>") + "<ul>" + "".join(inner) + "</ul></li>"
                    continue
                if lines[i].startswith("- "):
                    items.append(f"<li>{inline(lines[i][2:])}</li>")
                else:
                    # A point carried on to the next line.
                    items[-1] = items[-1].removesuffix("</li>") + " " + inline(lines[i].strip()) + "</li>"
                i += 1
            out.append("<ul>" + "".join(items) + "</ul>")
        elif line.strip():
            para = []
            while i < len(lines) and lines[i].strip() and not re.match(r"(#|\||- |> |```|!\[|\d+\. )", lines[i]):
                para.append(lines[i].strip())
                i += 1
            (out if started() else lead).append(f"<p>{inline(' '.join(para))}</p>")
        else:
            i += 1
    return description, title, "\n".join(lead), "\n".join(out), sections


def contents(sections):
    if len(sections) < 2:
        return ""
    items = "\n".join(f'<li><a href="#{slug}">{inline(name)}</a></li>' for slug, name in sections)
    return f'<nav class="contents" aria-label="On this page">\n<ul>\n{items}\n</ul>\n</nav>'


def main():
    template = (WEB / "tools" / "guide-template.html").read_text(encoding="utf-8")
    pages = {}
    for name in ["index"] + PAGES:
        pages[name] = convert((SOURCE / f"{name}.md").read_text(encoding="utf-8"))
    order = ["index"] + PAGES

    for n, name in enumerate(order):
        description, title, lead, body, sections = pages[name]
        sidebar = "\n".join(
            f'<li><a href="{other}.html"{" aria-current=" + chr(34) + "page" + chr(34) if other == name else ""}>{html.escape(pages[other][1])}</a></li>'
            for other in order
        )
        pager = []
        if n > 0:
            back = order[n - 1]
            pager.append(f'<a class="pager-back" href="{back}.html"><span>Back</span>{html.escape(pages[back][1])}</a>')
        if n + 1 < len(order):
            on = order[n + 1]
            pager.append(f'<a class="pager-next" href="{on}.html"><span>Next</span>{html.escape(pages[on][1])}</a>')
        if name == "index":
            # The front page lists every page, with what it covers.
            body += '\n<ol class="guide-list">\n' + "\n".join(
                f'<li><a href="{p}.html">{html.escape(pages[p][1])}</a><p>{html.escape(pages[p][0])}</p></li>' for p in PAGES
            ) + "\n</ol>"
        page_title = "Limoniter user guide" if name == "index" else f"{title} · Limoniter user guide"
        text = template
        for key, value in {
            "{{page_title}}": html.escape(page_title),
            "{{title}}": html.escape(title),
            "{{description}}": html.escape(description),
            "{{file}}": f"{name}.html",
            "{{source}}": f"guide/src/{name}.md",
            "{{lead}}": lead,
            "{{contents}}": contents(sections),
            "{{body}}": body,
            "{{sidebar}}": sidebar,
            "{{pager}}": "\n".join(pager),
        }.items():
            text = text.replace(key, value)
        (WEB / "guide" / f"{name}.html").write_text(text, encoding="utf-8", newline="\n")
        print(f"Wrote web/guide/{name}.html")


main()
