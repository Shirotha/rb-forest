use std::{
    marker::PhantomData,
    mem::ManuallyDrop,
    ptr::read
};

#[cfg(feature = "sorted-iter")]
pub use sorted_iter::sorted_pair_iterator::SortedByKey;

use crate::{
    discard,
    arena::{Meta, MetaMut, Port, PortAllocGuard},
    tree::{
        Bounds, Color, Node, NodeRef, Tree, Value,
        TreeReader, TreeWriter,
        TreeAllocGuard, TreeReadGuard, TreeWriteGuard,
    }
};

#[derive(Debug)]
pub struct Iter<'a, K: Ord, V, R: TreeReader<K, V>> {
    pub(crate) tree: &'a R,
    pub(crate) front: NodeRef,
    pub(crate) back: NodeRef,
    pub(crate) _phantom: PhantomData<(K, V)>
}
impl<'a, K: Ord + 'a, V: 'a, R: TreeReader<K, V>> Iterator for Iter<'a, K, V, R> {
    type Item = (&'a K, &'a V);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let current = self.front?;
        let node = &self.tree[current];
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.front = node.order[1];
        }
        Some((&node.key, &node.value))
    }
}
impl<'a, K: Ord + 'a, V: 'a, R: TreeReader<K, V>> DoubleEndedIterator for Iter<'a, K, V, R> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        let current = self.back?;
        let node = &self.tree[current];
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.back = node.order[0];
        }
        Some((&node.key, &node.value))
    }
}
#[cfg(feature = "sorted-iter")]
impl<'a, K: Ord, V, R: TreeReader<K, V>> SortedByKey for Iter<'a, K, V, R> {}

#[derive(Debug)]
pub struct IterMut<'a, K: Ord, V, W: TreeWriter<K, V>> {
    pub(crate) tree: &'a mut W,
    pub(crate) front: NodeRef,
    pub(crate) back: NodeRef,
    pub(crate) _phantom: PhantomData<(K, V)>
}
impl<'a, K: Ord + 'a, V: 'a, W: TreeWriter<K, V>> Iterator for IterMut<'a, K, V, W> {
    type Item = (&'a K, &'a mut V);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let current = self.front?;
        let node = &mut self.tree[current];
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.front = node.order[1];
        }
        // SAFETY: there is no other way to access tree
        let node = unsafe { (node as *mut Node<K, V>).as_mut().unwrap() };
        Some((&node.key, &mut node.value))
    }
}

impl<'a, K: Ord + 'a, V: 'a, W: TreeWriter<K, V>> DoubleEndedIterator for IterMut<'a, K, V, W> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        let current = self.back?;
        let node = &mut self.tree[current];
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.back = node.order[0];
        }
        // SAFETY: there is no other way to access tree
        let node = unsafe { (node as *mut Node<K, V>).as_mut().unwrap() };
        Some((&node.key, &mut node.value))
    }
}
#[cfg(feature = "sorted-iter")]
impl<'a, K: Ord, V, W: TreeWriter<K, V>> SortedByKey for IterMut<'a, K, V, W> {}

#[derive(Debug)]
pub struct IntoIter<K: Ord, V: Value> {
    port: Port<Node<K, V>, Bounds>
}
impl<K: Ord, V: Value> Iterator for IntoIter<K, V> {
    type Item = (K, V);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let mut port = self.port.alloc();
        let meta = port.meta();
        // SAFETY: tree is not empty
        let index = meta.range[0]?;
        // SAFETY: node exists in this tree
        let node = port.remove(index).unwrap();
        let meta = port.meta_mut();
        // SAFETY: either both range bounds are null, or neither
        if meta.range[1].unwrap() == index {
            meta.range = [None; 2];
        } else {
            meta.range[0] = node.order[1];
        }
        Some((node.key, node.value))
    }
}
impl<K: Ord, V: Value> DoubleEndedIterator for IntoIter<K, V> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        let mut port = self.port.alloc();
        let meta = port.meta();
        // SAFETY: tree is not empty
        let index = meta.range[1]?;
        // SAFETY: node exists in this tree
        let node = port.remove(index).unwrap();
        let meta = port.meta_mut();
        // SAFETY: either both range bounds are null, or neither
        if meta.range[0].unwrap() == index {
            meta.range = [None; 2];
        } else {
            meta.range[1] = node.order[0];
        }
        Some((node.key, node.value))
    }
}
impl<K: Ord, V: Value> ExactSizeIterator for IntoIter<K, V> {}
#[cfg(feature = "sorted-iter")]
impl<K: Ord, V: Value> SortedByKey for IntoIter<K, V> {}

