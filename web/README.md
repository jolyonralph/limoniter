# Limoniter's website

The promotional site for https://limoniter.com, in plain HTML, CSS and JavaScript, with no build step.

- `index.html`: the page.
- `css/style.css`: all the styling. Colours are set once at the top, for dark, and again for light.
- `js/main.js`: the cycling word in the heading, the machine picker (its facts are in the `machines` list), enlarged screenshots, the copy button and the small-screen menu. The page still reads with JavaScript off.
- `compare.html`, `features.html`, `formats.html` and `whats-new.html`: made from `../COMPARISON.md`, `../FEATURES.md`,
  `../FORMATS.md` and `../WHATS-NEW.md` by `tools/make-pages.py`, inside the templates of the same names in `tools/`.
  Publishing makes them afresh; to see a change before then, run `python web/tools/make-pages.py` from the app folder.
  Don't edit the four pages themselves.
- **What's new** shows the latest five releases in full and older ones by their first paragraph, with a link to
  their full notes on GitHub, so each release's first paragraph is written to stand alone. With each release, put its
  notes at the top of `WHATS-NEW.md` (as in `.github/release-notes.md`, without the downloads), under `## VERSION`
  and a `Released DAY MONTH YEAR.` line. Making the pages stops with an error if the version in `Cargo.toml` has no
  section there, so the site can't be published without it.
- `guide/`: the user guide, made from the Markdown in `guide/src/` by `tools/make-guide.py`, inside
  `tools/guide-template.html`. The order of the pages is set in the script. Publishing makes them afresh; to see a
  change before then, run `python web/tools/make-guide.py` from the app folder. Don't edit the pages themselves.
- `howtocode/`: How To Code v8, the Amiga demo coders' guide that's built into Limoniter, made from
  `../data/howtocode/` by `tools/make-howtocode.py` inside the user guide's template, with its pictures and examples
  copied beside the pages. It's CC0, and its pages say so. Publishing makes them afresh; to see a change before then,
  run `python web/tools/make-howtocode.py` from the app folder. Don't edit the pages themselves.
- `images/`: made by `tools/make-images.py SCREENSHOTS_FOLDER` from full-size screenshots: each as a whole window
  (`NAME.webp`) and a readable part of it (`NAME-close.webp`), the themes collage, and `social.png` for shared links.
- `images/`: the icon and screenshots.
- `robots.txt` and `sitemap.xml`: for search engines. The canonical address, and the links in social-media previews, are https://limoniter.com/.

## Looking at it

Open `index.html` in a browser, or serve the folder:

    python -m http.server --directory web 8000

then go to http://localhost:8000.

## Screenshots

The screenshots are taken by Limoniter itself:

    limoniter.exe FILE --screenshot OUT.png

`LIMONITER_SHOT_STEPS` lists steps to take first, separated by commas, such as `preview`, `devices` or `send`
(see `screenshot_steps` in `src/app.rs`). Only use files that are yours, or that are free to show: no one else's
artwork, and no paths or names from your own computer.

Publishing the site is up to you. The folder can be served as it is by any web host. On GitHub Pages, add a file named `CNAME` containing `limoniter.com`, and point the domain's DNS at GitHub.
