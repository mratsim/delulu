use crate::pipelines::DomNode;

/// Action returned by a walker callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkerAction {
    /// Continue walking (recurse into children).
    Continue,
    /// Remove this node from the tree.
    Remove,
    /// Keep this node but do not recurse into its children.
    SkipChildren,
    /// Replace this node with its children (splice them into the parent Vec).
    ///
    /// Only meaningful in `walk_post_mut` where children have already been
    /// visited. In `walk_pre_mut`, this action panics.
    ReplaceWithChildren,
}

/// A filter callback for use with `walk_post_mut`.
pub type WalkerFilter = dyn FnMut(&mut DomNode) -> WalkerAction;

/// A pipeline pass: a function that mutates a DOM tree in-place.
pub type PassFn = fn(&mut DomNode);

/// Post-order tree walk with bottom-up filter application.
///
/// # Precondition
/// - `node` is a valid DOM tree root (acyclic).
/// - `should_descend` must be infallible (must not panic).
///
/// # Postcondition
/// - Filters have been applied bottom-up (children before parent).
/// - Nodes for which any filter returned `WalkerAction::Remove` are removed.
/// - `ReplaceWithChildren` nodes are spliced in.
///
/// # Panic-if
/// - Any filter returns `WalkerAction::SkipChildren` — children are already
///   processed in post-order, so skipping is meaningless (caller bug).
/// - `should_descend` panics.
/// - `MAX_DEPTH` is NOT enforced. Callers processing untrusted HTML should
///   assess stack safety independently.
///
/// # Parameters
///
/// - `node`: The DOM node to walk (children are extracted internally).
/// - `filters`: Callbacks that receive each child node and return a `WalkerAction`.
/// - `should_descend`: Optional predicate checked before recursing into a node's
///   children. When it returns `false`, the walker does NOT recurse into children.
///
/// # WalkerAction semantics in post-order
///
/// - `Continue`: Node is left in place.
/// - `Remove`: Node is removed after its children have been processed.
/// - `SkipChildren`: Panics. Children were already processed before this filter
///   was called, so this action has no valid meaning in post-order.
/// - `ReplaceWithChildren`: The current node is replaced by its children (spliced
///   into the parent `Vec` at the same position). On non-Element nodes,
///   `ReplaceWithChildren` is silently treated as `Continue`.
///
/// # should_descend vs SkipChildren
///
/// - `WalkerAction::SkipChildren` in post-order **panics** — children were already
///   processed before the filter runs, so skipping has no valid meaning.
/// - The `should_descend` guard is checked **before** recursing into children.
///   When it returns `false`, the walker does NOT recurse, and no panic occurs.
///   This is the ONLY valid way to prevent child visitation in post-order.
///
/// # Recursion depth
///
/// This function recurses with stack depth equal to DOM tree depth.
/// `MAX_DEPTH` in `mod.rs` is NOT enforced here.
/// Callers processing untrusted HTML should assess stack safety independently.
///
/// # Remove cost
///
/// `WalkerAction::Remove` is O(N - i) in siblings via `Vec::remove`.
/// For large sibling lists, consider in-place mutation.
#[allow(clippy::collapsible_if)]
pub fn walk_post_mut(
    node: &mut DomNode,
    filters: &mut [&mut WalkerFilter],
    should_descend: Option<fn(&DomNode) -> bool>,
) {
    if let DomNode::Element { children, .. } = node {
        let mut i = 0;
        while i < children.len() {
            // Post-order: recurse into children FIRST
            if should_descend.is_none_or(|pred| pred(&children[i])) {
                walk_post_mut(&mut children[i], filters, should_descend);
            } else {
                tracing::debug!("should_descend blocked descent into element");
            }

            // Then run filters on the current node
            let mut removed_current = false;
            for filter in filters.iter_mut() {
                match filter(&mut children[i]) {
                    WalkerAction::Remove => {
                        // O(n) shift — Vec::remove moves all subsequent siblings left.
                        // Tolerable for typical DOM sibling counts (< 100). If this becomes
                        // a hotspot (e.g., removing thousands of siblings at one level),
                        // switch to swap_remove + post-pass reordering or a retain-based approach.
                        children.remove(i);
                        removed_current = true;
                        // Break out of filter loop since node is gone
                        break;
                    }
                    WalkerAction::Continue => {}
                    WalkerAction::SkipChildren => {
                        panic!(
                            "SkipChildren has no effect in post-order — children already processed"
                        );
                    }
                    WalkerAction::ReplaceWithChildren => {
                        if let DomNode::Element {
                            children: grand_children,
                            ..
                        } = &mut children[i]
                        {
                            let mut extracted = std::mem::take(grand_children);
                            let n = extracted.len();
                            // O(n) splice — shifts subsequent siblings. Same trade-offs as Vec::remove above.
                            children.splice(i..=i, extracted.drain(..));
                            removed_current = true;
                            i += n; // Skip past children already processed in recursion step
                            break;
                        }
                        // On non-Element nodes, ReplaceWithChildren is silently treated as Continue
                    }
                }
            }

            // Only increment if we did NOT remove the current node
            // (removal shifts the next sibling into position i)
            if !removed_current {
                i += 1;
            }
        }
    }
}

