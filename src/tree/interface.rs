use std::{
    cmp::Ordering,
    mem::take,
    ops::RangeInclusive
};

use crate::{
    Reader, Writer, discard,
    arena::{
        Meta, MetaMut,
        PortReadGuard, PortWriteGuard, PortAllocGuard
    },
    tree::{
        Error, Bounds, Tree, SearchResult,
        Node, NodeIndex, NodeRef,
        Value, ValueMut, Color
    }
};

impl<K: Ord, V: Value> Tree<K, V> {
    #[inline]
    pub fn read(&self) -> TreeReadGuard<K, V> {
        TreeReadGuard(self.port.read(), self)
    }
    #[inline]
    pub fn write(&mut self) -> TreeWriteGuard<K, V> {
        TreeWriteGuard(self.port.write(), self)
    }
    #[inline]
    pub fn alloc(&mut self) -> TreeAllocGuard<K, V> {
        TreeAllocGuard(self.port.alloc(), self)
    }
    #[inline]
    pub fn union_disjoint(mut self, mut other: Self) -> Result<Self, ((Self, Self), Error)> {
        {
            let this = self.read();
            match this.len_estimate() {
                LenEstimate::Empty => return Ok(other),
                LenEstimate::Single => {
                    let mut write = other.write();
                    // SAFETY: the root is the only node
                    unsafe { write.insert_node(this.0.meta().root.unwrap()).unwrap_unchecked() };
                    drop(write);
                    return Ok(other);
                },
                _ => ()
            }
        }
        {
            let that = other.read();
            match that.len_estimate() {
                LenEstimate::Empty => return Ok(self),
                LenEstimate::Single => {
                    let mut write = self.write();
                    // SAFETY: the root is the only node
                    unsafe { write.insert_node(that.0.meta().root.unwrap()).unwrap_unchecked() };
                    drop(write);
                    return Ok(self);
                },
                _ => ()
            }
        }
        let mut this = self.write();
        let that = other.read();
        // SAFETY: this is not an empty tree
        let pivot = if this.min() < that.min() {
            if this.max() >= that.min() {
                drop((this, that));
                return Err(((self, other), Error::Overlapping))
            }
            unsafe { this.0.meta().range[1].unwrap_unchecked() }
        } else {
            if that.max() >= this.min() {
                drop((this, that));
                return Err(((self, other), Error::Overlapping))
            }
            unsafe { this.0.meta().range[0].unwrap_unchecked() }
        };
        // SAFETY: pivot was retrived from this tree
        let pivot = unsafe { this.remove_node(pivot) };
        drop((this, that));
        // SAFETY: checks were done before this
        Ok(unsafe { Self::join(self, pivot, other).unwrap_unchecked() })
    }
    #[inline]
    pub fn union_merge<F>(mut self, mut other: Self, merge: F) -> Self
        where F: Fn(&mut V::Mut<'_>, V) + Clone
    {
        {
            let this = self.read();
            match this.len_estimate() {
                LenEstimate::Empty => return other,
                LenEstimate::Single => {
                    let root = this.0.meta().root.unwrap();
                    let mut alloc = other.alloc();
                    match unsafe { alloc.insert_node(root) } {
                        Ok(()) => (),
                        Err(Error::DuplicateKey) => {
                            // SAFETY: root exists
                            let root = alloc.0.remove(root).unwrap();
                            // SAFETY: this error was causes because this exists
                            let mut value = alloc.get_mut(&root.key).unwrap();
                            merge(&mut value, root.value);
                        },
                        _ => panic!("unhandled error")
                    }
                    drop(alloc);
                    return other;
                },
                _ => ()
            }
        }
        {
            let that = other.read();
            match that.len_estimate() {
                LenEstimate::Empty => return self,
                LenEstimate::Single => {
                    let root = that.0.meta().root.unwrap();
                    let mut alloc = self.alloc();
                    match unsafe { alloc.insert_node(root) } {
                        Ok(()) => (),
                        Err(Error::DuplicateKey) => {
                            // SAFETY: root exists
                            let root = alloc.0.remove(root).unwrap();
                            // SAFETY: this error was causes because this exists
                            let mut value = alloc.get_mut(&root.key).unwrap();
                            merge(&mut value, root.value);
                        },
                        _ => panic!("unhandled error")
                    }
                    drop(alloc);
                    return self;
                },
                _ => ()
            }
        }
        // SAFETY: other is not empty
        let (other_left, Some(other_root), other_right) = other.split_at_root()
            else { panic!() };
        let (mut left, pivot, right) = {
            let read = other_left.read();
            let node = &read.0[other_root];
            self.split_node(&node.key)
        };
        if let Some(pivot) = pivot {
            let mut alloc = left.alloc();
            // SAFETY: pivot exists
            let other = alloc.0.remove(pivot).unwrap();
            let node = &mut alloc.0[other_root];
            // TODO: actually to the propagation after building the tree
            merge(&mut unsafe { node.value.get_mut_unchecked() }, other.value);
        }
        let left = left.union_merge(other_left, merge.clone());
        let right = right.union_merge(other_right, merge);
        // SAFETY: left and right are disjoint by other_root by construction
        unsafe { Self::join(left, other_root, right).unwrap_unchecked() }
    }
    #[inline]
    pub(crate) fn split_at_root(mut self) -> (Self, NodeRef, Self) {
        let mut write = self.write();
        let Some(index) = write.0.meta().root
            else {
                drop(write);
                let port = self.port.split_with_meta(Bounds::default());
                return (self, None, Tree::new(port));
            };
        let node = &mut write.0[index];
        let children = take(&mut node.children);
        let order = take(&mut node.order);
        let left_bounds = write.0.meta_mut();
        let mut right_bounds = *left_bounds;
        if let Some(index) = children[0] {
            left_bounds.root = children[0];
            left_bounds.range[1] = order[0];
            discard! {
                write.0[order[0]?].order[1] = None
            };
            let root = &mut write.0[index];
            root.parent = None;
            if root.is_red() {
                root.color = Color::Black;
            } else {
                write.0.meta_mut().black_height -= 1;
            }
        } else {
            *left_bounds = Bounds::default();
        }
        if let Some(index) = children[1] {
            right_bounds.root = children[1];
            right_bounds.range[0] = order[1];
            discard! {
                write.0[order[1]?].order[0] = None
            };
            let root = &mut write.0[index];
            root.parent = None;
            if root.is_red() {
                root.color = Color::Black;
            } else {
                right_bounds.black_height -= 1;
            }
        } else {
            right_bounds = Bounds::default();
        }
        drop(write);
        let port = self.port.split_with_meta(right_bounds);
        (self, Some(index), Tree::new(port))
    }
    #[inline]
    pub(crate) fn split_node(mut self, key: &K) -> (Self, NodeRef, Self) {
        let mut write = self.write();
        if write.is_empty() {
            drop(write);
            let port = self.port.split_with_meta(Bounds::default());
            let other = Tree::new(port);
            return (self, None, other);
        }
        // SAFETY: tree is not empty
        match write.min().unwrap().cmp(key) {
            Ordering::Greater => {
                drop(write);
                let port = self.port.split_with_meta(Bounds::default());
                let other = Tree::new(port);
                return (other, None, self);
            },
            Ordering::Equal => {
                let min = write.0.meta().range[0].unwrap();
                let min = unsafe { write.remove_node(min) };
                drop(write);
                let port = self.port.split_with_meta(Bounds::default());
                let other = Tree::new(port);
                return (other, Some(min), self);
            },
            _ => ()
        }
        match write.max().unwrap().cmp(key) {
            Ordering::Less => {
                drop(write);
                let port = self.port.split_with_meta(Bounds::default());
                let other = Tree::new(port);
                return (self, None, other);
            },
            Ordering::Equal => {
                let max = write.0.meta().range[1].unwrap();
                let max = unsafe { write.remove_node(max) };
                drop(write);
                let port = self.port.split_with_meta(Bounds::default());
                let other = Tree::new(port);
                return (self, Some(max), other);
            },
            _ => ()
        }
        let index = write.0.meta().root.unwrap();
        let node = &mut write.0[index];
        let root_cmp = node.key.cmp(key);
        match root_cmp {
            Ordering::Equal => {
                drop(write);
                self.split_at_root()
            },
            order => {
                if write.is_single() {
                    drop(write);
                    let port = self.port.split_with_meta(Bounds::default());
                    let other = Tree::new(port);
                    return if order.is_lt() {
                            (self, None, other)
                        } else {
                            (other, None, self)
                        };
                }
                drop(write);
                // SAFETY: tree is not empty
                let (left, Some(root), right) = self.split_at_root()
                    else { panic!() };
                if order.is_gt() {
                    let (left, left_child, center) = left.split_node(key);
                    // SAFETY: center and right are disjoint by construction
                    let right = unsafe { Self::join(center, root, right).unwrap_unchecked() };
                    (left, left_child, right)
                } else {
                    let (center, right_child, right) = right.split_node(key);
                    // SAFETY: left and center are disjoint by construction
                    let left = unsafe { Self::join(left, root, center).unwrap_unchecked() };
                    (left, right_child, right)
                }
            }
        }

    }
    #[inline]
    pub fn split(self, key: &K) -> (Self, Option<V::Into>, Self) {
        let (mut left, pivot, right) = self.split_node(key);
        let value = if let Some(index) = pivot {
            let mut alloc = left.alloc();
            // SAFETY: pivot belongs to the original tree
            let node = unsafe { alloc.0.remove(index).unwrap_unchecked() };
            Some(node.value.into())
        } else { None };
        (left, value, right)
    }
}

#[derive(Debug)]
pub struct TreeReadGuard<'a, K: Ord, V: Value>(pub(crate) PortReadGuard<'a, Node<K, V>, Bounds>, &'a Tree<K, V>);

#[derive(Debug)]
pub struct TreeWriteGuard<'a, K: Ord, V: Value>(pub(crate) PortWriteGuard<'a, Node<K, V>, Bounds>, &'a Tree<K, V>);

#[derive(Debug)]
pub struct TreeAllocGuard<'a, K: Ord, V: Value>(pub(crate) PortAllocGuard<'a, Node<K, V>, Bounds>, &'a Tree<K, V>);
impl<'a, K: Ord, V: Value> TreeAllocGuard<'a, K, V> {
    #[inline]
    pub fn downgrade(self) -> TreeWriteGuard<'a, K, V> {
        TreeWriteGuard(self.0.downgrade(), self.1)
    }
    #[inline]
    pub fn insert(&mut self, key: K, value: V::Local) -> bool {
        let value = V::new(value);
        // SAFETY: root is a node in tree
        match unsafe { Tree::search(self.0.meta().root, &key, &self.0) } {
            SearchResult::Here(ptr) => {
                self.0[ptr].value = value;
                return false;
            },
            SearchResult::Empty => {
                let ptr = Some(self.0.insert(Node::new(key, value, Color::Black)));
                let meta = self.0.meta_mut();
                meta.root = ptr;
                meta.range = [ptr, ptr];
                meta.black_height = 1;
            },
            SearchResult::LeftOf(parent) => {
                let ptr = self.0.insert(Node::new(key, value, Color::Red));
                // SAFETY: parent is a leaf
                unsafe { Tree::insert_at::<0>(ptr, parent, &mut self.0); }
            },
            SearchResult::RightOf(parent) => {
                let ptr = self.0.insert(Node::new(key, value, Color::Red));
                // SAFETY: parent is a leaf
                unsafe { Tree::insert_at::<1>(ptr, parent, &mut self.0); }
            }
        }
        true
    }
    #[inline]
    pub fn remove(&mut self, key: K) -> Option<V::Into> {
        // SAFETY: root is a node in tree
        match unsafe { Tree::search(self.0.meta().root, &key, &self.0) } {
            SearchResult::Here(ptr) => {
                // SAFETY: node is the result of a search in tree
                let ptr = unsafe { Tree::remove_at(ptr, &mut self.0) };
                // SAFETY: node was found, so it exists
                Some(self.0.remove(ptr).unwrap().value.into())
            }
            _ => None
        }
    }
    #[inline]
    pub fn clear(&mut self) {
        let mut ptr = self.0.meta().range[0];
        while let Some(index) = ptr {
            ptr = self.0[index].order[1];
            self.0.remove(index);
        }
        let meta = self.0.meta_mut();
        meta.root = None;
        meta.range = [None, None];
    }
}

