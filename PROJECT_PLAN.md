# My Notes — Project Plan

## 1. Project summary

My Notes is a publicly readable notes and blogging application with a private,
single-user editing experience.

The public React application will be hosted by GitHub Pages. AWS will store and
serve note data and attachments. Only the owner will be able to create, edit,
move, publish, unpublish, restore, or delete content.

The editing experience should feel closer to Notion or Quip than a traditional
admin form: notes are edited directly in the reading view, the hierarchy is
always available, and saving happens without navigating to a separate CMS.

## 2. Product goals

- Anyone can browse and read published notes without signing in.
- One invited owner account can manage all content.
- Notes can be organized into nested folders.
- Editing happens inline using a structured rich-text editor.
- The system remains inexpensive and operationally simple at low traffic.
- Infrastructure and deployments are reproducible from the repository.
- The design leaves a migration path to a database if the project eventually
  outgrows S3 manifests.

## 3. Initial scale assumptions

- Approximately 100 notes.
- A few content updates per month.
- One editor.
- Very low public traffic.
- Text-oriented content with occasional images and attachments.
- No real-time collaboration or conflict-free replicated data types.

These assumptions deliberately favor simple object storage over a database.

## 4. Agreed architecture

```text
┌─────────────────────────────┐
│ GitHub Pages                │
│ React + TypeScript          │
│                             │
│ • Public reader             │
│ • Authenticated editor      │
└─────────────┬───────────────┘
              │
              ├── Public reads ───────────────┐
              │                               ▼
              │                    ┌────────────────────┐
              │                    │ CloudFront         │
              │                    │ Public S3 content  │
              │                    └─────────┬──────────┘
              │                              ▼
              │                    ┌────────────────────┐
              │                    │ Private S3 bucket  │
              │                    │ with versioning    │
              │                    └────────────────────┘
              │
              └── Authenticated writes
                              │
                              ▼
                    ┌────────────────────┐
                    │ API Gateway        │
                    │ HTTP API           │
                    └─────────┬──────────┘
                              │
                    Cognito JWT validation
                              │
                              ▼
                    ┌────────────────────┐
                    │ Lambda             │
                    │ Content operations │
                    └─────────┬──────────┘
                              ▼
                    ┌────────────────────┐
                    │ Private S3 bucket  │
                    └────────────────────┘
```

### 4.1 Frontend

- React and TypeScript.
- Vite for local development and production builds.
- GitHub Pages for static hosting.
- A hash-based router for the first release because GitHub Pages does not
  provide standard single-page application route rewriting.
- Tiptap for structured rich-text editing and read-only rendering.
- Cognito authorization-code authentication with PKCE.

### 4.2 Management API backend

- Rust, built as a release binary for Linux ARM64.
- Axum for HTTP routing, adapted to Lambda through `lambda_http`.
- `serde` for request and document deserialization, `thiserror` for domain
  errors, `tracing` for structured logs, and the AWS SDK for Rust for S3.
- One `notes-admin` function for all authenticated management routes.
- Lambda's `provided.al2023` runtime, packaged as a zip artifact rather than a
  container image.
- API Gateway HTTP API payload format 2.0.
- The public reader does not invoke this function; it reads published objects
  through CloudFront.

The implementation should keep Axum handlers thin. Folder operations, draft
saves, publishing, and S3 access remain Rust services that can be unit-tested
without HTTP or Lambda event fixtures.

### 4.3 AWS services

- **S3:** source of truth for manifests, note documents, revisions, and
  attachments.
- **CloudFront:** public, cached access to the published S3 prefix while the
  underlying bucket remains private.
- **API Gateway HTTP API:** authenticated content-management endpoints.
- **Lambda:** validation and all S3 mutations.
- **Cognito User Pool:** invitation-only owner authentication.
- **CloudWatch:** short-retention operational logs.
- **AWS Budgets:** low-cost alert, initially set to USD 1 per month.

### 4.4 Infrastructure as code

- AWS CDK with TypeScript.
- One initial environment-agnostic stack.
- No VPC or NAT Gateway.
- No DynamoDB in the MVP.
- No resources are deployed until their configuration and removal policies are
  reviewed.

## 5. Repository structure

```text
my-notes/
├── infra/                 # AWS CDK application
│   ├── bin/
│   ├── lib/
│   └── test/
├── web/                   # React application
│   ├── public/
│   └── src/
├── backend/                # Rust workspace for the management API
│   ├── Cargo.toml
│   └── crates/
│       └── notes-admin/
├── package.json           # npm workspace commands
├── package-lock.json
├── .nvmrc                 # Node.js 24
└── PROJECT_PLAN.md
```

