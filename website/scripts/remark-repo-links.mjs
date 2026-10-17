import { basename, posix } from 'node:path';
import { base, github, pages } from './docs.mjs';

const bySlug = new Map(pages.map((page) => [page.slug, page.source]));
const bySource = new Map(pages.map((page) => [page.source, page.slug]));

// Rewrite Markdown link nodes only. Commands and code samples stay verbatim.
export default function remarkRepoLinks() {
  return (tree, file) => {
    const source = bySlug.get(basename(file.path ?? '', '.md'));
    if (!source) return;
    function visit(node) {
      if (['link', 'image', 'definition'].includes(node.type) && typeof node.url === 'string') {
        const url = node.url;
        if (url && !/^(?:[a-z][a-z\d+.-]*:|\/|#)/i.test(url)) {
          const match = url.match(/^([^?#]*)(.*)$/);
          const path = posix.normalize(posix.join(posix.dirname(source), decodeURI(match[1])));
          const suffix = match[2];
          const slug = bySource.get(path);
          if (slug) node.url = `${base}/${slug}/${suffix}`;
          else if (/^(?:examples|schema|docs\/evidence)\/.+\.(?:yaml|json)$/.test(path)) {
            node.url = `${base}/${path}${suffix}`;
          } else {
            node.url = `${github}/blob/main/${path}${suffix}`;
          }
        }
      }
      for (const child of node.children ?? []) visit(child);
    }
    visit(tree);
  };
}