macro_rules! impl_Reader {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            #[inline]
            pub fn get(&self, key: &K) -> Option<V::Ref<'_>> {
                // SAFETY: root is a node in tree
                let ptr = unsafe { Tree::search(self.0.meta().root, key, &self.0) }
                    .into_here()?;
                Some(self.0[ptr].value.get())
            }
            #[inline]
            pub fn contains(&self, key: &K) -> bool {
                // SAFETY: root is a node in tree
                unsafe { Tree::search(self.0.meta().root, key, &self.0) }
                    .is_here()
            }
        }
    };
}
impl_Reader!(TreeReadGuard);
impl_Reader!(TreeWriteGuard);
impl_Reader!(TreeAllocGuard);

macro_rules! impl_Writer {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            #[inline]
            pub fn get_mut(&mut self, key: &K) -> Option<ValueMut<K, V>> {
                // SAFETY: root is a node in tree
                let ptr = unsafe { Tree::search(self.0.meta().root, key, &self.0) }
                    .into_here()?;
                Some(self.0[ptr].value.get_mut(ptr, self.1))
            }
            #[inline]
            pub fn get_pair_mut(&mut self, a: &K, b: &K) -> Result<[Option<ValueMut<K, V>>; 2], Error> {
                if a == b {
                    return Err(Error::KeyAlias);
                }
                let root = self.0.meta().root;
                // SAFETY: root is a node in tree
                let a = unsafe { Tree::search(root, a, &self.0) };
                let b = unsafe { Tree::search(root, b, &self.0) };
                match (a, b) {
                    (SearchResult::Here(a), SearchResult::Here(b)) => {
                        // SAFETY: a and b are checked before this
                        let [node_a, node_b] = self.0.get_pair_mut(a, b).unwrap();
                        Ok([
                            node_a.map( |node_a| node_a.value.get_mut(a, self.1) ),
                            node_b.map( |node_b| node_b.value.get_mut(b, self.1) )
                        ])
                    },
                    (SearchResult::Here(a), _) => Ok([Some(self.0[a].value.get_mut(a, self.1)), None]),
                    (_, SearchResult::Here(b)) => Ok([None, Some(self.0[b].value.get_mut(b, self.1))]),
                    _ => Ok([None, None])
                }
            }
            #[inline]
            pub fn get_mut_with<const N: usize>(&mut self, key: &K, others: [Option<&K>; N]) -> Result<(Option<ValueMut<K, V>>, [Option<V::Ref<'_>>; N]), Error> {
                if others.iter().any( |k| k.is_some_and( |k| k == key ) ) {
                    return Err(Error::KeyAlias)
                }
                let root = self.0.meta().root;
                // SAFETY: root is a node in tree
                if let SearchResult::Here(x) = unsafe { Tree::search(root, key, &self.0) } {
                    let others = others.map( |k| k.and_then( |k|
                        unsafe { Tree::search(root, k, &self.0) }
                            .into_here()
                    ) );
                    // SAFETY: all keys are checked before this
                    let (x_node, others) = self.0.get_mut_with(x, others).unwrap();
                    Ok((
                        x_node.map( |x_node| x_node.value.get_mut(x, self.1) ),
                        others.map( |x| x.map( |x| x.value.get() ) )
                    ))
                } else {
                    Ok((None, others.map( |k| k.and_then( |k| self.get(k) ) )))
                }
            }
        }
    };
}
impl_Writer!(TreeWriteGuard);
impl_Writer!(TreeAllocGuard);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LenEstimate {
    Empty,
    Single,
    /// this is an upper bound on len
    More(usize)
}

