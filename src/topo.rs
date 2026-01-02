use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// Assign levels to functions based on call dependencies.
/// Level 0: functions that call no resolved functions (leaf nodes)
/// Level N: functions that only call functions at levels 0..N-1
///
/// Returns a map from qualified_name to level.
/// Functions in cycles are assigned the same level (the max of their dependencies + 1).
pub fn assign_levels(
    functions: &HashSet<String>,
    calls: &HashMap<String, HashSet<String>>,
) -> HashMap<String, usize> {
    // Build reverse graph: who depends on me (who calls me)?
    // This isn't needed for level assignment, but useful for cycle handling.

    // First, identify strongly connected components (cycles)
    let sccs = find_sccs(functions, calls);

    // Map each function to its SCC index
    let mut func_to_scc: HashMap<&str, usize> = HashMap::new();
    for (scc_idx, scc) in sccs.iter().enumerate() {
        for func in scc {
            func_to_scc.insert(func, scc_idx);
        }
    }

    // Build SCC-level call graph
    let mut scc_calls: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (caller, callees) in calls {
        let Some(&caller_scc) = func_to_scc.get(caller.as_str()) else {
            continue;
        };
        for callee in callees {
            let Some(&callee_scc) = func_to_scc.get(callee.as_str()) else {
                continue;
            };
            // Don't add self-edges at SCC level
            if caller_scc != callee_scc {
                scc_calls.entry(caller_scc).or_default().insert(callee_scc);
            }
        }
    }

    // Compute SCC levels using Kahn's algorithm (reverse direction)
    // Level 0 = SCCs with no outgoing edges (no callees)
    let num_sccs = sccs.len();
    let mut scc_levels: HashMap<usize, usize> = HashMap::new();

    // Count outgoing edges for each SCC
    let mut out_degree: Vec<usize> = vec![0; num_sccs];
    for (scc_idx, callees) in &scc_calls {
        out_degree[*scc_idx] = callees.len();
    }

    // Build reverse edges: who calls this SCC?
    let mut reverse_edges: HashMap<usize, Vec<usize>> = HashMap::new();
    for (caller_scc, callees) in &scc_calls {
        for callee_scc in callees {
            reverse_edges.entry(*callee_scc).or_default().push(*caller_scc);
        }
    }

    // Start with SCCs that have no callees (out_degree = 0)
    let mut queue: VecDeque<usize> = VecDeque::new();
    for scc_idx in 0..num_sccs {
        if out_degree[scc_idx] == 0 {
            queue.push_back(scc_idx);
            scc_levels.insert(scc_idx, 0);
        }
    }

    // Process in topological order (from leaves up)
    while let Some(scc_idx) = queue.pop_front() {
        let current_level = scc_levels[&scc_idx];

        // Update all SCCs that call this one
        if let Some(callers) = reverse_edges.get(&scc_idx) {
            for &caller_scc in callers {
                out_degree[caller_scc] -= 1;

                // Caller's level is max of all callee levels + 1
                let new_level = current_level + 1;
                let existing = scc_levels.entry(caller_scc).or_insert(0);
                *existing = (*existing).max(new_level);

                if out_degree[caller_scc] == 0 {
                    queue.push_back(caller_scc);
                }
            }
        }
    }

    // Map SCC levels back to function levels
    let mut levels: HashMap<String, usize> = HashMap::new();
    for func in functions {
        if let Some(&scc_idx) = func_to_scc.get(func.as_str()) {
            let level = scc_levels.get(&scc_idx).copied().unwrap_or(0);
            levels.insert(func.clone(), level);
        }
    }

    levels
}

/// Find strongly connected components using Kosaraju's algorithm.
/// Returns SCCs in reverse topological order (leaves first).
fn find_sccs(
    functions: &HashSet<String>,
    calls: &HashMap<String, HashSet<String>>,
) -> Vec<Vec<String>> {
    // Sort functions for deterministic iteration order
    let sorted_functions: BTreeSet<&String> = functions.iter().collect();

    // First DFS pass: compute finish order
    let mut visited: HashSet<&str> = HashSet::new();
    let mut finish_order: Vec<&str> = Vec::new();

    for func in &sorted_functions {
        if !visited.contains(func.as_str()) {
            dfs_first(func, calls, functions, &mut visited, &mut finish_order);
        }
    }

    // Build reverse graph
    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    for func in &sorted_functions {
        reverse.entry(func.as_str()).or_default();
    }
    for (caller, callees) in calls {
        if !functions.contains(caller) {
            continue;
        }
        // Sort callees for deterministic order
        let mut sorted_callees: Vec<&String> = callees.iter().collect();
        sorted_callees.sort();
        for callee in sorted_callees {
            if functions.contains(callee) {
                reverse.entry(callee.as_str()).or_default().push(caller.as_str());
            }
        }
    }

    // Second DFS pass: find SCCs in reverse finish order
    let mut visited: HashSet<&str> = HashSet::new();
    let mut sccs: Vec<Vec<String>> = Vec::new();

    for func in finish_order.into_iter().rev() {
        if !visited.contains(func) {
            let mut scc: Vec<String> = Vec::new();
            dfs_second(func, &reverse, &mut visited, &mut scc);
            sccs.push(scc);
        }
    }

    sccs
}

