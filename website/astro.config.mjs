import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import starlight from '@astrojs/starlight';
import { base, github, site } from './scripts/docs.mjs';
import remarkRepoLinks from './scripts/remark-repo-links.mjs';

export default defineConfig({
  site,
  base,
  trailingSlash: 'always',
  markdown: { processor: unified({ remarkPlugins: [remarkRepoLinks] }) },
  integrations: [
    starlight({
      title: 'Takt',
      description: 'Run project commands within CPU, memory, and concurrency budgets. Takt documentation and guides.',
      logo: {
        light: '../brand/svg/takt-horizontal-on-light.svg',
        dark: '../brand/svg/takt-horizontal-on-dark.svg',
        alt: 'Takt',
        replacesTitle: true,
      },
      favicon: '/favicon.ico',
      social: [{ icon: 'github', label: 'GitHub', href: github }],
      editLink: { baseUrl: `${github}/edit/main/website/` },
      customCss: ['./src/styles/custom.css'],
      sidebar: [
        { label: 'Start here', items: [
          { slug: 'quick-start', label: 'Quick start' },
          { slug: 'installation', label: 'Installation & wrapper' },
        ] },
        { label: 'Use Takt', items: [
          { slug: 'configuration', label: 'Configuration' },
          { slug: 'cli', label: 'Command reference' },
          { slug: 'examples', label: 'Examples' },
          { slug: 'runtime', label: 'Runtime & protection' },
        ] },
        { label: 'Project', items: [
          { slug: 'release', label: 'Releases & compatibility' },
          { slug: 'performance', label: 'Performance' },
          { slug: 'testing', label: 'Testing' },
          { slug: 'decisions', label: 'Design decisions' },
          { slug: 'implementation-status', label: 'Implementation status' },
        ] },
      ],
    }),
  ],
});
