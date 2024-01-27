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
    unwrap, discard,
    Reader, Writer,
    arena::{Port, Index, Meta, MetaMut, Error as ArenaError},
};

pub type TreeIndex = Index;
pub type TreeRef = Option<TreeIndex>;
pub trait TreeReader<K, V> = Reader<Index, Item = Node<K, V>> + IndexRO<NodeIndex, Output = Node<K, V>> + Meta<Type = Bounds>;
pub trait TreeWriter<K, V> = Writer<Index, ArenaError, Item = Node<K, V>> + IndexMut<NodeIndex, Output = Node<K, V>> + MetaMut<Type = Bounds>;


#[derive_const(Debug, Error)]
pub enum Error {
    #[error("key already exists")]
    DuplicateKey,
    #[error("invalid key combination")]
    GetManyMut,
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

#[derive(Debug)]
pub struct Bounds {
    root: NodeRef,
    range: [NodeRef; 2],
    len: usize
}

#[derive(Debug)]
pub struct Tree<K, V> {
    port: Port<Node<K, V>, Bounds>
}

impl<K, V> Tree<K, V> {
    #[inline]
    unsafe fn rotate<const I: usize>(ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) where [(); 1 - I]: {
        // ASSERT: node has a non-null {1 - I} child
        let node = &tree[ptr];
        let parent = node.parent;
        // SAFETY: guarantied by caller
        let other = node.children[1 - I].unwrap();
        let child_other = tree[other].children[I];
        discard! {
            tree[child_other?].parent = Some(ptr)
        };
        if let Some(parent) = parent {
            let parent_node = &mut tree[parent];
            if parent_node.children[I].is_some_and( |child| child == ptr ) {
                parent_node.children[I] = Some(other);
            } else {
                parent_node.children[1 - I] = Some(other);
            }
        } else {
            tree.meta_mut().root = Some(other);
        }
    }
}

impl<K: Ord, V> Tree<K, V>
{
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
    #[inline]
    unsafe fn insert_at<const I: usize>(ptr: NodeIndex, parent: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) where [(); 1 - I]: {
        // ASSERT: child I is null
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
            Self::fix_insert(ptr, tree)
        }
    }
    #[inline]
    unsafe fn fix_insert(mut ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        // ASSERT: node has a non-null grand-parent
        #[inline]
        unsafe fn helper<const I: usize, const J: usize, K, V>(mut ptr: NodeIndex, parent: NodeIndex, grandparent: NodeIndex,
            tree: &mut impl TreeWriter<K, V>
        ) -> NodeIndex
            where [(); 1 - I]:, [(); 1 - J]:, [(); 1 - (1 - I)]:
        {
            let grandparent_node = &tree[grandparent];
            // SAFETY: tree is balanced, so nodes on parent level cannot be null
            let uncle = grandparent_node.children[I].unwrap();
            let uncle_node = &mut tree[uncle];
            if uncle_node.is_red() {
                // Case 3.1
                uncle_node.color = Color::Black;
                tree[parent].color = Color::Black;
                tree[grandparent].color = Color::Red;
                ptr = grandparent;
            } else {
                if I == J {
                    // Case 3.2.2
                    ptr = parent;
                    Tree::rotate::<{1 - I}>(ptr, tree);
                }
                // Case 3.2.1
                // SAFETY: guarantied by caller
                let parent = tree[ptr].parent.unwrap();
                let parent_node = &mut tree[parent];
                parent_node.color = Color::Black;
                // SAFETY: guarantied by caller
                let grandparent = parent_node.parent.unwrap();
                tree[grandparent].color = Color::Red;
                Tree::rotate::<I>(grandparent, tree);
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
        let root = tree.meta().root.unwrap();
        tree[root].color = Color::Black
    }
    #[inline]
    unsafe fn remove_at(ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        // ASSERT: root is the root of node
        let node = &tree[ptr];
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
    }
    #[inline]
    unsafe fn transplant(ptr: NodeIndex, child: NodeRef,
        tree: &mut impl TreeWriter<K, V>
    ) {
        // ASSERT node is part of tree and child is child of node
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
    #[inline]
    unsafe fn fix_remove(mut ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        // ASSERT: node is black
        #[inline]
        unsafe fn helper<const I: usize, K, V>(mut ptr: NodeIndex, mut parent: NodeIndex,
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
                    // SAFETY: tree is balanced, so nodes on parent level cannot be null
                    parent = tree[ptr].parent.unwrap();
                    // SAFETY: tree is balanced, so nodes on node level cannot be null
                    sibling = tree[parent].children[1 - I].unwrap();
                }
                // Case 3.4
                // SAFETY: sibling is child of parent, both exist
                let [sibling_node, parent_node] = tree.get_many_mut([sibling, parent]).unwrap();
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
}