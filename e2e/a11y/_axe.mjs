// The a11y specs' axe step (axe-*.spec.mjs): axe-core over a fixed set of states per page and dialog (lib/axe.mjs:
// WCAG 2.2 A/AA + best practices), in the a11y projects (desktop light + dark, phone light). A finding fails unless
// e2e/a11y-allow.txt accepts it (rule + key: the colour pair for contrast, the target without positions and values
// else); a11y-check.mjs then fails on allow-list lines no state showed — the list only shrinks. Two spec files: two
// workers per project (fresh servers per file, lib/test.mjs).
import fs from 'node:fs';
import path from 'node:path';
import { expect, settle, test } from '../lib/test.mjs';
import { FINDINGS, allowList, axe, findings } from '../lib/axe.mjs';

const ALLOW = allowList();
export const phone = testInfo => testInfo.project.name.includes('phone');
const scheme = testInfo => (testInfo.project.name.includes('dark') ? 'dark' : 'light');
export const DEMO = ['alice/5eed0000-0000-4000-8000-000000000002/demo-event-full@xarif.de', '2026-10-13'];

/** `check(name)`: axe over the page as it is now; findings the allow-list does not hold are collected, every finding
 *  is written for a11y-check.mjs. The test fails at its end with the list of new findings. */
export function a11y(title, body) {
  test(title, async ({ page }, testInfo) => {
    const bad = [];
    const dir = path.join(FINDINGS, testInfo.project.name);
    fs.mkdirSync(dir, { recursive: true });
    const check = async name => {
      const t0 = Date.now();
      await settle(page);
      const t1 = Date.now();
      const f = findings(await axe(page), scheme(testInfo));
      if (process.env.A11Y_TIME) console.log(`${testInfo.project.name} ${name}: settle ${t1 - t0} ms, axe ${Date.now() - t1} ms`);
      fs.writeFileSync(path.join(dir, name + '.json'), JSON.stringify([...f.keys()]));
      for (const [k, x] of f) if (!ALLOW.has(k)) bad.push(`${name}: ${k.replace('\t', ' ')} [${x.impact}] ${x.help}\n    ${x.target}  ${x.html.slice(0, 120)}${k.startsWith('color-contrast') ? '\n    ' + x.summary.split('\n').pop() : ''}`);
    };
    await body(page, check, testInfo);
    expect(bad, 'axe findings e2e/a11y-allow.txt does not accept').toEqual([]);
  });
}
export const go = async (page, url, ready) => { await page.goto(url); await page.locator(ready).first().waitFor(); };
