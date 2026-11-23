# The introduction site

The pages published at https://hypertetrahedron.github.io/imperatorium/: an
introduction for new users, grouped by function area. The full reference stays
in the top-level `README.md`.

Plain HTML and one stylesheet, no build step. Each page carries the same
header, nav and footer, so a new page means adding its link to every page's
`<nav>` and to the overview's cards.

`.github/workflows/pages.yml` publishes it on a push to `main` that touches
`site/`, `docs/images/` or `imperatorium/fonts/`. It copies the screenshots
(`docs/images` -> `images/`) and the window's fonts (`imperatorium/fonts` ->
`fonts/`) in beside the pages, and fails if a page links to a file that is not
there. To preview locally, do the same:

```
mkdir -p _site/images _site/fonts
cp site/*.html site/style.css _site/
cp docs/images/*.png _site/images/ && cp imperatorium/fonts/*.woff2 _site/fonts/
```
