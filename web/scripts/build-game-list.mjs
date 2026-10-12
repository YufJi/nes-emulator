#!/usr/bin/env node
// Build `public/games.json` — the game list offered by the "游戏列表" dialog.
//
// Reads the file tree of a GitHub repository and writes a `[{ name, url }]`
// array pointing at raw.githubusercontent.com, which serves the files with
// `Access-Control-Allow-Origin: *` so the browser can download them directly.
//
// Usage:
//   node scripts/build-game-list.mjs [owner/repo] [branch]
//
// The unauthenticated GitHub API allows 60 requests per hour; set GITHUB_TOKEN
// to raise the limit when regenerating often. When the API is rate limited the
// script falls back to walking the repository's tree pages, which need no quota.

import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const [repoSlug = 'mengfei523060/nesrom', branch = 'master'] = process.argv.slice(2);
const [owner, repo] = repoSlug.split('/');
if (!owner || !repo) {
  throw new Error(`仓库参数应为 owner/repo，收到 “${repoSlug}”。`);
}

/** Guard against walking an enormous repository by accident. */
const MAX_DIRECTORIES = 200;

const headers = {
  accept: 'application/vnd.github+json',
  'user-agent': 'nes-emulator-game-list',
};
if (process.env.GITHUB_TOKEN) {
  headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
}

/** One entry per file in the repository, from the API or from the tree pages. */
async function listFiles() {
  const treeUrl = `https://api.github.com/repos/${owner}/${repo}/git/trees/${branch}?recursive=1`;
  const response = await fetch(treeUrl, { headers });

  if (response.ok) {
    const tree = await response.json();
    if (tree.truncated) {
      throw new Error('文件树被 GitHub 截断，拒绝生成不完整的列表。');
    }
    return tree.tree.filter((entry) => entry.type === 'blob').map((entry) => entry.path);
  }

  const limited = response.status === 403 || response.status === 429;
  if (!limited) {
    throw new Error(`读取文件树失败：${response.status} ${response.statusText}。请检查 owner/repo 与分支名。`);
  }

  console.warn('GitHub API 限流，改用仓库页面遍历（无配额限制，较慢）。');
  return listFilesViaPages();
}

/**
 * Walk a repository's tree pages, one directory at a time.
 *
 * Used when the API is rate limited. GitHub embeds the current directory's
 * entries in the page, so the only cost is one request per directory.
 */
async function listFilesViaPages() {
  // The payload is JSON inside the HTML, so escapes such as \u0026 must be
  // decoded before the name can be used as a path.
  const entryPattern =
    /\{"name":"((?:[^"\\]|\\.)*)","path":"((?:[^"\\]|\\.)*)","contentType":"(file|directory)"\}/g;

  const files = [];
  const pending = [''];
  const visited = new Set();

  while (pending.length > 0) {
    const directory = pending.shift();
    if (visited.has(directory)) continue;
    if (visited.size >= MAX_DIRECTORIES) {
      throw new Error(`目录数超过 ${MAX_DIRECTORIES}，未生成不完整的列表。`);
    }
    visited.add(directory);

    const suffix = directory
      ? `/${directory.split('/').map(encodeURIComponent).join('/')}`
      : '';
    const response = await fetch(`https://github.com/${owner}/${repo}/tree/${branch}${suffix}`);
    if (!response.ok) {
      throw new Error(`读取目录 “${directory || '/'}” 失败：${response.status} ${response.statusText}。`);
    }

    const html = await response.text();
    for (const [, , rawPath, kind] of html.matchAll(entryPattern)) {
      const entryPath = JSON.parse(`"${rawPath}"`);
      if (kind === 'directory') pending.push(entryPath);
      else files.push(entryPath);
    }
  }

  if (files.length === 0) {
    throw new Error('页面中没有解析到任何文件，GitHub 页面结构可能已改变。');
  }
  return [...new Set(files)];
}

// Only iNES images the emulator can actually load; the repo also holds APKs,
// text files and the like.
const isLoadable = (filePath) => /\.(nes|fds|unf|unif)$/i.test(filePath);

const games = (await listFiles())
  .filter((filePath) => isLoadable(filePath))
  .map((filePath) => ({
    // Path segments must be escaped: the names contain Chinese, spaces, '#!'…
    url: `https://raw.githubusercontent.com/${owner}/${repo}/refs/heads/${branch}/${filePath
      .split('/')
      .map(encodeURIComponent)
      .join('/')}`,
    name: filePath
      .split('/')
      .pop()
      .replace(/\.[^.]+$/, ''),
  }))
  .sort((a, b) => a.name.localeCompare(b.name, 'zh-Hans-CN'));

if (games.length === 0) {
  throw new Error('没有找到任何 ROM 文件，未生成列表。');
}

// One game per line keeps the committed file readable and its diffs small.
const json = `[\n${games.map((game) => `  ${JSON.stringify(game)}`).join(',\n')}\n]\n`;

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const target = path.join(scriptDir, '..', 'public', 'games.json');
mkdirSync(path.dirname(target), { recursive: true });
writeFileSync(target, json);

console.log(
  `已写入 ${games.length} 个游戏到 ${path.relative(process.cwd(), target)}（${(
    Buffer.byteLength(json) / 1024
  ).toFixed(1)} KB），来源 ${repoSlug}@${branch}。`,
);
