import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const IGNORED_DIRS = new Set([
  '.git',
  'node_modules',
  'target',
  'dist',
  'build',
  '.expo',
  '.next',
  '__pycache__',
  '.venv',
]);

const MIME_BY_EXT: Record<string, string> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.bmp': 'image/bmp',
  '.svg': 'image/svg+xml',
  '.html': 'text/html; charset=utf-8',
  '.htm': 'text/html; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.ts': 'text/plain; charset=utf-8',
  '.tsx': 'text/plain; charset=utf-8',
  '.js': 'text/plain; charset=utf-8',
  '.rs': 'text/plain; charset=utf-8',
  '.py': 'text/plain; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
  '.pdf': 'application/pdf',
};

export function mimeForPath(filePath: string): string {
  const ext = path.extname(filePath).toLowerCase();
  return MIME_BY_EXT[ext] ?? 'application/octet-stream';
}

export function isImagePath(filePath: string): boolean {
  const ext = path.extname(filePath).toLowerCase();
  return ['.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp'].includes(ext);
}

export function isHtmlPath(filePath: string): boolean {
  const ext = path.extname(filePath).toLowerCase();
  return ext === '.html' || ext === '.htm';
}

export function resolveAllowedWorkspace(
  cwd: string,
  allowedProjects: string[],
): string | null {
  try {
    const real = fs.realpathSync(cwd);
    if (!fs.statSync(real).isDirectory()) return null;
    for (const proj of allowedProjects) {
      try {
        const realProj = fs.realpathSync(proj);
        if (real === realProj || real.startsWith(realProj + path.sep)) {
          return real;
        }
      } catch {
        // ignore
      }
    }
    return null;
  } catch {
    return null;
  }
}

export function resolveSafeFile(
  cwd: string,
  relOrAbs: string,
  allowedProjects: string[],
  allowedArtifacts?: Set<string>,
): { root: string; fullPath: string; relPath: string } | null {
  if (path.isAbsolute(relOrAbs) && allowedArtifacts && allowedArtifacts.size > 0) {
    try {
      const real = fs.realpathSync(relOrAbs);
      for (const art of allowedArtifacts) {
        try {
          if (fs.realpathSync(art) === real) {
            return { root: path.dirname(real), fullPath: real, relPath: relOrAbs };
          }
        } catch {
          // ignore
        }
      }
    } catch {
      // fall through to workspace check
    }
  }
  const root = resolveAllowedWorkspace(cwd, allowedProjects);
  if (!root) return null;
  const candidate = path.isAbsolute(relOrAbs) ? relOrAbs : path.join(root, relOrAbs);
  try {
    const real = fs.realpathSync(candidate);
    if (real !== root && !real.startsWith(root + path.sep)) return null;
    return {
      root,
      fullPath: real,
      relPath: path.relative(root, real).replace(/\\/g, '/'),
    };
  } catch {
    return null;
  }
}

export function listFileTree(cwd: string, dir: string, allowedProjects: string[]) {
  const resolved = resolveSafeFile(cwd, dir || '.', allowedProjects);
  if (!resolved) {
    throw Object.assign(new Error('Path outside allowed workspace.'), { status: 403 });
  }
  const items = fs.readdirSync(resolved.fullPath, { withFileTypes: true });
  const entries = items
    .filter((ent) => !ent.name.startsWith('.git') && ent.name !== 'node_modules')
    .map((ent) => {
      const full = path.join(resolved.fullPath, ent.name);
      const rel = path.relative(resolved.root, full).replace(/\\/g, '/');
      let size = 0;
      try {
        if (!ent.isDirectory()) size = fs.statSync(full).size;
      } catch {
        // ignore
      }
      return {
        name: ent.name,
        path: rel,
        is_dir: ent.isDirectory(),
        size,
      };
    })
    .sort((a, b) => {
      if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
      return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
    });

  return {
    cwd: resolved.root,
    dir: resolved.relPath === '.' ? '' : resolved.relPath,
    entries,
  };
}

