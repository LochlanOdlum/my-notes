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

After deployment, create the initial owner account (using the `AdminUserPoolId`
stack output), set a permanent password, and grant it the admin group:

```sh
aws cognito-idp admin-create-user --user-pool-id <pool-id> --username <email>
aws cognito-idp admin-set-user-password --user-pool-id <pool-id> --username <email> --password '<password>' --permanent
aws cognito-idp admin-add-user-to-group --user-pool-id <pool-id> --username <email> --group-name admins
```