macro_rules! impl_ReadOnly {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            #[inline(always)]
            pub fn is_empty(&self) -> bool {
                self.0.meta().root.is_none()
            }
            #[inline]
            pub fn is_single(&self) -> bool {
                let range = &self.0.meta().range;
                range[0].is_some() && range[0] == range[1]
            }
            #[inline]
            pub fn len_estimate(&self) -> LenEstimate {
                match self.0.meta().range {
                    [None, None] => LenEstimate::Empty,
                    [Some(min), Some(max)] if min == max => LenEstimate::Single,
                    _ => {
                        let bh = self.0.meta().black_height as usize;
                        let h = bh << 2;
                        LenEstimate::More((1 << (h + 1)) - 1)
                    }
                }
            }
            #[inline]
            pub fn min(&self) -> Option<&K> {
                let index = self.0.meta().range[0]?;
                Some(&self.0[index].key)
            }
            #[inline]
            pub fn max(&self) -> Option<&K> {
                let index = self.0.meta().range[1]?;
                Some(&self.0[index].key)
            }
            #[inline]
            pub fn range(&self) -> Option<RangeInclusive<&K>> {
                let [Some(min), Some(max)] = self.0.meta().range else { return None };
                Some((&self.0[min].key)..=(&self.0[max].key))
            }
            /// Searches the tree using the given comparison function.
            /// The tree has to be sorted by compare or the result of this are meaningless.
            ///
            /// This behavious like the standard libary binary search for slices.
            #[inline]
            pub fn search_by<F>(&self, compare: F) -> SearchResult<&K>
                where F: Fn(&K, V::Ref<'_>) -> Ordering
            {
                // SAFETY: root is part of tree
                unsafe {
                    Tree::search_by(
                        self.0.meta().root,
                        |node| compare(&node.key, node.value.get()) ,
                        &self.0
                    ).map( |index| &self.0[index].key )
                }
            }
        }
    };
}
impl_ReadOnly!(TreeReadGuard);
impl_ReadOnly!(TreeWriteGuard);
impl_ReadOnly!(TreeAllocGuard);

