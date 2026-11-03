# sv

Everything you need to build a Svelte project, powered by [`sv`](https://github.com/sveltejs/cli).

## Creating a project

If you're seeing this, you've probably already done this step. Congrats!

```sh
# create a new project
npx sv create my-app
```

To recreate this project with the same configuration:

```sh
# recreate this project
npx sv@0.13.0 create --template minimal --types ts --no-install frontend
```

## Developing

Once you've created a project and installed dependencies with `npm install` (or `pnpm install` or `yarn`), start a development server:

```sh
npm run dev

# or start the server and open the app in a new browser tab
npm run dev -- --open
```

The dev server proxies `/api` and `/ws` to a backend on port 8080, which refuses foreign origins unless told about the dev page:

```sh
ILOLD_DEV_ORIGIN=http://localhost:5173 ilold serve <path> --port 8080
```

## Building

To create a production version of your app:

```sh
npm run build
```

You can preview the production build with `npm run preview`.

> To deploy your app, you may need to install an [adapter](https://svelte.dev/docs/kit/adapters) for your target environment.

## Testing

No frontend test infrastructure yet. E2E coverage comes via backend integration tests (cargo test --workspace) plus manual browser verification. A vitest setup is future work.
