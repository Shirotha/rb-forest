use std::marker::PhantomData;

#[cfg(feature = "sorted-iter")]
pub use sorted_iter::sorted_pair_iterator::SortedByKey;

use crate::arena::{Port, PortReadGuard, PortWriteGuard};
use super::{
    Bounds, Node, NodeRef,
    TreeReadGuard, TreeWriteGuard, TreeAllocGuard,
    TreeReader, TreeWriter
};

pub struct Iter<'a, K: Ord, V, R: TreeReader<K, V>> {
    tree: &'a R,
    front: NodeRef,
    back: NodeRef,
    _phantom: PhantomData<(K, V)>
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
impl<'a, K: Ord, V> SortedByKey for Iter<'a, K, V> {}

pub struct IterMut<'a, K: Ord, V, W: TreeWriter<K, V>> {
    tree: &'a mut W,
    front: NodeRef,
    back: NodeRef,
    _phantom: PhantomData<(K, V)>
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
        todo!("lifetime issue")
        //Some((&node.key, &mut node.value))
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
        todo!("lifetime issue")
        //Some((&node.key, &mut node.value))
    }
}
#[cfg(feature = "sorted-iter")]
impl<'a, K: Ord, V> SortedByKey for IterMut<'a, K, V> {}

pub struct IntoIter<K: Ord, V> {
    port: Port<Node<K, V>, Bounds>
}
// TODO: impl this (should lock be held all the time, or only while next runs?)

macro_rules! impl_Iter {
    ( $type:ident ) => {
        impl<'a, K: Ord, V> $type <'a, K, V> {
            #[inline]
            pub fn iter(&self) -> Iter<K, V, impl TreeReader<K, V> + 'a> {
                let [front, back] = self.0.meta().range;
                Iter { tree: &self.0, front, back, _phantom: PhantomData }
            }
        }
    };
}
impl_Iter!(TreeReadGuard);
impl_Iter!(TreeWriteGuard);
impl_Iter!(TreeAllocGuard);

macro_rules! impl_IterMut {
    ( $type:ident ) => {
        impl<'a, K: Ord, V> $type <'a, K, V> {
            #[inline]
            pub fn iter_mut(&mut self) -> IterMut<K, V, impl TreeWriter<K, V> + 'a> {
                let [front, back] = self.0.meta().range;
                IterMut { tree: &mut self.0, front, back, _phantom: PhantomData }
            }
        }
    };
}
impl_IterMut!(TreeWriteGuard);
impl_IterMut!(TreeAllocGuard);