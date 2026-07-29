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

The initial API exposes public `GET /health` and reserves `/admin/*` for
owner-only management operations. A Cognito User Pool protects the admin route;
create the owner account administratively and place it in the `admins` group.
The stack outputs the User Pool and Hosted UI details. Its current OAuth
callback is `http://localhost:5173/auth/callback`; add the final GitHub Pages
HTTPS callback URL before deploying the admin frontend. Admin routes currently
return a JSON `501` response while content persistence is built.

Authentication is disabled by default until the admin frontend is ready. Enable
both API Gateway JWT validation and the Lambda's `admins` group check with:

```sh
npm run cdk -- deploy -c adminAuthEnabled=true
```

This flag must be set explicitly for any shared or production deployment. The
User Pool remains deployed while authentication is disabled.

## Content storage and delivery

The stack creates one private, versioned `ContentBucket` as the source of
truth. It stores drafts under `private/` and public reading content under
`published/`; all S3 public access is blocked. The admin Lambda has read/write
access for future publishing operations.

The `PublishedContentUrl` stack output is a CloudFront URL. The frontend fetches
the public manifest, note revisions, and assets from this URL, for example
`<PublishedContentUrl>/tree.json`. CloudFront can read only the `published/`
prefix; it cannot read `private/`. Published reads are static, cached, and do
not invoke the API or Lambda.

Future publishing writes immutable revision objects first, then updates
`published/tree.json` last. Set short cache headers on the manifest and long
cache headers on immutable revisions and assets.

After deployment, create the initial owner account (using the `AdminUserPoolId`
stack output), set a permanent password, and grant it the admin group:

```sh
aws cognito-idp admin-create-user --user-pool-id <pool-id> --username <email>
aws cognito-idp admin-set-user-password --user-pool-id <pool-id> --username <email> --password '<password>' --permanent
aws cognito-idp admin-add-user-to-group --user-pool-id <pool-id> --username <email> --group-name admins
```
