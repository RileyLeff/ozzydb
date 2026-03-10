# Review Notes — v4

Persistent notes on settled implementation choices and review caveats for the v4 rewrite.

## Current Notes

### Review tooling may degrade independently of the code under review

Gemini CLI currently emits MCP startup noise in this environment, and one-shot Claude CLI review calls may hang. Use available external output when it is useful, but do not block implementation checkpoints on flaky reviewer process behavior. Record the degradation and continue with explicit self-review plus tests.

### Early v4 scaffolding should prefer deletion over speculative abstraction

Phase 1 crate scaffolding is allowed to be narrow and incomplete. It should model the v4 architecture directly, avoid compatibility layers, and delete redundant fields or identifiers as soon as they are discovered.