macro_rules! impl_Iter {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            #[inline]
            pub fn iter(&self) -> Iter<K, V, impl TreeReader<K, V> + 'a> {
                let [front, back] = self.0.meta().range;
                Iter { tree: &self.0, front, back, _phantom: PhantomData }
            }
            #[inline]
            pub fn iter_range<const LI: bool, const RI: bool>(&self, min: &K, max: &K) -> Iter<K, V, impl TreeReader<K, V> + 'a> {
                // SAFETY: root is a node in tree
                unsafe {
                    let meta = self.0.meta();
                    let front = Tree::closest::<0, LI>(meta.root, min, &self.0)
                        .or_else( || meta.range[0] );
                    let back = Tree::closest::<1, RI>(meta.root, max, &self.0)
                        .or_else( || meta.range[1] );
                    Iter { tree: &self.0, front, back, _phantom: PhantomData }
                }
            }
        }
    };
}
impl_Iter!(TreeReadGuard);
impl_Iter!(TreeWriteGuard);
impl_Iter!(TreeAllocGuard);

macro_rules! impl_IterMut {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            #[inline]
            pub fn iter_mut(&mut self) -> IterMut<K, V, impl TreeWriter<K, V> + 'a> {
                let [front, back] = self.0.meta().range;
                IterMut { tree: &mut self.0, front, back, _phantom: PhantomData }
            }
            #[inline]
            pub fn iter_range_mut<const LI: bool, const RI: bool>(&mut self, min: &K, max: &K) -> IterMut<K, V, impl TreeWriter<K, V> + 'a> {
                // SAFETY: root is a node in tree
                unsafe {
                    let meta = self.0.meta();
                    let front = Tree::closest::<0, LI>(meta.root, min, &self.0)
                        .or_else( || meta.range[0] );
                    let back = Tree::closest::<1, RI>(meta.root, max, &self.0)
                        .or_else( || meta.range[1] );
                    IterMut { tree: &mut self.0, front, back, _phantom: PhantomData }
                }
            }
        }
    };
}
impl_IterMut!(TreeWriteGuard);
impl_IterMut!(TreeAllocGuard);

macro_rules! impl_IntoIterator_Ref {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> IntoIterator for &'a $type <'a, K, V> {
            type IntoIter = Iter<'a, K, V, impl TreeReader<K, V> + 'a>;
            type Item = <Self::IntoIter as Iterator>::Item;
            #[inline]
            fn into_iter(self) -> Self::IntoIter {
                let [front, back] = self.0.meta().range;
                Iter { tree: &self.0, front, back, _phantom: PhantomData }
            }
        }
    };
}
impl_IntoIterator_Ref!(TreeReadGuard);
impl_IntoIterator_Ref!(TreeWriteGuard);
impl_IntoIterator_Ref!(TreeAllocGuard);

macro_rules! impl_IntoIterator_Mut {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> IntoIterator for &'a mut $type <'a, K, V> {
            type IntoIter = IterMut<'a, K, V, impl TreeWriter<K, V> + 'a>;
            type Item = <Self::IntoIter as Iterator>::Item;
            #[inline]
            fn into_iter(self) -> Self::IntoIter {
                let [front, back] = self.0.meta().range;
                IterMut { tree: &mut self.0, front, back, _phantom: PhantomData }
            }
        }
    };
}

