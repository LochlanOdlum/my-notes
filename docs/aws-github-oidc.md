# AWS authentication for infrastructure deployment

The infrastructure workflow uses GitHub OpenID Connect (OIDC), not long-lived
AWS access keys. GitHub exchanges an OIDC token for short-lived AWS credentials
only while the `aws-production` deployment job is running.

The workflow expects three GitHub environment variables on the
`aws-production` environment:

| Variable | Example | Secret? |
| --- | --- | --- |
| `AWS_REGION` | `eu-west-2` | No |
| `AWS_ACCOUNT_ID` | `123456789012` | No |
| `AWS_DEPLOY_ROLE_ARN` | `arn:aws:iam::123456789012:role/my-notes-infra-deployer` | No |

Do not store `AWS_ACCESS_KEY_ID` or `AWS_SECRET_ACCESS_KEY` in GitHub.

## 1. Create the GitHub environment

In the GitHub repository, create an environment named `aws-production` under
**Settings → Environments**.

Configure these safeguards:

- Limit deployment branches to `main`.
- Add yourself as a required reviewer if you want every infrastructure change
  to need an explicit approval.
- Add the three environment variables listed above.

The workflow can validate CDK changes in pull requests, but it cannot deploy
from a pull request.

## 2. Bootstrap the CDK environment once

Sign in locally with an AWS identity that is allowed to create the CDK bootstrap
resources in the target account and region. Then run:

```sh
nvm use
npm install
npm run cdk -- bootstrap aws://ACCOUNT_ID/REGION
```

For example:

```sh
npm run cdk -- bootstrap aws://123456789012/eu-west-2
```

CDK bootstrapping is required before CDK can deploy stacks. The default
bootstrap stack creates the artifact bucket and IAM roles that CDK uses during
deployment.

## 3. Create the AWS OIDC identity provider

In AWS IAM, add an OpenID Connect identity provider with:

| Setting | Value |
| --- | --- |
| Provider URL | `https://token.actions.githubusercontent.com` |
| Audience | `sts.amazonaws.com` |

Do this once per AWS account. AWS no longer requires a manually configured
thumbprint for this provider.

## 4. Create the deployment role

Create an IAM role named `my-notes-infra-deployer`. Its trusted entity is the
GitHub OIDC provider created above.

Use the following trust policy after replacing the placeholders:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "arn:aws:iam::ACCOUNT_ID:oidc-provider/token.actions.githubusercontent.com"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringEquals": {
          "token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
          "token.actions.githubusercontent.com:sub": "repo:OWNER/REPOSITORY:environment:aws-production"
        }
      }
    }
  ]
}
```

The `sub` condition is essential: it prevents workflows from other repositories
from assuming the role.

### Repositories using immutable subject claims

GitHub repositories created after July 15, 2026 may use an immutable OIDC
subject claim. If this repository does, replace the `sub` value above with this
form instead:

```text
repo:OWNER@OWNER_ID/REPOSITORY@REPOSITORY_ID:environment:aws-production
```

Use the exact subject format supplied by GitHub for this repository. The GitHub
OIDC documentation explains how subject claims are configured.

To discover the exact value without granting AWS access, run the **Inspect
GitHub OIDC claims** workflow manually from the repository's **Actions** tab.
It prints only the non-secret claims needed for the trust policy, including
`sub`, `aud`, repository IDs, branch, and environment. Copy the displayed
`sub` value exactly into the IAM trust policy.

## 5. Grant the role CDK deployment access

After the default CDK bootstrap succeeds, attach this policy to
`my-notes-infra-deployer`, replacing `ACCOUNT_ID` and `REGION`:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "AssumeCdkBootstrapRoles",
      "Effect": "Allow",
      "Action": [
        "sts:AssumeRole",
        "sts:TagSession"
      ],
      "Resource": [
        "arn:aws:iam::ACCOUNT_ID:role/cdk-hnb659fds-deploy-role-ACCOUNT_ID-REGION",
        "arn:aws:iam::ACCOUNT_ID:role/cdk-hnb659fds-file-publishing-role-ACCOUNT_ID-REGION",
        "arn:aws:iam::ACCOUNT_ID:role/cdk-hnb659fds-image-publishing-role-ACCOUNT_ID-REGION",
        "arn:aws:iam::ACCOUNT_ID:role/cdk-hnb659fds-lookup-role-ACCOUNT_ID-REGION"
      ]
    },
    {
      "Sid": "ReadCdkDeploymentState",
      "Effect": "Allow",
      "Action": [
        "cloudformation:DescribeChangeSet",
        "cloudformation:DescribeStackEvents",
        "cloudformation:DescribeStacks",
        "cloudformation:GetTemplate",
        "cloudformation:GetTemplateSummary"
      ],
      "Resource": "*"
    },
    {
      "Sid": "ReadBootstrapVersion",
      "Effect": "Allow",
      "Action": "ssm:GetParameter",
      "Resource": "arn:aws:ssm:REGION:ACCOUNT_ID:parameter/cdk-bootstrap/hnb659fds/version"
    }
  ]
}
```

The default CDK qualifier is `hnb659fds`. If a custom qualifier was used during
bootstrapping, replace it in every role and parameter ARN above.

The CDK bootstrap deployment role controls which AWS resources CloudFormation
can create. The default bootstrap template is intentionally broad for a new CDK
project. Once this application has settled, review and narrow that execution
role to the exact AWS services used by My Notes.

## 6. Run the first deployment

1. Commit and push the workflow to `main`.
2. Open **Actions → Validate and deploy infrastructure**.
3. Approve the `aws-production` environment deployment if required.
4. Confirm the workflow's `Configure AWS credentials` step reports the expected
   account ID.
5. Confirm the CDK deployment completes.

The initial stack is intentionally empty, so this first deployment primarily
verifies authentication and CDK bootstrapping. Future infrastructure changes
will use the same deployment path.

## Troubleshooting

| Symptom | Likely cause |
| --- | --- |
| `Not authorized to perform sts:AssumeRoleWithWebIdentity` | The trust policy's `sub`, `aud`, account ID, or OIDC provider is wrong. |
| `Credentials could not be loaded` | One of the three `aws-production` environment variables is missing, the role ARN is invalid, or OIDC trust was rejected. The workflow's configuration check and OIDC inspection workflow isolate the cause. |
| `Environment is not bootstrapped` | Run the bootstrap command for the exact account and region. |
| `AccessDenied` while CDK deploys | The OIDC role cannot assume one of the bootstrap roles, or the bootstrap execution role is too restricted. |
| Workflow deploys from an unexpected branch | Add or correct the `aws-production` environment deployment-branch rule. |
