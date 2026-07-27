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
    MemorySize: 256,
    Runtime: 'provided.al2023',
    Timeout: 10,
  });
});

test('routes the health endpoint through an HTTP API', () => {
  const app = new cdk.App();
  const stack = new InfraStack(app, 'TestStack');
  const template = Template.fromStack(stack);

  template.hasResourceProperties('AWS::ApiGatewayV2::Api', {
    Name: 'my-notes-admin',
    ProtocolType: 'HTTP',
  });
  template.hasResourceProperties('AWS::ApiGatewayV2::Route', {
    RouteKey: 'GET /health',
  });
  template.hasResourceProperties('AWS::Lambda::Permission', {
    Action: 'lambda:InvokeFunction',
    Principal: 'apigateway.amazonaws.com',
  });
});
