use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

/// Returns functions grouped by dependency level, with deterministic ordering.
///
/// Level 0 contains leaf functions (no calls to other in-source functions).
/// Level N contains functions that only call functions at levels 0..N-1.
/// Functions in mutual recursion cycles (SCCs) collapse to the same level:
/// `max(callee levels outside SCC) + 1`, or 0 if no external callees.
/// Within each level, functions are sorted alphabetically.
///
/// Given the same input, the output is identical across every run.
pub fn hierarchy(
    functions: &HashSet<String>,
    calls: &HashMap<String, HashSet<String>>,
) -> Vec<Vec<String>> {
    let (funcs, calls) = to_sorted(functions, calls);
    let (sccs, func_to_scc) = find_sccs(&funcs, &calls);
    let dag = build_scc_dag(&func_to_scc, &calls);
    let scc_levels = assign_scc_levels(sccs.len(), &dag);

    group_functions_by_level(&funcs, &func_to_scc, &scc_levels)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert HashMap/HashSet inputs to BTreeMap/BTreeSet for deterministic iteration.
fn to_sorted(
    functions: &HashSet<String>,
    calls: &HashMap<String, HashSet<String>>,
) -> (BTreeSet<String>, BTreeMap<String, BTreeSet<String>>) {
    let funcs: BTreeSet<String> = functions.iter().cloned().collect();
    let calls: BTreeMap<String, BTreeSet<String>> = calls
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
        .collect();
    (funcs, calls)
}

/// SCC-level directed acyclic graph.
struct SccDag {
    /// deps[i] = set of SCC indices that SCC i calls (dependencies)
    deps: BTreeMap<usize, BTreeSet<usize>>,
    /// rdeps[i] = set of SCC indices that call SCC i (reverse dependencies)
    rdeps: BTreeMap<usize, BTreeSet<usize>>,
}

/// Find strongly connected components using Kosaraju's algorithm.
///
/// Returns (sccs, func_to_scc) where each SCC's functions are sorted alphabetically
/// and SCC indices are deterministic for the same input.
fn find_sccs(
    functions: &BTreeSet<String>,
    calls: &BTreeMap<String, BTreeSet<String>>,
) -> (Vec<Vec<String>>, HashMap<String, usize>) {
    let finish_order = compute_finish_order(functions, calls);
    let reverse = build_reverse_graph(functions, calls);
    let sccs = extract_sccs(&finish_order, &reverse);
    let func_to_scc = build_scc_index(&sccs);
    (sccs, func_to_scc)
}

/// First pass of Kosaraju: DFS in forward direction to compute finish order.
fn compute_finish_order<'a>(
    functions: &'a BTreeSet<String>,
    calls: &'a BTreeMap<String, BTreeSet<String>>,
) -> Vec<&'a str> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut finish_order: Vec<&str> = Vec::new();

    for func in functions {
        if !visited.contains(func.as_str()) {
            dfs_forward(func, calls, functions, &mut visited, &mut finish_order);
        }
    }

    finish_order
}

/// Build the reverse (transposed) call graph for the second Kosaraju pass.
fn build_reverse_graph<'a>(
    functions: &'a BTreeSet<String>,
    calls: &'a BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
    let mut reverse: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for func in functions {
        reverse.entry(func.as_str()).or_default();
    }
    for (caller, callees) in calls {
        if !functions.contains(caller) {
            continue;
        }
        for callee in callees {
            if functions.contains(callee) {
                reverse
                    .entry(callee.as_str())
                    .or_default()
                    .insert(caller.as_str());
            }
        }
    }
    reverse
}

/// Second pass of Kosaraju: DFS on reverse graph in reverse finish order to extract SCCs.
fn extract_sccs(
    finish_order: &[&str],
    reverse: &BTreeMap<&str, BTreeSet<&str>>,
) -> Vec<Vec<String>> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut sccs: Vec<Vec<String>> = Vec::new();

    for &func in finish_order.iter().rev() {
        if !visited.contains(func) {
            let mut scc: Vec<String> = Vec::new();
            dfs_reverse(func, reverse, &mut visited, &mut scc);
            scc.sort();
            sccs.push(scc);
        }
    }

    sccs
}

