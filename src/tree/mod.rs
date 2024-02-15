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
    mem::swap,
    ops::{Index as IndexRO, IndexMut}
};

use thiserror::Error;

use crate::{
    discard,
    Reader, Writer,
    arena::{Port, Index, Meta, MetaMut, Error as ArenaError},
};

// SAFETY: these have to be public for generic bounds only, there is no way to access an actual object of this type publically
#[allow(private_bounds)]
pub trait TreeReader<K: Ord, V: Value> = Reader<Index, Item = Node<K, V>> + IndexRO<NodeIndex, Output = Node<K, V>> + Meta<Type = Bounds>;
#[allow(private_bounds)]
pub trait TreeWriter<K: Ord, V: Value> = Writer<Index, ArenaError, Item = Node<K, V>> + IndexMut<NodeIndex, Output = Node<K, V>> + MetaMut<Type = Bounds>;


#[derive_const(Debug, Error)]
pub enum Error {
    #[error("key already exists")]
    DuplicateKey,
    #[error("keys have to be pairwise different")]
    KeyAlias,
    #[error(transparent)]
    Arena(#[from] ArenaError),
    #[error("can only join disjoint trees")]
    Overlapping
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchResult<T> {
    Empty,
    LeftOf(T),
    Here(T),
    RightOf(T)
}
impl<T> SearchResult<T> {
    #[inline]
    pub fn into_here(self) -> Option<T> {
        let Self::Here(value) = self else { return None };
        Some(value)
    }
    #[inline(always)]
    pub fn is_here(&self) -> bool {
        matches!(self, Self::Here(_))
    }
    #[inline]
    pub fn map<R, F>(self, f: F) -> SearchResult<R>
        where F: FnOnce(T) -> R
    {
        match self {
            Self::Here(value) => SearchResult::Here(f(value)),
            Self::LeftOf(value) => SearchResult::LeftOf(f(value)),
            Self::RightOf(value) => SearchResult::RightOf(f(value)),
            Self::Empty => SearchResult::Empty
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Bounds {
    pub(crate) root: NodeRef,
    pub(crate) range: [NodeRef; 2],
    pub(crate) black_height: u8
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
    /// Calculate only the cumulant on the given node.
    ///
    /// # Safety
    /// After this all ancestors of the node have invalid cumulants.
    ///
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
    /// Calculate cumulants starting from the given node and updating all ancestors
    ///
    /// # Safety
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
    /// Calculate cumulants of the sub-tree rooted at the given node.
    ///
    /// # Safety
    /// After this all ancestors of the node have invalid cumulants.
    ///
    /// The node pointer has to be owned by tree.
    #[inline(always)]
    unsafe fn update_cumulants(ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        fn helper<K: Ord, V: Value>(ptr: NodeIndex,
            tree: &mut impl TreeWriter<K, V>
        ) -> *const V::Cumulant {
            let [left, right] = tree[ptr].children;
            let left = left.map( |left| helper(left, tree) );
            let right = right.map( |right| helper(right, tree) );
            let node = &mut tree[ptr];
            // SAFETY: cumulants will always be the already final values from nested call
            let left = left.and_then( |left| unsafe { left.as_ref() } );
            let right = right.and_then( |left| unsafe { left.as_ref() } );
            node.value.update_cumulant([left, right]);
            node.value.cumulant()
        }

        helper(ptr, tree);
    }
    /// # Safety
    /// The node pointers hve to be owned by tree.
    #[inline]
    unsafe fn replace(old: NodeIndex, new: NodeRef,
        tree: &mut impl TreeWriter<K, V>
    ) {
        let parent = tree[old].parent;
        discard! {
            tree[new?].parent = parent
        };
        if let Some(parent) = parent {
            let parent_node = &mut tree[parent];
            if parent_node.children[0].is_some_and( |left| left == old ) {
                parent_node.children[0] = new;
            } else {
                parent_node.children[1] = new;
            }
        } else {
            tree.meta_mut().root = new;
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
        let pivot = tree[ptr].children[1 - I];
        Self::replace(ptr, pivot, tree);
        // SAFETY: guarantied by caller
        let pivot_node = &mut tree[pivot.unwrap()];
        let child = pivot_node.children[I].replace(ptr);
        let node = &mut tree[ptr];
        node.parent = pivot;
        node.children[1 - I] = child;
        discard! {
            tree[child?].parent = Some(ptr)
        };
    }
    /// # Safety
    /// The node pointer has to be owned by tree.
    #[inline(always)]
    unsafe fn search(ptr: NodeRef, key: &K,
        tree: &impl TreeReader<K, V>
    ) -> SearchResult<NodeIndex> {
        // SAFETY: ordering is guarantied by definition
        Self::search_by(ptr, |node| node.key.cmp(key), tree)
    }
    /// The result of this is meaningless,
    /// unless the tree is ordered by `compare`
    ///
    /// # Safety
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn search_by<F>(mut ptr: NodeRef, compare: F,
        tree: &impl TreeReader<K, V>
    ) -> SearchResult<Index>
        where F: Fn(&Node<K, V>) -> Ordering
    {
        let [Some(min), Some(max)] = tree.meta().range
            else { return SearchResult::Empty };
        match compare(&tree[min]) {
            Ordering::Greater => return SearchResult::LeftOf(min),
            Ordering::Equal => return SearchResult::Here(min),
            _ => ()
        }
        match compare(&tree[max]) {
            Ordering::Less => return SearchResult::RightOf(max),
            Ordering::Equal => return SearchResult::Here(max),
            _ => ()
        }
        let (mut parent, mut left) = (None, false);
        while let Some(index) = ptr {
            parent = ptr;
            let node = &tree[index];
            match compare(node) {
                Ordering::Greater => {
                    left = true;
                    ptr = node.children[0];
                },
                Ordering::Equal => return SearchResult::Here(index),
                Ordering::Less => {
                    left = false;
                    ptr = node.children[1];
                }
            }
        }
        // SAFETY: this could only fail for empty tees, which are handled separatly
        let parent = unsafe { parent.unwrap_unchecked() };
        if left {
            SearchResult::LeftOf(parent)
        } else {
            SearchResult::RightOf(parent)
        }
    }
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
        if V::has_cumulant() {
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
                // SAFETY: check in surrounding if
                tree[uncle.unwrap()].color = Color::Black;
                tree[parent].color = Color::Black;
                tree[grandparent].color = Color::Red;
                ptr = grandparent;
            } else {
                if I == J {
                    Tree::rotate::<{1 - I}>(parent, tree);
                    if V::has_cumulant() {
                        Tree::update_cumulant(parent, tree);
                    }
                    ptr = parent;
                }
                // SAFETY: guarantied by caller
                let parent = tree[ptr].parent.unwrap();
                let parent_node = &mut tree[parent];
                parent_node.color = Color::Black;
                // SAFETY: guarantied by caller
                let grandparent = parent_node.parent.unwrap();
                let grandparent_node = &mut tree[grandparent];
                grandparent_node.color = Color::Red;
                Tree::rotate::<I>(grandparent, tree);
                if V::has_cumulant() {
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
                break;
            }
            let is_left = parent_node.children[0].is_some_and( |left| left == ptr );
            // SAFETY: guarantied by caller
            let grandparent = parent_node.parent.unwrap();
            // SAFETY: tree is balanced, so nodes on parent level cannot be null
            ptr = if tree[grandparent].children[1].is_some_and( |uncle| uncle == parent ) {
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
        let root = &mut tree[root];
        if root.is_red() {
            root.color = Color::Black;
            tree.meta_mut().black_height += 1;
        }
    }
    /// # Safety
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn remove_at(mut ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) -> NodeIndex {
        let node = &tree[ptr];
        let mut children = node.children;
        if let [Some(_), Some(_)] = children {
            // SAFETY: node has a right child, so it also has a successor
            let next = node.order[1].unwrap();
            // SAFETY: both nodes exist and are not the same
            let Ok([Some(node), Some(next_node)]) = tree.get_pair_mut(ptr, next)
                else { panic!() };
            swap(&mut node.key, &mut next_node.key);
            swap(&mut node.value, &mut next_node.value);
            ptr = next;
            children = tree[ptr].children;
        }
        let node = &tree[ptr];
        let parent = node.parent;
        let color = node.color;
        let [prev, next] = node.order;
        match children {
            [Some(left), None] => {
                Self::replace(ptr, Some(left), tree);
                tree[left].color = Color::Black;
            },
            [None, Some(right)] => {
                Self::replace(ptr, Some(right), tree);
                tree[right].color = Color::Black;
            },
            [None, None] => if let Some(parent) = parent {
                if color == Color::Red {
                    let parent_node = &mut tree[parent];
                    if parent_node.children[0].is_some_and( |left| left == ptr ) {
                        parent_node.children[0] = None;
                    } else {
                        parent_node.children[1] = None;
                    }
                } else {
                    Self::fix_remove(ptr, tree);
                }
            } else {
                *tree.meta_mut() = Bounds::default();
                return ptr;
            },
            // SAFETY: case of both children was transformed into max one child earlier
            _ => panic!()
        }
        match prev {
            Some(prev) => tree[prev].order[1] = next,
            None => tree.meta_mut().range[0] = next
        }
        match next {
            Some(next) => tree[next].order[0] = prev,
            None => tree.meta_mut().range[1] = prev
        }
        if let Some(parent) = parent {
            if V::has_cumulant() {
                Self::propagate_cumulant(parent, tree);
            }
        } else {
            // DEBUG: commentend out for testing only
            //tree.meta_mut().black_height -= 1;
        }
        ptr
    }
    /// # Safety
    /// The node pointer has to point to a black non-root leaf node.
    ///
    /// The node pointer has to be owned by tree.
    // FIXME: update algorithm to match new call-site
    #[inline]
    unsafe fn fix_remove(mut ptr: NodeIndex,
        tree: &mut impl TreeWriter<K, V>
    ) {
        #[inline]
        unsafe fn helper<const I: usize, K: Ord, V: Value>(parent: NodeIndex,
            tree: &mut impl TreeWriter<K, V>
        ) -> NodeRef
            where [(); 1 - I]:, [(); 1 - (1 - I)]:
        {
            let parent_node = &tree[parent];
            // SAFETY: tree is balanced, so nodes on node level cannot be null
            let mut sibling = parent_node.children[1 - I].unwrap();
            let sibling_node = &mut tree[sibling];
            if sibling_node.is_red() {
                sibling_node.color = Color::Black;
                let nephew = sibling_node.children[I];
                tree[parent].color = Color::Red;
                Tree::rotate::<I>(parent, tree);
                // SAFETY: tree is balanced, so nodes on node level cannot be null
                sibling = nephew.unwrap();
            }
            let nephews = tree[sibling].children;
            let close_red = nephews[I].is_some_and( |nephew| tree[nephew].is_red() );
            if nephews[1 - I].is_some_and( |nephew| tree[nephew].is_red() ) || close_red {
                if close_red {
                    // SAFETY: this is a red node, so it exists
                    let close = nephews[I].unwrap();
                    tree[close].color = Color::Black;
                    tree[sibling].color = Color::Red;
                    Tree::rotate::<{1 - I}>(sibling, tree);
                    sibling = close;
                    if V::has_cumulant() {
                        Tree::update_cumulant(sibling, tree);
                        discard! {
                            Tree::update_cumulant(nephews[I]?, tree)
                        };
                    }
                }
                // SAFETY: sibling is child of parent, both exist
                let [Some(sibling_node), Some(parent_node)] = tree.get_pair_mut(sibling, parent).unwrap() else { panic!() };
                sibling_node.color = parent_node.color;
                parent_node.color = Color::Black;
                // SAFETY: node is red, so it exists
                let far = sibling_node.children[1 - I].unwrap();
                tree[far].color = Color::Black;
                Tree::rotate::<I>(parent, tree);
                return None;
            }
            tree[sibling].color = Color::Red;
            let parent_node = &mut tree[parent];
            if parent_node.is_red() {
                parent_node.color = Color::Black;
                return None;
            }
            Some(parent)
        }
        // SAFETY: node is not the root
        let mut parent = tree[ptr].parent.unwrap();
        let parent_node = &mut tree[parent];
        let mut is_right = parent_node.children[1].is_some_and( |right| right == ptr );
        parent_node.children[is_right as usize] = None;
        loop {
            let next = if is_right {
                helper::<1, K, V>(parent, tree)
            } else {
                helper::<0, K, V>(parent, tree)
            };
            if let Some(next) = next {
                let node = &tree[next];
                if let Some(par) = node.parent {
                    (ptr, parent) = (next, par);
                    is_right = tree[parent].children[1].is_some_and( |right| right == ptr );
                } else {
                    tree.meta_mut().black_height -= 1;
                    return;
                }
            } else { return; }
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
    /// # Safety
    /// The node pointer has to be owned by tree.
    #[inline]
    unsafe fn closest<const I: usize, const INCLUSIVE: bool>(ptr: NodeRef, key: &K,
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
    /// # Note
    /// There is no fast-pass for empty trees, that should be checked by the caller.
    ///
    /// # Safety
    /// The pivot will be moved into this tree and should not be referenced by any other tree after this.
    ///
    /// `a->max->key < pivot->key < b->min->key` (this is reversed for I == 1)
    #[inline]
    unsafe fn join_unchecked<const I: usize>(
        this: &mut impl TreeWriter<K, V>,
        pivot: NodeIndex, that: Port<Node<K, V>, Bounds>
    ) where [(); 1 - I]: {
        #[inline]
        unsafe fn helper<const I: usize, K: Ord, V: Value>(
            this: &mut impl TreeWriter<K, V>,
            parent: NodeRef, this_child: NodeRef,
            pivot: NodeIndex, that_meta: Bounds
        ) where [(); 1 - I]: {
            let ptr = Some(pivot);
            let this_meta = *this.meta();
            let pivot_node = &mut this[pivot];
            pivot_node.color = Color::Red;
            pivot_node.parent = parent;
            let mut children = [None; 2];
            children[I] = this_child;
            children[1 - I] = that_meta.root;
            pivot_node.children = children;
            let mut order = [None; 2];
            order[I] = this_meta.range[1 - I];
            order[1 - I] = that_meta.range[I];
            pivot_node.order = order;
            discard! {
                this[this_child?].parent = ptr
            };
            discard! {
                this[that_meta.root?].parent = ptr
            };
            discard! {
                this[this_meta.range[1 - I]?].order[1 - I] = ptr
            };
            discard! {
                this[that_meta.range[I]?].order[I] = ptr
            };
            let this_meta = this.meta_mut();
            this_meta.range[1 - I] = that_meta.range[1 - I];
            if let Some(parent) = parent {
                if this[parent].parent.is_some() {
                    Tree::fix_insert(pivot, this);
                }
            } else {
                this_meta.root = ptr;
                this_meta.black_height += 1;
                this[pivot].color = Color::Black;
            }
            if V::has_cumulant() {
                Tree::propagate_cumulant(pivot, this);
            }
        }
        let ptr = Some(pivot);
        let this_meta = this.meta();
        let that_meta = that.free();
        // SAFETY: this is never negative
        let mut diff = this_meta.black_height - that_meta.black_height;
        if diff == 0 {
            helper::<I, K, V>(this, None, this_meta.root, pivot, that_meta);
            return;
        }
        // SAFETY: at this point this treee cannot be empty
        let mut index = this_meta.root.unwrap();
        while diff != 0 {
            let node = &this[index];
            if node.is_black() {
                diff -= 1;
            }
            if let Some(next) = node.children[1 - I] {
                index = next;
            } else {
                // SAFETY: join point will be found before node is null
                index = node.children[I].unwrap();
            }
        }
        Self::replace(index, ptr, this);
        helper::<I, K, V>(this, this[index].parent, Some(index), pivot, that_meta);
    }
    /// # Note
    /// There is no fast-pass for empty trees, that should be checked by the caller.
    ///
    /// # Safety
    /// The pivot will be moved into this tree and should not be referenced by any other tree after this.
    // TODO: make the compiler realize it can automatically drop this/that
    #[inline]
    unsafe fn join(mut self, pivot: NodeIndex, mut other: Self) -> Result<Self, ((Self, Self), Error)> {
        {
            let this = self.read();
            if this.is_empty() {
                let mut write = other.write();
                if let Err(err) = write.insert_node(pivot) {
                    drop((this, write));
                    return Err(((self, other), err));
                }
                drop(write);
                return Ok(other);
            }
        }
        {
            let that = other.read();
            if that.is_empty() {
                let mut write = self.write();
                if let Err(err) = write.insert_node(pivot) {
                    drop((that, write));
                    return Err(((self, other), err));
                }
                drop(write);
                return Ok(self);
            }
        }
        let mut this = self.write();
        let mut that = other.write();
        let center = &this.0[pivot].key;
        // SAFETY: both trees are not empty here
        if this.max().unwrap() < center
            && center < that.min().unwrap()
        {
            if this.0.meta().black_height >= that.0.meta().black_height {
                drop(that);
                Self::join_unchecked::<0>(&mut this.0, pivot, other.port);
                drop(this);
                Ok(self)
            } else {
                drop(this);
                Self::join_unchecked::<1>(&mut that.0, pivot, self.port);
                drop(that);
                Ok(other)
            }
        } else if that.max().unwrap() < center
            && center < this.min().unwrap()
        {
            if this.0.meta().black_height >= that.0.meta().black_height {
                drop(that);
                Self::join_unchecked::<1>(&mut this.0, pivot, other.port);
                drop(this);
                Ok(self)
            } else {
                drop(this);
                Self::join_unchecked::<0>(&mut that.0, pivot, self.port);
                drop(that);
                Ok(other)
            }
        } else {
            drop((this, that));
            Err(((self, other), Error::Overlapping))
        }
    }
}