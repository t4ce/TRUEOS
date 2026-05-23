use alloc::vec;
use core::{
    hash::{BuildHasher, Hash},
    iter::{from_fn, FromIterator},
};

use hashbrown::HashSet;
use indexmap::IndexSet;

use crate::{
    visit::{IntoNeighborsDirected, NodeCount},
    Direction::Outgoing,
};

/// Calculate all simple paths with specified constraints from node `from` to node `to`.
///
/// A simple path is a path without repeating nodes.
/// The number of simple paths between a given pair of vertices can grow exponentially,
/// reaching `O(|V|!)` on complete graphs with `|V|` vertices.
///
/// So if you have a large enough graph, be prepared to wait on the results for years.
/// Or consider extracting only part of the simple paths using the adapter [`Iterator::take`].
/// Also note, that this algorithm does not check that a path exists between `from` and `to`. This may lead to very long running times and it may be worth it to check if a path exists before running this algorithm on large graphs.
///
/// This algorithm is adapted from [NetworkX](https://networkx.github.io/documentation/stable/reference/algorithms/generated/networkx.algorithms.simple_paths.all_simple_paths.html).
/// # Arguments
/// * `graph`: an input graph.
/// * `from`: an initial node of desired paths.
/// * `to`: a target node of desired paths.
/// * `min_intermediate_nodes`: the minimum number of nodes in the desired paths.
/// * `max_intermediate_nodes`: the maximum number of nodes in the desired paths (optional).
/// # Returns
/// Returns an iterator that produces all simple paths from `from` node to `to`, which contains at least `min_intermediate_nodes`
/// and at most `max_intermediate_nodes` intermediate nodes, if given, or limited by the graph's order otherwise.
///
/// # Complexity
/// * Time complexity: for computing the first **k** paths, the running time will be **O(k|V| + k|E|)**.
/// * Auxillary space: **O(|V|)**.
///
/// where **|V|** is the number of nodes and **|E|** is the number of edges.
///
/// # Example
/// ```
/// use std::collections::hash_map::RandomState;
/// use petgraph::{algo, prelude::*};
///
/// let mut graph = DiGraph::<&str, i32>::new();
///
/// let a = graph.add_node("a");
/// let b = graph.add_node("b");
/// let c = graph.add_node("c");
/// let d = graph.add_node("d");
///
/// graph.extend_with_edges(&[(a, b, 1), (b, c, 1), (c, d, 1), (a, b, 1), (b, d, 1)]);
///
/// let paths = algo::all_simple_paths::<Vec<_>, _, RandomState>(&graph, a, d, 0, None)
///   .collect::<Vec<_>>();
///
/// assert_eq!(paths.len(), 4);
///
///
/// // Take only 2 paths.
/// let paths = algo::all_simple_paths::<Vec<_>, _, RandomState>(&graph, a, d, 0, None)
///   .take(2)
///   .collect::<Vec<_>>();
///
/// assert_eq!(paths.len(), 2);
///
/// ```
pub fn all_simple_paths<TargetColl, G, S>(
    graph: G,
    from: G::NodeId,
    to: G::NodeId,
    min_intermediate_nodes: usize,
    max_intermediate_nodes: Option<usize>,
) -> impl Iterator<Item = TargetColl>
where
    G: NodeCount,
    G: IntoNeighborsDirected,
    G::NodeId: Eq + Hash,
    TargetColl: FromIterator<G::NodeId>,
    S: BuildHasher + Default,
{
    // how many nodes are allowed in simple path up to target node
    // it is min/max allowed path length minus one, because it is more appropriate when implementing lookahead
    // than constantly add 1 to length of current path
    let max_length = if let Some(l) = max_intermediate_nodes {
        l + 1
    } else {
        graph.node_count() - 1
    };

    let min_length = min_intermediate_nodes + 1;

    // list of visited nodes
    let mut visited: IndexSet<G::NodeId, S> = IndexSet::from_iter(Some(from));
    // list of childs of currently exploring path nodes,
    // last elem is list of childs of last visited node
    let mut stack = vec![graph.neighbors_directed(from, Outgoing)];

    from_fn(move || {
        while let Some(children) = stack.last_mut() {
            if let Some(child) = children.next() {
                if visited.contains(&child) {
                    continue;
                }
                if visited.len() < max_length {
                    if child == to {
                        if visited.len() >= min_length {
                            let path = visited
                                .iter()
                                .cloned()
                                .chain(Some(to))
                                .collect::<TargetColl>();
                            return Some(path);
                        }
                    } else {
                        visited.insert(child);
                        stack.push(graph.neighbors_directed(child, Outgoing));
                    }
                } else {
                    if (child == to || children.any(|v| v == to && !visited.contains(&v)))
                        && visited.len() >= min_length
                    {
                        let path = visited
                            .iter()
                            .cloned()
                            .chain(Some(to))
                            .collect::<TargetColl>();
                        return Some(path);
                    }
                    stack.pop();
                    visited.pop();
                }
            } else {
                stack.pop();
                visited.pop();
            }
        }
        None
    })
}

