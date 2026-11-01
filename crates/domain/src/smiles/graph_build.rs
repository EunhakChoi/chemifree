//! Graph helpers shared by the reader and the writer.

/// Marks every bridge (an edge in no cycle) of the graph with `count` vertices. Linear time:
/// an iterative depth-first search with low-link values, so deep input cannot overflow the stack.
pub(crate) fn bridges(count: usize, edges: &[(usize, usize)]) -> Vec<bool> {
    let mut adjacency: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    for (index, &(a, b)) in edges.iter().enumerate() {
        adjacency[a].push((b, index));
        adjacency[b].push((a, index));
    }
    let mut discovered = vec![0_usize; count];
    let mut low = vec![0_usize; count];
    let mut bridge = vec![false; edges.len()];
    let mut clock = 1_usize;
    // (vertex, edge used to arrive, next neighbour to look at)
    let mut stack: Vec<(usize, Option<usize>, usize)> = Vec::new();
    for root in 0..count {
        if discovered[root] != 0 {
            continue;
        }
        discovered[root] = clock;
        low[root] = clock;
        clock += 1;
        stack.push((root, None, 0));
        while let Some(top) = stack.last_mut() {
            let (vertex, arrival, next) = *top;
            if let Some(&(neighbour, edge)) = adjacency[vertex].get(next) {
                top.2 += 1;
                if Some(edge) == arrival {
                    continue;
                }
                if discovered[neighbour] == 0 {
                    discovered[neighbour] = clock;
                    low[neighbour] = clock;
                    clock += 1;
                    stack.push((neighbour, Some(edge), 0));
                } else {
                    low[vertex] = low[vertex].min(discovered[neighbour]);
                }
            } else {
                stack.pop();
                if let Some(&(parent, _, _)) = stack.last() {
                    low[parent] = low[parent].min(low[vertex]);
                    if low[vertex] > discovered[parent] {
                        bridge[arrival.expect("a non-root vertex has an arrival edge")] = true;
                    }
                }
            }
        }
    }
    bridge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_on_a_ring_is_a_bridge_and_the_ring_is_not() {
        // 0-1-2-0 triangle, 2-3 tail.
        let bridge = bridges(4, &[(0, 1), (1, 2), (2, 0), (2, 3)]);
        assert_eq!(bridge, vec![false, false, false, true]);
    }

    #[test]
    fn a_long_chain_does_not_overflow_the_stack() {
        let edges: Vec<_> = (0..200_000).map(|i| (i, i + 1)).collect();
        assert!(bridges(200_001, &edges).iter().all(|&b| b));
    }
}
