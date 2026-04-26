## US-004b: Undo `talon embed` subcommand and fold embed pass into `talon sync`

### Goal
Remove the standalone `talon embed` CLI command and fold the embed pass into `talon sync`. Per spec §5, `--fast` on sync skips the embed pass; there is no separate `talon embed` command.

### Acceptance Criteria
1. Delete the `"embed" => emit_embed(...)` arm from talon-cli/src/command.rs
2. Delete emit_embed function entirely (do NOT leave as hidden alias)
3. Remove EmbedInput / EmbedResponse from TalonInput / TalonResponse in talon-core/src/tool.rs
4. Extend run_sync(conn, vault_root, lock_path, indexer_config, embed_config: Option<EmbedPassOptions>, inference: Option<&InferenceClient>) — when embed_config is Some AND inference is Some AND not --fast, run embed pass after reconcile_deletions
5. Extend SyncResponse with: { embedded: u32, embed_failed: u32, dimension_mismatch: bool, embed_remediation: Option<String>, embed_diagnostics: Vec<String> }
6. Update emit_sync in command.rs: when --fast absent, build InferenceClient + EmbedPassOptions from config; --fast skips inference build
7. Update output.rs::emit_sync_human to print embed sub-line
8. Existing wiremock embed tests in talon-core/src/embed/runner.rs continue to pass
9. Add integration test crates/talon-core/tests/sync_embed.rs

### Current State
- `talon embed` command exists as CLI subcommand
- `talon sync` runs but doesn't embed
- Embed pass exists in talon-core/src/embed/runner.rs
- InferenceClient exists for LLM calls

### Steps
1. Explore codebase: understand current command.rs, tool.rs, sync code, embed module, output.rs
2. Remove `talon embed` CLI command (command.rs)
3. Remove EmbedInput/EmbedResponse from tool.rs
4. Extend run_sync signature and body to optionally run embed pass
5. Extend SyncResponse with embed fields
6. Update emit_sync to build inference client when needed
7. Update emit_sync_human to print embed info
8. Run tests: `cargo test` and `just verify`
9. Add integration test sync_embed.rs
10. Commit changes