# Aria

## Abstract

Aria (short for Ariadne, the Greek figure who gave Theseus a thread to navigate the labyrinth) is a git-native codebase indexer that provides LLMs with structural and semantic understanding of code without runtime file scanning. It maintains incrementally-updated indexes tied to git commits, enabling efficient context retrieval for LLM-assisted development workflows.

Aria serves the research phase of the Research → Plan → Implement workflow.[1] Its output (structured metadata with concise semantics) feeds directly into planning prompts, front-loading codebase discovery so coding agents can focus on implementation rather than navigation.

```
Developer Query → Aria → Structured Context → LLM Planning → Implementation
```


## 1. Problem Statement

### 1.1 The Context Window Constraint

LLMs operate under a fundamental constraint: they are stateless. The only input that influences output is the current context window. Every token spent on codebase discovery is a token unavailable for reasoning about the actual task.

Practitioner observations suggest quality degradation begins around 40% context utilization, a phenomenon termed the "dumb zone." The threshold varies by task complexity; simpler tasks tolerate higher utilization, while complex multi-step reasoning degrades earlier.[2]

An LLM that consumes 60% of its context window understanding code structure has limited capacity remaining for high-quality tool selection and code generation.

### 1.2 The Redundant Discovery Problem

When an LLM agent queries "what does function X call?" or "where is authentication handled?", it must either:

1. **Scan files.** Expensive in tokens and latency. Limited by context window.
2. **Request user context.** Friction. Breaks flow.
3. **Infer from training data.** Hallucination risk. Stale.

This discovery process is repeated every session, every query, every context window. Yet the information being discovered (function signatures, call relationships, import graphs) is static between commits.

### 1.3 The Staleness-Truth Tradeoff

Existing solutions fall into two categories:

1. **Static documentation.** Human-written or AI-generated descriptions of code. These tend to drift from reality over time. Empirically, accuracy decreases as you move further from source code: function names are usually accurate, comments less so, external documentation least of all.[3]

2. **Runtime analysis.** Tools like Aider's repo-map that regenerate context every session. Accurate but wasteful: the same computation repeats indefinitely.


## 2. Prior Art

| Tool | Approach | Limitation |
|------|----------|------------|
| **Aider repo-map** | Extracts symbols with tree-sitter and ranks them per request with PageRank. | Recomputes from scratch every session. |
| **Sourcegraph SCIP** | Compiler-accurate indexing with protobufs for IDE navigation. | Requires enterprise infrastructure. |
| **Repomix** | Reduces entire repository into a single file. | No intelligence about relevance or context budget. |
| **AskCodebase/codeindex** | Builds call graphs via local server. | Unmaintained since 2023 with no incremental updates. |
| **Static markdown docs** | Human or AI-written codebase descriptions. | Drifts from source code over time. |

The gap: no tool provides persistent, git-native indexes with incremental updates and LLM-targeted output without requiring infrastructure.


## 3. Design Principles

### 3.1 Code Is Truth

Documentation and comments often drift from actual behavior. Use the AST as the sole source of structural truth. Summaries describe what functions *do*, not what comments *claim*.

### 3.2 Signal Over Noise

Every token in an LLM's context window has opportunity cost. Dumping thousands of tokens of "potentially relevant" code pushes the LLM into degraded performance territory before it can reason. Use only concise and relevant data.

### 3.3 Compute Once, Query Many

Code structure only changes when code changes. Re-discovering it every session is redundant. Index on commit, query from cache, re-index only what changed.

### 3.4 Use Git for Change Detection

Git already tracks which files changed. No need to reinvent it. Use `git diff` as the sole source of truth for re-indexing.

### 3.5 Explicit Depth Control

Different queries require different levels of detail. Assumptions about depth lead to either missing context or wasted tokens. Require explicit depth specification. Never assume what the caller wants.


## 4. Solution Overview

Aria is a codebase indexer that:

1. Parses source files via tree-sitter (structural indexing, no LLM)
2. Resolves symbols to their definitions across files (static analysis, no LLM)
3. Stores indexes in git-tracked JSON files (.aria/)
4. Updates incrementally based on git diffs
5. Summarizes function behavior via LLM (enables semantic context)
6. Embeds summaries as vectors (enables semantic search)
7. Exposes a query API optimized for LLM context injection

Aria is not a coding agent. It is infrastructure that pre-computes codebase structure so coding agents don't spend context on discovery.