/// Map each function name to its SCC index.
fn build_scc_index(sccs: &[Vec<String>]) -> HashMap<String, usize> {
    let mut func_to_scc: HashMap<String, usize> = HashMap::new();
    for (scc_idx, scc) in sccs.iter().enumerate() {
        for func in scc {
            func_to_scc.insert(func.clone(), scc_idx);
        }
    }
    func_to_scc
}

fn dfs_forward<'a>(
    node: &'a str,
    calls: &'a BTreeMap<String, BTreeSet<String>>,
    functions: &'a BTreeSet<String>,
    visited: &mut HashSet<&'a str>,
    finish_order: &mut Vec<&'a str>,
) {
    visited.insert(node);
    if let Some(callees) = calls.get(node) {
        for callee in callees {
            if functions.contains(callee) && !visited.contains(callee.as_str()) {
                dfs_forward(callee, calls, functions, visited, finish_order);
            }
        }
    }
    finish_order.push(node);
}

fn dfs_reverse<'a>(
    node: &'a str,
    reverse: &BTreeMap<&str, BTreeSet<&'a str>>,
    visited: &mut HashSet<&'a str>,
    scc: &mut Vec<String>,
) {
    visited.insert(node);
    scc.push(node.to_string());
    if let Some(callers) = reverse.get(node) {
        for &caller in callers {
            if !visited.contains(caller) {
                dfs_reverse(caller, reverse, visited, scc);
            }
        }
    }
}

/// Build the SCC-level DAG from the function-to-SCC mapping and call graph.
fn build_scc_dag(
    func_to_scc: &HashMap<String, usize>,
    calls: &BTreeMap<String, BTreeSet<String>>,
) -> SccDag {
    let mut deps: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();

    for (caller, callees) in calls {
        let Some(&caller_scc) = func_to_scc.get(caller) else {
            continue;
        };
        for callee in callees {
            let Some(&callee_scc) = func_to_scc.get(callee) else {
                continue;
            };
            if caller_scc != callee_scc {
                deps.entry(caller_scc).or_default().insert(callee_scc);
            }
        }
    }

    let mut rdeps: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (&scc_idx, scc_deps) in &deps {
        for &dep in scc_deps {
            rdeps.entry(dep).or_default().insert(scc_idx);
        }
    }

    SccDag { deps, rdeps }
}

/// Assign levels to SCCs using Kahn's algorithm from leaves.
///
/// Level 0 = SCCs with no callees. Level N = max(callee levels) + 1.
fn assign_scc_levels(num_sccs: usize, dag: &SccDag) -> Vec<usize> {
    let mut out_degree: Vec<usize> = vec![0; num_sccs];
    for (&scc_idx, deps) in &dag.deps {
        out_degree[scc_idx] = deps.len();
    }

    let mut levels: Vec<usize> = vec![0; num_sccs];
    let mut queue: VecDeque<usize> = VecDeque::new();

    for scc_idx in 0..num_sccs {
        if out_degree[scc_idx] == 0 {
            queue.push_back(scc_idx);
        }
    }

    while let Some(scc_idx) = queue.pop_front() {
        if let Some(callers) = dag.rdeps.get(&scc_idx) {
            for &caller in callers {
                levels[caller] = levels[caller].max(levels[scc_idx] + 1);
                out_degree[caller] -= 1;
                if out_degree[caller] == 0 {
                    queue.push_back(caller);
                }
            }
        }
    }

    levels
}

