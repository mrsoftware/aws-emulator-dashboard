# Service coverage

Every service in the baked catalog — [assets/services.json](assets/services.json), 364 rows — and whether a page in this dashboard can manage it. This is the worklist for ROADMAP M1; regenerate it with `python3 tools/gen-services-doc.py > SERVICES.md` so it never disagrees with [src/views/catalog.rs:11](src/views/catalog.rs#L11).

Measured 2026-11-14 against the emulator's 338-service build.

## Where things stand

| | count | operations | meaning |
|---|---|---|---|
| ✅ managed | 13 | 1,794 | a driver lists its resources and a page acts on them |
| 🚧 served, no page | 325 | 10,662 | the emulator answers it; the focus panel says so and offers nothing |
| ❌ not in build | 26 | — | `registered: false`; the emulator refuses it |
| **catalog total** | **364** | **12,456** | |

Status marks:

- **✅ managed** — in `MANAGED`, with a driver under `src/aws/drivers/` (or [src/aws/services.rs](src/aws/services.rs) for S3, DynamoDB and SQS, which predate that layout) and a page reachable from the sidebar.
- **🚧 served, no page** — `registered: true`, so `aws --endpoint-url` works against it, but `render_operations` returns nothing because `is_managed` is false ([src/views/detail.rs:1251](src/views/detail.rs#L1251)).
- **❌ not in build** — `registered: false`. Not a dashboard gap; the emulator image does not contain that service.

## The 13 that are managed

| service | page | driver | ops in service | create action | resource noun |
|---|---|---|---|---|---|
| `ec2` | EC2 | [src/aws/drivers/ec2.rs](src/aws/drivers/ec2.rs) | 807 | yes | instance |
| `iam` | IAM | [src/aws/drivers/iam.rs](src/aws/drivers/iam.rs) | 180 | yes | user |
| `rds` | RDS | [src/aws/drivers/rds.rs](src/aws/drivers/rds.rs) | 168 | yes | DB instance |
| `ssm` | SSM Parameters | [src/aws/drivers/ssm.rs](src/aws/drivers/ssm.rs) | 152 | yes | parameter |
| `logs` | CloudWatch Logs | [src/aws/drivers/cloudwatch_logs.rs](src/aws/drivers/cloudwatch_logs.rs) | 118 | yes | log group |
| `s3` | S3 Buckets | [src/aws/services.rs](src/aws/services.rs) | 112 | yes | bucket |
| `dynamodb` | DynamoDB | [src/aws/services.rs](src/aws/services.rs) | 58 | yes | table |
| `monitoring` | CloudWatch | [src/aws/drivers/cloudwatch.rs](src/aws/drivers/cloudwatch.rs) | 55 | read-only | alarm |
| `kinesis` | Kinesis | [src/aws/drivers/kinesis.rs](src/aws/drivers/kinesis.rs) | 45 | yes | stream |
| `sns` | SNS | [src/aws/drivers/sns.rs](src/aws/drivers/sns.rs) | 42 | yes | topic |
| `secretsmanager` | Secrets Manager | [src/aws/drivers/secrets_manager.rs](src/aws/drivers/secrets_manager.rs) | 23 | yes | secret |
| `sqs` | SQS | [src/aws/services.rs](src/aws/services.rs) | 23 | yes | queue |
| `sts` | STS | [src/aws/drivers/sts.rs](src/aws/drivers/sts.rs) | 11 | read-only | session |

Every one of these is served by the emulator (`registered: true`), and together they hold 1,794 of the 12,456 operations the build answers — 14.4%.

## By category

Services are grouped by the taxonomy this project chose for the sidebar (`category` in `assets/services.json`, our label, not AWS's). Within a group: managed first, then served-but-uncovered by operation count, then the services this emulator build does not contain. Operation count is a size measure, not a verdict on what to build next — see *Where to start*.

### Management & Governance — 63 services, 3 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| ✅ | Amazon Simple Systems Manager (SSM) | `ssm` | awsJson1_1 | 152 |
| ✅ | Amazon CloudWatch Logs | `logs` | awsJson1_1 | 118 |
| ✅ | Amazon CloudWatch | `monitoring` | awsJson1_0 | 55 |
| 🚧 | AWS Config | `config` | awsJson1_1 | 102 |
| 🚧 | AWS Well-Architected Tool | `wellarchitected` | restJson1 | 95 |
| 🚧 | Amazon CloudDirectory | `clouddirectory` | restJson1 | 66 |
| 🚧 | AWS Organizations | `organizations` | awsJson1_1 | 63 |
| 🚧 | AWS License Manager | `license-manager` | awsJson1_1 | 62 |
| 🚧 | CloudWatch Omni | `cloudwatch` | None | 61 |
| 🚧 | AWS CloudTrail | `cloudtrail` | awsJson1_1 | 60 |
| 🚧 | AWS Cost Explorer Service | `ce` | awsJson1_1 | 47 |
| 🚧 | AWS Systems Manager Incident Manager Contacts | `ssm-contacts` | awsJson1_1 | 39 |
| 🚧 | Amazon CloudWatch Application Insights | `applicationinsights` | awsJson1_1 | 33 |
| 🚧 | Amazon DevOps Guru | `devops-guru` | restJson1 | 31 |
| 🚧 | AWS Systems Manager Incident Manager | `ssm-incidents` | restJson1 | 31 |
| 🚧 | AWS Compute Optimizer | `compute-optimizer` | awsJson1_0 | 28 |
| 🚧 | AWS Systems Manager for SAP | `ssm-sap` | restJson1 | 27 |
| 🚧 | AWS Budgets | `budgets` | awsJson1_1 | 26 |
| 🚧 | AWS Fault Injection Simulator | `fis` | restJson1 | 26 |
| 🚧 | Amazon HealthLake | `healthlake` | awsJson1_0 | 26 |
| 🚧 | Service Quotas | `servicequotas` | awsJson1_1 | 26 |
| 🚧 | Amazon CloudWatch Application Signals | `application-signals` | restJson1 | 25 |
| 🚧 | Compute Optimizer Automation | `aco-automation` | awsJson1_0 | 23 |
| 🚧 | AWS Resource Groups | `resource-groups` | restJson1 | 23 |
| 🚧 | AWS Resource Explorer | `resource-explorer-2` | restJson1 | 22 |
| 🚧 | Synthetics | `synthetics` | restJson1 | 22 |
| 🚧 | AWS Support | `support` | awsJson1_1 | 20 |
| 🚧 | AWS re:Post Private | `repostspace` | restJson1 | 19 |
| 🚧 | AWS License Manager User Subscriptions | `license-manager-user-subscriptions` | restJson1 | 17 |
| 🚧 | Connect Health | `health-agent` | restJson1 | 16 |
| 🚧 | Tax Settings | `tax` | restJson1 | 16 |
| 🚧 | AWS Billing and Cost Management Dashboards | `bcm-dashboards` | awsJson1_0 | 15 |
| 🚧 | AWS Health APIs and Notifications | `health` | awsJson1_1 | 15 |
| 🚧 | CloudWatch Observability Access Manager | `oam` | restJson1 | 15 |
| 🚧 | AWS Performance Insights | `pi` | awsJson1_1 | 14 |
| 🚧 | AWS Systems Manager QuickSetup | `ssm-quicksetup` | restJson1 | 14 |
| 🚧 | AWS Proton | `proton` | awsJson1_0 | 13 |
| 🚧 | TrustedAdvisor Public API | `trustedadvisor` | restJson1 | 12 |
| 🚧 | AWS License Manager Linux Subscriptions | `license-manager-linux-subscriptions` | restJson1 | 11 |
| 🚧 | SupportAuthZ | `supportauthz` | restJson1 | 11 |
| 🚧 | AWS Support App | `supportapp` | restJson1 | 10 |
| 🚧 | PricingPlanManager | `pricingplanmanager` | restJson1 | 9 |
| 🚧 | AWS Resource Groups Tagging API | `tagging` | awsJson1_1 | 9 |
| 🚧 | AWS Clean Rooms ML | `cleanrooms-ml` | restJson1 | 8 |
| 🚧 | AWS Cloud Control API | `cloudcontrolapi` | awsJson1_0 | 8 |
| 🚧 | Cost Optimization Hub | `cost-optimization-hub` | awsJson1_0 | 8 |
| 🚧 | AWS Billing and Cost Management Data Exports | `bcm-data-exports` | awsJson1_1 | 7 |
| 🚧 | AWS Cost and Usage Report Service | `cur` | awsJson1_1 | 7 |
| 🚧 | AWS Application Cost Profiler | `application-cost-profiler` | restJson1 | 6 |
| 🚧 | AWS User Notifications | `notifications` | restJson1 | 6 |
| 🚧 | AWS Billing and Cost Management Pricing Calculator | `bcm-pricing-calculator` | awsJson1_0 | 5 |
| 🚧 | AWS Price List Service | `pricing` | awsJson1_1 | 5 |
| 🚧 | Amazon Managed Grafana | `grafana` | restJson1 | 4 |
| 🚧 | CloudWatch RUM | `rum` | restJson1 | 4 |
| 🚧 | AWS Clean Rooms Service | `cleanrooms` | restJson1 | 3 |
| 🚧 | AWS Launch Wizard | `launchwizard` | restJson1 | 3 |
| 🚧 | AWS User Notifications Contacts | `notifications-contacts` | restJson1 | 3 |
| 🚧 | AWS Billing and Cost Management Recommended Actions | `bcm-recommended-actions` | awsJson1_0 | 1 |
| 🚧 | AWS CloudTrail Data Service | `cloudtrail-data` | restJson1 | 1 |
| 🚧 | AWS SSM-GUIConnect | `ssm-guiconnect` | restJson1 | 0 |
| ❌ | AWS Service Catalog | `servicecatalog` | awsJson1_1 | 90 |
| ❌ | AWS Resilience Hub V2 | `resiliencehub` | restJson1 | 73 |
| ❌ | Amazon AppConfig | `appconfig` | restJson1 | 56 |

### Security & Identity — 47 services, 3 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| ✅ | AWS Identity and Access Management | `iam` | awsQuery | 180 |
| ✅ | AWS Secrets Manager | `secretsmanager` | awsJson1_1 | 23 |
| ✅ | AWS Security Token Service | `sts` | awsQuery | 11 |
| 🚧 | Amazon Cognito Identity Provider | `cognito-idp` | awsJson1_1 | 132 |
| 🚧 | AWS SecurityHub | `securityhub` | restJson1 | 123 |
| 🚧 | Amazon GuardDuty | `guardduty` | restJson1 | 102 |
| 🚧 | Inspector2 | `inspector2` | restJson1 | 81 |
| 🚧 | Amazon Macie 2 | `macie2` | restJson1 | 81 |
| 🚧 | AWS WAF Regional | `waf-regional` | awsJson1_1 | 81 |
| 🚧 | AWS Directory Service | `ds` | awsJson1_1 | 80 |
| 🚧 | AWS WAF | `waf` | awsJson1_1 | 77 |
| 🚧 | AWS Single Sign-On Admin | `sso` | awsJson1_1 | 67 |
| 🚧 | AWS Security Agent | `securityagent` | restJson1 | 66 |
| 🚧 | AWS Audit Manager | `auditmanager` | restJson1 | 62 |
| 🚧 | AWS WAFV2 | `wafv2` | awsJson1_1 | 59 |
| 🚧 | AWS Key Management Service | `kms` | awsJson1_1 | 54 |
| 🚧 | Firewall Management Service | `fms` | awsJson1_1 | 42 |
| 🚧 | AWS Certificate Manager | `acm` | awsJson1_1 | 40 |
| 🚧 | CloudWatch Observability Admin Service | `observabilityadmin` | restJson1 | 40 |
| 🚧 | Amazon Inspector | `inspector` | awsJson1_1 | 37 |
| 🚧 | Amazon Detective | `detective` | restJson1 | 29 |
| 🚧 | Access Analyzer | `access-analyzer` | restJson1 | 28 |
| 🚧 | AWS Certificate Manager Private Certificate Authority | `acm-pca` | awsJson1_1 | 23 |
| 🚧 | Amazon Cognito Identity | `cognito-identity` | awsJson1_1 | 23 |
| 🚧 | Amazon CloudHSM | `cloudhsm` | awsJson1_1 | 20 |
| 🚧 | AWS Signer | `signer` | restJson1 | 19 |
| 🚧 | Amazon Cognito Sync | `cognito-sync` | restJson1 | 17 |
| 🚧 | AWS Directory Service Data | `ds-data` | restJson1 | 17 |
| 🚧 | AWS Sign-In Service | `signin` | restJson1 | 11 |
| 🚧 | Amazon Security Lake | `securitylake` | restJson1 | 10 |
| 🚧 | AWS Multi-party Approval | `mpa` | restJson1 | 8 |
| 🚧 | IAM Roles Anywhere | `rolesanywhere` | restJson1 | 5 |
| 🚧 | AWS Single Sign-On | `awsssoportal` | restJson1 | 4 |
| 🚧 | AWS SSO OIDC | `sso-oauth` | restJson1 | 4 |
| 🚧 | Account Access | `account-access` | restJson1 | 3 |
| 🚧 | AWS Artifact | `artifact` | restJson1 | 3 |
| 🚧 | PcaConnectorAd | `pca-connector-ad` | restJson1 | 3 |
| 🚧 | Private CA Connector for SCEP | `pca-connector-scep` | restJson1 | 3 |
| 🚧 | Security Incident Response | `security-ir` | restJson1 | 3 |
| 🚧 | Amazon Verified Permissions | `verifiedpermissions` | awsJson1_0 | 3 |
| 🚧 | Account Access Runtime | `account-access-runtime` | restJson1 | 2 |
| 🚧 | AWS Control Catalog | `controlcatalog` | restJson1 | 1 |
| 🚧 | AWS Control Tower | `controltower` | restJson1 | 1 |
| 🚧 | Inspector Scan | `inspector-scan` | restJson1 | 1 |
| 🚧 | AWS Account | `account` | restJson1 | 0 |
| 🚧 | AWS SSO Identity Store | `identitystore` | awsJson1_1 | 0 |
| ❌ | Payment Cryptography Control Plane | `payment-cryptography` | awsJson1_0 | 18 |

### Application Integration — 29 services, 2 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| ✅ | Amazon Simple Notification Service | `sns` | awsQuery | 42 |
| ✅ | Amazon Simple Queue Service | `sqs` | awsJson1_0 | 23 |
| 🚧 | Amazon Simple Email Service | `ses` | restJson1 | 116 |
| 🚧 | Amazon Connect Customer Profiles | `profile` | restJson1 | 115 |
| 🚧 | AWS AppSync | `appsync` | restJson1 | 74 |
| 🚧 | Managed Streaming for Kafka | `kafka` | restJson1 | 64 |
| 🚧 | Amazon EventBridge | `events` | awsJson1_1 | 57 |
| 🚧 | Amazon Simple Workflow Service | `swf` | awsJson1_0 | 39 |
| 🚧 | AWS Amplify | `amplify` | restJson1 | 37 |
| 🚧 | AWS App Runner | `apprunner` | awsJson1_0 | 37 |
| 🚧 | AWS Step Functions | `states` | awsJson1_0 | 37 |
| 🚧 | AmplifyBackend | `amplifybackend` | restJson1 | 31 |
| 🚧 | Schemas | `schemas` | restJson1 | 31 |
| 🚧 | AWS Chatbot | `chatbot` | restJson1 | 29 |
| 🚧 | AWS Transfer Family | `transfer` | awsJson1_1 | 29 |
| 🚧 | AWS End User Messaging | `end-user-messaging` | restJson1 | 27 |
| 🚧 | Amazon Appflow | `appflow` | restJson1 | 25 |
| 🚧 | AmazonMQ | `mq` | restJson1 | 25 |
| 🚧 | Amazon AppIntegrations Service | `app-integrations` | restJson1 | 23 |
| 🚧 | Managed Streaming for Kafka Connect | `kafkaconnect` | restJson1 | 19 |
| 🚧 | AmazonMWAA | `airflow` | restJson1 | 12 |
| 🚧 | AWS Amplify UI Builder | `amplifyuibuilder` | restJson1 | 7 |
| 🚧 | AWSMainframeModernization | `m2` | restJson1 | 5 |
| 🚧 | AmazonMWAAServerless | `airflow-serverless` | awsJson1_0 | 3 |
| 🚧 | Amazon EventBridge Pipes | `pipes` | restJson1 | 3 |
| 🚧 | Amazon EventBridge Scheduler | `scheduler` | restJson1 | 3 |
| ❌ | Amazon API Gateway | `apigateway` | restJson1 | 124 |
| ❌ | Amazon Pinpoint SMS Voice V2 | `sms-voice` | awsJson1_0 | 110 |
| ❌ | Amazon Connect Participant Service | `execute-api` | restJson1 | 11 |

### Machine Learning — 27 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | Amazon Comprehend | `comprehend` | awsJson1_1 | 85 |
| 🚧 | Amazon Rekognition | `rekognition` | awsJson1_1 | 75 |
| 🚧 | Amazon Fraud Detector | `frauddetector` | awsJson1_1 | 73 |
| 🚧 | AWSKendraFrontendService | `kendra` | awsJson1_1 | 66 |
| 🚧 | Amazon Lookout for Equipment | `lookoutequipment` | awsJson1_0 | 49 |
| 🚧 | QBusiness | `qbusiness` | restJson1 | 48 |
| 🚧 | Amazon Transcribe Service | `transcribe` | awsJson1_1 | 44 |
| 🚧 | AWS Wickr Admin API | `wickr` | restJson1 | 44 |
| 🚧 | Amazon Machine Learning | `machinelearning` | awsJson1_1 | 28 |
| 🚧 | AWS Comprehend Medical | `comprehendmedical` | awsJson1_1 | 26 |
| 🚧 | Amazon Textract | `textract` | awsJson1_1 | 25 |
| 🚧 | Amazon Translate | `translate` | awsJson1_1 | 19 |
| 🚧 | Amazon CodeGuru Reviewer | `codeguru-reviewer` | restJson1 | 14 |
| 🚧 | Amazon CodeGuru Security | `codeguru-security` | restJson1 | 13 |
| 🚧 | Amazon Polly | `polly` | restJson1 | 10 |
| 🚧 | Amazon Kendra Intelligent Ranking | `kendra-ranking` | awsJson1_0 | 9 |
| 🚧 | Amazon CodeGuru Profiler | `codeguru-profiler` | restJson1 | 4 |
| 🚧 | AWS AI Ops | `aiops` | restJson1 | 3 |
| 🚧 | Braket | `braket` | restJson1 | 3 |
| 🚧 | Amazon SageMaker geospatial capabilities | `sagemaker-geospatial` | restJson1 | 3 |
| ❌ | Amazon SageMaker Service | `sagemaker` | awsJson1_1 | 409 |
| ❌ | Amazon Lex Model Building V2 | `lex` | restJson1 | 107 |
| ❌ | Amazon Personalize | `personalize` | awsJson1_1 | 71 |
| ❌ | Amazon Forecast Service | `forecast` | awsJson1_1 | 63 |
| ❌ | Amazon Bedrock AgentCore | `bedrock-agentcore` | restJson1 | 9 |
| ❌ | Agents for Amazon Bedrock | `bedrock` | restJson1 | 8 |
| ❌ | Agent Registry | `agent-registry` | restJson1 | 3 |

### Analytics — 27 services, 1 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| ✅ | Amazon Kinesis | `kinesis` | awsJson1_1 | 45 |
| 🚧 | AWS Glue | `glue` | awsJson1_1 | 306 |
| 🚧 | Amazon QuickSight | `quicksight` | restJson1 | 299 |
| 🚧 | AWS IoT SiteWise | `iotsitewise` | restJson1 | 149 |
| 🚧 | Amazon Redshift | `redshift` | awsQuery | 145 |
| 🚧 | Amazon DataZone | `datazone` | restJson1 | 129 |
| 🚧 | Amazon Athena | `athena` | awsJson1_1 | 70 |
| 🚧 | AWS Lake Formation | `lakeformation` | restJson1 | 69 |
| 🚧 | Amazon EMR | `elasticmapreduce` | awsJson1_1 | 65 |
| 🚧 | AWS Glue DataBrew | `databrew` | restJson1 | 44 |
| 🚧 | AWS EntityResolution | `entityresolution` | restJson1 | 38 |
| 🚧 | AWS Data Exchange | `dataexchange` | restJson1 | 37 |
| 🚧 | Amazon CloudSearch | `cloudsearch` | awsQuery | 26 |
| 🚧 | Amazon EMR Containers | `emr-containers` | restJson1 | 25 |
| 🚧 | AWS Data Pipeline | `datapipeline` | awsJson1_1 | 19 |
| 🚧 | AWS Elemental MediaPackage | `mediapackage` | restJson1 | 19 |
| 🚧 | AWS Elemental MediaPackage VOD | `mediapackage-vod` | restJson1 | 17 |
| 🚧 | Redshift Serverless | `redshift-serverless` | awsJson1_1 | 15 |
| 🚧 | OpenSearch Service Serverless | `aoss` | awsJson1_0 | 14 |
| 🚧 | Amazon Kinesis Firehose | `firehose` | awsJson1_1 | 12 |
| 🚧 | EMR Serverless | `emr-serverless` | restJson1 | 3 |
| 🚧 | AWS Elemental MediaPackage v2 | `mediapackagev2` | restJson1 | 3 |
| 🚧 | Amazon Omics | `omics` | restJson1 | 3 |
| 🚧 | AWS Marketplace Commerce Analytics | `marketplacecommerceanalytics` | awsJson1_1 | 2 |
| ❌ | Amazon OpenSearch Service | `es` | restJson1 | 96 |
| ❌ | Amazon Kinesis Analytics | `kinesisanalytics` | awsJson1_1 | 33 |
| ❌ | Amazon Kinesis Video Streams | `kinesisvideo` | restJson1 | 32 |

### Networking — 26 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | Amazon CloudFront | `cloudfront` | restXml | 167 |
| 🚧 | AWS Network Manager | `networkmanager` | restJson1 | 95 |
| 🚧 | AWS Network Firewall | `network-firewall` | awsJson1_0 | 85 |
| 🚧 | AWS Direct Connect | `directconnect` | awsJson1_1 | 73 |
| 🚧 | Amazon Route 53 Resolver | `route53resolver` | awsJson1_1 | 72 |
| 🚧 | Amazon Route 53 | `route53` | restXml | 71 |
| 🚧 | AWS Global Accelerator | `globalaccelerator` | awsJson1_1 | 56 |
| 🚧 | Elastic Load Balancing | `elasticloadbalancing` | awsQuery | 51 |
| 🚧 | AWS Shield | `shield` | awsJson1_1 | 36 |
| 🚧 | AWS Resource Access Manager | `ram` | restJson1 | 35 |
| 🚧 | Amazon Route 53 Domains | `route53domains` | awsJson1_1 | 35 |
| 🚧 | AWS Route53 Recovery Readiness | `route53-recovery-readiness` | restJson1 | 32 |
| 🚧 | AWS Cloud Map | `servicediscovery` | awsJson1_1 | 30 |
| 🚧 | AWS Route53 Recovery Control Config | `route53-recovery-control-config` | restJson1 | 25 |
| 🚧 | Route 53 Profiles | `route53profiles` | restJson1 | 16 |
| 🚧 | ARC - Region switch | `arc-region-switch` | awsJson1_0 | 14 |
| 🚧 | Amazon VPC Lattice | `vpc-lattice` | restJson1 | 11 |
| 🚧 | AWS Network Security Manager Customer API | `network-security-manager` | restJson1 | 10 |
| 🚧 | Amazon CloudFront KeyValueStore | `cloudfront-keyvaluestore` | restJson1 | 6 |
| 🚧 | Interconnect | `interconnect` | awsJson1_0 | 6 |
| 🚧 | Amazon Route 53 Global Resolver | `route53globalresolver` | restJson1 | 5 |
| 🚧 | Route53 Recovery Cluster | `route53-recovery-cluster` | awsJson1_0 | 4 |
| 🚧 | AWS App Mesh | `appmesh` | restJson1 | 3 |
| 🚧 | Amazon CloudWatch Internet Monitor | `internetmonitor` | restJson1 | 3 |
| 🚧 | Network Flow Monitor | `networkflowmonitor` | restJson1 | 3 |
| 🚧 | Amazon CloudWatch Network Monitor | `networkmonitor` | restJson1 | 3 |

### Compute & Containers — 24 services, 1 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| ✅ | Amazon Elastic Compute Cloud | `ec2` | ec2Query | 807 |
| 🚧 | Amazon Lightsail | `lightsail` | awsJson1_1 | 162 |
| 🚧 | Amazon GameLift | `gamelift` | awsJson1_1 | 120 |
| 🚧 | Amazon AppStream | `appstream` | awsJson1_1 | 89 |
| 🚧 | EC2 Image Builder | `imagebuilder` | restJson1 | 77 |
| 🚧 | Amazon Elastic Kubernetes Service | `eks` | restJson1 | 70 |
| 🚧 | Auto Scaling | `autoscaling` | awsQuery | 68 |
| 🚧 | Amazon Elastic Container Registry | `ecr` | awsJson1_1 | 58 |
| 🚧 | AWS Batch | `batch` | restJson1 | 48 |
| 🚧 | AWS Elastic Beanstalk | `elasticbeanstalk` | awsQuery | 47 |
| 🚧 | AWS Outposts | `outposts` | restJson1 | 45 |
| 🚧 | Amazon Elastic Container Registry Public | `ecr-public` | awsJson1_1 | 23 |
| 🚧 | Amazon GameLift Streams | `gameliftstreams` | restJson1 | 20 |
| 🚧 | Application Auto Scaling | `application-autoscaling` | awsJson1_1 | 14 |
| 🚧 | AWSServerlessApplicationRepository | `serverlessrepo` | restJson1 | 14 |
| 🚧 | AWS Cloud9 | `cloud9` | awsJson1_1 | 13 |
| 🚧 | Amazon EC2 Container Service | `ecs` | awsJson1_1 | 13 |
| 🚧 | AWS Auto Scaling Plans | `autoscaling-plans` | awsJson1_1 | 6 |
| 🚧 | Amazon Elastic Block Store | `ebs` | restJson1 | 6 |
| 🚧 | AWS Parallel Computing Service | `pcs` | awsJson1_0 | 3 |
| 🚧 | AWS EC2 Instance Connect | `ec2-instance-connect` | awsJson1_1 | 2 |
| 🚧 | Amazon EKS Auth | `eks-auth` | restJson1 | 1 |
| 🚧 | AWS ARC - Zonal Shift | `arc-zonal-shift` | restJson1 | 0 |
| ❌ | AWS Lambda | `lambda` | restJson1 | 16 |

### Other — 24 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | Amazon Mechanical Turk | `mturk-requester` | awsJson1_1 | 39 |
| 🚧 | Amazon Managed Blockchain | `managedblockchain` | restJson1 | 27 |
| 🚧 | Amazon OpenSearch Ingestion | `osis` | restJson1 | 22 |
| 🚧 | Partner Central Benefits API | `partnercentral-benefits` | awsJson1_0 | 17 |
| 🚧 | Amazon WorkSpaces Thin Client | `thinclient` | restJson1 | 16 |
| 🚧 | odb | `odb` | awsJson1_0 | 15 |
| 🚧 | AWS Health Imaging | `medical-imaging` | restJson1 | 14 |
| 🚧 | Amazon Managed Blockchain Query | `managedblockchain-query` | restJson1 | 9 |
| 🚧 | AWS Supply Chain | `scn` | restJson1 | 8 |
| 🚧 | Partner Central Selling API | `partnercentral-selling` | awsJson1_0 | 7 |
| 🚧 | Amazon Elastic VMware Service | `evs` | awsJson1_0 | 6 |
| 🚧 | Partner Central Account API | `partnercentral-account` | awsJson1_0 | 6 |
| 🚧 | AWS Free Tier | `freetier` | awsJson1_0 | 5 |
| 🚧 | Amazon Prometheus Service | `aps` | restJson1 | 4 |
| 🚧 | AWS Sustainability | `sustainability` | restJson1 | 4 |
| 🚧 | AWS Continuum | `continuum` | restJson1 | 3 |
| 🚧 | Partner Central Revenue Measurement API | `partnercentral` | None | 3 |
| 🚧 | Partner Central Channel API | `partnercentral-channel` | awsJson1_0 | 3 |
| 🚧 | AWS User Experience Customization | `uxc` | restJson1 | 3 |
| 🚧 | Amazon Location Service | `geo` | restJson1 | 0 |
| 🚧 | Amazon Location Service Maps V2 | `geo-maps` | restJson1 | 0 |
| 🚧 | Amazon Location Service Places V2 | `geo-places` | restJson1 | 0 |
| 🚧 | Amazon Location Service Routes V2 | `geo-routes` | restJson1 | 0 |
| ❌ | AWS Marketplace Agreement Service | `aws-marketplace` | awsJson1_0 | 25 |

### Developer Tools — 17 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | AWS CloudFormation | `cloudformation` | awsQuery | 90 |
| 🚧 | AWS CodeCommit | `codecommit` | awsJson1_1 | 80 |
| 🚧 | AWS Device Farm | `devicefarm` | awsJson1_1 | 77 |
| 🚧 | AWS CodeBuild | `codebuild` | awsJson1_1 | 59 |
| 🚧 | CodeArtifact | `codeartifact` | restJson1 | 48 |
| 🚧 | AWS CodeDeploy | `codedeploy` | awsJson1_1 | 47 |
| 🚧 | AWS CodePipeline | `codepipeline` | awsJson1_1 | 44 |
| 🚧 | AWS DevOps Agent Service | `aidevops` | restJson1 | 38 |
| 🚧 | AWS X-Ray | `xray` | restJson1 | 38 |
| 🚧 | QApps | `qapps` | restJson1 | 35 |
| 🚧 | AWS CodeConnections | `codeconnections` | awsJson1_0 | 27 |
| 🚧 | AWS CodeStar connections | `codestar-connections` | awsJson1_0 | 27 |
| 🚧 | AppFabric | `appfabric` | restJson1 | 26 |
| 🚧 | AWS Migration Hub Refactor Spaces | `refactor-spaces` | restJson1 | 24 |
| 🚧 | AWS CodeStar Notifications | `codestar-notifications` | restJson1 | 13 |
| 🚧 | Amazon CodeCatalyst | `codecatalyst` | restJson1 | 2 |
| 🚧 | Nova Act Service | `nova-act` | restJson1 | 0 |

### Storage — 16 services, 1 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| ✅ | Amazon Simple Storage Service | `s3` | restXml | 112 |
| 🚧 | AWS Backup | `backup` | restJson1 | 115 |
| 🚧 | AWS DataSync | `datasync` | awsJson1_1 | 53 |
| 🚧 | Amazon FSx | `fsx` | awsJson1_1 | 48 |
| 🚧 | Amazon Glacier | `glacier` | restJson1 | 33 |
| 🚧 | Amazon Elastic File System | `elasticfilesystem` | restJson1 | 31 |
| 🚧 | Amazon Import/Export Snowball | `snowball` | awsJson1_1 | 27 |
| 🚧 | Amazon S3 Files | `s3files` | restJson1 | 21 |
| 🚧 | Amazon Recycle Bin | `rbin` | restJson1 | 10 |
| 🚧 | Amazon Data Lifecycle Manager | `dlm` | restJson1 | 8 |
| 🚧 | AWS Backup Search | `backup-search` | restJson1 | 5 |
| 🚧 | Amazon S3 on Outposts | `s3-outposts` | restJson1 | 5 |
| 🚧 | AWS Backup Gateway | `backup-gateway` | awsJson1_0 | 3 |
| 🚧 | Amazon S3 Tables | `s3tables` | restJson1 | 3 |
| 🚧 | Amazon S3 Vectors | `s3vectors` | restJson1 | 3 |
| 🚧 | AWS Snow Device Management | `snow-device-management` | restJson1 | 3 |

### Database — 16 services, 2 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| ✅ | Amazon Relational Database Service | `rds` | awsQuery | 168 |
| ✅ | Amazon DynamoDB | `dynamodb` | awsJson1_0 | 58 |
| 🚧 | AWS Database Migration Service | `dms` | awsJson1_1 | 119 |
| 🚧 | Amazon ElastiCache | `elasticache` | awsQuery | 77 |
| 🚧 | Amazon MemoryDB | `memorydb` | awsJson1_1 | 45 |
| 🚧 | Amazon NeptuneData | `neptune-db` | restJson1 | 43 |
| 🚧 | Amazon DynamoDB Accelerator (DAX) | `dax` | awsJson1_1 | 21 |
| 🚧 | Amazon Keyspaces | `cassandra` | awsJson1_0 | 19 |
| 🚧 | Amazon DocumentDB Elastic Clusters | `docdb-elastic` | restJson1 | 19 |
| 🚧 | Amazon Timestream Write | `timestream` | awsJson1_0 | 19 |
| 🚧 | Redshift Data API Service | `redshift-data` | awsJson1_1 | 12 |
| 🚧 | Amazon Neptune Graph | `neptune-graph` | restJson1 | 8 |
| 🚧 | AWS RDS DataService | `rds-data` | restJson1 | 6 |
| 🚧 | Amazon Aurora DSQL | `dsql` | restJson1 | 3 |
| 🚧 | Amazon SimpleDB v2 | `sdb` | restJson1 | 3 |
| 🚧 | Timestream InfluxDB | `timestream-influxdb` | awsJson1_0 | 3 |

### IoT & Edge — 12 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | AWS IoT | `iot` | restJson1 | 272 |
| 🚧 | AWS IoT Wireless | `iotwireless` | restJson1 | 112 |
| 🚧 | AWS IoT TwinMaker | `iottwinmaker` | restJson1 | 40 |
| 🚧 | AWS IoT Things Graph | `iotthingsgraph` | awsJson1_1 | 35 |
| 🚧 | AWS IoT Core Device Advisor | `iotdeviceadvisor` | restJson1 | 14 |
| 🚧 | AWS IoT FleetWise | `iotfleetwise` | awsJson1_0 | 12 |
| 🚧 | AWS IoT Data Plane | `iotdata` | restJson1 | 11 |
| 🚧 | AWS IoT Secure Tunneling | `IoTSecuredTunneling` | awsJson1_1 | 8 |
| 🚧 | Managed integrations for AWS IoT Device Management | `iotmanagedintegrations` | restJson1 | 6 |
| 🚧 | AWS Ground Station | `groundstation` | restJson1 | 5 |
| 🚧 | AWS IoT Jobs Data Plane | `iot-jobs-data` | restJson1 | 5 |
| ❌ | AWS Greengrass | `greengrass` | restJson1 | 92 |

### Media Services — 9 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | AWS Elemental MediaLive | `medialive` | restJson1 | 123 |
| 🚧 | AWS Elemental MediaConvert | `mediaconvert` | restJson1 | 34 |
| 🚧 | AWSDeadlineCloud | `deadline` | restJson1 | 29 |
| 🚧 | Amazon Voice ID | `voiceid` | awsJson1_0 | 24 |
| 🚧 | AWS Elemental MediaStore | `mediastore` | awsJson1_1 | 21 |
| 🚧 | AWS MediaConnect | `mediaconnect` | restJson1 | 7 |
| 🚧 | AWS Elemental Inference | `elemental-inference` | restJson1 | 5 |
| 🚧 | AWS MediaTailor | `mediatailor` | restJson1 | 5 |
| ❌ | Amazon Interactive Video Service | `ivs` | restJson1 | 41 |

### Customer Engagement — 8 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | Amazon Pinpoint | `mobiletargeting` | restJson1 | 122 |
| 🚧 | Amazon Interactive Video Service Chat | `ivschat` | restJson1 | 17 |
| 🚧 | Amazon Connect Cases | `cases` | restJson1 | 3 |
| 🚧 | AWS End User Messaging Social | `social-messaging` | restJson1 | 3 |
| ❌ | Amazon Connect Service | `connect` | restJson1 | 402 |
| ❌ | Amazon Chime SDK Voice | `chime` | restJson1 | 96 |
| ❌ | AmazonConnectCampaignServiceV2 | `connect-campaigns` | restJson1 | 37 |
| ❌ | Amazon Q Connect | `wisdom` | restJson1 | 3 |

### Migration & Transfer — 7 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | AWS Storage Gateway | `storagegateway` | awsJson1_1 | 96 |
| 🚧 | AWS Application Discovery Service | `discovery` | awsJson1_1 | 28 |
| 🚧 | Elastic Disaster Recovery Service | `drs` | restJson1 | 28 |
| 🚧 | Migration Hub Strategy Recommendations | `migrationhub-strategy` | restJson1 | 22 |
| 🚧 | Application Migration Service | `mgn` | restJson1 | 7 |
| 🚧 | AWS Migration Hub Orchestrator | `migrationhub-orchestrator` | restJson1 | 3 |
| ❌ | AWS Migration Hub | `mgh` | awsJson1_1 | 21 |

### Financial Services — 6 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | AWS Billing | `billing` | awsJson1_0 | 23 |
| 🚧 | AWS Invoicing | `invoicing` | awsJson1_0 | 21 |
| 🚧 | AWS B2B Data Interchange | `b2bi` | awsJson1_0 | 10 |
| 🚧 | AWS Savings Plans | `savingsplans` | restJson1 | 10 |
| 🚧 | AWSBillingConductor | `billingconductor` | restJson1 | 8 |
| 🚧 | RTBFabric | `rtbfabric` | restJson1 | 5 |

### End User Computing — 6 services, 0 managed

| status | service | signing name | protocol | ops |
|---|---|---|---|---|
| 🚧 | Amazon WorkMail | `workmail` | awsJson1_1 | 92 |
| 🚧 | Amazon WorkSpaces | `workspaces` | awsJson1_1 | 91 |
| 🚧 | Amazon WorkDocs | `workdocs` | restJson1 | 44 |
| 🚧 | Amazon Workspaces Instances | `workspaces-instances` | awsJson1_0 | 14 |
| 🚧 | Amazon WorkSpaces Web | `workspaces-web` | restJson1 | 6 |
| 🚧 | Amazon WorkMail Message Flow | `workmailmessageflow` | restJson1 | 2 |

## Where to start

Two different orders, and choosing between them is the M1 decision.

**Raw operation count.** 🚧 services with the most operations. Read this honestly: the top of the list is Glue, QuickSight and IoT — services nobody provisions on a laptop. Volume is not the same as usefulness.

| rank | status | service | signing name | ops | category |
|---|---|---|---|---|---|
| 1 | 🚧 | AWS Glue | `glue` | 306 | Analytics |
| 2 | 🚧 | Amazon QuickSight | `quicksight` | 299 | Analytics |
| 3 | 🚧 | AWS IoT | `iot` | 272 | IoT & Edge |
| 4 | 🚧 | Amazon CloudFront | `cloudfront` | 167 | Networking |
| 5 | 🚧 | Amazon Lightsail | `lightsail` | 162 | Compute & Containers |
| 6 | 🚧 | AWS IoT SiteWise | `iotsitewise` | 149 | Analytics |
| 7 | 🚧 | Amazon Redshift | `redshift` | 145 | Analytics |
| 8 | 🚧 | Amazon Cognito Identity Provider | `cognito-idp` | 132 | Security & Identity |
| 9 | 🚧 | Amazon DataZone | `datazone` | 129 | Analytics |
| 10 | 🚧 | AWS Elemental MediaLive | `medialive` | 123 | Media Services |
| 11 | 🚧 | AWS SecurityHub | `securityhub` | 123 | Security & Identity |
| 12 | 🚧 | Amazon Pinpoint | `mobiletargeting` | 122 | Customer Engagement |
| 13 | 🚧 | Amazon GameLift | `gamelift` | 120 | Compute & Containers |
| 14 | 🚧 | AWS Database Migration Service | `dms` | 119 | Database |
| 15 | 🚧 | Amazon Simple Email Service | `ses` | 116 | Application Integration |
| 16 | 🚧 | AWS Backup | `backup` | 115 | Storage |
| 17 | 🚧 | Amazon Connect Customer Profiles | `profile` | 115 | Application Integration |
| 18 | 🚧 | AWS IoT Wireless | `iotwireless` | 112 | IoT & Edge |
| 19 | 🚧 | AWS Config | `config` | 102 | Management & Governance |
| 20 | 🚧 | Amazon GuardDuty | `guardduty` | 102 | Security & Identity |
| 21 | 🚧 | AWS Storage Gateway | `storagegateway` | 96 | Migration & Transfer |
| 22 | 🚧 | AWS Network Manager | `networkmanager` | 95 | Networking |
| 23 | 🚧 | AWS Well-Architected Tool | `wellarchitected` | 95 | Management & Governance |
| 24 | 🚧 | Amazon WorkMail | `workmail` | 92 | End User Computing |
| 25 | 🚧 | Amazon WorkSpaces | `workspaces` | 91 | End User Computing |

The remaining 300 🚧 services are in the category tables above.

**What a local emulator user reaches for.** ROADMAP M1 option A's curated set, where operation count is a tie-break rather than the sort key. The membership is the roadmap's judgement; the counts below come from the catalog, so they cannot go stale.

| service | signing name | ops | category |
|---|---|---|---|
| AWS Config | `config` | 102 | Management & Governance |
| AWS Network Manager | `networkmanager` | 95 | Networking |
| Amazon WorkSpaces | `workspaces` | 91 | End User Computing |
| AWS CloudFormation | `cloudformation` | 90 | Developer Tools |
| Amazon ElastiCache | `elasticache` | 77 | Database |
| Amazon Route 53 | `route53` | 71 | Networking |
| Amazon Athena | `athena` | 70 | Analytics |
| Auto Scaling | `autoscaling` | 68 | Compute & Containers |
| AWS CloudTrail | `cloudtrail` | 60 | Management & Governance |
| AWS CodeBuild | `codebuild` | 59 | Developer Tools |
| Amazon Elastic Container Registry | `ecr` | 58 | Compute & Containers |
| Amazon EventBridge | `events` | 57 | Application Integration |
| AWS Key Management Service | `kms` | 54 | Security & Identity |
| Elastic Load Balancing | `elasticloadbalancing` | 51 | Networking |

Each is one `aws-sdk-*` crate, one driver file, one `MANAGED` entry, and rows in [src/views/operations.rs](src/views/operations.rs); the pattern is proven eleven times over.

Beyond the 25 above, 300 more 🚧 services are waiting in the category tables, and no order reaches them for free. A generic runner needs a crate per service anyway, because the AWS SDK is per-service — see ROADMAP Q1 for why that is an open decision, not a solved one.