### 4.1 Core Workflow

```
Source Files → Parse → Resolve → Summarize → Index
```

| Stage | Description | LLM Required |
|-------|-------------|--------------|
| Parse | Extracts nodes (functions, classes) via tree-sitter | No |
| Resolve | Creates bidirectional edges (calls ↔ called_by) via static analysis | No |
| Summarize | Generates behavior summaries via LLM | Yes (optional) |
| Embed | Generates vectors for semantic search | Yes (optional) |

Structural indexing (parse, resolve) requires no LLM and provides full call graph functionality. Summarization and embeddings require LLM API access and enable semantic search.


## 5. Success Metrics

Targets TBD after initial baseline measurement. The following will be measured:

| Metric | Measurement Method | Target Source |
|--------|--------------------| --------------|
| Symbol extraction recall | Compare against ctags on reference repo | Set after baseline |
| Resolution rate | aria stats on reference repo | Set after baseline |
| Incremental update latency | Wall-clock, 50 file change simulation | Set after baseline |
| Query response time | Wall-clock, depth=3 trace | User feedback on acceptable wait |

## 6. Architecture

### 6.1 Component Overview

| Component | Purpose | LLM Required | Called By |
|-----------|---------|--------------|-----------|
| **Parser** | Tree-sitter AST extraction | No | Indexer |
| **Resolver** | Map symbols to definitions | No | Indexer |
| **Indexer** | Orchestrate parsing and storage | No | CLI / CI |
| **Differ** | Identify changed files via git | No | Indexer |
| **Summarizer** | Generate function behavior summaries | Yes (optional) | Indexer |
| **Embedder** | Generate vector embeddings for semantic search | Yes (optional) | Indexer |
| **Querier** | Structural queries (trace, usages) | No | CLI |
| **Searcher** | Semantic search over embeddings | No | CLI |
| **Validator** | Index integrity verification | No | CLI / CI |

### 6.2 Supported Languages

Aria uses tree-sitter grammars. Initial implementation targets ordered by priority: Go, C, Rust, Python. 

Additional languages can be added by implementing a language-specific resolver. The parser itself is language-agnostic (tree-sitter handles grammar).


## 7. Design Details

### 7.1 Index Storage

Indexes are stored as JSON in `.aria/` at the repository root.

**Rationale:**
- Human-readable and git-friendly
- No external database dependency
- Sufficient performance likely for large repositories (beyond this, query latency may exceed targets; see §9.4 for sharding consideration)

**File structure:**
```
.aria/
├── index.json        # Structural index (functions, types, calls, summaries)
├── config.toml       # Configuration
├── embeddings.idx    # Qualified names for embeddings (optional)
├── embeddings.bin    # Vector embeddings as raw f32 (optional)
└── cache/            # Transient computation cache
```

### 7.2 Index Schema

```json
{
  "version": "string",
  "commit": "string",
  "indexed_at": "ISO8601 timestamp",
  "files": {
    "<file_path>": {
      "ast_hash": "string",
      "functions": [
        {
          "name": "string",
          "qualified_name": "string",
          "ast_hash": "string",
          "line_start": "integer",
          "line_end": "integer",
          "signature": "string",
          "summary": "string | null",
          "receiver": "string | null",
          "scope": "string",
          "calls": [
            {
              "target": "string",
              "raw": "string",
              "line": "integer"
            }
          ],
          "called_by": ["string"]
        }
      ],
      "types": [
        {
          "name": "string",
          "qualified_name": "string",
          "kind": "string",
          "line_start": "integer",
          "line_end": "integer",
          "summary": "string | null",
          "methods": ["string"]
        }
      ]
    }
  }
}
```

**Field notes:**
- `ast_hash` (file): Hash of file contents for quick change detection
- `ast_hash` (function): Hash of function source bytes for per-function change detection
- `receiver`: Go receiver type, null for languages without receivers
- `scope`: One of "public", "static", "internal"
- `kind`: One of "struct", "interface", "typedef", "enum"
- `methods`: Qualified names of methods with this receiver/type
- `calls[].target`: Resolved qualified name of the called function (or `[unresolved]` if resolution fails)
- `calls[].raw`: Original call expression as written in source (e.g., `pkg.Foo`, `obj.Method()`, `Bar`)
- `calls[].line`: 1-indexed line number of the call site
- `called_by`: Qualified names of functions that call this function (populated during resolution)

