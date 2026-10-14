<div align="center">

<h1><img src="build/assets/logo.svg" alt="PractiCal" width="344"></h1>

**Self-hosted calendar, contacts, and tasks — featuring the fastest, most beautiful web UI you’ve ever seen.**

[![Release](https://img.shields.io/gitlab/v/tag/xarif/docker/practical?sort=semver&label=release&color=0a84ff)](https://gitlab.com/xarif/docker/practical/-/tags)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0--or--later-30b46c)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust-dea584)](build/Cargo.toml)
[![Languages](https://img.shields.io/badge/UI-EN%20·%20DE%20·%20FR%20·%20ES-af52de)](build/locales)

[![Code on GitLab](https://img.shields.io/badge/code-GitLab-fc6d26?logo=gitlab&logoColor=white)](https://gitlab.com/xarif/docker/practical)
[![Code on GitHub](https://img.shields.io/badge/code-GitHub-24292f?logo=github&logoColor=white)](https://github.com/xarif-de/practical)

<img src="docs/images/03-month.webp" alt="PractiCal month view" width="100%">

</div>

PractiCal is a self-hosted web app for calendars, contacts and tasks with [RustiCal](https://github.com/lennart-k/rustical)
as its backend. Phones and desktop apps keep syncing over CalDAV/CardDAV, and the browser gets a complete, fast UI on
the very same data.

## What makes PractiCal special

<table>
<tr>
<td width="50%" valign="top">

✨ **Modern, clean interface**<br>
Light and dark, desktop and phone, trackpad and touch gestures — in English, German, French and Spanish.

</td>
<td width="50%" valign="top">

⚡ **Extremely fast and frugal**<br>
Written in Rust. About 13 MiB of memory and a fraction of a millicore in production; one 27 MB image carries
PractiCal and RustiCal.

</td>
</tr>
<tr>
<td valign="top">

🔎 **Powerful search across everything**<br>
One search box for events, contacts and tasks — with dates, prefixes and a search history.

</td>
<td valign="top">

🧩 **Configurable templates for events and tasks**<br>
Rules per category pre-fill or lock colour, reminders, location, priority and more; earlier events serve as title
templates.

</td>
</tr>
<tr>
<td valign="top">

🗓️ **Great views for calendar, tasks and contacts**<br>
Day, week, month, year planner, free weekends, vacation planner, countdown wall, year in numbers; task groups,
table, board, timeline and cards; contacts as list or table.

</td>
<td valign="top">

🎨 **Smart colour manager**<br>
Every colour in one place, and an optimiser that unifies what belongs together and pulls apart colours that are
hard to tell apart — every change shown with its reason before it is applied.

</td>
</tr>
<tr>
<td valign="top">

🏖️ **Public and school holidays**<br>
Per region, in every view, and counted by the vacation planner.

</td>
<td valign="top">

↩️ **And more**<br>
Change history with undo for every change (also from phones), shared group calendars, app tokens per device,
`.ics`/`.vcf` import and export, push reminders, live updates. [All features →](https://xarif.gitlab.io/docker/practical/features.html)

</td>
</tr>
</table>

<div align="center">

## [Gallery, features and documentation →](https://xarif.gitlab.io/docker/practical/)

</div>

## Try it locally with Docker

One command starts RustiCal and PractiCal in one throwaway container, with a user `demo` (password `demo`):

```bash
docker run --rm -it -p 4001:4001 --platform linux/amd64 \
  -e RUSTICAL_DATA_STORE__SQLITE__DB_URL=/tmp/db.sqlite3 -e PRACTICAL__DB_PATH=/tmp/db.sqlite3 \
  -e PRACTICAL__ALLOW_PASSWORD_LOGIN=true --entrypoint sh registry.gitlab.com/xarif/docker/practical:v0 \
  -c 'rustical principals create demo --for-testing-password-from-arg demo && { rustical serve & } && exec /practical serve'
```

Open <http://localhost:4001> and sign in as `demo` / `demo`. The data lives inside the container and is gone when you
stop it with Ctrl+C.

## Quick start on Kubernetes

One pod runs RustiCal (CalDAV/CardDAV, port 4000) and PractiCal (web UI, port 4001) from the same image on one
SQLite volume.

```bash
# 1. get the manifest and put in your two hostnames (web UI + DAV)
curl -LO https://gitlab.com/xarif/docker/practical/-/raw/main/deploy/kubernetes.yaml
sed -i.bak 's/plan.example.com/plan.your.domain/g; s/dav.example.com/dav.your.domain/g' kubernetes.yaml

# 2. deploy
kubectl create namespace practical
kubectl -n practical apply -f kubernetes.yaml

# 3. create the first user (asks for a password)
kubectl -n practical exec -it deploy/practical -c rustical -- rustical principals create alice --password
```

Open `https://plan.your.domain` and sign in. Calendar apps (DAVx⁵, Thunderbird, Apple Calendar) connect to
`https://dav.your.domain` with an app token from **Settings → App tokens**.

> [!tip]
> The manifest expects TLS at the ingress and a default IngressClass (see the comments in
> [`deploy/kubernetes.yaml`](deploy/kubernetes.yaml)). Single sign-on, push reminders, backups and hardening:
> [deployment](https://xarif.gitlab.io/docker/practical/deployment.html) and [configuration](https://xarif.gitlab.io/docker/practical/configuration.html).

## Why PractiCal?

**tl;dr: I could not find a calendar app for the browser that was as good as I wanted.**

<details>
<summary>For many years …</summary>

For many years I have run [Nextcloud](https://nextcloud.com/) on my private server. I wanted to replace it with
several smaller tools, because a few things about it kept bothering me. The only reason it stayed was its excellent
calendar app.

So I looked at the alternatives: InfCloud, AgenDAV, dayGLANCE, MMDL, SOGo and Radicale. Each of them was either too
ugly, too heavy, or built on technology that felt outdated to me. [RustiCal](https://github.com/lennart-k/rustical)
convinced me as the backend, but it comes without a browser UI.

So I sat down and built a UI prototype together with Claude Opus. Even the first result was so much better than
Nextcloud's calendar that I decided to invest my time, and my money (two months of Claude Max 20x), into a finished
product. Thanks to my perfectionism, my love of detail, my joy in experimenting and the sheer fun of AI-assisted
development in Rust, the result became far more than I had first imagined.

I hope you enjoy it as much as I do. PractiCal is my way of saying thank you to all the great open-source projects I
use every day. Feedback, ideas for improvements, bug reports and contributions are very welcome: please open an
issue or a merge/pull request on [GitLab](https://gitlab.com/xarif/docker/practical/-/issues) or [GitHub](https://github.com/xarif-de/practical/issues).

— Thomas Steinbach

</details>

## lennart-k/RustiCal

PractiCal is only the web frontend. The actual CalDAV/CardDAV server is
[RustiCal](https://github.com/lennart-k/rustical) by lennart-k, a wonderful project in its own right. It stores your
calendars, contacts and tasks, syncs them with your phones and desktop apps, and its binary ships unmodified in the
PractiCal image.

All kudos for the backend belong to RustiCal. So do all backend issues: if something goes wrong with syncing, DAV
clients or the server itself, please report it at
[lennart-k/rustical/issues](https://github.com/lennart-k/rustical/issues). Anything about the web UI belongs
here, on [GitLab](https://gitlab.com/xarif/docker/practical/-/issues) or [GitHub](https://github.com/xarif-de/practical/issues).

## License

PractiCal is written by Thomas Steinbach and licensed under [AGPL-3.0-or-later](LICENSE). It builds on [RustiCal](https://github.com/lennart-k/rustical) (AGPL-3.0-or-later), whose binary ships
unmodified in the image. Details: [licensing](https://xarif.gitlab.io/docker/practical/licensing.html).
