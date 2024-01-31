mod node;
pub use node::*;
mod interface;
pub use interface::*;
mod iter;
pub use iter::*;
mod cursor;
pub use cursor::*;

use std::{
    cmp::Ordering,
    ops::{Index as IndexRO, IndexMut}
};

use thiserror::Error;

use crate::{
    discard,
    Reader, Writer,
    arena::{Port, Index, Meta, MetaMut, Error as ArenaError},
};

pub trait TreeReader<K: Ord, V: Value> = Reader<Index, Item = Node<K, V>> + IndexRO<NodeIndex, Output = Node<K, V>> + Meta<Type = Bounds>;
pub trait TreeWriter<K: Ord, V: Value> = Writer<Index, ArenaError, Item = Node<K, V>> + IndexMut<NodeIndex, Output = Node<K, V>> + MetaMut<Type = Bounds>;


#[derive_const(Debug, Error)]
pub enum Error {
    #[error("key already exists")]
    DuplicateKey,
    #[error("keys have to be pairwise different")]
    KeyAlias,
    #[error(transparent)]
    Arena(#[from] ArenaError)
}

#[derive(Debug)]
enum SearchResult<T> {
    Empty,
    LeftOf(T),
    Here(T),
    RightOf(T)
}
impl<T> SearchResult<T> {
    #[inline]
    fn into_here(self) -> Option<T> {
        let Self::Here(value) = self else { return None };
        Some(value)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Bounds {
    root: NodeRef,
    range: [NodeRef; 2],
    len: usize
}

#[derive(Debug)]
pub struct Tree<K: Ord, V: Value> {
    port: Port<Node<K, V>, Bounds>
}

impl<K: Ord, V: Value> Tree<K, V> {
    #[inline(always)]
    pub(crate) fn new(port: Port<Node<K, V>, Bounds>) -> Self {
        Self { port }
    }
    /// # Safety
    /// The left and right pointers have to be pointing to the children of the node pointer.
    ///
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn update_cumulant(ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        let [left, right] = tree[ptr].children;
        let (Some(node), [left, right]) = tree.get_mut_with(ptr, [left, right]).unwrap() else { panic!() };
        let left = left.map( |left| left.value.cumulant() );
        let right = right.map( |right| right.value.cumulant() );
        node.value.update_cumulant([left, right]);
    }
    /// # Safety
    ///
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn propagate_cumulant(ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        let mut ptr = Some(ptr);
        while let Some(index) = ptr {
            let node = &tree[index];
            ptr = node.parent;
            Self::update_cumulant(index, tree);
        }
    }
    /// # Safety
    /// the node at `ptr->children[1 - I]` cannot be None.
    ///
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn rotate<const I: usize>(ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) where [(); 1 - I]: {
        let node = &tree[ptr];
        let parent = node.parent;
        // SAFETY: guarantied by caller
        let pivot = node.children[1 - I].unwrap();
        let pivot_node = &mut tree[pivot];
        let child = pivot_node.children[I];
        pivot_node.parent = parent;
        pivot_node.children[I] = Some(ptr);
        discard! {
            tree[child?].parent = Some(ptr)
        };
        let node = &mut tree[ptr];
        node.parent = Some(pivot);
        node.children[1 - I] = child;
        if let Some(parent) = parent {
            let parent_node = &mut tree[parent];
            if parent_node.children[I].is_some_and( |child| child == ptr ) {
                parent_node.children[I] = Some(pivot);
            } else {
                parent_node.children[1 - I] = Some(pivot);
            }
        } else {
            tree.meta_mut().root = Some(pivot);
        }
    }
    #[inline]
    fn search(mut ptr: NodeRef, key: &K,
        tree: &impl TreeReader<K, V>
    ) -> SearchResult<NodeIndex> {
        let (mut parent, mut left) = (None, false);
        while let Some(valid) = ptr {
            parent = ptr;
            let node = &tree[valid];
            match node.key.cmp(key) {
                Ordering::Greater => {
                    left = true;
                    ptr = node.children[0];
                },
                Ordering::Equal => return SearchResult::Here(valid),
                Ordering::Less => {
                    left = false;
                    ptr = node.children[1];
                }
            }
        }
        if let Some(parent) = parent {
            if left {
                SearchResult::LeftOf(parent)
            } else {
                SearchResult::RightOf(parent)
            }
        } else { SearchResult::Empty }
    }
    // TODO: propagate cumulants after insert/delete
    /// # Safety
    /// The node at `ptr->children[I]` cannot be None.
    ///
    /// The node pointers have to be owned by tree.
    #[inline]
    unsafe fn insert_at<const I: usize>(ptr: NodeIndex, parent: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) where [(); 1 - I]: {
        let mut order = [None, None];
        order[I] = tree[parent].order[I];
        order[1 - I] = Some(parent);
        let node = &mut tree[ptr];
        node.parent = Some(parent);
        node.order = order;
        match order[I] {
            Some(far) => tree[far].order[1 - I] = Some(ptr),
            None => tree.meta_mut().range[I] = Some(ptr)
        }
        let parent_node = &mut tree[parent];
        parent_node.children[I] = Some(ptr);
        parent_node.order[I] = Some(ptr);
        if parent_node.parent.is_some() {
            Self::fix_insert(ptr, tree);
        }
        if V::need_update() {
            Self::propagate_cumulant(ptr, tree);
        }
    }
    /// # Safety
    /// The node at `ptr->parent->parent` cannot be None.
    #[inline]
    unsafe fn fix_insert(mut ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        #[inline]
        unsafe fn helper<const I: usize, const J: usize, K: Ord, V: Value>(mut ptr: NodeIndex, parent: NodeIndex, grandparent: NodeIndex,
            tree: &mut impl TreeWriter<K, V>
        ) -> NodeIndex
            where [(); 1 - I]:, [(); 1 - J]:, [(); 1 - (1 - I)]:
        {
            let grandparent_node = &tree[grandparent];
            // SAFETY: tree is balanced, so nodes on parent level cannot be null
            let uncle = grandparent_node.children[I];
            if uncle.is_some_and( |uncle| tree[uncle].is_red() ) {
                // Case 3.1
                // SAFETY: check in surrounding if
                tree[uncle.unwrap()].color = Color::Black;
                tree[parent].color = Color::Black;
                tree[grandparent].color = Color::Red;
                ptr = grandparent;
            } else {
                if I == J {
                    // Case 3.2.2
                    Tree::rotate::<{1 - I}>(parent, tree);
                    if V::need_update() {
                        Tree::update_cumulant(parent, tree);
                    }
                    ptr = parent;
                }
                // Case 3.2.1
                // SAFETY: guarantied by caller
                let parent = tree[ptr].parent.unwrap();
                let parent_node = &mut tree[parent];
                parent_node.color = Color::Black;
                // SAFETY: guarantied by caller
                let grandparent = parent_node.parent.unwrap();
                let grandparent_node = &mut tree[grandparent];
                grandparent_node.color = Color::Red;
                Tree::rotate::<I>(grandparent, tree);
                if V::need_update() {
                    Tree::update_cumulant(grandparent, tree);
                }
            }
            ptr
        }

        loop {
            let node = &tree[ptr];
            // SAFETY: node cannot be the root
            let parent = node.parent.unwrap();
            let parent_node = &tree[parent];
            if parent_node.is_black() {
                // Case 2
                break;
            }
            let is_left = parent_node.children[0].is_some_and( |left| left == ptr );
            // SAFETY: guarantied by caller
            let grandparent = parent_node.parent.unwrap();
            // SAFETY: tree is balanced, so nodes on parent level cannot be null
            ptr = if tree[grandparent].children[1].unwrap() == parent {
                if is_left {
                    helper::<0, 0, K, V>(ptr, parent, grandparent, tree)
                } else {
                    helper::<0, 1, K, V>(ptr, parent, grandparent, tree)
                }
            } else if is_left {
                helper::<1, 0, K, V>(ptr, parent, grandparent, tree)
            } else {
                helper::<1, 1, K, V>(ptr, parent, grandparent, tree)
            };
            if Some(ptr) == tree.meta().root { break }
        }
        // SAFETY: tree is not empty
        // NOTE: remove when adding join
        let root = tree.meta().root.unwrap();
        tree[root].color = Color::Black
    }
    /// # Safety
    ///
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn remove_at(ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        let node = &tree[ptr];
        let parent = node.parent;
        let mut color = node.color;
        let [prev, next] = node.order;
        let fix = if node.children[0].is_none() {
            let fix = node.children[1];
            Self::transplant(ptr, fix, tree);
            fix
        } else if node.children[1].is_none() {
            let fix = node.children[0];
            Self::transplant(ptr, fix, tree);
            fix
        } else {
            // SAFETY: node has a right child, so has to have a succsesor
            let min = tree[ptr].order[1].unwrap();
            let min_node = &tree[min];
            color = min_node.color;
            let fix = min_node.children[1];
            if min_node.parent.is_some_and( |parent| parent == ptr ) {
                // SAFETY: node has both children in this branch
                tree[fix.unwrap()].parent = Some(min);
            } else {
                Self::transplant(min, tree[min].children[1], tree);
                let right = tree[ptr].children[1];
                tree[min].children[1] = right;
                // SAFETY: node has both children in this branch
                tree[right.unwrap()].parent = Some(min);
            }
            Self::transplant(ptr, Some(min), tree);
            let node = &tree[ptr];
            let left = node.children[0];
            let color = node.color;
            let min_node = &mut tree[min];
            min_node.children[0] = left;
            min_node.color = color;
            // SAFETY: node has both children in this branch
            tree[left.unwrap()].parent = Some(min);
            fix
        };
        match prev {
            Some(prev) => tree[prev].order[1] = next,
            None => tree.meta_mut().range[0] = next
        }
        match next {
            Some(next) => tree[next].order[0] = prev,
            None => tree.meta_mut().range[1] = prev
        }
        if let (Some(fix), Color::Black) = (fix, color) {
            // SAFETY: search was successful, so tree cannot be empty
            Self::fix_remove(fix, tree)
        }
        if let (Some(parent), true) = (parent, V::need_update()) {
            Self::propagate_cumulant(parent, tree);
        }
    }
    /// # Safety
    /// The child pointer has to be a child node of the given node.
    ///
    /// The node pointers hve to be owned by tree.
    #[inline]
    unsafe fn transplant(ptr: NodeIndex, child: NodeRef,
        tree: &mut impl TreeWriter<K, V>
    ) {
        let parent = tree[ptr].parent;
        discard! {
            tree[child?].parent = parent
        };
        if let Some(parent) = parent {
            let parent_node = &mut tree[parent];
            if parent_node.children[0].is_some_and( |left| left == ptr ) {
                parent_node.children[0] = child;
            } else {
                parent_node.children[1] = child;
            }
        } else {
            tree.meta_mut().root = child;
        }
    }
    /// # Safety
    /// The node pointer has to point to a black node.
    ///
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn fix_remove(mut ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        #[inline]
        unsafe fn helper<const I: usize, K: Ord, V: Value>(mut ptr: NodeIndex, mut parent: NodeIndex,
            tree: &mut impl TreeWriter<K, V>
        ) -> NodeIndex
            where [(); 1 - I]:, [(); 1 - (1 - I)]:
        {
            let parent_node = &tree[parent];
            // SAFETY: tree is balanced, so nodes on node level cannot be null
            let mut sibling = parent_node.children[1 - I].unwrap();
            let sibling_node = &mut tree[sibling];
            if sibling_node.is_red() {
                // Case 3.1
                sibling_node.color = Color::Black;
                tree[parent].color = Color::Red;
                Tree::rotate::<I>(parent, tree);
                // SAFETY: tree is balanced, so nodes on parent level cannot be null
                parent = tree[ptr].parent.unwrap();
                // SAFETY: tree is balanced, so nodes on node level cannot be null
                sibling = tree[parent].children[1 - I].unwrap();
            }
            let nephews = tree[sibling].children;
            let is_black = !nephews[1 - I].is_some_and( |nephew| tree[nephew].is_red() );
            if !nephews[I].is_some_and( |nephew| tree[nephew].is_red() ) && is_black {
                // Case 3.2
                tree[sibling].color = Color::Red;
                ptr = parent;
            } else {
                if is_black {
                    // Case 3.3
                    discard! {
                        tree[nephews[I]?].color = Color::Black
                    };
                    tree[sibling].color = Color::Red;
                    Tree::rotate::<{1 - I}>(sibling, tree);
                    if V::need_update() {
                        Tree::update_cumulant(sibling, tree);
                        discard! {
                            Tree::update_cumulant(nephews[I]?, tree)
                        };
                    }
                    // SAFETY: tree is balanced, so nodes on parent level cannot be null
                    parent = tree[ptr].parent.unwrap();
                    // SAFETY: tree is balanced, so nodes on node level cannot be null
                    sibling = tree[parent].children[1 - I].unwrap();
                }
                // Case 3.4
                // SAFETY: sibling is child of parent, both exist
                let [Some(sibling_node), Some(parent_node)] = tree.get_pair_mut(sibling, parent).unwrap() else { panic!() };
                sibling_node.color = parent_node.color;
                parent_node.color = Color::Black;
                // SAFETY: tree is balanced, so nodes on node level cannot be null
                let nephew = sibling_node.children[1 - I].unwrap();
                tree[nephew].color = Color::Black;
                Tree::rotate::<I>(parent, tree);
                ptr = tree.meta().root.unwrap();
            }
            ptr
        }

        loop {
            let node = &mut tree[ptr];
            if let (Some(parent), Color::Black) = (node.parent, node.color) {
                // Case 3
                ptr = if tree[parent].children[0].is_some_and( |left| left == ptr ) {
                    helper::<0, K, V>(ptr, parent, tree)
                } else {
                    helper::<1, K, V>(ptr, parent, tree)
                };
            } else {
                // Case 1
                node.color = Color::Black;
                return;
            }
        }
    }
    #[inline]
    fn limit<const I: usize>(mut ptr: NodeIndex,
        tree: &impl TreeReader<K, V>
    ) -> NodeIndex
        where [(); 1 - I]:
    {
        while let Some(left) = tree[ptr].children[I] {
            ptr = left;
        }
        ptr
    }
    #[inline]
    fn closest<const I: usize, const INCLUSIVE: bool>(ptr: NodeRef, key: &K,
        tree: &impl TreeReader<K, V>
    ) -> NodeRef
        where [(); 1 - I]:
    {
        match Self::search(ptr, key, tree) {
            SearchResult::Empty => None,
            SearchResult::Here(node) =>
                if INCLUSIVE { tree[node].order[I] }
                else { Some(node) },
            SearchResult::LeftOf(node) =>
                if I == 0 { Some(node) }
                else { tree[node].order[1] },
            SearchResult::RightOf(node) =>
                if I == 1 { Some(node) }
                else { tree[node].order[0] }
        }
    }
}