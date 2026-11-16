// Optional browser validation for the HTML study, NOT native Pui evidence.
// PLAYWRIGHT_MODULE=/absolute/path/to/playwright CHROME_BIN=/path/to/chrome node scripts/check-workspace-prototype.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const file = path.resolve(__dirname, '../docs/mockups/pui-workspace.html');
const storage = 'pui-workspace-design-layout-v1';
const widths = [1500, 1100, 900, 620, 390];
(async () => {
  const browser = await chromium.launch({
    ...(process.env.CHROME_BIN ? { executablePath: process.env.CHROME_BIN } : {}),
    headless: true,
  });
  try {
    const context = await browser.newContext({ viewport: { width: 1500, height: 1040 } });
    let page = await context.newPage();
    const errors = [];
    const attach = () => page.on('pageerror', error => errors.push(error.message));
    attach();
    const url = pathToFileURL(file).href;
    await page.goto(url);
    await page.locator('.toolbar [data-action="open-files"]').click();
    await page.locator('.app>.sidebar [data-resize="left"]').focus();
    await page.keyboard.press('Shift+ArrowRight');
    await page.locator('.tool-dock>[data-resize="right"]').focus();
    await page.keyboard.press('Shift+ArrowRight');
    await page.locator('[data-terminal="tests"]').click();
    await page.locator('[data-dock-tab="browser"]').click();
    // Real disclosure click must not be intercepted by the overflow button.
    await page.locator('.app>.sidebar [data-collapse="workflow"]').click();
    await page.locator('#themeButton').click();
    await page.locator('#draft').fill('DRAFT MUST NOT BE PERSISTED');
    await page.waitForTimeout(220);
    const saved = await page.evaluate(key => JSON.parse(localStorage.getItem(key)), storage);
    assert.equal(saved.sidebarWidth, 290);
    assert.equal(saved.terminalTab, 'tests');
    assert.equal(saved.dockTab, 'browser');
    assert.equal(saved.theme, 'light');
    assert.ok(!saved.collapsed.includes('workflow'));
    assert.ok(!JSON.stringify(saved).includes('DRAFT'));
    await page.setViewportSize({ width: 900, height: 1040 });
    await page.locator('#widthButton').click();
    await page.close();
    page = await context.newPage(); attach();
    await page.setViewportSize({ width: 900, height: 1040 });
    await page.goto(url);
    assert.equal(await page.locator('#draft').inputValue(), '');
    assert.equal(await page.locator('[data-dock-tab="browser"]').getAttribute('aria-selected'), 'true');
    assert.equal(await page.locator('#stage').getAttribute('class'), 'stage narrow');
    await page.locator('#widthButton').click();
    await page.setViewportSize({ width: 1500, height: 1040 });
    await page.waitForTimeout(100);
    assert.ok(Math.abs(await page.locator('.tool-dock').evaluate(el => el.getBoundingClientRect().width) - saved.dockWidth) < 2);
    const trigger = () => page.locator('.app>.sidebar [data-action="view-options"]');
    for (const width of widths) {
      await page.setViewportSize({ width, height: 1040 });
      if (width <= 640 && !await trigger().isVisible()) await page.locator('.toolbar [data-action="toggle-sidebar"]').click();
      for (const theme of ['light', 'dark']) {
        await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
        await trigger().click();
        for (const branch of ['group', 'sort', 'show']) {
          if (await page.locator('.view-menu').evaluate(el => getComputedStyle(el).visibility === 'hidden')) await page.locator('[data-view-back]').click();
          await page.locator(`[data-view-branch="${branch}"]`).focus();
          await page.keyboard.press('ArrowRight');
          assert.ok(await trigger().evaluate(el => {
            const r = el.getBoundingClientRect();
            return el.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2));
          }));
        }
        await page.keyboard.press('Escape'); await page.keyboard.press('Escape');
        assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
      }
    }
    await page.setViewportSize({ width: 1500, height: 1040 });
    await trigger().click(); await page.locator('.view-menu [data-action="view-all"]').click();
    const list = page.locator('.app>.sidebar .session-scroll');
    await list.evaluate(el => { el.scrollTop = 0; });
    await page.waitForTimeout(50);
    const thumb = await page.locator('.app>.sidebar .custom-scrollbar-thumb').boundingBox();
    assert.ok(thumb && thumb.height > 0);
    // The custom overlay is inset from the list edge; drag the actual live thumb.
    await page.mouse.move(thumb.x + thumb.width / 2, thumb.y + 12);
    await page.mouse.down();
    await page.mouse.move(thumb.x + thumb.width / 2, thumb.y + 93, { steps: 5 });
    await page.mouse.up();
    assert.ok(await list.evaluate(el => el.scrollTop > 0));
    const track = page.locator('.app>.sidebar .custom-scrollbar');
    await track.focus(); await page.keyboard.press('End');
    assert.ok(await list.evaluate(el => el.scrollTop > 0));
    await page.keyboard.press('Home');
    assert.equal(await list.evaluate(el => el.scrollTop), 0);
    // beforeunload/pagehide normally persist the current live layout. Close it
    // BEFORE replacing storage so that a valid save cannot overwrite the fixture.
    await page.close();
    const storagePage = await context.newPage();
    await storagePage.goto(url);
    await storagePage.evaluate(key => {
      restoringLayout = true;
      clearTimeout(layoutSaveTimer);
      localStorage.setItem(key, JSON.stringify({ version: 1, screen: 'workspace', sidebarWidth: -123, dockWidth: 'bad', sidebarView: { group: 'invalid' } }));
    }, storage);
    await storagePage.close();
    page = await context.newPage(); attach(); await page.goto(url);
    assert.equal(await page.locator('.app>.sidebar').evaluate(el => el.getBoundingClientRect().width), 250);
    assert.equal(await page.evaluate(() => sidebarView.group), 'Folder');
    const opaque = await context.newPage();
    await opaque.setContent('<iframe sandbox="allow-scripts" style="width:1400px;height:900px;border:0"></iframe>');
    await opaque.locator('iframe').evaluate((el, html) => { el.srcdoc = html; }, fs.readFileSync(file, 'utf8'));
    assert.match(await opaque.frameLocator('iframe').locator('#layoutSaveState').innerText(), /unavailable/);
    assert.deepEqual(errors, []);
    console.log('PASS: prototype disclosure, bounded layout restore/storage failures, actual custom thumb+keyboard, menu anchors at 5 widths × 2 themes. Not native Pui validation.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
