# Aria Codebase Index

This project was indexed with [aria](https://github.com/zydee3/aria), a codebase indexer for AI agents.

## Quick Reference

```bash
# Build the index
aria index

# Print source code for any symbol (functions, types, variables)
aria source <name>                         # Search all symbol kinds
aria source <name> --kind function         # Filter to functions only
aria source <name> --kind struct           # Filter to structs only

# Show call graph
aria callstack <name>                      # Both directions (callers + callees)
aria callstack <name> -f                   # Forward only (what does this call?)
aria callstack <name> -b                   # Backward only (what calls this?)
aria callstack <name> -d 3                 # Depth limit (default: 2, 0 = unlimited)
```

## Finding Symbols

`aria source` searches functions, types (struct, enum, typedef, interface), and variables by name. It matches exact names first, then partial (contains).

```bash
# Find any symbol by name
$ aria source handle_request
--- pkg.handle_request (./handler.go:10-45) ---
func handle_request(w http.ResponseWriter, r *http.Request) {
    ...
}

# Filter to a specific kind
$ aria source Config --kind struct
--- pkg.Config (./config.go:5-12) ---
type Config struct {
    Port int
    Host string
}
```

Available kinds: `function`, `struct`, `enum`, `typedef`, `interface`, `variable`

## Call Graph

`aria callstack` shows the call graph for a function. By default it shows both directions.

### Forward Trace (what does this function call?)
```bash
$ aria callstack main -f
[0] main (./main.go:10-50)
[1] - process (./proc.go:20-80)
[2] -- handler (./handler.go:5-30)
[3] --- [external] [libc:malloc]
```

### Backward Trace (what calls this function?)
```bash
$ aria callstack handler -b
handler (./handler.go:5-30)
  called by:
  └── process (./proc.go:20-80)
      └── main (./main.go:10-50)
```

### Both Directions (default)
```bash
$ aria callstack process
process (./proc.go:20-80)
  called by:
  └── main (./main.go:10-50)

[0] process (./proc.go:20-80)
[1] - handler (./handler.go:5-30)
```
