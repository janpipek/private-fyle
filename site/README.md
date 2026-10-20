# private-fyle user guide

This Hugo site documents the Linux build of **fyle**, the app in the
private-fyle project. It lives separately from `docs/`, which contains
engineering notes.

Install [Hugo](https://gohugo.io/installation/) to build or preview it (tested
with Hugo 0.168.0). No
theme, JavaScript package manager, or network access is needed after Hugo is
installed. From the repository root:

```sh
hugo server --source site
```

Open `http://localhost:1313/private-fyle/` while the server runs.

For a production build:

```sh
hugo --source site --destination /tmp/private-fyle-site --panicOnWarning
```

`site/hugo.toml` sets the public base URL to
`https://janpipek.github.io/private-fyle/`. The generated files can be served
from that path on GitHub Pages. Building does not publish the site; there is
no deployment workflow in this repository. To publish manually, upload the
contents of the output directory to the hosting branch or service that serves
that path. For another host, change `baseURL` before building.

The two images in `static/images/` were captured from the running fyle app,
using temporary example folders and isolated `XDG_CONFIG_HOME` and
`XDG_DATA_HOME` directories. `fyle-panels.png` shows the default two-panel
layout. `fyle-preview.png` shows the two-panels-with-inspector layout with a
Markdown file selected. To capture a new image, build fyle, start it under
Xvfb with scratch XDG directories, size the window with `xdotool`, and use
ImageMagick's `import -window <window-id>`. Do not use your regular fyle
configuration or file history when taking site screenshots.
