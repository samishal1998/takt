# Takt documentation website

The [Takt documentation](https://samishal1998.github.io/takt/) uses Astro and
Starlight, built and published from this repository with GitHub Pages.

## Develop locally

Use Node.js 24 or later. From the repository root:

```sh
cd website
npm ci
npm run dev
```

Open the local URL printed by Astro, including the `/takt/` path. To check and
preview the production build:

```sh
npm run check
npm run build
npm run check:links
npm run preview
```

`check` runs Astro's content and type checks. `build` writes the static site to
`dist/`. `check:links` validates links and assets in that build. Starlight's search
index is generated during production builds and can be tested with `preview`.

Browser checks cover navigation, search, theme persistence, and mobile layout:

```sh
npx playwright install chromium
npm test
```

The tests start the preview server themselves and require a completed build.
If Chromium is unavailable, install Firefox with `npx playwright install firefox`
and run `PLAYWRIGHT_BROWSER=firefox npm test`. CI uses Chromium and saves browser
screenshots and any failure traces as workflow artifacts.

## Edit documentation

Edit the Markdown in the repository's [`docs/`](../docs/) directory and the
[`examples/README.md`](../examples/README.md) guide. The sync step imports these
into `src/content/docs/`, adds Starlight metadata from their headings, and adapts
the content for Starlight. The website build resolves repository links to their
published pages or GitHub sources. Generated Markdown is ignored by Git; edit
its original source instead.

The landing page, `src/content/docs/index.mdx`, is maintained directly in this
website. Site navigation and deployment URLs are configured in
`astro.config.mjs`. Brand assets, examples, and schemas come from their existing
repository directories.

The `dev`, `check`, and `build` commands run the sync step automatically. If you
edit a source Markdown file while the development server is running, run
`npm run sync` in another terminal to refresh the imported content.

Node.js and the website dependencies are needed to build the documentation;
Takt's executable remains independent of them.

## Publish

The [`Documentation` workflow](../.github/workflows/docs.yml) checks and builds
changes to the website, documentation, brand assets, examples, and schemas.
Pull requests run the same validation. A successful build on `main` is deployed
to GitHub Pages, and the workflow can also be run manually on `main`.

In this repository's **Settings → Pages**, the publishing source is
**GitHub Actions**. The workflow uploads `website/dist/` and deploys it through
the `github-pages` environment. Deployment permissions are scoped to the deploy
job. A separate publishing branch is not needed.

The project site uses `site: 'https://samishal1998.github.io'` and `base: '/takt'`.
Keep both values aligned with the hosting URL when changing the repository or
configuring a custom domain. Update links that include `/takt/` at the same time,
then rebuild and run `npm run check:links`.