### 7.3 Diff-Based Incremental Updates

Use `git diff` to identify changed files between commits. For each changed file:

1. **Deleted files:** Remove from index
2. **Added/Modified files:** Re-parse with tree-sitter, extract symbols/calls, update index

For summaries (when enabled), compare the normalized AST hash before regenerating. If the hash matches, reuse the existing summary. This avoids LLM calls for formatting-only changes.

### 7.4 Cross-File Propagation Algorithm

After updating individual files, propagate changes through the graph:

**Step 1: Collect changed symbols**

For each modified file, diff old vs new symbol sets:
- `added`: symbols in new but not old
- `removed`: symbols in old but not new
- `modified`: symbols in both but with changed signature or AST hash

**Step 2: Update outbound edges (`calls`)**

For each added/modified function, re-extract its `calls` list from the new AST.

**Step 3: Update inbound edges (`called_by`)**

For each removed symbol:
- Look up the symbol's `called_by` list from the index (O(1) lookup)
- For each caller in that list, remove the symbol from the caller's `calls` array
- If the call site still exists in the caller's AST but the target is gone, mark as `[unresolved]`

For each added symbol:
- Query existing `[unresolved]` references that match the new qualified name
- Promote matching references to resolved edges
- Update both `calls` (on the caller) and `called_by` (on the new symbol)

For each modified symbol (signature change):
- Qualified name unchanged, so no edge changes needed
- Mark for potential caller re-summarization (Step 4)

**Complexity note:** Edge updates are O(1) per edge due to the bidirectional `calls`/`called_by` structure. Total propagation is O(changed_symbols * average_edge_count).

**Step 4: Summary invalidation (when summaries enabled)**

A caller's summary is invalidated if:
1. Its own AST hash changed, OR
2. Any callee's *signature* changed (behavior may have changed)

Invalidation is non-recursive: if A calls B calls C, and C's signature changes, B is invalidated but A is not (unless B's signature also changes).

**Rationale:** Recursive invalidation causes cascade re-summarization on any leaf change. Non-recursive bounds the blast radius to direct callers only. This is a pragmatic tradeoff; users who need full propagation can run `aria index --rebuild`.

**Not yet measured:** The actual blast radius in practice. If callers-of-callers frequently need re-summarization, this policy should be revisited.

### 7.5 AST Normalization for Summary Caching

Not all code changes affect behavior. Normalization strips non-semantic elements before hashing.

**Normalization rules:**
1. Remove all comments
2. Remove all documentation
3. Normalize whitespace
4. Normalize semantically-equivalent syntax variations (e.g., trailing commas)

**Result:** Refactoring that doesn't change logic doesn't trigger re-summarization.

### 7.6 Symbol Resolution

A call graph edge "A calls B" is only useful if we know which B.

**General resolution strategy (in order):**

1. **Same-file definition:** Check if symbol is defined in current file
2. **Local scope:** Check enclosing scope (package, module, translation unit)
3. **Explicit dependency:** Check imports, includes, or equivalent
4. **Standard library:** Check against known language builtins/stdlib
5. **Unresolved:** Mark as unresolved with best-guess source

**Confidence levels:**

| Confidence | Criteria |
|------------|----------|
| `high` | One match via direct dependency |
| `medium` | One match via indirect means |
| `low` | Multiple matches, selected by heuristic |
| `unresolved` | No match |

**Unresolved symbols are retained in the index.** Partial information is better than no information. The index reports resolution rate as a quality metric via `aria stats`.

### 7.7 Summary Generation

**When enabled**, Aria generates behavior summaries for functions.

**Input to LLM:**
```
Summarize what this function does in 1-2 sentences. Focus on behavior, not implementation details. Do not repeat documentation comments.

Function: {signature}
Body: {function_body}
```

**Output format:**
```
Validates credentials against database, creates session token on success, returns error on failure.
```

**Prompt-level Constraints:**
- Configurable max tokens per summary (default: 100)
- No markdown formatting
- No implementation details ("uses a for loop")
- Focus on observable behavior ("returns X", "raises Y", "modifies Z")

**Cost control:**
- Summaries generated only on AST change
- Batch requests where possible (up to 20 functions per API call)
- Model configurable (default: claude-3-haiku)

### 7.8 Query Output Format

Query output is optimized for LLM context injection: minimal tokens, maximum signal.

**Design principle:** A human debugging would want prose. An LLM context window wants coordinates.

