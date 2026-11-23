export const GITHUB_REPO = 'RebliNk17/pui';
export const ANDROID_APK_ASSET_NAME = 'pui-android.apk';

export interface ReleaseInfo {
  tagName: string;
  version: string;
  releaseUrl: string;
  apkUrl: string | null;
  notes: string;
  publishedAt: string | null;
}

export interface UpdateCheckResult {
  currentVersion: string;
  latest: ReleaseInfo | null;
  updateAvailable: boolean;
  checkedAt: number;
  error?: string;
}

export function getPlayStoreUrl(packageId = 'com.reblink17.pui'): string {
  return `https://play.google.com/store/apps/details?id=${encodeURIComponent(packageId)}`;
}

/** Parses `v1.2.3`, `1.2.3-beta.1`, or `0.1.0` into `[major, minor, patch]`. */
export function parseSemver(input: string): [number, number, number] | null {
  const cleaned = input.trim().replace(/^v/i, '').split('-')[0].split('+')[0];
  const parts = cleaned.split('.');
  if (parts.length < 2 || parts.length > 3) return null;
  const nums = parts.map((p) => Number(p));
  if (nums.some((n) => !Number.isInteger(n) || n < 0)) return null;
  return [nums[0] ?? 0, nums[1] ?? 0, nums[2] ?? 0];
}

/** Returns >0 if `a > b`, <0 if `a < b`, 0 if equal (or unparseable). */
export function compareSemver(a: string, b: string): number {
  const pa = parseSemver(a);
  const pb = parseSemver(b);
  if (!pa || !pb) return 0;
  for (let i = 0; i < 3; i += 1) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

export function isNewerVersion(candidate: string, current: string): boolean {
  return compareSemver(candidate, current) > 0;
}

interface RawGithubAsset {
  name?: unknown;
  browser_download_url?: unknown;
}

interface RawGithubRelease {
  tag_name?: unknown;
  html_url?: unknown;
  body?: unknown;
  published_at?: unknown;
  draft?: unknown;
  prerelease?: unknown;
  assets?: unknown;
}

export function parseGithubRelease(raw: RawGithubRelease): ReleaseInfo | null {
  if (!raw || raw.draft === true) return null;
  const tagName = typeof raw.tag_name === 'string' ? raw.tag_name.trim() : '';
  const releaseUrl = typeof raw.html_url === 'string' ? raw.html_url.trim() : '';
  if (!tagName || !parseSemver(tagName)) return null;

  const assets = Array.isArray(raw.assets) ? (raw.assets as RawGithubAsset[]) : [];
  const apkAsset =
    assets.find(
      (a) =>
        typeof a?.name === 'string' &&
        a.name.toLowerCase().startsWith('pui-android') &&
        a.name.toLowerCase().endsWith('.apk') &&
        typeof a?.browser_download_url === 'string',
    ) ??
    assets.find(
      (a) =>
        typeof a?.name === 'string' &&
        a.name.toLowerCase().endsWith('.apk') &&
        typeof a?.browser_download_url === 'string',
    );

  return {
    tagName,
    version: tagName.replace(/^v/i, ''),
    releaseUrl: releaseUrl || `https://github.com/${GITHUB_REPO}/releases/tag/${tagName}`,
    apkUrl:
      typeof apkAsset?.browser_download_url === 'string'
        ? apkAsset.browser_download_url
        : null,
    notes: typeof raw.body === 'string' ? raw.body.trim() : '',
    publishedAt: typeof raw.published_at === 'string' ? raw.published_at : null,
  };
}

export async function checkForMobileUpdate(
  currentVersion: string,
  repo = GITHUB_REPO,
): Promise<UpdateCheckResult> {
  const checkedAt = Date.now();
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 8000);
    let res: Response;
    try {
      res = await fetch(`https://api.github.com/repos/${repo}/releases?per_page=10`, {
        headers: {
          Accept: 'application/vnd.github+json',
          'User-Agent': 'pui-mobile',
        },
        signal: controller.signal,
      });
    } finally {
      clearTimeout(timer);
    }

    if (!res.ok) {
      return {
        currentVersion,
        latest: null,
        updateAvailable: false,
        checkedAt,
        error: `GitHub API returned HTTP ${res.status}`,
      };
    }

    const list = (await res.json()) as RawGithubRelease[];
    const parsed = (Array.isArray(list) ? list : [])
      .filter((r) => !r?.draft && !r?.prerelease)
      .map(parseGithubRelease)
      .filter((r): r is ReleaseInfo => r !== null);

    // Prefer the latest release that ships an Android APK; fall back to latest tagged release.
    const withApk = parsed.find((r) => Boolean(r.apkUrl));
    const latest = withApk ?? parsed[0] ?? null;
    const updateAvailable = Boolean(latest && isNewerVersion(latest.version, currentVersion));

    return {
      currentVersion,
      latest,
      updateAvailable,
      checkedAt,
    };
  } catch (err: unknown) {
    return {
      currentVersion,
      latest: null,
      updateAvailable: false,
      checkedAt,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}
