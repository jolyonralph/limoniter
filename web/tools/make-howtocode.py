"""Makes the How To Code guides for the website, inside the user guide's template (tools/guide-template.html), with
their pictures and examples copied beside the pages: How To Code v8 (the Amiga), web/howtocode/*.html from
data/howtocode/, and How To Code: the ZX Spectrum, web/howtocode-spectrum/*.html from data/howtocode-spectrum/.

    python web/tools/make-howtocode.py

The publishing workflow runs it, after make-guide.py. It borrows make-guide.py's Markdown, and adds what How To Code
needs: pictures given by their path (![What it shows](images/NAME.png)), links between chapters (05-interrupts.md
becomes 05-interrupts.html), and links to its examples, which are copied as they are. README.md is the front page.
How To Code is CC0, so the pages say so; the rest of the site is Limoniter's.
"""
import ast
import html
import re
import shutil
from pathlib import Path

APP = Path(__file__).resolve().parents[2]
WEB = APP / "web"
# Each guide: its folder in data/ and on the site, and its name.
GUIDES = [("howtocode", "How To Code v8"), ("howtocode-spectrum", "How To Code: the ZX Spectrum")]


def borrow_from_make_guide():
    """make-guide.py's functions, without running it: it writes its pages as soon as it's loaded."""
    path = Path(__file__).with_name("make-guide.py")
    tree = ast.parse(path.read_text(encoding="utf-8"))
    tree.body = [node for node in tree.body if not (isinstance(node, ast.Expr) and isinstance(node.value, ast.Call))]
    space = {"__file__": str(path), "__name__": "make_guide"}
    exec(compile(tree, str(path), "exec"), space)
    return space


GUIDE = borrow_from_make_guide()


def chapters(source):
    """The chapters, in their order: NN-name.md."""
    return sorted(p.stem for p in source.glob("[0-9][0-9]-*.md"))


FIGURES = []


def prepare(markdown):
    """How To Code's Markdown, made into what make-guide.py's reads: each picture a placeholder, put back as HTML
    once the page is made (the guide's Markdown would show HTML as text), and its links to chapters made links to
    their pages."""
    out = []
    for line in markdown.splitlines():
        # The Back / Contents / Next line at a chapter's top and bottom is for reading it in Limoniter; the website
        # has its own.
        if re.match(r"\[(‹|Contents\])", line):
            continue
        picture = re.match(r"!\[([^\]]*)\]\((images/[\w.-]+\.png)\)\s*$", line)
        if picture:
            alt, src = html.escape(picture.group(1)), picture.group(2)
            FIGURES.append(f'<figure class="guide-shot"><img src="{src}" alt="{alt}" loading="lazy"></figure>')
            out.append(f"HTCFIGURE{len(FIGURES) - 1}END")
            continue
        line = re.sub(r"\]\((\d\d-[\w-]+)\.md(#[\w-]+)?\)", r"](\1.html\2)", line)
        line = re.sub(r"\]\(README\.md(#[\w-]+)?\)", r"](index.html\1)", line)
        out.append(line)
    return "\n".join(out)


def convert(markdown):
    """The page's description, title, lead, body and sections. The guide's converter puts the paragraphs before the
    first section in the lead and anything else (a list, a picture) after them, so the part before the first
    section is converted on its own, as a section that's then left unnamed, to keep its order."""
    text = prepare(markdown)
    lines = text.splitlines()
    first = next((i for i, line in enumerate(lines) if line.startswith("## ")), len(lines))
    head, rest = lines[:first], lines[first:]
    title_lines = [line for line in head if line.startswith("# ")]
    intro = [line for line in head if not line.startswith("# ")]
    _, _, _, lead, _ = GUIDE["convert"]("\n".join(["## HTCINTRO"] + intro))
    lead = re.sub(r'<h2 id="htcintro">HTCINTRO</h2>\n?', "", lead)
    description, title, _, body, sections = GUIDE["convert"]("\n".join(title_lines + rest))
    # The pictures, kept apart from the Markdown, come back as they were made.
    fix = lambda text: re.sub(r"(?:<p>)?HTCFIGURE(\d+)END(?:</p>)?", lambda m: FIGURES[int(m.group(1))], text)
    return description, title, italics(fix(lead)), italics(fix(body)), sections


