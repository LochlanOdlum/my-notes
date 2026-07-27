import * as cdk from 'aws-cdk-lib/core';
import * as apigatewayv2 from 'aws-cdk-lib/aws-apigatewayv2';
import * as apigatewayv2Authorizers from 'aws-cdk-lib/aws-apigatewayv2-authorizers';
import * as apigatewayv2Integrations from 'aws-cdk-lib/aws-apigatewayv2-integrations';
import * as cognito from 'aws-cdk-lib/aws-cognito';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { Construct } from 'constructs';
import * as path from 'node:path';

export class InfraStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    const backendPath = path.join(__dirname, '../../backend');
    const lambdaBuilder = cdk.DockerImage.fromBuild(backendPath, {
      file: 'Dockerfile.lambda-builder',
    });

    const notesAdminFunction = new lambda.Function(this, 'NotesAdminFunction', {
      architecture: lambda.Architecture.ARM_64,
      code: lambda.Code.fromAsset(backendPath, {
        bundling: {
          command: [
            'bash',
            '-c',
            [
              'export CARGO_TARGET_DIR=/tmp/cargo-target',
              'cargo lambda build --release --arm64 --bin notes-admin',
              'cp /tmp/cargo-target/lambda/notes-admin/bootstrap /asset-output/bootstrap',
            ].join(' && '),
          ],
          image: lambdaBuilder,
          workingDirectory: '/asset-input',
        },
      }),
      handler: 'bootstrap',
      memorySize: 512,
      runtime: lambda.Runtime.PROVIDED_AL2023,
      timeout: cdk.Duration.seconds(10),
    });

    const notesAdminApi = new apigatewayv2.HttpApi(this, 'NotesAdminApi', {
      apiName: 'my-notes-admin',
      createDefaultStage: false,
    });

    const adminUserPool = new cognito.UserPool(this, 'AdminUserPool', {
      accountRecovery: cognito.AccountRecovery.EMAIL_ONLY,
      autoVerify: { email: true },
      selfSignUpEnabled: false,
      signInAliases: { email: true },
      userPoolName: 'my-notes-admin',
    });

    const adminUserPoolDomain = adminUserPool.addDomain('AdminUserPoolDomain', {
      cognitoDomain: {
        domainPrefix: `my-notes-admin-${cdk.Aws.ACCOUNT_ID}`,
      },
    });

    const adminUserPoolClient = adminUserPool.addClient('AdminWebClient', {
      authFlows: { userSrp: true },
      oAuth: {
        callbackUrls: ['http://localhost:5173/auth/callback'],
        flows: { authorizationCodeGrant: true },
        logoutUrls: ['http://localhost:5173/'],
        scopes: [
          cognito.OAuthScope.OPENID,
          cognito.OAuthScope.EMAIL,
          cognito.OAuthScope.PROFILE,
        ],
      },
      preventUserExistenceErrors: true,
    });

    new cognito.CfnUserPoolGroup(this, 'AdminsGroup', {
      groupName: 'admins',
      userPoolId: adminUserPool.userPoolId,
    });

    const adminAuthorizer = new apigatewayv2Authorizers.HttpUserPoolAuthorizer(
      'AdminAuthorizer',
      adminUserPool,
      { userPoolClients: [adminUserPoolClient] },
    );

    notesAdminApi.addRoutes({
      path: '/health',
      methods: [apigatewayv2.HttpMethod.GET],
      integration: new apigatewayv2Integrations.HttpLambdaIntegration(
        'HealthIntegration',
        notesAdminFunction,
      ),
    });

    const adminIntegration = new apigatewayv2Integrations.HttpLambdaIntegration(
      'AdminIntegration',
      notesAdminFunction,
    );

    notesAdminApi.addRoutes({
      path: '/admin',
      methods: [apigatewayv2.HttpMethod.ANY],
      integration: adminIntegration,
      authorizer: adminAuthorizer,
    });

    notesAdminApi.addRoutes({
      path: '/admin/{proxy+}',
      methods: [apigatewayv2.HttpMethod.ANY],
      integration: adminIntegration,
      authorizer: adminAuthorizer,
    });

    // Keep this construct ID aligned with HttpApi's former implicit default
    // stage. That preserves the CloudFormation logical ID during the migration
    // from `createDefaultStage: true`, avoiding a duplicate `$default` stage.
    notesAdminApi.addStage('DefaultStage', {
      autoDeploy: true,
      throttle: {
        burstLimit: 5,
        rateLimit: 5,
      },
    });

    new cdk.CfnOutput(this, 'NotesAdminApiUrl', {
      value: notesAdminApi.apiEndpoint,
    });

    new cdk.CfnOutput(this, 'AdminUserPoolId', {
      value: adminUserPool.userPoolId,
    });

    new cdk.CfnOutput(this, 'AdminUserPoolClientId', {
      value: adminUserPoolClient.userPoolClientId,
    });

    new cdk.CfnOutput(this, 'AdminHostedUiUrl', {
      value: adminUserPoolDomain.signInUrl(adminUserPoolClient, {
        redirectUri: 'http://localhost:5173/auth/callback',
      }),
    });

    new cdk.CfnOutput(this, 'AdminUserPoolIssuerUrl', {
      value: adminUserPool.userPoolProviderUrl,
    });
  }
}
