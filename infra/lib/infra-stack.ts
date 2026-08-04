import * as cdk from 'aws-cdk-lib/core';
import * as apigatewayv2 from 'aws-cdk-lib/aws-apigatewayv2';
import * as apigatewayv2Authorizers from 'aws-cdk-lib/aws-apigatewayv2-authorizers';
import * as apigatewayv2Integrations from 'aws-cdk-lib/aws-apigatewayv2-integrations';
import * as cloudfront from 'aws-cdk-lib/aws-cloudfront';
import * as cloudfrontOrigins from 'aws-cdk-lib/aws-cloudfront-origins';
import * as cognito from 'aws-cdk-lib/aws-cognito';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as s3 from 'aws-cdk-lib/aws-s3';
import { Construct } from 'constructs';
import * as path from 'node:path';

export interface InfraStackProps extends cdk.StackProps {
  /**
   * Test-only escape hatch for synthesizing infrastructure assertions without
   * invoking the several-minute cross-compiled Lambda bundle.
   */
  readonly bundleLambda?: boolean;
}

export class InfraStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: InfraStackProps) {
    super(scope, id, props);

    // Keep authentication off until the admin frontend is ready. Opt in with
    // CDK context `adminAuthEnabled=true` before a shared deployment.
    const adminAuthEnabled = String(
      this.node.tryGetContext('adminAuthEnabled'),
    ) === 'true';

    const backendPath = path.join(__dirname, '../../backend');
    const testLambdaPath = path.join(__dirname, '../test/fixtures/lambda');
    const lambdaCode =
      props?.bundleLambda === false
        // A custom runtime cannot use CloudFormation inline code. This asset is
        // only synthesized in infrastructure-only tests; the real build path
        // below is covered by the workflow's CDK synth packaging step.
        ? lambda.Code.fromAsset(testLambdaPath)
        : lambda.Code.fromAsset(backendPath, {
            bundling: {
              command: [
                'bash',
                '-c',
                [
                  // These directories are inside the mounted backend asset.
                  // They are ignored by Git and can be reused locally and by
                  // the GitHub Actions cache between CDK bundle invocations.
                  'export CARGO_HOME=/asset-input/target/cargo-home',
                  'export CARGO_TARGET_DIR=/asset-input/target/cargo-lambda',
                  'cargo lambda build --release --arm64 --bin notes-admin',
                  'cp /asset-input/target/cargo-lambda/lambda/notes-admin/bootstrap /asset-output/bootstrap',
                ].join(' && '),
              ],
              image: cdk.DockerImage.fromBuild(backendPath, {
                file: 'Dockerfile.lambda-builder',
              }),
              workingDirectory: '/asset-input',
            },
          });

    const notesAdminFunction = new lambda.Function(this, 'NotesAdminFunction', {
      architecture: lambda.Architecture.ARM_64,
      code: lambdaCode,
      handler: 'bootstrap',
      memorySize: 512,
      runtime: lambda.Runtime.PROVIDED_AL2023,
      timeout: cdk.Duration.seconds(10),
      environment: {
        ADMIN_AUTH_ENABLED: String(adminAuthEnabled),
      },
    });

