# Governance

How Mister Download Manager (MDM) is run today: who decides, how a change gets in, and how a
release is made. It describes what happens now, not a plan.

## Roles

- **Maintainer:** Ahmed Nefzaoui ([@anefzaoui](https://github.com/anefzaoui)) created MDM and
  maintains it alone. He is the only person who can merge pull requests, push version tags and
  publish releases, and the one who reviews and approves every change.
- **Contributors:** anyone who reports a bug, suggests an idea or sends a pull request. How to
  build MDM and what CI checks are in [CONTRIBUTING.md](CONTRIBUTING.md).
- **Translators:** contributors who review a language. The catalogs start as machine
  translations; a native speaker fixes a row and removes its `#, machine` mark, and from then on
  that row is theirs: it is never machine-translated again
  ([Translations](CONTRIBUTING.md#translations)).

Everyone who takes part follows the [Code of conduct](CODE_OF_CONDUCT.md).

## How decisions are made

The maintainer decides. Proposals and discussion happen in the open:
[Issues](https://github.com/anefzaoui/misterdownloadmanager/issues) for bugs,
[Discussions](https://github.com/anefzaoui/misterdownloadmanager/discussions/categories/ideas) for
ideas, and pull requests for concrete changes. What is planned, being built and done is in
[ROADMAP.md](ROADMAP.md).

Decisions follow MDM's documented principles:

- **Clean room.** MDM contains no code or assets from Internet Download Manager or any other
  download manager, and nothing is ever taken apart
  ([Not a reverse-engineering project](README.md#not-a-reverse-engineering-project)).
- **Privacy.** No telemetry, no analytics, no ads; MDM sends nothing about you to the project
  ([privacy policy](https://misterdownloadmanager.com/privacy.html)).
- **The classic layout.** Nothing new to learn for people who know IDM: the toolbar, the
  categories, the list of downloads and the progress window are where they expect them
  ([Why switch from IDM?](README.md#why-switch-from-idm)).
- **Free for everyone.** No paid edition, and nothing in the app is locked
  ([Support MDM](README.md#support-mdm)).

A request that goes against them is declined, with the reason.

## How changes land

Every change, the maintainer's included, goes through a pull request:

1. CI runs the checks listed in [CONTRIBUTING.md](CONTRIBUTING.md#before-you-open-a-pull-request)
   (formatting, lints, tests, the translation catalogs, the browser extension's tests).
2. The maintainer reviews it against [REVIEW.md](REVIEW.md), read from `main`, never from the pull
   request under review. A pull request that changes the review rules, the CI workflows or the
   tests gets the stricter review.
3. The change is run the way a user runs it, on real systems (Windows and Linux), not only through
   its tests. The pull request says how it was checked (`Verified:`) and what wasn't
   (`Not verified:`).
4. It is merged as one commit, at the commit that was reviewed.

Code from outside contributors that touches the build, CI or tests is read line by line, and runs
only in CI or on an isolated test machine before it is merged.

## Releases

The maintainer decides when a version is released and pushes its tag. GitHub Actions then builds
every download from the public source code, on GitHub's servers, never on a personal computer, and
publishes the release with its notes from [CHANGELOG.md](CHANGELOG.md).

Release files are signed: each release's list of checksums is signed with MDM's release key, so
anyone can check a download ([Check your download](https://misterdownloadmanager.com/download.html#check-your-download)).
Which Windows files are code-signed, and who approves each signature, is in the
[code signing policy](https://misterdownloadmanager.com/code-signing-policy.html).

## Security

Security problems are reported privately, never in a public issue, and a fix is released before
the details are made public. How to report one: [SECURITY.md](SECURITY.md).

## AI-assisted development

Much of MDM's code is written with AI coding agents (Claude Code), working under the maintainer's
direction. The maintainer decides what gets built, reviews every change and verifies it on real
systems before it is merged, the same way as any other pull request (above). Commits written this
way say so in a `Co-Authored-By` line. Whoever or whatever wrote a change, the maintainer is
responsible for what is merged.

## Funding

What MDM needs and how to support it is in [funding.json](funding.json), and
[GitHub Sponsors](https://github.com/sponsors/anefzaoui) takes any amount. Support never unlocks
anything in the app. Money given for a specific purpose, such as a grant, is spent on the work it
was given for.

## If the maintainer is unavailable

MDM is a single-maintainer project. If the maintainer can't continue, no one else can merge or
release in this repository, and there is no named successor. The code is free software under the
[GNU GPL, version 3 or later](LICENSE), so anyone can fork it and carry it on as their own project,
with their own releases.

## Changing this document

Like everything else, by pull request, decided by the maintainer. When the way MDM is run changes,
this file changes in the same pull request.
