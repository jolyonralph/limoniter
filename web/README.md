# Limoniter's website

The promotional site for https://limoniter.com, in plain HTML, CSS and JavaScript, with no build step.

- `index.html`: the page.
- `css/style.css`: all the styling. Colours are set once at the top, for dark, and again for light.
- `js/main.js`: the cycling word in the heading, the machine picker (its facts are in the `machines` list), enlarged screenshots, the copy button and the small-screen menu. The page still reads with JavaScript off.
- `compare.html`: the comparison with other editors, made from `../COMPARISON.md` by `tools/make-compare.py`
  (with `tools/compare-template.html` around it). After changing COMPARISON.md, run `python web/tools/make-compare.py`
  from the app folder. Don't edit compare.html itself.
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