Likely additions during implementation:

```text
backend/
├── Cargo.toml
└── crates/
    └── notes-admin/
        └── src/

contracts/                  # JSON Schema / OpenAPI API contracts

infra/
├── lib/constructs/         # Focused CDK constructs
└── test/

web/src/
├── api/                   # Public and authenticated API clients
├── auth/                  # Cognito session integration
├── components/
├── editor/
├── notes/
├── routes/
└── styles/
```

Contracts must be language-neutral because the browser is TypeScript and the
management API is Rust. JSON Schema or OpenAPI definitions in `contracts/`
will describe requests and responses. Rust remains authoritative for server-side
validation; TypeScript consumes generated or checked types for the browser.

## 6. S3 content model

The bucket will use separate published and private prefixes. Only the published
prefix will be reachable through CloudFront.

```text
published/
├── tree.json
├── notes/
│   └── {noteId}/
│       └── {revisionId}.json
└── assets/
    └── {assetId}/{filename}

private/
├── tree.json
├── drafts/
│   └── {noteId}.json
├── trash/
└── uploads/
```

### 6.1 Tree manifest

The tree manifest contains hierarchy and lightweight metadata, not full note
bodies.

Example:

```json
{
  "schemaVersion": 1,
  "revision": "01J...",
  "updatedAt": "2026-07-25T12:00:00.000Z",
  "nodes": [
    {
      "id": "01J...",
      "type": "folder",
      "parentId": null,
      "title": "Engineering",
      "position": 1000
    },
    {
      "id": "01K...",
      "type": "note",
      "parentId": "01J...",
      "title": "Example note",
      "slug": "example-note",
      "position": 1000,
      "status": "published",
      "publishedRevision": "01L...",
      "createdAt": "2026-07-25T12:00:00.000Z",
      "updatedAt": "2026-07-25T12:00:00.000Z"
    }
  ]
}
```

### 6.2 Note document

Each note document will contain:

- Schema version.
- Stable note ID.
- Revision ID.
- Tiptap JSON document.
- Plain-text excerpt or derived text if useful.
- Creation and update timestamps.
- References to uploaded assets.

The Tiptap JSON document is authoritative. Arbitrary stored HTML will not be
accepted.

### 6.3 Identifiers and URLs

- Nodes use opaque, stable IDs such as UUIDs or ULIDs.
- Notes have globally unique human-readable slugs.
- Public URLs use the slug.
- Moving a note or folder does not change its ID or public URL.
- Slug changes may create a redirect entry in a later milestone.

### 6.4 Ordering

Each node has a sortable `position` value within its parent. Moving or
reordering a node should normally update only that node. Positions can be
rebalanced when gaps become too small.

## 7. Consistency and revision strategy

S3 cannot atomically update multiple objects, so mutation order must prevent
broken public references.

### 7.1 Draft save

1. Client sends the document and its last known revision or ETag.
2. Lambda validates the document and authorization.
3. Lambda conditionally writes the draft using the last known ETag.
4. A stale ETag returns a conflict instead of overwriting newer content.
5. The client displays `Saving`, `Saved`, `Offline`, or `Conflict`.

### 7.2 Publish

1. Validate the latest draft.
2. Write a new immutable object under
   `published/notes/{noteId}/{revisionId}.json`.
3. Conditionally update the published tree manifest to point at the new
   revision.
4. Return the new manifest and note revisions.

The content object is written before the manifest. A failed manifest update can
leave an unreferenced object, but cannot expose a broken published note.

### 7.3 Unpublish

1. Remove the note from the published manifest.
2. Keep prior published revisions temporarily for recovery.
3. Remove or expire old public objects according to the retention policy.

### 7.4 Delete and restore

- Deleting initially means moving a node to Trash.
- Trashed nodes are removed from the published manifest immediately.
- Restoring returns the node to its previous parent when possible.
- Permanent deletion is a separate, confirmed operation.
- S3 Versioning provides an additional recovery layer.

## 8. Security model

### 8.1 Public access

- No authentication is required to read published manifests, note documents,
  or published assets.
- The S3 bucket itself remains private.
- CloudFront uses Origin Access Control to retrieve published objects.
- Draft and trash prefixes are not exposed by CloudFront.

### 8.2 Owner access

- Cognito self-registration is disabled.
- The owner account is created administratively.
- The frontend is a public OAuth client and contains no client secret.
- Authorization uses the code flow with PKCE.
- API Gateway validates Cognito access tokens on mutation routes.
- Lambda still validates the expected identity or scope before writing.
- Hiding editor controls is only a user-interface behavior, not a security
  boundary.

