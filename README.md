# Aria

Git-native codebase indexer for LLMs. Extracts functions, types, variables, and call graphs so LLMs can query code structure and semantics without wasting context on discovery.

### Supported Languages

- Go
- Rust
- C

## Build

```bash
# debug build
make build

# optimized build
make release
```

## Usage

```bash
# Build the index (run from project root)
aria index

# Print source code for any symbol
aria source <name>

# Print source code for a target symbol
aria source <name> --kind struct

# Show call graph in both directions
aria callstack <name> 

# Show forward call graph (callees)
aria callstack <name> -f

# Show backwards call graph (callers)
aria callstack <name> -b

# Limit call graph depth (default: 2, 0 = unlimited)
aria callstack <name> -d 3
```

## How it works

`aria index` will:
- Walk the source tree
- Parse each file with tree-sitter
- Resolve call targets across files
- Write to `.aria`: 
    - `.aria/index.json` with function indexes
    - `.aria/README.md` with usage instructions

Per-function LLM summaries are optional. Enable `features.summaries` in `.aria/config.toml`.

## Goals

- **Incremental updates.** Re-index only changed files using `git diff`. Reuse summaries when function AST hasn't changed.
- **More languages.** Python, TypeScript, Java, and others via tree-sitter grammars.
- **Semantic search.** Embed function summaries as vectors for natural language queries.
- **Git hooks and CI.** Keep the index in sync with code automatically.

## Documentation

See [SPEC.md](SPEC.md) for the full specification.