fn dfs_first<'a>(
    node: &'a str,
    calls: &'a HashMap<String, HashSet<String>>,
    functions: &'a HashSet<String>,
    visited: &mut HashSet<&'a str>,
    finish_order: &mut Vec<&'a str>,
) {
    visited.insert(node);

    if let Some(callees) = calls.get(node) {
        // Sort callees for deterministic traversal order
        let mut sorted_callees: Vec<&String> = callees.iter().collect();
        sorted_callees.sort();
        for callee in sorted_callees {
            if functions.contains(callee) && !visited.contains(callee.as_str()) {
                dfs_first(callee, calls, functions, visited, finish_order);
            }
        }
    }

    finish_order.push(node);
}

fn dfs_second<'a>(
    node: &'a str,
    reverse: &HashMap<&str, Vec<&'a str>>,
    visited: &mut HashSet<&'a str>,
    scc: &mut Vec<String>,
) {
    visited.insert(node);
    scc.push(node.to_string());

    if let Some(callers) = reverse.get(node) {
        // Sort callers for deterministic traversal order
        let mut sorted_callers: Vec<&str> = callers.iter().copied().collect();
        sorted_callers.sort();
        for caller in sorted_callers {
            if !visited.contains(caller) {
                dfs_second(caller, reverse, visited, scc);
            }
        }
    }
}

/// Group functions by level, returning a vector where index is level.
/// Functions within each level are sorted for deterministic ordering.
pub fn group_by_level(levels: &HashMap<String, usize>) -> Vec<Vec<String>> {
    let max_level = levels.values().copied().max().unwrap_or(0);
    let mut groups: Vec<Vec<String>> = vec![Vec::new(); max_level + 1];

    for (func, &level) in levels {
        groups[level].push(func.clone());
    }

    // Sort each level for deterministic order
    for group in &mut groups {
        group.sort();
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_chain() {
        // A -> B -> C (A calls B, B calls C)
        let functions: HashSet<String> = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
        let mut calls: HashMap<String, HashSet<String>> = HashMap::new();
        calls.insert("A".to_string(), ["B".to_string()].into_iter().collect());
        calls.insert("B".to_string(), ["C".to_string()].into_iter().collect());

        let levels = assign_levels(&functions, &calls);

        assert_eq!(levels["C"], 0); // C calls nothing
        assert_eq!(levels["B"], 1); // B calls C (level 0)
        assert_eq!(levels["A"], 2); // A calls B (level 1)
    }

    #[test]
    fn test_diamond() {
        // A -> B, A -> C, B -> D, C -> D
        let functions: HashSet<String> = ["A", "B", "C", "D"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut calls: HashMap<String, HashSet<String>> = HashMap::new();
        calls.insert(
            "A".to_string(),
            ["B".to_string(), "C".to_string()].into_iter().collect(),
        );
        calls.insert("B".to_string(), ["D".to_string()].into_iter().collect());
        calls.insert("C".to_string(), ["D".to_string()].into_iter().collect());

        let levels = assign_levels(&functions, &calls);

        assert_eq!(levels["D"], 0);
        assert_eq!(levels["B"], 1);
        assert_eq!(levels["C"], 1);
        assert_eq!(levels["A"], 2);
    }

    #[test]
    fn test_cycle() {
        // A -> B -> A (cycle)
        let functions: HashSet<String> = ["A", "B"].iter().map(|s| s.to_string()).collect();
        let mut calls: HashMap<String, HashSet<String>> = HashMap::new();
        calls.insert("A".to_string(), ["B".to_string()].into_iter().collect());
        calls.insert("B".to_string(), ["A".to_string()].into_iter().collect());

        let levels = assign_levels(&functions, &calls);

        // Both should be at same level (collapsed SCC)
        assert_eq!(levels["A"], levels["B"]);
        assert_eq!(levels["A"], 0); // No external callees, so level 0
    }

    #[test]
    fn test_cycle_with_external() {
        // A -> B -> A, B -> C
        let functions: HashSet<String> = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
        let mut calls: HashMap<String, HashSet<String>> = HashMap::new();
        calls.insert("A".to_string(), ["B".to_string()].into_iter().collect());
        calls.insert(
            "B".to_string(),
            ["A".to_string(), "C".to_string()].into_iter().collect(),
        );

        let levels = assign_levels(&functions, &calls);

        assert_eq!(levels["C"], 0); // C calls nothing
        assert_eq!(levels["A"], 1); // A and B are in same SCC, which calls C
        assert_eq!(levels["B"], 1);
    }

    #[test]
    fn test_no_calls() {
        let functions: HashSet<String> = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
        let calls: HashMap<String, HashSet<String>> = HashMap::new();

        let levels = assign_levels(&functions, &calls);

        // All at level 0
        assert_eq!(levels["A"], 0);
        assert_eq!(levels["B"], 0);
        assert_eq!(levels["C"], 0);
    }

    #[test]
    fn test_group_by_level() {
        let mut levels: HashMap<String, usize> = HashMap::new();
        levels.insert("A".to_string(), 2);
        levels.insert("B".to_string(), 1);
        levels.insert("C".to_string(), 0);
        levels.insert("D".to_string(), 1);

        let groups = group_by_level(&levels);

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], vec!["C".to_string()]);
        assert!(groups[1].contains(&"B".to_string()));
        assert!(groups[1].contains(&"D".to_string()));
        assert_eq!(groups[2], vec!["A".to_string()]);
    }
}
