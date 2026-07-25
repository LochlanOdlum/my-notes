# My Notes

This repository contains two intentionally minimal TypeScript applications:

- `web` — React and Vite
- `infra` — AWS CDK

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