    const contentBucket = new s3.Bucket(this, 'ContentBucket', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      cors: [
        {
          allowedHeaders: ['*'],
          allowedMethods: [s3.HttpMethods.GET, s3.HttpMethods.HEAD],
          allowedOrigins: [
            'http://localhost:5173',
            'https://lochlanodlum.github.io',
          ],
        },
      ],
      encryption: s3.BucketEncryption.S3_MANAGED,
      enforceSSL: true,
      lifecycleRules: [
        { abortIncompleteMultipartUploadAfter: cdk.Duration.days(7) },
      ],
      removalPolicy: cdk.RemovalPolicy.RETAIN,
      versioned: true,
    });

    contentBucket.grantReadWrite(notesAdminFunction);
    notesAdminFunction.addEnvironment('CONTENT_BUCKET_NAME', contentBucket.bucketName);

    const publishedContentCachePolicy = new cloudfront.CachePolicy(
      this,
      'PublishedContentCachePolicy',
      {
        defaultTtl: cdk.Duration.days(1),
        enableAcceptEncodingBrotli: true,
        enableAcceptEncodingGzip: true,
        maxTtl: cdk.Duration.days(365),
        minTtl: cdk.Duration.seconds(0),
      },
    );

    const publishedContentCorsPolicy = new cloudfront.ResponseHeadersPolicy(
      this,
      'PublishedContentCorsPolicy',
      {
        corsBehavior: {
          accessControlAllowCredentials: false,
          accessControlAllowHeaders: ['*'],
          accessControlAllowMethods: ['GET', 'HEAD', 'OPTIONS'],
          accessControlAllowOrigins: [
            'http://localhost:5173',
            'https://lochlanodlum.github.io',
          ],
          originOverride: true,
        },
      },
    );

    const publishedContentOrigin = cloudfrontOrigins.S3BucketOrigin.withOriginAccessControl(
      contentBucket,
      { originPath: '/published' },
    );

    const publishedContentDistribution = new cloudfront.Distribution(
      this,
      'PublishedContentDistribution',
      {
        defaultBehavior: {
          allowedMethods: cloudfront.AllowedMethods.ALLOW_GET_HEAD,
          cachePolicy: publishedContentCachePolicy,
          origin: publishedContentOrigin,
          originRequestPolicy: cloudfront.OriginRequestPolicy.CORS_S3_ORIGIN,
          responseHeadersPolicy: publishedContentCorsPolicy,
          viewerProtocolPolicy:
            cloudfront.ViewerProtocolPolicy.REDIRECT_TO_HTTPS,
        },
        // This manifest selects the current immutable note revision. It must
        // reflect a publish immediately, unlike revisioned note files which
        // are safe (and desirable) to cache for a long time.
        additionalBehaviors: {
          'tree.json': {
            allowedMethods: cloudfront.AllowedMethods.ALLOW_GET_HEAD,
            cachePolicy: cloudfront.CachePolicy.CACHING_DISABLED,
            origin: publishedContentOrigin,
            originRequestPolicy: cloudfront.OriginRequestPolicy.CORS_S3_ORIGIN,
            responseHeadersPolicy: publishedContentCorsPolicy,
            viewerProtocolPolicy:
              cloudfront.ViewerProtocolPolicy.REDIRECT_TO_HTTPS,
          },
        },
        priceClass: cloudfront.PriceClass.PRICE_CLASS_100,
      },
    );

    // S3BucketOrigin grants the distribution access to the bucket. This explicit
    // deny narrows that grant to the published prefix, keeping drafts private.
    contentBucket.addToResourcePolicy(
      new iam.PolicyStatement({
        actions: ['s3:GetObject'],
        conditions: {
          StringEquals: {
            'AWS:SourceArn': `arn:${cdk.Aws.PARTITION}:cloudfront::${cdk.Aws.ACCOUNT_ID}:distribution/${publishedContentDistribution.distributionId}`,
          },
        },
        effect: iam.Effect.DENY,
        notResources: [contentBucket.arnForObjects('published/*')],
        principals: [new iam.ServicePrincipal('cloudfront.amazonaws.com')],
        sid: 'DenyCloudFrontReadsOutsidePublishedPrefix',
      }),
    );

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
      authorizer: adminAuthEnabled ? adminAuthorizer : undefined,
    });

    notesAdminApi.addRoutes({
      path: '/admin/{proxy+}',
      methods: [apigatewayv2.HttpMethod.ANY],
      integration: adminIntegration,
      authorizer: adminAuthEnabled ? adminAuthorizer : undefined,
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

    new cdk.CfnOutput(this, 'ContentBucketName', {
      value: contentBucket.bucketName,
    });

    new cdk.CfnOutput(this, 'PublishedContentUrl', {
      value: `https://${publishedContentDistribution.distributionDomainName}`,
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

    new cdk.CfnOutput(this, 'AdminAuthEnabled', {
      value: String(adminAuthEnabled),
    });
  }
}
