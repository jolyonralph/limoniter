"""Tells search engines (Bing, Yandex, Seznam, Naver and Yep, through IndexNow) which of the site's pages have changed.

    python web/tools/indexnow.py changed SITE_FOLDER LIST_FILE   (before publishing: the pages that differ from
                                                                  those on the site now, written to LIST_FILE)
    python web/tools/indexnow.py submit LIST_FILE                (after publishing: those pages sent to IndexNow)

The site's key is the 32-hex-digit .txt file in web/, holding its own name, as IndexNow checks. Only pages in the
sitemap are considered, and only those whose contents changed are sent, as IndexNow asks; nothing is sent when none
did. A failure to reach IndexNow is reported but doesn't fail the publish.
"""
import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

SITE = "https://limoniter.com"
WEB = Path(__file__).resolve().parents[1]


def key():
    for path in WEB.glob("*.txt"):
        if re.fullmatch(r"[0-9a-f]{32}", path.stem) and path.read_text().strip() == path.stem:
            return path.stem
    return None


def pages():
    return re.findall(r"<loc>([^<]+)</loc>", (WEB / "sitemap.xml").read_text(encoding="utf-8"))


def local_file(site_folder, url):
    path = url[len(SITE):].lstrip("/") or "index.html"
    return Path(site_folder) / path


def changed(site_folder, list_file):
    found = []
    for url in pages():
        built = local_file(site_folder, url)
        if not built.is_file():
            continue
        try:
            live = urllib.request.urlopen(url, timeout=30).read()
        except (urllib.error.URLError, TimeoutError):
            live = None
        if live != built.read_bytes():
            found.append(url)
    Path(list_file).write_text("\n".join(found) + ("\n" if found else ""), encoding="utf-8")
    print(f"{len(found)} of the site's pages changed")


def submit(list_file):
    urls = [line for line in Path(list_file).read_text(encoding="utf-8").splitlines() if line.strip()]
    the_key = key()
    if not urls or not the_key:
        print("Nothing to tell IndexNow" if the_key else "No IndexNow key in web/")
        return
    body = json.dumps({"host": "limoniter.com", "key": the_key, "keyLocation": f"{SITE}/{the_key}.txt", "urlList": urls}).encode()
    request = urllib.request.Request("https://api.indexnow.org/indexnow", data=body, headers={"Content-Type": "application/json; charset=utf-8"})
    try:
        answer = urllib.request.urlopen(request, timeout=30)
        print(f"IndexNow took {len(urls)} pages ({answer.status})")
    except urllib.error.HTTPError as e:
        print(f"IndexNow said {e.code}: {e.read()[:200]!r}")
    except (urllib.error.URLError, TimeoutError) as e:
        print(f"IndexNow couldn't be reached: {e}")


if __name__ == "__main__":
    if sys.argv[1:2] == ["changed"] and len(sys.argv) == 4:
        changed(sys.argv[2], sys.argv[3])
    elif sys.argv[1:2] == ["submit"] and len(sys.argv) == 3:
        submit(sys.argv[2])
    else:
        print(__doc__)
        sys.exit(2)