impl_IntoIterator_Mut!(TreeWriteGuard);
impl_IntoIterator_Mut!(TreeAllocGuard);

impl<K: Ord, V: Value> IntoIterator for Tree<K, V> {
    type IntoIter = IntoIter<K, V>;
    type Item = <Self::IntoIter as Iterator>::Item;
    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        IntoIter { port: self.port }
    }
}

impl<K: Ord, V: Value> Tree<K, V> {
    #[inline]
    /// # Safety
    /// It is assumed that the given iterator is sorted by K in incresing order.
    /// Port->meta is expected to be set to its default value
    ///
    /// For a safe version of this function use the 'sorted-iter' feature.
    pub(crate) unsafe fn from_sorted_iter_unchecked(port: Port<Node<K, V>, Bounds>, iter: impl IntoIterator<Item = (K, V)>) -> Self {
        fn build_tree<K: Ord, V: Value>(
            port: &mut PortAllocGuard<Node<K, V>, Bounds>,
            items: &[(K, V)], parent: NodeRef, color: Color
        ) -> [NodeRef; 3]
        {
            let len = items.len();
            match len {
                0 => return [None, None, None],
                1 => {
                    // SAFETY: items is not empty
                    let (key, value) = unsafe { read(&items[0]) };
                    let mut leaf = Node::new(key, value, color);
                    leaf.parent = parent;
                    let this = Some(port.insert(leaf));
                    return [this, this, this];
                },
                _ => ()
            }
            let pivot = len >> 1;
            // SAFETY: index is smaller then from len
            let (lower, rest) = unsafe { items.split_at_unchecked(pivot) };
            // SAFETY: pivot exists, to rest is not empty
            let (this, upper) = unsafe { rest.split_at_unchecked(1) };
            // SAFETY: index corresponds to pivot
            let (key, value) = unsafe { read(&this[0]) };
            let mut root = Node::new(key, value, color);
            root.parent = parent;
            let index = port.insert(root);
            let this = Some(index);
            let color = !color;
            let [min, left, prev] = build_tree(port, lower, this, color);
            let [next, right, max] = build_tree(port, upper, this, color);
            let root = &mut port[index];
            root.children = [left, right];
            root.order = [prev, next];
            discard! {
                port[prev?].order[1] = this
            };
            discard! {
                port[next?].order[0] = this
            };
            [
                if pivot == 0 { this } else { min },
                this,
                if pivot == len - 1 { this } else { max }
            ]
        }

        let items = ManuallyDrop::new(iter.into_iter().collect::<Box<[_]>>());
        if items.is_empty() {
            return Self { port }
        }
        let len = items.len();
        let height = usize::BITS - (len + 1).leading_zeros();
        let color = if height & 1 == 0 { Color::Black }
            else { Color::Red };
        {
            let mut port = port.alloc();
            // NOTE: recursion depth = height + 1
            let [min, root, max] = build_tree(&mut port, &items, None, color);
            let meta = port.meta_mut();
            meta.root = root;
            meta.range = [min, max];
            meta.black_height = ((height + 1) >> 1) as u8;
            if let Some(root) = root {
                let node = &mut port[root];
                if node.is_red() {
                    node.color = Color::Black;
                    port.meta_mut().black_height += 1;
                }
            }
        }
        Self { port }
    }
    #[cfg(feature = "sorted-iter")]
    #[inline(always)]
    pub(crate) fn from_sorted_iter(port: Port<Node<K, V>, Bounds>, iter: impl IntoIterator<Item = (K, V)> + SortedByKey) -> Self {
        // SAFETY: guarantied by trait
        unsafe { Self::from_sorted_iter_unchecked(port, iter) }
    }
}

// TODO: Filter/FilterMut iterators (-> use cumulant for early exit)