**Format:**

```
# aria query trace auth/login.c:validate_credentials --depth 2

auth/login.c:validate_credentials (auth/login.c:10-45)
│ Validates username and password, returns user_id or -1 on failure.
│
├── db/query.c:db_execute (db/query.c:23-45)
│   │ Executes SQL query with parameter binding, returns result set.
│   │
│   ├── db/pool.c:pool_get_conn (db/pool.c:10-20)
│   └── db/pool.c:pool_release_conn (db/pool.c:22-30)
│
├── auth/hash.c:hash_password (auth/hash.c:8-15)
│   │ Hashes password using bcrypt with configured rounds.
│   │
│   └── [external] libcrypt:crypt_r
│
└── [unresolved] LOG_INFO
```

**Properties:**
- File paths are exact (copy-pasteable)
- Line numbers are exact (navigable)
- Summaries are single-line (scannable)
- Tree structure shows relationships (comprehensible)
- Unresolved symbols are explicit (no hidden gaps)

**Token budget:**
```bash
aria query trace src.auth.login --depth 2 --budget 500
```

When budget is specified, output truncates least-relevant branches first (configurable by PageRank or call frequency).


## 8. Error Handling and Degraded Modes

Aria should fail gracefully and configurably report partial results rather than abort entirely.

### 8.1 Parse Failures

**Condition:** tree-sitter fails to parse a file (syntax error, unsupported language, binary file)

**Behavior:**
- Log warning: `WARN: Failed to parse {filepath}: {error}`
- Skip file, continue with remaining files
- Report in `aria stats`: "Files skipped: N (parse errors)"

**User action:** Fix syntax errors or add file pattern to `.aria/config.toml` ignore list.

### 8.2 LLM API Failures

**Condition:** Summarization API call times out or returns error

**Behavior:**
- Retry with exponential backoff (max 3 attempts)
- If all retries fail, set `summary: null` for affected functions
- Log warning: `WARN: Summary generation failed for {qualified_name}: {error}`
- Continue indexing; structural index is still valid

**User action:** Check API key, rate limits, or network connectivity. Re-run `aria update --resummarize` to retry failed summaries.

### 8.3 Shallow Git History

**Condition:** `git diff` fails because history is shallow (common in CI)

**Behavior:**
- Detect shallow clone: check if `.git/shallow` exists
- Fall back to full re-index with warning: `WARN: Shallow clone detected, performing full index`
- Alternatively, if `--from` commit is specified but unreachable, error with actionable message: `ERROR: Commit {sha} not found. Fetch deeper history with: git fetch --deepen=N`

**User action:** Configure CI to fetch sufficient history, or accept full re-index cost.

### 8.4 Index Corruption

**Condition:** `aria validate` detects schema violations, missing files, or hash mismatches

**Behavior:**
- Report specific corruption: `ERROR: Index references {filepath} but file does not exist`
- Exit with non-zero status
- Do not attempt auto-repair (user should decide)

**User action:** Run `aria index --rebuild` to regenerate from scratch.

### 8.5 Concurrent Modification

**Condition:** Another process modifies `.aria/index.json` during update

**Behavior:**
- Use file locking (flock on Unix) to prevent concurrent writes
- If lock acquisition fails after timeout (5s), abort with: `ERROR: Could not acquire index lock. Another aria process may be running.`

**User action:** Wait for other process to complete, or manually remove stale lock file if process crashed.


## 9. CLI Interface

### 9.1 Indexing Commands

```bash
# Full index build (first time or rebuild)
aria index

# Incremental update (auto-detects HEAD~1..HEAD)
aria update

# Incremental update with explicit range
aria update --from <commit> --to <commit>

# Update only staged files (for pre-commit hook)
aria update --staged

# Check if index is current (exits non-zero if stale)
aria check

# Validate index integrity
aria validate

# Show index statistics
aria stats

# Show functions changed since last index
aria diff
```

### 9.2 Query Commands

```bash
# Get function details
aria query function <qualified_name>

# Trace call graph (default depth: 2)
aria query trace <qualified_name> [--depth N] [--budget TOKENS]

# Find all usages of a symbol
aria query usages <symbol> [--budget TOKENS]

# Get file overview
aria query file <path>

# List all functions in a file
aria query list <path>

# Semantic search (requires embeddings)
aria search "<natural language query>" [--limit N]
```

### 9.3 Configuration Commands