/// Calculate all simple paths from a source node to any of several target nodes.
///
/// This function is a variant of [`all_simple_paths`] that accepts a `HashSet` of
/// target nodes instead of a single one. A path is yielded as soon as it reaches any
/// node in the `to` set.
///
/// # Performance Considerations
///
/// The efficiency of this function hinges on the graph's structure. It provides significant
/// performance gains on graphs where paths share long initial segments (e.g., trees and DAGs),
/// as the benefit of a single traversal outweighs the `HashSet` lookup overhead.
///
/// Conversely, in dense graphs where paths diverge quickly or for targets very close
/// to the source, the lookup overhead could make repeated calls to [`all_simple_paths`]
/// a faster alternative.
///
/// **Note**: If security is not a concern, a faster hasher (e.g., `FxBuildHasher`)
/// can be specified to minimize the `HashSet` lookup overhead.
///
/// # Arguments
/// * `graph`: an input graph.
/// * `from`: an initial node of desired paths.
/// * `to`: a `HashSet` of target nodes. A path is yielded as soon as it reaches any node in this set.
/// * `min_intermediate_nodes`: the minimum number of nodes in the desired paths.
/// * `max_intermediate_nodes`: the maximum number of nodes in the desired paths (optional).
/// # Returns
/// Returns an iterator that produces all simple paths from `from` node to any node in the `to` set, which contains at least `min_intermediate_nodes`
/// and at most `max_intermediate_nodes` intermediate nodes, if given, or limited by the graph's order otherwise.
///
/// # Complexity
/// * Time complexity: for computing the first **k** paths, the running time will be **O(k|V| + k|E|)**.
/// * Auxillary space: **O(|V|)**.
///
/// where **|V|** is the number of nodes and **|E|** is the number of edges.
///
/// # Example
/// ```
/// use petgraph::{algo, prelude::*};
/// use hashbrown::HashSet;
/// use std::collections::hash_map::RandomState;
///
/// let mut graph = DiGraph::<&str, i32>::new();
///
/// let a = graph.add_node("a");
/// let b = graph.add_node("b");
/// let c = graph.add_node("c");
/// let d = graph.add_node("d");
/// graph.extend_with_edges(&[(a, b, 1), (b, c, 1), (b, d, 1)]);
///
/// // Find paths from "a" to either "c" or "d".
/// let targets = HashSet::from_iter([c, d]);
/// let mut paths = algo::all_simple_paths_multi::<Vec<_>, _, RandomState>(&graph, a, &targets, 0, None)
///     .collect::<Vec<_>>();
///
/// paths.sort_by_key(|p| p.clone());
/// let expected_paths = vec![
///     vec![a, b, c],
///     vec![a, b, d],
/// ];
///
/// assert_eq!(paths, expected_paths);
///
/// ```
pub fn all_simple_paths_multi<'a, TargetColl, G, S>(
    graph: G,
    from: G::NodeId,
    to: &'a HashSet<G::NodeId, S>,
    min_intermediate_nodes: usize,
    max_intermediate_nodes: Option<usize>,
) -> impl Iterator<Item = TargetColl> + 'a
where
    G: NodeCount + IntoNeighborsDirected + 'a,
    <G as IntoNeighborsDirected>::NeighborsDirected: 'a,
    G::NodeId: Eq + Hash,
    TargetColl: FromIterator<G::NodeId>,
    S: BuildHasher + Default,
{
    let max_nodes = if let Some(l) = max_intermediate_nodes {
        l + 2
    } else {
        graph.node_count()
    };

    let min_nodes = min_intermediate_nodes + 2;

    // list of visited nodes
    let mut visited: IndexSet<G::NodeId, S> = IndexSet::from_iter(Some(from));
    // list of childs of currently exploring path nodes,
    // last elem is list of childs of last visited node
    let mut stack = vec![graph.neighbors_directed(from, Outgoing)];

    from_fn(move || {
        while let Some(children) = stack.last_mut() {
            if let Some(child) = children.next() {
                if visited.contains(&child) {
                    continue;
                }

                let current_nodes = visited.len();
                let mut valid_path: Option<TargetColl> = None;

                // Check if we've reached a target node
                if to.contains(&child) && (current_nodes + 1) >= min_nodes {
                    valid_path = Some(
                        visited
                            .iter()
                            .cloned()
                            .chain(Some(child))
                            .collect::<TargetColl>(),
                    );
                }

                // Expand the search only if within max length and unexplored target nodes remain
                if (current_nodes < max_nodes)
                    && to.iter().any(|n| *n != child && !visited.contains(n))
                {
                    visited.insert(child);
                    stack.push(graph.neighbors_directed(child, Outgoing));
                }

                // yield the valid path if found
                if valid_path.is_some() {
                    return valid_path;
                }
            } else {
                // All neighbors of the current node have been explored
                stack.pop();
                visited.pop();
            }
        }
        None
    })
}