/// Post-order walk with bottom-up accumulation and removal.
///
/// Each node's filter receives the node and its children's accumulated values
/// (already computed, since children are processed first). Returns `(WalkerAction, A)`
/// where `A` is the accumulated value for this node.
///
/// The walker returns the accumulated values for all top-level nodes.
///
/// # Type parameters
/// - `A`: Accumulator type. Must implement `Default` (for empty/leaf nodes).
///
/// # should_descend
///
/// Same semantics as [`walk_post_mut`]: when `should_descend` returns `false`,
/// children are skipped and the filter receives an empty `&[]`.
pub fn walk_post_acc_mut<A: Default>(
    nodes: &mut Vec<DomNode>,
    should_descend: Option<fn(&DomNode) -> bool>,
    filter: &mut dyn FnMut(&mut DomNode, &[A]) -> (WalkerAction, A),
) -> Vec<A> {
    let mut results = Vec::with_capacity(nodes.len());
    let mut i = 0;
    while i < nodes.len() {
        let child_results = if should_descend.is_none_or(|pred| pred(&nodes[i])) {
            if let DomNode::Element { children, .. } = &mut nodes[i] {
                walk_post_acc_mut(children, should_descend, filter)
            } else {
                Vec::new()
            }
        } else {
            tracing::debug!("walk_post_acc_mut: should_descend blocked descent");
            Vec::new()
        };

        let (action, acc) = filter(&mut nodes[i], &child_results);
        match action {
            WalkerAction::Remove => {
                // O(n) shift — same as walk_post_mut above.
                nodes.remove(i);
            }
            WalkerAction::Continue => {
                results.push(acc);
                i += 1;
            }
            WalkerAction::SkipChildren => {
                panic!("SkipChildren has no effect in post-order");
            }
            WalkerAction::ReplaceWithChildren => {
                if let DomNode::Element { children, .. } = &mut nodes[i] {
                    let mut extracted = std::mem::take(children);
                    let n = extracted.len();
                    // O(n) splice — same as walk_post_mut above.
                    nodes.splice(i..=i, extracted.drain(..));
                    i += n;
                } else {
                    results.push(acc);
                    i += 1;
                }
            }
        }
    }
    results
}

// ---------------------------------------------------------------------------
// walk_pre_mut (filtering pass, supports removal)
// ---------------------------------------------------------------------------

/// Pre-order traversal with removal support.
///
/// # Precondition
/// - `node` is a valid DOM tree root (acyclic).
/// - `f` must not panic (intentional pre-alpha behavior).
///
/// # Postcondition
/// - All nodes in the tree rooted at `node` have been visited in pre-order.
/// - Nodes for which `f` returned `WalkerAction::Remove` are removed from their parent's children list.
/// - Nodes for which `f` returned `WalkerAction::SkipChildren` are kept but children are not visited.
///
/// # Panic-if
/// - `f` panics (intentional — pre-alpha crash-loudly principle).
/// - `f` returns `WalkerAction::ReplaceWithChildren` (invalid in pre-order).
/// - The DOM tree has cycles (stack overflow — not caught).
/// - `MAX_DEPTH` is NOT enforced. Callers processing untrusted HTML should assess stack safety independently.
pub fn walk_pre_mut(node: &mut DomNode, f: &impl Fn(&mut DomNode) -> WalkerAction) {
    if let DomNode::Element { children, .. } = node {
        let mut i = 0;
        while i < children.len() {
            let action = f(&mut children[i]);
            match action {
                WalkerAction::Continue => {
                    walk_pre_mut(&mut children[i], f);
                    i += 1;
                }
                WalkerAction::SkipChildren => {
                    i += 1;
                }
                WalkerAction::Remove => {
                    // O(n) shift — Vec::remove moves all subsequent siblings left.
                    // Tolerable for typical DOM sibling counts (< 100).
                    children.remove(i);
                    // Do not increment i; next element shifts into position i.
                }
                WalkerAction::ReplaceWithChildren => {
                    panic!("ReplaceWithChildren is not supported in pre-order traversal");
                }
            }
        }
    }
    // Non-Element nodes (Text, Comment, Doctype) are no-ops — no children to walk.
}

#[cfg(test)]
#[path = "walkers_test.rs"]
mod tests;
