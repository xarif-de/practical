# PractiCal — gallery and documentation

PractiCal is a fast, modern web app for calendars, contacts and tasks on your own server, built on
[RustiCal](https://github.com/lennart-k/rustical). Phones and desktop apps keep syncing over CalDAV/CardDAV; the
browser gets a complete UI on the very same data.

<p><a href="https://gitlab.com/xarif/docker/practical"><b>Source code, image and quick start on GitLab →</b></a></p>

## Gallery

<!-- gallery:start (site/build.mjs puts the screenshot gallery from e2e/demo/shots.json here) -->
The interactive gallery is on the [published site](https://xarif.gitlab.io/docker/practical/). Every screenshot
also appears on the [features](features.md) page.
<!-- gallery:end -->

## Documentation

<!-- docs-index:start -->
| Guide | What's inside |
| --- | --- |
| [Features](features.md) | everything PractiCal does, view by view |
| [Deployment](deployment.md) | production setup: image tags, pod hardening, network, backups |
| [Configuration](configuration.md) | all `PRACTICAL__*` variables, login and SSO, push, holidays, RustiCal settings |
| [Commands](commands.md) | `practical` subcommands, user and group administration with RustiCal's CLI |
| [How it works](architecture.md) | two programs on one database, the image, the RustiCal version contract |
| [Contributing](../CONTRIBUTING.md) | local development, tests and gates, CI and releases |
| [API documentation](https://xarif.gitlab.io/docker/practical/api/practical/index.html) | the Rust code: request flow, every module, invariants, caches (built from `main`) |
| [Repo overview](repo-overview.md) | how the repo is organised: everything outside the Rust API doc |
| [Performance](performance-2026-10.md) | measurements, release policy, known limits |
| [Licensing](licensing.md) | AGPL-3.0, RustiCal, third-party assets |
<!-- docs-index:end -->
