#![feature(trait_alias, derive_const, const_trait_impl, impl_trait_in_assoc_type)]
#![feature(vec_push_within_capacity, slice_split_at_unchecked)]
#![feature(get_many_mut, maybe_uninit_uninit_array, maybe_uninit_array_assume_init)]
#![feature(try_blocks, try_trait_v2)]
#![feature(sync_unsafe_cell)]

#![allow(internal_features)]
#![feature(core_intrinsics, rustc_attrs)]

#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod arena;
pub mod tree;

use std::ops::{ControlFlow, FromResidual, Try};

use sorted_iter::sorted_pair_iterator::SortedByKey;

use crate::{
    arena::{Arena, Port},
    tree::{Tree, Node, Bounds, Value}
};

pub struct DeferDiscard(bool);
impl<R> FromResidual<R> for DeferDiscard {
    #[inline(always)]
    fn from_residual(_residual: R) -> Self {
        Self(false)
    }
}
impl Try for DeferDiscard {
    type Output = ();
    type Residual = ();
    #[inline(always)]
    fn from_output(_output: Self::Output) -> Self {
        Self(true)
    }
    #[inline]
    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        if self.0 {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }
}
macro_rules! discard {
    { $expr:expr } => { {
        let result: crate::DeferDiscard = try { $expr };
        result.0
    } }
}
pub(crate) use discard;

#[allow(unused_macros)]
macro_rules! option {
    { $t:ty : $expr:expr } => { {
        let result: Option< $t > = try { $expr };
        result
    } }
}
#[allow(unused_imports)]
pub(crate) use option;

pub trait Reader<T> {
    type Item;
    fn get(&self, index: T) -> Option<&Self::Item>;
    fn contains(&self, index: T) -> bool;
}

pub trait Writer<T, E>: Reader<T> {
    fn get_mut(&mut self, index: T) -> Option<&mut Self::Item>;
    fn get_pair_mut(&mut self, a: T, b: T) -> Result<[Option<&mut Self::Item>; 2], E>;
    #[allow(clippy::type_complexity)]
    fn get_mut_with<const N: usize>(&mut self, idnex: T, others: [Option<T>; N]) -> Result<(Option<&mut Self::Item>, [Option<&Self::Item>; N]), E>;
}

#[derive(Debug)]
pub struct WeakForest<K: Ord, V: Value> {
    free_port: Port<Node<K, V>, Bounds>,
}
impl<K: Ord, V: Value> WeakForest<K, V> {
    #[inline]
    pub fn new() -> Self {
        Self { free_port: Arena::new().into_port(Bounds::default()) }
    }
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { free_port: Arena::with_capacity(capacity).into_port(Bounds::default()) }
    }
    #[inline]
    pub fn insert(&mut self) -> Tree<K, V> {
        Tree::new(self.free_port.split_with_meta(Bounds::default()))
    }
    /// # Safety
    /// It is assumed that the given iterator is sorted by K in incresing order.
    ///
    /// For a safe version of this function use the 'sorted-iter' feature.
    #[inline]
    pub unsafe fn insert_sorted_iter_unchecked(&mut self, iter: impl IntoIterator<Item = (K, V)>) -> Tree<K, V> {
        Tree::from_sorted_iter_unchecked(self.free_port.split_with_meta(Bounds::default()), iter)
    }
    #[cfg(feature = "sorted-iter")]
    #[inline]
    pub fn insert_sorted_iter(&mut self, iter: impl IntoIterator<Item = (K, V)> + SortedByKey) -> Tree<K, V> {
        Tree::from_sorted_iter(self.free_port.split_with_meta(Bounds::default()), iter)
    }
}
impl<K: Ord, V: Value> Default for WeakForest<K, V> {
    #[inline(always)]
    fn default() -> Self { Self::new() }
}

pub mod prelude {
    pub use crate::{
        WeakForest,
        tree::{
            NoCumulant, WithCumulant,
            SearchResult
        }
    };
}

#[cfg(test)]
mod test {
    use sorted_iter::assume::AssumeSortedByKeyExt;

    use crate::prelude::*;

    #[test]
    fn insert_remove() {
        let mut forest = WeakForest::new();
        let mut tree = forest.insert();
        {
            let mut alloc = tree.alloc();
            alloc.insert(1, NoCumulant(42));
            let value = alloc.remove(1);
            assert_eq!(value, Some(NoCumulant(42)));
        }
    }
    #[test]
    fn iter() {
        let mut values = vec![1, 7, 8, 9, 10, 6, 5, 2, 3, 4, 0, 11];
        let mut forest = WeakForest::with_capacity(values.len());
        let mut tree = forest.insert();
        {
            let mut alloc = tree.alloc();
            for x in values.iter().copied() {
                // FIXME: insertion failing (indices moved wrong?)
                alloc.insert(x, NoCumulant(x));
            }
        }
        values.sort_unstable();
        {
            let read = tree.read();
            let result = read.iter().map( |(_, v)| v.0 ).collect::<Vec<_>>();
            assert_eq!(&values, &result);
        }
    }
    #[test]
    fn join() {
        let mut forest = WeakForest::with_capacity(20);
        let even = unsafe { forest.insert_sorted_iter_unchecked(
            (0..10).map( |n| (2*n, NoCumulant(n)) )
        ) };
        let odd = unsafe { forest.insert_sorted_iter_unchecked(
            (0..10).map( |n| (2*n+1, NoCumulant(n)) )
        ) };
        let all = odd.union(even);
        let (lower, pivot, upper) = all.split(&5);
        assert_eq!(pivot, Some(NoCumulant(5)));
    }
    #[cfg(feature = "sorted-iter")]
    #[test]
    fn search() {
        let mut forest = WeakForest::with_capacity(10);
        let tree = forest.insert_sorted_iter(
            (0..10)
                .map( |i| (i, NoCumulant(10 + i)) )
                .assume_sorted_by_key()
        );
        {
            let read = tree.read();
            let key = read.search_by( |_, v| v.0.cmp(&14) );
            assert_eq!(key, SearchResult::Here(&4));
        }
    }
}