# DBCrab documentation

This directory contains the Astro and Starlight source for the DBCrab user
documentation published at <https://akhansari.gitlab.io/dbcrab/>.

## Requirements

- Node.js 22.12 or newer.
- pnpm 11.19.0, pinned by `package.json`.

Node.js 22 through 24 bundle Corepack. Enable it before installing dependencies:

```sh
corepack enable
pnpm install --frozen-lockfile
```

With Node.js 25 or newer, install Corepack or pnpm separately first.

## Development

Documentation pages live under `src/content/docs`. Navigation and production
URL settings live in `astro.config.mjs`. Product images are shared from the
repository-level `docs/assets` directory.

Start Astro using its managed background mode:

```sh
pnpm astro dev --background
```

Inspect or stop the server with:

```sh
pnpm astro dev status
pnpm astro dev logs
pnpm astro dev stop
```

## Production build

```sh
pnpm build
pnpm preview
```

The production build uses `/dbcrab` as its base path. GitLab CI checks website
changes in merge requests and deploys `website/dist` to GitLab Pages from
`main`.

The GitLab Pages project setting **Use unique domain** must remain disabled for
the configured `https://akhansari.gitlab.io/dbcrab/` URL.
