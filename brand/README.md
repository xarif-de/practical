# PractiCal brand

<img src="../build/assets/logo.svg" alt="PractiCal" width="344">

| file | use |
|---|---|
| [build/assets/logo.svg](../build/assets/logo.svg) | the logo: icon + lettering. README, GitLab Pages site header |
| [build/assets/lettering.svg](../build/assets/lettering.svg) | the lettering alone, cut to the letters: the app's top bar (14 px high, as wide as the old text) |
| [build/assets/icon.svg](../build/assets/icon.svg) | the icon alone: favicon = the browser tab (app and site), push notifications, rustdoc |

All three files are generated: `mise run logo` (brand/build.py). Edit the script, not the SVGs.

## Design

- **Icon:** a white outline calendar with one marked day on a rounded square, gradient indigo `#5856d6` (top
  left) → PractiCal blue `#0a84ff` (bottom right). Readable down to 16 px.
- **Lettering:** "Practi" in grey `#7c7c82`, "Cal" in `#0a84ff`. The grey has the same contrast on white (4.15:1)
  and on the app's dark background `#1c1c1e` (4.10:1), so one file works in light and dark mode; both are above
  the WCAG 3:1 minimum for large text.
- **Typeface:** Inter Display ExtraBold, [Inter 4.1](https://github.com/rsms/inter) by Rasmus Andersson, SIL Open
  Font License 1.1. The letters are outlined into paths, so the logo needs no font to display.
- **Proportions:** 64 units high; the lettering is 34 units, its cap height centred on the icon, 14 units after it.
- Keep the logo on a plain light or dark background; do not recolour, stretch or re-letter it.