macro_rules! impl_ReadWrite {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            /// # Safety
            /// Calling this function implicitly moves the node pointer into this tree,
            /// using the same pointer in a different tree is undefined behaviour.
            #[inline]
            pub(crate) unsafe fn insert_node(&mut self, ptr: NodeIndex) -> Result<(), Error> {
                let key = &self.0[ptr].key;
                // SAFETY: root is a node in tree
                match Tree::search(self.0.meta().root, key, &self.0) {
                    SearchResult::Here(_) => return Err(Error::DuplicateKey),
                    SearchResult::Empty => {
                        // Case 1
                        let meta = self.0.meta_mut();
                        meta.root = Some(ptr);
                        meta.range = [Some(ptr), Some(ptr)];
                        meta.black_height = 1;
                        self.0[ptr].clear(Color::Black);
                    },
                    SearchResult::LeftOf(parent) => {
                        self.0[ptr].clear(Color::Red);
                        // SAFETY: parent is a leaf
                        Tree::insert_at::<0>(ptr, parent, &mut self.0)
                    },
                    SearchResult::RightOf(parent) => {
                        self.0[ptr].clear(Color::Red);
                        // SAFETY: parent is a leaf
                        Tree::insert_at::<1>(ptr, parent, &mut self.0)
                    }
                }
                Ok(())
            }
            /// # Safety
            /// The node pointer has to point to a node in this tree.
            /// This will also implicitly move the node pointer out of this tree,
            /// using the pointer for anything else than inserting it into a different tree is undefined behaviour.
            #[allow(unused)]
            #[inline(always)]
            pub(crate) unsafe fn remove_node(&mut self, ptr: NodeIndex) -> NodeIndex {
                Tree::remove_at(ptr, &mut self.0)
            }
        }
    };
}

impl_ReadWrite!(TreeWriteGuard);
impl_ReadWrite!(TreeAllocGuard);