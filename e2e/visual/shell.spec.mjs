// The shared shell: avatar menu (settings, source, versions, logout). The tab bar (phone), the scheme button
// and the toast are part of every page / the settings spec.
import { shot } from '../lib/visual.mjs';

shot('shell avatar menu', async (page, snap) => {
  await page.goto('/contacts');
  await page.locator('.filter.user > .avatar').click();
  await page.locator('.filter.user.open').waitFor();
  await snap('sh-avatar-menu', { segments: 0 });
});