```bash
# Initialize aria in a repository and creates initial index
aria init

# Initialize with index excluded from git (adds .aria/ to .gitignore)
aria init --local

# Configure LLM API for summaries
aria config set llm.provider anthropic
aria config set llm.api_key <key>
aria config set llm.model claude-3-haiku

# Enable/disable features
aria config set features.summaries true
aria config set features.embeddings true

# Install git hooks
aria hooks install

# Uninstall git hooks
aria hooks uninstall
```


## 10. Git Integration

### 10.1 The CI Commit Problem

If CI generates and commits index updates, developers must pull before their next push. This creates friction and noisy git history.

### 10.2 Solution: Pre-Commit Hook + CI Enforcement

**Pre-commit hook** updates index atomically with code:

```bash
#!/bin/sh
# .git/hooks/pre-commit
aria update --staged
git add .aria/
```

**Flow:**
1. Developer stages code changes
2. Pre-commit hook runs `aria update --staged`
3. Index changes are staged automatically
4. Commit includes both code and index
5. Push contains everything atomically

**CI enforces** but does not generate:

```yaml
# .github/workflows/aria.yml
name: Aria Index Verification
on: [push, pull_request]

jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 2  # Need parent commit for diff
      - name: Install Aria
        run: cargo install aria
      - name: Verify index is current
        run: aria check
```

CI fails if index is stale. It does not fix, only enforces.


## 11. Appendix

### 11.1 Why Not SCIP

SCIP provides compiler-accurate indexing but requires language-specific compiler plugins and infrastructure (Sourcegraph instance or similar). Its output format is optimized for IDE navigation, not LLM context, and doesn't include behavior summaries.

**Decision:** Build independent for control and LLM-optimization. Consider SCIP adapter later if users need compiler accuracy for specific languages.

### 11.2 Known Limitations

Aria performs static analysis only. The following cannot be resolved:

- Dynamic dispatch and runtime function selection
- Function pointers and indirect calls
- Macro-generated or template-generated code (before expansion)
- Runtime code generation and eval
- Reflection-based calls
- Symbols from transitive dependencies not explicitly declared
- Build-generated code (protobuf, codegen) unless pre-generated and committed
- Monorepo virtual paths (Bazel/Buck runfiles)

### 11.3 Embedding Specification

Embeddings enable semantic search over function summaries.

- **Input:** Function signature + summary (falls back to signature only if no summary)
- **Model:** Configurable, default `nomic-embed-text` via Ollama (768 dimensions)
- **Storage:** Binary format in `.aria/`:
  - `embeddings.idx`: Newline-separated qualified names (sorted alphabetically)
  - `embeddings.bin`: Raw little-endian f32 values, 768 floats per function, in same order as `.idx`
- **Updates:** Re-embed only functions missing from the store
- **Query:** `aria search "<query>" --limit N` returns top-k by cosine similarity

### 11.4 Future Consideration: Index Sharding

For single-developer or small-team usage, a single `index.json` works. At scale (large functions counts, multiple developers touching different files), merge conflicts on `.aria/index.json` become problematic.

**Potential solutions:**
- Shard index by file: `.aria/files/<path_hash>.json`
- Regenerate on conflict rather than merge
- Store index outside git, fetch from shared cache

Not required for initial implementation. Revisit if team adoption becomes a goal or if query latency exceeds targets at scale.

### 11.5 References

- Aider repo-map: https://aider.chat/docs/repomap.html
- Tree-sitter: https://tree-sitter.github.io/tree-sitter/
- SCIP: https://github.com/sourcegraph/scip
- Context engineering: 12 Factor Agents (AI Engineer, 2024)
- RPI workflow: Anthropic cookbook, community patterns


## Footnotes

[1]: The Research → Plan → Implement workflow is described in community practice and the Anthropic cookbook. The core insight is that separating discovery (research) from execution (implement) via an explicit planning step improves LLM output quality by keeping each phase's context focused.

[2]: Dex, "Advanced Context Engineering for Coding Agents," AI Engineer Conference, June 2025. https://www.youtube.com/watch?v=rmvDxxNubIg (timestamp 5:55–6:26). The 40% threshold is presented as a rough guideline that varies by task complexity.

[3]: This observation about documentation accuracy decreasing with distance from source code is from the same talk (timestamp ~14:00), where it's framed as "the amount of lies you can find" increasing from code → comments → documentation.