### 8.3 Input and upload validation

- Validate all request bodies with runtime schemas.
- Reject unknown editor node types and unsupported marks.
- Apply explicit maximum sizes for notes and uploads.
- Restrict accepted attachment MIME types.
- Generate server-controlled S3 object keys.
- Never accept an arbitrary bucket or object key from the client.
- Escape or safely render external links and user-entered text.
- Configure CORS for localhost and the exact GitHub Pages origin.

## 9. Initial API surface

Exact paths may change during implementation, but the first authenticated API
should cover:

```text
GET    /admin/tree
GET    /admin/notes/{noteId}

POST   /admin/folders
POST   /admin/notes

PATCH  /admin/nodes/{nodeId}
PUT    /admin/notes/{noteId}/draft
POST   /admin/notes/{noteId}/publish
POST   /admin/notes/{noteId}/unpublish

POST   /admin/nodes/{nodeId}/trash
POST   /admin/nodes/{nodeId}/restore
DELETE /admin/nodes/{nodeId}

POST   /admin/assets/upload
```

Public reading should normally happen directly through CloudFront rather than
Lambda:

```text
GET /published/tree.json
GET /published/notes/{noteId}/{revisionId}.json
GET /published/assets/{assetId}/{filename}
```

## 10. Frontend experience

### 10.1 Public reader

- Desktop sidebar containing nested folders and notes.
- Expand and collapse folders.
- Mobile drawer for the hierarchy.
- Breadcrumbs and note title.
- Clean, readable typography.
- Loading, empty, unavailable, and not-found states.
- Client-side title filtering over the small tree manifest.
- Shareable note URLs.

### 10.2 Owner mode

- Sign-in entry point that does not distract public readers.
- Inline switch between reading and editing.
- Create note and folder actions.
- Rename, move, and reorder hierarchy nodes.
- Draft and published status.
- Debounced autosave plus a manual save shortcut.
- Visible save status.
- Publish, unpublish, restore, and delete confirmations.
- Warning before leaving with unsaved changes.
- Useful keyboard navigation and shortcuts.

### 10.3 Editor MVP schema

Initial editor support:

- Paragraphs.
- Headings.
- Ordered and unordered lists.
- Bold and italic text.
- Links.
- Block quotes.
- Inline code and code blocks.
- Horizontal rules.
- Images once attachment uploads are implemented.

Tables, embeds, comments, collaborative cursors, and custom blocks are deferred.

## 11. Milestones

### Milestone 0 — Foundation

Status: complete.

Deliverables:

- npm workspace.
- Empty Vite React application.
- Empty TypeScript CDK application.
- Node.js version pin.
- Root build, test, lint, development, and CDK commands.
- Successful React build, infrastructure type check, unit test, and CDK
  synthesis.

Exit criteria:

- A clean checkout can install and verify both applications.
- No AWS resources have been deployed.

### Milestone 1 — Public reader with local fixtures

Goal: validate the navigation and reading experience before introducing AWS.

Work:

- Define runtime TypeScript schemas for nodes, manifests, and note documents.
- Add a small nested fixture tree.
- Implement the responsive application layout.
- Implement folder expansion and note selection.
- Render read-only Tiptap JSON.
- Implement hash-based routing and title filtering.
- Add empty, loading, error, and not-found states.

Exit criteria:

- A visitor can navigate a representative 100-node fixture hierarchy.
- Directly opening a supported note URL selects the correct note.
- The interface works on desktop and mobile.
- No AWS connection is required.

### Milestone 2 — S3 and CloudFront

Goal: serve published content from AWS.

Work:

- Add a versioned, encrypted S3 bucket with public access blocked.
- Define removal and retention policies explicitly.
- Add a CloudFront distribution with Origin Access Control.
- Restrict CloudFront to the published content path.
- Configure sensible cache policies:
  - Immutable note revisions and assets receive long cache lifetimes.
  - `tree.json` receives a short cache lifetime.
- Add development seed content.
- Configure browser CORS where required.
- Export the CloudFront base URL.

Exit criteria:

- Published fixtures load through CloudFront.
- Direct S3 object access is denied.
- Draft and private prefixes are inaccessible publicly.
- CDK tests verify encryption, versioning, public-access blocking, and
  CloudFront origin restrictions.

### Milestone 3 — Authentication and management API

Goal: establish a secure vertical slice for owner-only writes.

Work:

