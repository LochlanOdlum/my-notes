import * as cdk from 'aws-cdk-lib/core';
import { Template } from 'aws-cdk-lib/assertions';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { InfraStack } from '../lib/infra-stack';

test('defines an ARM64 custom-runtime Lambda', () => {
  const app = new cdk.App();
  const stack = new InfraStack(app, 'TestStack');

  assert.equal(stack.stackName, 'TestStack');

  const template = Template.fromStack(stack);
  template.hasResourceProperties('AWS::Lambda::Function', {
    Architectures: ['arm64'],
    Handler: 'bootstrap',
    MemorySize: 512,
    Environment: {
      Variables: {
        ADMIN_AUTH_ENABLED: 'false',
      },
    },
    Runtime: 'provided.al2023',
    Timeout: 10,
  });
});

test('routes health publicly and requires Cognito authentication for admin paths', () => {
  const app = new cdk.App({ context: { adminAuthEnabled: true } });
  const stack = new InfraStack(app, 'TestStack');
  const template = Template.fromStack(stack);

  template.hasResourceProperties('AWS::ApiGatewayV2::Api', {
    Name: 'my-notes-admin',
    ProtocolType: 'HTTP',
  });
  template.hasResourceProperties('AWS::ApiGatewayV2::Route', {
    RouteKey: 'GET /health',
  });
  template.hasResourceProperties('AWS::ApiGatewayV2::Route', {
    RouteKey: 'ANY /admin/{proxy+}',
    AuthorizationType: 'JWT',
  });
  template.hasResourceProperties('AWS::ApiGatewayV2::Route', {
    RouteKey: 'ANY /admin',
    AuthorizationType: 'JWT',
  });
  template.hasResourceProperties('AWS::ApiGatewayV2::Stage', {
    AutoDeploy: true,
    DefaultRouteSettings: {
      ThrottlingBurstLimit: 5,
      ThrottlingRateLimit: 5,
    },
    StageName: '$default',
  });
  template.hasResourceProperties('AWS::Lambda::Permission', {
    Action: 'lambda:InvokeFunction',
    Principal: 'apigateway.amazonaws.com',
  });
  template.hasResourceProperties('AWS::Cognito::UserPool', {
    AccountRecoverySetting: {
      RecoveryMechanisms: [{ Name: 'verified_email', Priority: 1 }],
    },
    AutoVerifiedAttributes: ['email'],
    UsernameAttributes: ['email'],
    UserPoolName: 'my-notes-admin',
  });
  template.hasResourceProperties('AWS::Cognito::UserPoolClient', {
    AllowedOAuthFlows: ['code'],
    AllowedOAuthFlowsUserPoolClient: true,
    CallbackURLs: ['http://localhost:5173/auth/callback'],
    ExplicitAuthFlows: ['ALLOW_USER_SRP_AUTH', 'ALLOW_REFRESH_TOKEN_AUTH'],
    PreventUserExistenceErrors: 'ENABLED',
  });
  template.hasResourceProperties('AWS::Cognito::UserPoolGroup', {
    GroupName: 'admins',
  });
  template.hasResourceProperties('AWS::ApiGatewayV2::Authorizer', {
    AuthorizerType: 'JWT',
    IdentitySource: ['$request.header.Authorization'],
  });
});
