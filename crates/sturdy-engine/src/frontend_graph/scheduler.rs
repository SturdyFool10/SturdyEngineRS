use std::collections::HashMap;

use sturdy_engine_core as core;

use crate::{ImageHandle, PassDesc, SubresourceRange};

/// Add a directed scheduler edge, deduplicating edges so duplicate resource
/// accesses do not inflate in-degree.
fn sched_add_edge(adj: &mut [Vec<usize>], in_degree: &mut [usize], i: usize, j: usize) {
    if !adj[i].contains(&j) {
        adj[i].push(j);
        in_degree[j] += 1;
    }
}

#[cfg(test)]
pub(super) fn has_read_after_write_dependency(writer: &PassDesc, reader: &PassDesc) -> bool {
    for write in &writer.writes {
        if reader
            .reads
            .iter()
            .any(|read| super::image_uses_overlap(write, read))
        {
            return true;
        }
    }

    let writer_buf_writes: Vec<_> = writer.buffer_writes.iter().map(|u| u.buffer).collect();
    let reader_buf_reads: Vec<_> = reader.buffer_reads.iter().map(|u| u.buffer).collect();

    for h in &writer_buf_writes {
        if reader_buf_reads.contains(h) {
            return true;
        }
    }

    false
}

/// Returns true when `later` must preserve declaration order after `earlier`.
///
/// WAW and WAR hazards do not express data flow, but they still need a stable
/// ordering. Adding these edges in both directions creates cycles, so callers
/// only check this for declaration-ordered pass pairs.
#[cfg(test)]
pub(super) fn has_declaration_order_hazard(earlier: &PassDesc, later: &PassDesc) -> bool {
    // WAW
    for earlier_write in &earlier.writes {
        if later
            .writes
            .iter()
            .any(|later_write| super::image_uses_overlap(earlier_write, later_write))
        {
            return true;
        }
    }
    // WAR
    for earlier_read in &earlier.reads {
        if later
            .writes
            .iter()
            .any(|later_write| super::image_uses_overlap(earlier_read, later_write))
        {
            return true;
        }
    }

    let e_buf_writes: Vec<_> = earlier.buffer_writes.iter().map(|u| u.buffer).collect();
    let e_buf_reads: Vec<_> = earlier.buffer_reads.iter().map(|u| u.buffer).collect();
    let l_buf_writes: Vec<_> = later.buffer_writes.iter().map(|u| u.buffer).collect();

    for h in &e_buf_writes {
        if l_buf_writes.contains(h) {
            return true;
        }
    }
    for h in &e_buf_reads {
        if l_buf_writes.contains(h) {
            return true;
        }
    }

    false
}

/// Returns the indices of `passes` in dependency-correct execution order.
///
/// Uses Kahn's topological sort. Passes with no outstanding dependencies are
/// processed in declaration order as a deterministic tie-breaker.
pub(super) fn schedule_pass_order(
    passes: &[PassDesc],
    ordering_constraints: &[(ImageHandle, ImageHandle)],
) -> Vec<usize> {
    let n = passes.len();
    if n <= 1 {
        return (0..n).collect();
    }

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];

    let mut image_access: HashMap<ImageHandle, Vec<(usize, SubresourceRange, bool)>> =
        HashMap::new();
    let mut buffer_access: HashMap<core::BufferHandle, Vec<(usize, bool)>> = HashMap::new();

    for (i, pass) in passes.iter().enumerate() {
        for u in &pass.writes {
            image_access
                .entry(u.image)
                .or_default()
                .push((i, u.subresource, true));
        }
        for u in &pass.reads {
            image_access
                .entry(u.image)
                .or_default()
                .push((i, u.subresource, false));
        }
        for u in &pass.buffer_writes {
            buffer_access.entry(u.buffer).or_default().push((i, true));
        }
        for u in &pass.buffer_reads {
            buffer_access.entry(u.buffer).or_default().push((i, false));
        }
    }

    for accesses in image_access.values() {
        for a in 0..accesses.len() {
            for b in (a + 1)..accesses.len() {
                let (ai, a_sub, a_write) = accesses[a];
                let (bi, b_sub, b_write) = accesses[b];
                if !a_sub.overlaps(b_sub) {
                    continue;
                }
                match (a_write, b_write) {
                    (true, false) => sched_add_edge(&mut adj, &mut in_degree, ai, bi),
                    (false, true) => sched_add_edge(&mut adj, &mut in_degree, bi, ai),
                    _ => {
                        let (earlier, later) = if ai < bi { (ai, bi) } else { (bi, ai) };
                        sched_add_edge(&mut adj, &mut in_degree, earlier, later);
                    }
                }
            }
        }
    }

    for accesses in buffer_access.values() {
        for a in 0..accesses.len() {
            for b in (a + 1)..accesses.len() {
                let (ai, a_write) = accesses[a];
                let (bi, b_write) = accesses[b];
                match (a_write, b_write) {
                    (true, false) => sched_add_edge(&mut adj, &mut in_degree, ai, bi),
                    (false, true) => sched_add_edge(&mut adj, &mut in_degree, bi, ai),
                    _ => {
                        let (earlier, later) = if ai < bi { (ai, bi) } else { (bi, ai) };
                        sched_add_edge(&mut adj, &mut in_degree, earlier, later);
                    }
                }
            }
        }
    }

    for (before_img, after_img) in ordering_constraints {
        let before_pass = passes
            .iter()
            .position(|p| p.writes.iter().any(|u| u.image == *before_img));
        let after_pass = passes
            .iter()
            .position(|p| p.writes.iter().any(|u| u.image == *after_img));
        if let (Some(i), Some(j)) = (before_pass, after_pass) {
            if i != j {
                sched_add_edge(&mut adj, &mut in_degree, i, j);
            }
        }
    }

    let mut ready: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut result = Vec::with_capacity(n);

    while !ready.is_empty() {
        ready.sort_unstable();
        let wave = std::mem::take(&mut ready);
        for idx in wave {
            result.push(idx);
            for &dep in &adj[idx] {
                in_degree[dep] -= 1;
                if in_degree[dep] == 0 {
                    ready.push(dep);
                }
            }
        }
    }

    if result.len() < n {
        for (i, degree) in in_degree.iter().enumerate() {
            if *degree > 0 {
                result.push(i);
            }
        }
    }

    result
}
