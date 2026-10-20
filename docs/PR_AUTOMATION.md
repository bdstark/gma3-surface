# Independent review and PR acceptance

The pinned shared workflows in gma3-mcp independently inspect this repository. No product code is imported by this integration. The repository currently has no top-level licence; review must respect the separate notices on vendored modules and must not infer a licence grant for the rest of the repository.

The OPENAI_API_KEY must be configured in the main-only codex-review environment. After the workflow PR is reviewed and merged, set PR_AUTOMATION_ENABLED=true and PR_MERGE_ENABLED=false for review-only validation. Manual workflow dispatch can review an eligible PR on main. The CI workflow runs Rust unit tests against a fake plugin and a Lua console mock; it never runs live console or USB tests.

Require clean official code and security reviews, an independent ready verdict, passing current-head/base CI and resolved discussions. Keep merge disabled until the end-to-end smoke tests pass. Then permit Actions to approve PRs and enable PR_MERGE_ENABLED=true. GitHub protections remain enforced. Stacked branches are preserved and retargeted before cleanup. Workflow and policy changes cannot accept themselves.

Disable acceptance with PR_MERGE_ENABLED=false, or all automation with PR_AUTOMATION_ENABLED=false. The existing chat monitor stays until rollout validation completes.

## Protected API key

Add `OPENAI_API_KEY` under Settings → Environments → **codex-review** → Environment secrets. This environment permits only the **main branch** (custom branch policy, no tag rules), rejecting PR merge refs and feature branches. The independent job receives this environment secret directly; callers explicitly forward the secret name through each reusable-workflow boundary; the main-only environment supplies its value. Remove the repository-level OPENAI_API_KEY copy: repository secrets remain available to PR-authored same-repository workflows regardless of the intended listener permissions. Do not enable reviews until the environment secret is populated. Missing credentials fail closed. Workflow/policy changes require review before reaching main.

GitHub requires explicit named-secret forwarding at every reusable-workflow boundary even for environment-only secrets. This does not restore a repository secret; codex-review remains the only key store.

Independent reviews are reused for the exact PR head/base across automatic and manual triggers. A queued job rechecks evidence after acquiring its per-PR lock, before spending API tokens. New head or base revisions require a fresh review.