- Add an invitation-only Cognito User Pool.
- Add a public browser app client with authorization-code and PKCE support.
- Configure local and GitHub Pages callback/logout URLs.
- Add API Gateway HTTP API.
- Add JWT authorization to all management routes.
- Create the Rust workspace and the `notes-admin` Lambda.
- Build the Lambda through `cargo-lambda` as a release ARM64 artifact for
  `provided.al2023`.
- Add the first Axum route through the Lambda HTTP adapter.
- Implement authenticated tree and note reads.
- Implement a simple health or identity endpoint.
- Integrate sign-in, sign-out, session restoration, and expired-session handling
  in React.

Exit criteria:

- Public users cannot invoke management routes.
- The invited owner can sign in and call an authenticated endpoint.
- Self-service account creation is unavailable.
- No AWS credentials or OAuth client secret exist in the frontend.

### Milestone 4 — Note creation and inline editing

Goal: complete the primary writing workflow.

Work:

- Integrate Tiptap.
- Create new folders and notes.
- Load and edit drafts inline.
- Implement conditional draft saves using ETags.
- Add debounced autosave and explicit save.
- Create unique slugs.
- Publish and unpublish notes.
- Refresh the public manifest after publishing.
- Handle stale-tab conflicts.

Exit criteria:

- The owner can create, edit, save, publish, and unpublish a note.
- A published note becomes publicly readable.
- Draft content never becomes publicly accessible.
- Stale saves fail visibly rather than silently overwriting content.

### Milestone 5 — Hierarchy management and recovery

Goal: make the note collection practical to maintain.

Work:

- Rename nodes.
- Move notes and folders.
- Reorder siblings.
- Prevent hierarchy cycles.
- Enforce unique IDs and slugs.
- Move nodes to Trash.
- Restore trashed nodes.
- Add explicit permanent-delete confirmation.
- Define cleanup behavior for unreferenced revisions.

Exit criteria:

- Moving a node preserves its stable ID and note URL.
- A folder cannot be moved into itself or one of its descendants.
- Published manifests never reference missing content.
- Accidental deletion can be recovered.

### Milestone 6 — Attachments and polish

Goal: support normal blog content and improve day-to-day usability.

Work:

- Add authenticated upload initialization.
- Validate upload type and size.
- Insert uploaded images into editor documents.
- Publish assets through the public CloudFront path.
- Add upload progress and failure recovery.
- Improve keyboard and screen-reader accessibility.
- Add mobile and narrow-screen refinements.
- Add metadata useful to browsers and link sharing where GitHub Pages permits.

Exit criteria:

- The owner can upload and publish an image.
- Unsupported or oversized files are rejected.
- Published documents contain stable asset URLs.
- Core workflows are keyboard accessible.

### Milestone 7 — Delivery and operational safeguards

Goal: make releases repeatable and safe.

Work:

- Add a GitHub Actions workflow for tests and builds.
- Add GitHub Pages deployment.
- Configure Vite's GitHub Pages base path.
- Add AWS deployment through GitHub OIDC, without long-lived AWS keys.
- Add a USD 1 AWS Budget alert.
- Set CloudWatch log retention.
- Add production smoke tests.
- Document bootstrap, deployment, backup, restore, and owner-account recovery.

Exit criteria:

- Merging an approved frontend change deploys GitHub Pages.
- AWS deployments use short-lived GitHub OIDC credentials.
- A failed build cannot replace the last healthy deployment.
- Recovery procedures are written and tested at least once.

## 12. Testing strategy

### 12.1 Frontend

- Unit tests for hierarchy building, sorting, slug resolution, and route state.
- Component tests for the tree, note reader, editor status, and confirmations.
- Accessibility checks for core controls.
- End-to-end tests for public reading and the main owner workflow.

### 12.2 Lambda and content operations

- Rust unit tests for domain services, manifest parsing, and error mapping.
- Lambda HTTP adapter tests for API Gateway payload format 2.0.
- Runtime schema-validation tests.
- Conditional-write conflict tests.
- Publish-ordering and partial-failure tests.
- Folder-cycle and slug-conflict tests.
- Authorization tests for every mutation.
- Tests confirming private paths cannot be returned as public URLs.

### 12.3 Infrastructure

- CDK assertions for:
  - S3 versioning.
  - Encryption.
  - Public-access blocking.
  - CloudFront Origin Access Control.
  - Cognito self-sign-up configuration.
  - API route authorization.
  - Least-privilege Lambda access.
  - Log retention and budget resources.

### 12.4 Deployment smoke tests

