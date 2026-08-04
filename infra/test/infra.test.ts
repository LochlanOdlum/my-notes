import * as cdk from 'aws-cdk-lib/core';
import { Match, Template } from 'aws-cdk-lib/assertions';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { InfraStack } from '../lib/infra-stack';

test('defines an ARM64 custom-runtime Lambda', { concurrency: false }, () => {
  const app = new cdk.App();
  const stack = new InfraStack(app, 'TestStack', { bundleLambda: false });

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

test('stores content privately and exposes only published content through CloudFront', { concurrency: false }, () => {
  const app = new cdk.App();
  const stack = new InfraStack(app, 'TestStack', { bundleLambda: false });
  const template = Template.fromStack(stack);

  template.hasResourceProperties('AWS::S3::Bucket', {
    BucketEncryption: {
      ServerSideEncryptionConfiguration: [
        { ServerSideEncryptionByDefault: { SSEAlgorithm: 'AES256' } },
      ],
    },
    PublicAccessBlockConfiguration: {
      BlockPublicAcls: true,
      BlockPublicPolicy: true,
      IgnorePublicAcls: true,
      RestrictPublicBuckets: true,
    },
    VersioningConfiguration: { Status: 'Enabled' },
  });
  template.resourceCountIs('AWS::CloudFront::Distribution', 1);
  template.resourceCountIs('AWS::CloudFront::OriginAccessControl', 1);
  template.hasResourceProperties('AWS::CloudFront::ResponseHeadersPolicy', {
    ResponseHeadersPolicyConfig: {
      CorsConfig: Match.objectLike({
        AccessControlAllowOrigins: {
          Items: [
            'http://localhost:5173',
            'https://lochlanodlum.github.io',
          ],
        },
        OriginOverride: true,
      }),
    },
  });
  const distributions = template.findResources('AWS::CloudFront::Distribution');
  const distribution = Object.values(distributions)[0] as {
    Properties: { DistributionConfig: { CacheBehaviors?: Array<{ PathPattern: string }> } };
  };
  assert.ok(
    distribution.Properties.DistributionConfig.CacheBehaviors?.some(
      ({ PathPattern }) => PathPattern === 'tree.json',
    ),
    'tree.json must have its own uncached CloudFront behavior',
  );
  template.hasResourceProperties('AWS::S3::BucketPolicy', {
    PolicyDocument: {
      Statement: Match.arrayWith([
        Match.objectLike({
          Effect: 'Deny',
          NotResource: Match.anyValue(),
          Sid: 'DenyCloudFrontReadsOutsidePublishedPrefix',
        }),
      ]),
    },
  });
  template.hasResourceProperties('AWS::IAM::Policy', {
    PolicyDocument: {
      Statement: Match.arrayWith([
        Match.objectLike({
          Action: Match.arrayWith(['s3:GetObject*', 's3:PutObject']),
          Effect: 'Allow',
        }),
      ]),
    },
  });
});

test('routes health publicly and requires Cognito authentication for admin paths', { concurrency: false }, () => {
  const app = new cdk.App({ context: { adminAuthEnabled: true } });
  const stack = new InfraStack(app, 'TestStack', { bundleLambda: false });
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
