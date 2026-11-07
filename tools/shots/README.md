# The help pages' pictures

Every picture in `docs/images/help/` is made by `make.py` from one list, `shots.toml`. Nothing
there is drawn, cropped or annotated by hand, so the final pictures for a release (#186) are one
command away.

## Taking them all again

On Windows, from the repository's root:

```powershell
cargo build --release -p mdm-app -p mdm-labserver
python tools/shots/make.py
python tools/shots/make.py check
```

`make.py` (no step named: `shoot`, then `build`) runs `tools/lab/docs-shots.ps1` twice, light
then dark, with every window the list needs:

- its own MDM (its own data folder and download folder, `MDM_DATA_DIR`), off the screens
  (`MDM_OFFSCREEN=1`: no window, taskbar button, tray icon or sound on the screen) and its own lab
  server, whose neutral files fill the list;
- each window opened by MDM's own command line (`/show <window>`), never by a click or a key;
- each window's controls read from its accessibility tree (`tools/lab/ui-rects.ps1`), so the
  list rings and numbers controls by name and a moved button needs no new coordinates.

The raw pictures stay in `target/shots/raw/` (light and dark). `make.py build` crops and
annotates them again without shooting (to try a crop); `--only add-url,scheduler` limits a run
to some shots. Options: `--bin` (where the release build is; `CARGO_TARGET_DIR` is followed) and
`--work` (the run's folder, whose path shows in a few windows: keep it short and neutral, such as
`C:\Demo`, on a machine where that folder is free).

The output is lossless WebP (`cwebp -lossless -z 9 -metadata none` when `cwebp` is on PATH,
Pillow's lossless WebP otherwise), at 1x, `<id>.webp` and `<id>-dark.webp`. `make.py check` fails
when a page lacks one of its pictures, an alt text or caption differs from the list, a file is
left over, or a picture is over 300 KB (#186's budget).

## Every picture for a release, from GitHub's runner

The published pictures are taken on a stock Windows, so the file icons in them are Windows' own
and not those of programs someone installed: push the branch to `docs-shots/<name>` (or run
**Actions → Docs screenshots**). The workflow takes the README's and the docs' windows light and
dark (`tools/lab/docs-shots.ps1`), a video's and a torrent's progress window
(`tools/lab/progress-shots.ps1`, its own yt-dlp and a swarm on the runner) and this list's raw
pictures, and returns them as the `docs-shots` artifact. Then, from the repository's root:

```powershell
gh run download <run id> -n docs-shots -D target\docs-shots-run
python tools/shots/make.py docs target\docs-shots-run                  # docs/images/*.webp
python -I tools/website/screens.py target\docs-shots-run\site-dark --light target\docs-shots-run\docs
Copy-Item -Recurse -Force target\docs-shots-run\help-raw\* target\shots\raw\
python tools/shots/make.py build                                        # docs/images/help/*.webp
python tools/shots/make.py check
```

`make.py docs` also takes single PNGs, each as `docs/images/<its name>.webp`: the browser's
pictures (`node tools/readme/browser-shots.mjs`, a real Chrome window, so not on the runner) and
the phone's (`tools/lab/remote-page.mjs`), with `--png <file>` once for each.

`make.py check` keeps every picture in `docs/images` and the site's `website/assets/screens`,
`browser` and `win` within the budget too: none over 300 KB, no PNG where lossless WebP belongs
(only the social card, the logo and the README's icons stay PNG), and none that nothing shows. CI
runs it (`help-pictures.yml`).

The main window's title shows the version: take the pictures again once the version is the
release's.

## Adding a picture

1. Add a `[[shot]]` to `shots.toml` (its header says what each key does).
2. `python tools/shots/make.py --only <id>`, then look at `docs/images/help/<id>.webp`.
3. `python tools/shots/make.py snippet <id>` prints its Markdown; paste it into the page under
   its heading.
4. `python tools/website/translate_site.py --all` translates its alt text and caption.

## In the Markdown

A picture and its caption, readable on GitHub as they are:

```markdown
![What it shows](images/help/add-url.webp)
*The caption.*
```

A step strip, a flow in two to four pictures (`<!-- steps -->`, then a blank line, then a
numbered list of a picture and its caption each):

```markdown
<!-- steps -->

1. ![What it shows](images/help/first-file-info.webp)
   *What to do.*
2. ![What it shows](images/help/first-progress.webp)
   *What to do next.*
```

`<!-- pictures -->` before a bulleted list does the same without numbers (pictures side by side
that aren't steps, such as the themes). The site (`tools/website/build.py`) makes each a
figure with its caption, with the picture's width and height (no layout shift), lazy loading
after a page's first picture, and the night version beside it, shown with the site's dark theme
(the visitor's system choice, or the header's switch). Clicking a picture opens it larger.

## Linux

The list's pictures are Windows'. Linux differs where the desktop takes part (the tray, the
file chooser, the window frames), and those can't be taken on a virtual screen: the WSL2 lab's
Xvfb has no tray host and no desktop portal (`tools/lab/wsl/README.md`). The Linux pictures
(`docs/images/*-linux.webp`, the site's `website/assets/screens/*-linux-*.webp`) are taken on a
real desktop in the lab's virtual machines instead: `tools/lab/linux-vm/docs-shots-host.ps1`
(its header says how), GNOME for the whole list, KDE Plasma, Cinnamon and Xfce for their main
window.