export function readFileContent(
  cwd: string,
  targetPath: string,
  allowedProjects: string[],
  allowedArtifacts?: Set<string>,
) {
  const resolved = resolveSafeFile(cwd, targetPath, allowedProjects, allowedArtifacts);
  if (!resolved) {
    throw Object.assign(new Error('File not found or outside allowed workspace.'), { status: 404 });
  }
  const stat = fs.statSync(resolved.fullPath);
  if (stat.isDirectory()) {
    throw Object.assign(new Error('Path is a directory.'), { status: 400 });
  }
  const mime = mimeForPath(resolved.fullPath);
  const is_image = isImagePath(resolved.fullPath);
  const is_html = isHtmlPath(resolved.fullPath);

  if (is_image || stat.size > 512 * 1024) {
    return {
      path: resolved.relPath,
      mime,
      size: stat.size,
      is_binary: is_image || !mime.startsWith('text/'),
      is_image,
      is_html,
      truncated: stat.size > 512 * 1024 && !is_image,
      content: null,
    };
  }

  const buf = fs.readFileSync(resolved.fullPath);
  const sample = buf.subarray(0, Math.min(buf.length, 4096));
  const is_binary = sample.includes(0);

  return {
    path: resolved.relPath,
    mime,
    size: stat.size,
    is_binary,
    is_image: false,
    is_html,
    truncated: false,
    content: is_binary ? null : buf.toString('utf8'),
  };
}

export function searchWorkspaceFiles(
  cwd: string,
  query: string,
  allowedProjects: string[],
  limit = 40,
): { path: string; is_dir: boolean }[] {
  const root = resolveAllowedWorkspace(cwd, allowedProjects);
  if (!root) return [];
  const q = query.trim().toLowerCase();
  const out: { path: string; is_dir: boolean; score: number }[] = [];

  const walk = (dir: string, depth: number) => {
    if (depth > 6 || out.length >= 400) return;
    let ents: fs.Dirent[];
    try {
      ents = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const ent of ents) {
      if (IGNORED_DIRS.has(ent.name)) continue;
      const full = path.join(dir, ent.name);
      const rel = path.relative(root, full).replace(/\\/g, '/');
      const lowerRel = rel.toLowerCase();
      const lowerName = ent.name.toLowerCase();
      if (!q || lowerRel.includes(q)) {
        const score = !q ? 3 : lowerName.startsWith(q) ? 0 : lowerName.includes(q) ? 1 : 2;
        out.push({ path: rel, is_dir: ent.isDirectory(), score });
      }
      if (ent.isDirectory()) {
        walk(full, depth + 1);
      }
    }
  };
  walk(root, 0);
  out.sort((a, b) => (a.score !== b.score ? a.score - b.score : a.path.localeCompare(b.path)));
  return out.slice(0, limit).map(({ path: p, is_dir }) => ({ path: p, is_dir }));
}

export function gitChanges(cwd: string, allowedProjects: string[]) {
  const root = resolveAllowedWorkspace(cwd, allowedProjects);
  if (!root) {
    throw Object.assign(new Error('Workspace not allowed.'), { status: 403 });
  }
  try {
    const branch = execFileSync('git', ['rev-parse', '--abbrev-ref', 'HEAD'], {
      cwd: root,
      encoding: 'utf8',
      timeout: 5000,
    }).trim();
    const porcelain = execFileSync('git', ['status', '--porcelain=v1'], {
      cwd: root,
      encoding: 'utf8',
      timeout: 5000,
    });
    const files = porcelain
      .split('\n')
      .filter((line) => line.length >= 4)
      .map((line) => {
        const status = line.slice(0, 2);
        const filePath = line.slice(3).trim();
        return {
          path: filePath,
          status,
          staged: status[0] !== ' ' && status[0] !== '?',
          unstaged: status[1] !== ' ',
        };
      });
    return { cwd: root, branch, is_git: true, error: null, files };
  } catch (err: unknown) {
    return {
      cwd: root,
      branch: '',
      is_git: false,
      error: err instanceof Error ? err.message : 'Not a Git repository',
      files: [],
    };
  }
}

export function gitDiff(cwd: string, filePath: string, status: string, allowedProjects: string[]): string {
  const root = resolveAllowedWorkspace(cwd, allowedProjects);
  if (!root) {
    throw Object.assign(new Error('Workspace not allowed.'), { status: 403 });
  }
  try {
    if (status.includes('?')) {
      const resolved = resolveSafeFile(root, filePath, allowedProjects);
      if (!resolved) return '';
      const content = fs.readFileSync(resolved.fullPath, 'utf8');
      return content
        .split('\n')
        .map((l) => `+${l}`)
        .join('\n');
    }
    const diff = execFileSync('git', ['diff', 'HEAD', '--', filePath], {
      cwd: root,
      encoding: 'utf8',
      timeout: 5000,
      maxBuffer: 2 * 1024 * 1024,
    });
    return diff || '(No diff output)';
  } catch (err: unknown) {
    return err instanceof Error ? err.message : 'Failed to compute diff';
  }
}
