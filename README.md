# My Notes

This repository contains two intentionally minimal TypeScript applications:

- `web` — React and Vite
- `infra` — AWS CDK
- `backend` — Rust management API Lambda

## Getting started

Use the Node.js version declared in `.nvmrc`, then install dependencies:

```sh
nvm use
npm install
```

Start the empty React application:

```sh
npm run dev
```

Verify both applications:

```sh
npm run build
npm test
npm run lint
npm run cdk -- synth
```

The first CDK synthesis builds the Lambda ZIP in Docker. Docker Desktop must be
running. To run Rust unit tests directly:

```sh
npm run backend:test
```
