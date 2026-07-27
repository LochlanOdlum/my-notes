import * as cdk from 'aws-cdk-lib/core';
import * as apigatewayv2 from 'aws-cdk-lib/aws-apigatewayv2';
import * as apigatewayv2Integrations from 'aws-cdk-lib/aws-apigatewayv2-integrations';
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
      memorySize: 256,
      runtime: lambda.Runtime.PROVIDED_AL2023,
      timeout: cdk.Duration.seconds(10),
    });

    const notesAdminApi = new apigatewayv2.HttpApi(this, 'NotesAdminApi', {
      apiName: 'my-notes-admin',
      createDefaultStage: true,
    });

    notesAdminApi.addRoutes({
      path: '/health',
      methods: [apigatewayv2.HttpMethod.GET],
      integration: new apigatewayv2Integrations.HttpLambdaIntegration(
        'HealthIntegration',
        notesAdminFunction,
      ),
    });

    new cdk.CfnOutput(this, 'NotesAdminApiUrl', {
      value: notesAdminApi.apiEndpoint,
    });
  }
}
