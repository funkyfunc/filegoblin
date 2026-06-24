use petgraph::Graph;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;

const DAMPING_FACTOR: f64 = 0.85;
const ITERATIONS: usize = 20;

/// Generates PageRank centrality scores for a set of files based on heuristic dependency graphs.
///
/// Instead of perfectly resolving language-specific AST imports, this uses a high-speed
/// heuristic: if File A contains the unique stem/module name of File B, we assume a dependency.
pub fn compute_pagerank(documents: &[(&String, &String)]) -> HashMap<String, f64> {
    let mut graph = Graph::<String, ()>::new();
    let mut path_to_node = HashMap::new();
    let mut node_to_path = HashMap::new();
    let mut module_names: HashMap<String, String> = HashMap::new();

    // 1. Add all nodes and identify their module names
    for (path, _) in documents {
        let node = graph.add_node((*path).clone());
        path_to_node.insert((*path).clone(), node);
        node_to_path.insert(node, (*path).clone());

        // Extract module name (e.g. src/auth/login.rs -> login)
        let stem = std::path::Path::new(path)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Avoid mapping overly generic names like "index" or "mod" which would create massive false positive edges
        if stem != "index" && stem != "mod" && stem != "main" && stem != "lib" {
            module_names.insert(stem, (*path).clone());
        }
    }

    // 2. Build edges
    for (path, content) in documents {
        let source_node = path_to_node[*path];

        // Scan the content for any of the known module names
        for (module, target_path) in &module_names {
            if *path == target_path {
                continue; // no self loops
            }
            // Fast heuristic: if the module name appears in the content, it's likely an import or usage
            if content.contains(module) {
                let target_node = path_to_node[target_path];
                // Edge direction: Source -> Target (Source depends on Target)
                // In PageRank, importance flows along edges. Since we want core dependencies to have high score,
                // importance flows from the file doing the importing TO the file being imported.
                graph.add_edge(source_node, target_node, ());
            }
        }
    }

    // 3. Compute PageRank
    let node_count = graph.node_count();
    if node_count == 0 {
        return HashMap::new();
    }

    let initial_rank = 1.0 / (node_count as f64);
    let mut ranks: HashMap<NodeIndex, f64> = HashMap::new();
    for node in graph.node_indices() {
        ranks.insert(node, initial_rank);
    }

    for _ in 0..ITERATIONS {
        let mut new_ranks = HashMap::new();

        for node in graph.node_indices() {
            // Base rank from random jumps
            let mut rank = (1.0 - DAMPING_FACTOR) / (node_count as f64);

            // Add rank from incoming edges
            // Since we want imported files to be important, incoming edges represent "files that import this node"
            let mut incoming_sum = 0.0;
            for neighbor in graph.neighbors_directed(node, petgraph::Direction::Incoming) {
                // Out-degree of the neighbor (how many things does the neighbor import?)
                let out_degree = graph
                    .neighbors_directed(neighbor, petgraph::Direction::Outgoing)
                    .count() as f64;
                if out_degree > 0.0 {
                    incoming_sum += ranks[&neighbor] / out_degree;
                }
            }

            rank += DAMPING_FACTOR * incoming_sum;
            new_ranks.insert(node, rank);
        }
        ranks = new_ranks;
    }

    // 4. Map back to paths
    let mut final_scores = HashMap::new();
    for (node, rank) in ranks {
        let path = node_to_path[&node].clone();
        final_scores.insert(path, rank);
    }

    final_scores
}