/// Place each function into its SCC's level, sorted alphabetically within each level.
fn group_functions_by_level(
    funcs: &BTreeSet<String>,
    func_to_scc: &HashMap<String, usize>,
    scc_levels: &[usize],
) -> Vec<Vec<String>> {
    let max_level = scc_levels.iter().copied().max().unwrap_or(0);
    let mut groups: Vec<Vec<String>> = vec![Vec::new(); max_level + 1];

    for func in funcs {
        if let Some(&scc_idx) = func_to_scc.get(func.as_str()) {
            groups[scc_levels[scc_idx]].push(func.clone());
        }
    }

    for group in &mut groups {
        group.sort();
    }

    groups
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn funcs(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn edges(pairs: &[(&str, &[&str])]) -> HashMap<String, HashSet<String>> {
        let mut map = HashMap::new();
        for (from, tos) in pairs {
            let set: HashSet<String> = tos.iter().map(|s| s.to_string()).collect();
            map.insert(from.to_string(), set);
        }
        map
    }

    #[test]
    fn test_diamond() {
        // A -> B, A -> C, B -> D, C -> D
        let f = funcs(&["A", "B", "C", "D"]);
        let c = edges(&[
            ("A", &["B", "C"]),
            ("B", &["D"]),
            ("C", &["D"]),
        ]);

        let h = hierarchy(&f, &c);
        assert_eq!(h, vec![
            vec!["D"],
            vec!["B", "C"],
            vec!["A"],
        ]);
    }

    #[test]
    fn test_simple_chain() {
        // A -> B -> C
        let f = funcs(&["A", "B", "C"]);
        let c = edges(&[("A", &["B"]), ("B", &["C"])]);

        let h = hierarchy(&f, &c);
        assert_eq!(h, vec![vec!["C"], vec!["B"], vec!["A"]]);
    }

    #[test]
    fn test_no_calls() {
        let f = funcs(&["A", "B", "C"]);
        let c = edges(&[]);

        let h = hierarchy(&f, &c);
        assert_eq!(h, vec![vec!["A", "B", "C"]]);
    }

    #[test]
    fn test_simple_cycle() {
        // A -> B -> A
        let f = funcs(&["A", "B"]);
        let c = edges(&[("A", &["B"]), ("B", &["A"])]);

        let h = hierarchy(&f, &c);
        assert_eq!(h, vec![vec!["A", "B"]]);
    }

    #[test]
    fn test_cycle_with_external_dep() {
        // A -> B -> A, B -> C
        let f = funcs(&["A", "B", "C"]);
        let c = edges(&[("A", &["B"]), ("B", &["A", "C"])]);

        let h = hierarchy(&f, &c);
        assert_eq!(h, vec![vec!["C"], vec!["A", "B"]]);
    }

    #[test]
    fn test_larger_graph() {
        // H -> G -> F -> D
        //              F -> E -> D
        // A -> B -> D
        // A -> C -> D
        // B -> C
        // I -> J -> I (cycle), J -> D
        let f = funcs(&["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"]);
        let c = edges(&[
            ("A", &["B", "C"]),
            ("B", &["D", "C"]),
            ("C", &["D"]),
            ("E", &["D"]),
            ("F", &["D", "E"]),
            ("G", &["F"]),
            ("H", &["G"]),
            ("I", &["J"]),
            ("J", &["I", "D"]),
        ]);

        let h = hierarchy(&f, &c);
        assert_eq!(h, vec![
            vec!["D"],
            vec!["C", "E", "I", "J"],
            vec!["B", "F"],
            vec!["A", "G"],
            vec!["H"],
        ]);
    }

    #[test]
    fn test_determinism_stress() {
        let f = funcs(&["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"]);
        let c = edges(&[
            ("A", &["B", "C"]),
            ("B", &["D", "C"]),
            ("C", &["D"]),
            ("E", &["D"]),
            ("F", &["D", "E"]),
            ("G", &["F"]),
            ("H", &["G"]),
            ("I", &["J"]),
            ("J", &["I", "D"]),
        ]);

        let expected = hierarchy(&f, &c);

        for i in 0..10 {
            assert_eq!(hierarchy(&f, &c), expected, "hierarchy diverged on iteration {}", i);
        }
    }

    #[test]
    fn test_calls_to_unknown_functions_ignored() {
        // B calls Z which isn't in the function set
        let f = funcs(&["A", "B"]);
        let c = edges(&[("A", &["B"]), ("B", &["Z"])]);

        let h = hierarchy(&f, &c);
        assert_eq!(h, vec![vec!["B"], vec!["A"]]);
    }
}
