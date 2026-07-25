import * as cdk from 'aws-cdk-lib/core';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { InfraStack } from '../lib/infra-stack';

test('stack is empty', () => {
  const app = new cdk.App();
  const stack = new InfraStack(app, 'TestStack');

  assert.equal(stack.stackName, 'TestStack');
});
