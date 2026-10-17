import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { descriptions, github, pages, repo } from './docs.mjs';

const content = fileURLToPath(new URL('../src/content/docs/', import.meta.url));
const publicDir = fileURLToPath(new URL('../public/', import.meta.url));
mkdirSync(content, { recursive: true });
// This directory's .md files are generated; the authored landing page is .mdx.
const current = new Set(pages.map(({ slug }) => `${slug}.md`));
for (const file of readdirSync(content)) {
  if (file.endsWith('.md') && !current.has(file)) rmSync(join(content, file));
}
for (const { source, slug } of pages) {
  const markdown = readFileSync(join(repo, source), 'utf8');
  const heading = markdown.match(/^# (.+)\r?\n/);
  if (!heading) throw new Error(`${source} must start with a Markdown title`);
  const frontmatter = [
    '---',
    `title: ${JSON.stringify(heading[1])}`,
    `description: ${JSON.stringify(descriptions[slug] ?? heading[1])}`,
    `editUrl: ${JSON.stringify(`${github}/edit/main/${source}`)}`,
    '---',
    '',
  ].join('\n');
  writeFileSync(join(content, `${slug}.md`), frontmatter + markdown.slice(heading[0].length));
}

for (const directory of ['examples', 'schema', 'docs/evidence']) {
  for (const file of readdirSync(join(repo, directory))) {
    if (!/\.(?:yaml|json)$/.test(file)) continue;
    const destination = join(publicDir, directory, file);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(join(repo, directory, file), destination);
  }
}
mkdirSync(publicDir, { recursive: true });
copyFileSync(join(repo, 'brand/icons/favicon.ico'), join(publicDir, 'favicon.ico'));
console.log(`Synced ${pages.length} canonical guides and downloadable examples/schema.`);
