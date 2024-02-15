#![feature(trait_alias, type_alias_impl_trait)]
#![feature(derive_const, const_trait_impl, impl_trait_in_assoc_type, const_mut_refs)]
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
    tree::{Tree, Node, Bounds, Value, NoCumulant}
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
    /// It is assumed that the given slice is sorted by K in incresing order.
    pub unsafe fn insert_sorted_slice_unchecked(&mut self, items: &[(K, V::Local)]) -> Tree<K, V> {
        Tree::from_sorted_slice_unchecked(self.free_port.split_with_meta(Bounds::default()), items)
    }
    /// # Safety
    /// It is assumed that the given iterator is sorted by K in incresing order.
    ///
    /// For a safe version of this function use the 'sorted-iter' feature.
    #[inline]
    pub unsafe fn insert_sorted_iter_unchecked(&mut self, iter: impl IntoIterator<Item = (K, V::Local)>) -> Tree<K, V> {
        Tree::from_sorted_iter_unchecked(self.free_port.split_with_meta(Bounds::default()), iter)
    }
    #[cfg(feature = "sorted-iter")]
    #[inline]
    pub fn insert_sorted_iter(&mut self, iter: impl IntoIterator<Item = (K, V::Local)> + SortedByKey) -> Tree<K, V> {
        Tree::from_sorted_iter(self.free_port.split_with_meta(Bounds::default()), iter)
    }
}
impl<K: Ord, V: Value> Default for WeakForest<K, V> {
    #[inline(always)]
    fn default() -> Self { Self::new() }
}

pub type SimpleWeakForest<K, V> = WeakForest<K, NoCumulant<V>>;

pub mod prelude {
    pub use crate::{
        WeakForest, SimpleWeakForest,
        tree::{
            NoCumulant, with_cumulant,
            SearchResult, SearchAction
        }
    };
}

#[cfg(test)]
mod test;