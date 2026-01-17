# Aria Codebase Index

This project was indexed with [aria](https://github.com/zydee3/aria), a codebase indexer for AI agents.

## Commands

```bash
# Structure Search - Search by function definition and call graph
aria query function <name>      # Get function details (signature, calls, callers)
aria query trace <name>         # Trace call graph (what does this call?)
aria query usages <name>        # Find callers (what calls this?)
aria query file <path>          # Get file overview (types, functions)
aria query list                 # List all functions

# Semantic Search - Search by natural language
aria search "natural language"  # Semantic search (requires: aria embed)
```

## Examples

```bash
# Find a function by partial name
aria query function HandleRequest

# Trace what main() calls, 3 levels deep
aria query trace main --depth 3

# Find all callers of a function
aria query usages Validate

# Semantic search
aria search "functions that handle errors"
```
