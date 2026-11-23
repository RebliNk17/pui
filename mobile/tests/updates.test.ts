import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  compareSemver,
  isNewerVersion,
  parseGithubRelease,
  parseSemver,
} from '../src/utils/updates.ts';

describe('mobile update version comparison and release parsing', () => {
  it('parses and compares semver strings with or without leading v', () => {
    assert.deepEqual(parseSemver('v1.2.3'), [1, 2, 3]);
    assert.deepEqual(parseSemver('0.2.0-beta.1'), [0, 2, 0]);
    assert.equal(parseSemver('not-a-version'), null);

    assert.equal(compareSemver('0.2.0', '0.1.9') > 0, true);
    assert.equal(compareSemver('v1.0.0', '1.0.0'), 0);
    assert.equal(isNewerVersion('0.1.1', '0.1.0'), true);
    assert.equal(isNewerVersion('0.1.0', '0.1.0'), false);
    assert.equal(isNewerVersion('0.0.9', '0.1.0'), false);
  });

  it('extracts pui-android.apk asset URL from a GitHub release payload', () => {
    const rel = parseGithubRelease({
      tag_name: 'v0.3.0',
      html_url: 'https://github.com/RebliNk17/pui/releases/tag/v0.3.0',
      body: 'Mobile polish and rate limits',
      published_at: '2026-04-20T12:00:00Z',
      draft: false,
      prerelease: false,
      assets: [
        {
          name: 'pui-macos.dmg',
          browser_download_url: 'https://example.com/pui-macos.dmg',
        },
        {
          name: 'pui-android.apk',
          browser_download_url: 'https://example.com/pui-android.apk',
        },
      ],
    });
    assert.ok(rel);
    assert.equal(rel.version, '0.3.0');
    assert.equal(rel.apkUrl, 'https://example.com/pui-android.apk');
  });

  it('ignores draft releases', () => {
    const rel = parseGithubRelease({
      tag_name: 'v0.4.0',
      draft: true,
      assets: [],
    });
    assert.equal(rel, null);
  });
});