- GitHub Pages returns the application.
- CloudFront returns the public manifest.
- A published fixture note is readable.
- Draft content returns an access error.
- Anonymous API mutation returns `401`.

## 13. Observability and operations

- Structured Lambda logs containing request IDs and operation names.
- Never log access tokens, note content, or upload contents.
- Short CloudWatch log retention for cost control.
- API Gateway and Lambda errors visible in CloudWatch.
- AWS Budget alert at a deliberately low threshold.
- S3 Versioning enabled from the beginning.
- Optional lifecycle rules for abandoned uploads and old unreferenced revisions.
- No always-on compute.

## 14. Cost controls

At the expected scale, usage should remain within or close to the relevant free
allowances, with negligible object storage and request charges.

Specific controls:

- No NAT Gateway.
- No VPC unless a future requirement makes it necessary.
- No provisioned database.
- No API Gateway cache.
- No WAF in the MVP.
- Long-lived caching for immutable published revisions.
- Short log retention.
- Lifecycle cleanup for abandoned uploads.
- AWS Budget notifications.

## 14.1 Rust Lambda build and cold-start policy

- Build a release ARM64 binary with `cargo-lambda`; do not compile Rust in the
  deployed Lambda environment.
- Use the `provided.al2023` runtime and a zip deployment package, not a
  container image.
- Enable release optimizations, link-time optimization, and symbol stripping.
- Keep one management Lambda instead of several lightly used functions.
- Initialize the S3 client once per execution environment and reuse it across
  warm invocations.
- Avoid provisioned concurrency in the MVP because its standing cost is not
  justified by low traffic.
- Keep dependencies and middleware intentionally small; public page reads stay
  on CloudFront and do not incur Lambda initialization.

Cold-start latency will be measured after the first deployed vertical slice.
Rust is a deliberate engineering choice and should remain only if the measured
experience and development workflow are satisfactory.

## 15. Risks and mitigations

### GitHub Pages routing

Risk: direct path-based single-page application routes are not natively
rewritten to `index.html`.

Mitigation: use hash routing for the MVP. Revisit static pre-rendering or a
different public host only if clean URLs and search-engine rendering become
priorities.

### Multi-object S3 updates

Risk: S3 does not provide a transaction spanning a note object and a manifest.

Mitigation: write immutable content before updating the manifest, use
conditional manifest writes, and tolerate harmless unreferenced objects.

### Stale browser tabs

Risk: one browser tab can overwrite edits made in another.

Mitigation: every write carries the last known ETag or revision. Conflicts
require reload or explicit resolution.

### Manifest growth

Risk: a single hierarchy manifest eventually becomes too large or contentious.

Mitigation: the expected 100-note scale is comfortably small. Hide storage
behind application interfaces so metadata can move to DynamoDB if scale or
query requirements change.

### Public revision retention

Risk: an unpublished or deleted note revision may remain reachable if someone
already knows its immutable URL.

Mitigation: remove it from all manifests immediately, then delete or expire the
public object according to the retention policy. Drafts must never be placed in
the published prefix.

### Rich-text compatibility

Risk: editor schema changes make old documents difficult to load.

Mitigation: version every document schema and implement migrations before
introducing breaking editor extensions.

## 16. Deferred features

The following are intentionally outside the MVP:

- Real-time collaboration.
- Comments and mentions.
- Multiple roles or editors.
- Per-note access control.
- Full-text search service.
- Backlinks and graph views.
- Tags beyond simple manifest metadata.
- Scheduled publishing.
- Custom editor blocks.
- Static HTML generation for every note.
- Analytics.
- Custom domain and Route 53 configuration.
- DynamoDB or OpenSearch.

## 17. Definition of MVP complete

The MVP is complete when:

- Public visitors can browse folders and read published notes.
- The owner can sign in securely.
- The owner can create folders and notes.
- Notes can be edited inline and saved reliably.
- Notes and folders can be moved and reordered.
- Notes can be published, unpublished, trashed, and restored.
- Published data and assets are served from AWS.
- The React application is deployed to GitHub Pages.
- All AWS infrastructure is defined by CDK.
- CI verifies builds and tests.
- Basic recovery and cost safeguards are enabled.
- No long-lived AWS credentials are stored in GitHub.

## 18. Immediate next steps

1. Implement the shared manifest and note schemas.
2. Build the public reader against local fixture data.
3. Add the versioned private S3 bucket and CloudFront distribution in CDK.
4. Connect the reader to a seeded public manifest through CloudFront.
5. Add Cognito, API Gateway, and the first authenticated Lambda operation.
