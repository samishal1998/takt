import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, posix, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'parse5';
import { base, site } from './docs.mjs';

const output = resolve(process.argv[2] ?? fileURLToPath(new URL('../dist/', import.meta.url)));
const origin = new URL(site).origin;
const basePath = base.replace(/\/$/, '');
const maxFiles = 20_000;
const maxHtmlBytes = 8 * 1024 * 1024;
const maxReferences = 200_000;
const maxReportedErrors = 50;

function collectFiles(directory, prefix = '', files = new Set()) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const relative = posix.join(prefix, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`Build output contains a symlink: ${relative}`);
    if (entry.isDirectory()) collectFiles(join(directory, entry.name), relative, files);
    else if (entry.isFile()) {
      files.add(relative);
      if (files.size > maxFiles) throw new Error(`Build output exceeds ${maxFiles} files`);
    }
  }
  return files;
}

function readDocument(relative) {
  const path = join(output, relative);
  if (statSync(path).size > maxHtmlBytes) throw new Error(`${relative} exceeds the 8 MiB HTML limit`);
  return parse(readFileSync(path, 'utf8'));
}

function* elements(document) {
  const pending = [document];
  while (pending.length) {
    const node = pending.pop();
    if (node.tagName) yield node;
    if (node.childNodes) pending.push(...node.childNodes);
    if (node.content) pending.push(node.content);
  }
}

// A srcset URL may itself contain commas (for example, a data URL).
function* srcsetUrls(value) {
  let offset = 0;
  while (offset < value.length) {
    while (/[\s,]/.test(value[offset] ?? '') && offset < value.length) offset++;
    const start = offset;
    while (offset < value.length && !/\s/.test(value[offset])) offset++;
    const token = value.slice(start, offset);
    if (!token) break;
    yield token.replace(/,+$/, '');
    if (token.endsWith(',')) continue;
    let parentheses = 0;
    while (offset < value.length) {
      const character = value[offset++];
      if (character === '(') parentheses++;
      if (character === ')') parentheses--;
      if (character === ',' && parentheses === 0) break;
    }
  }
}

function pageUrl(relative) {
  const route = relative === 'index.html' ? '' : relative.replace(/\/index\.html$/, '/');
  return new URL(`${basePath}/${route}`, origin);
}

function main() {
  const files = collectFiles(output);
  if (!files.has('index.html')) throw new Error('Missing dist/index.html; run npm run build first');
  const searchDirectory = ['pagefind', '_pagefind'].find((directory) => files.has(`${directory}/pagefind.js`));
  if (!searchDirectory) throw new Error('Missing Pagefind search runtime');
  if (![...files].some((file) => file.startsWith(`${searchDirectory}/`) && file.endsWith('.pf_index'))) {
    throw new Error('Missing Pagefind search index');
  }

  const pages = new Map();
  for (const relative of files) {
    if (!relative.endsWith('.html')) continue;
    const ids = new Set();
    for (const element of elements(readDocument(relative))) {
      for (const attribute of element.attrs) {
        if (attribute.name === 'id' || (element.tagName === 'a' && attribute.name === 'name')) {
          ids.add(attribute.value);
        }
      }
    }
    pages.set(relative, ids);
  }

  let errors = 0;
  let references = 0;
  function report(relative, value, reason) {
    errors++;
    if (errors <= maxReportedErrors) console.error(`${relative}: ${JSON.stringify(value)} — ${reason}`);
  }

  function checkReference(relative, value, metadata = false) {
    if (++references > maxReferences) throw new Error(`Build output exceeds ${maxReferences} references`);
    let url;
    try {
      url = new URL(value, pageUrl(relative));
    } catch {
      report(relative, value, 'invalid URL');
      return;
    }
    if (!['http:', 'https:'].includes(url.protocol) || url.origin !== origin) return;
    let pathname;
    try {
      pathname = posix.normalize(decodeURIComponent(url.pathname));
    } catch {
      report(relative, value, 'invalid URL path encoding');
      return;
    }
    if (pathname !== basePath && !pathname.startsWith(`${basePath}/`)) {
      report(relative, value, `same-origin URL is outside ${basePath}/`);
      return;
    }
    // A generated 404 page may declare /404/ as its canonical URL while GitHub
    // Pages serves its 404.html file for all missing paths.
    if (metadata && relative === '404.html') return;
    const local = pathname.slice(basePath.length).replace(/^\//, '');
    const candidates = [local, posix.join(local, 'index.html')];
    const target = candidates.find((candidate) => files.has(candidate));
    if (!target) {
      report(relative, value, 'target does not exist in the build');
      return;
    }
    if (!url.hash || !pages.has(target)) return;
    let fragment;
    try {
      fragment = decodeURIComponent(url.hash.slice(1)).split(':~:text=')[0];
    } catch {
      report(relative, value, 'invalid fragment encoding');
      return;
    }
    if (fragment && !pages.get(target).has(fragment)) report(relative, value, `missing #${fragment} in ${target}`);
  }

  for (const relative of pages.keys()) {
    for (const element of elements(readDocument(relative))) {
      const attributes = new Map(element.attrs.map(({ name, value }) => [name, value]));
      const metadata = element.tagName === 'link' && /\b(?:canonical|alternate)\b/.test(attributes.get('rel') ?? '');
      for (const attribute of ['href', 'src', 'poster']) {
        if (attributes.has(attribute)) checkReference(relative, attributes.get(attribute), metadata);
      }
      if (attributes.has('srcset')) {
        for (const value of srcsetUrls(attributes.get('srcset'))) checkReference(relative, value);
      }
    }
  }

  if (errors) {
    if (errors > maxReportedErrors) console.error(`Omitted ${errors - maxReportedErrors} additional errors.`);
    throw new Error(`${errors} broken documentation links or assets`);
  }
  console.log(`Checked ${pages.size} HTML pages, ${references} references, and the Pagefind search index.`);
}

try {
  main();
} catch (error) {
  console.error(`Documentation link check failed: ${error.message}`);
  process.exitCode = 1;
}