def italics(page):
    """*Words* in italics, as Markdown has them (the site's own pages have none): outside code, and only where the
    asterisks hug the words, so 64*2 and a list's bullets are left alone."""
    parts = re.split(r"(<pre>.*?</pre>|<code>.*?</code>|<[^>]+>)", page, flags=re.S)
    for n, part in enumerate(parts):
        if part and not part.startswith("<"):
            parts[n] = re.sub(r"(?<![\w*])\*([^*\s](?:[^*]*?[^*\s])?)\*(?![\w*])", r"<em>\1</em>", part)
    return "".join(parts)


def first_sentence(lead_html):
    text = re.sub(r"<[^>]+>", "", lead_html)
    text = html.unescape(text).strip()
    return (text.split(". ")[0] + ".") if text else ""


def template(folder, name):
    """The user guide's template, made the guide's: its address, its name, and its licence."""
    text = (WEB / "tools" / "guide-template.html").read_text(encoding="utf-8")
    text = text.replace("https://limoniter.com/guide/", f"https://limoniter.com/{folder}/")
    text = text.replace("by tools/make-guide.py", "by tools/make-howtocode.py")
    text = text.replace('<p class="guide-pages-title">In this guide</p>', f'<p class="guide-pages-title">{html.escape(name)}</p>')
    text = text.replace(
        "Limoniter is free software under the",
        f'{html.escape(name)} is CC0: copy it, change it, use it for anything. Limoniter is free software under the',
    )
    return text


def make(folder, name):
    """One guide's pages, pictures and examples."""
    source = APP / "data" / folder
    out = WEB / folder
    out.mkdir(exist_ok=True)
    order = ["README"] + chapters(source)
    pages = {page: convert((source / f"{page}.md").read_text(encoding="utf-8")) for page in order}
    files = {page: ("index" if page == "README" else page) for page in order}
    page_template = template(folder, name)
    for n, page in enumerate(order):
        description, title, lead, body, sections = pages[page]
        description = description or first_sentence(lead) or title
        sidebar = "\n".join(
            f'<li><a href="{files[other]}.html"{" aria-current=" + chr(34) + "page" + chr(34) if other == page else ""}>{html.escape(pages[other][1])}</a></li>'
            for other in order
        )
        pager = []
        if n > 0:
            back = order[n - 1]
            pager.append(f'<a class="pager-back" href="{files[back]}.html"><span>Back</span>{html.escape(pages[back][1])}</a>')
        if n + 1 < len(order):
            on = order[n + 1]
            pager.append(f'<a class="pager-next" href="{files[on]}.html"><span>Next</span>{html.escape(pages[on][1])}</a>')
        page_title = name if page == "README" else f"{title} · {name}"
        text = page_template
        for key, value in {
            "{{page_title}}": html.escape(page_title),
            "{{title}}": html.escape(title),
            "{{description}}": html.escape(description),
            "{{file}}": f"{files[page]}.html",
            "{{source}}": f"data/{folder}/{page}.md",
            "{{lead}}": lead,
            "{{contents}}": GUIDE["contents"](sections),
            "{{body}}": body,
            "{{sidebar}}": sidebar,
            "{{pager}}": "\n".join(pager),
        }.items():
            text = text.replace(key, value)
        (out / f"{files[page]}.html").write_text(text, encoding="utf-8", newline="\n")
        print(f"Wrote web/{folder}/{files[page]}.html")
    # The pictures and the examples, as they are.
    for sub in ["images", "examples"]:
        target = out / sub
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(source / sub, target)
        print(f"Copied {sub} to web/{folder}/{sub}")


def main():
    for folder, name in GUIDES:
        make(folder, name)


main()
