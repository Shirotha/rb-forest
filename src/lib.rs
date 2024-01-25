#![feature(trait_alias, derive_const, const_trait_impl)]
#![feature(vec_push_within_capacity)]
#![feature(get_many_mut, maybe_uninit_uninit_array, maybe_uninit_array_assume_init)]
#![feature(try_blocks, try_trait_v2)]
#![feature(sync_unsafe_cell)]

#![allow(internal_features)]
#![feature(core_intrinsics, rustc_attrs)]

#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod arena;
mod tree;

use std::{
    convert::Infallible, fmt::Debug, ops::{ControlFlow, FromResidual, Try}
};

use crate::{
    arena::{Arena, Index},
    tree::{Tree, Node, TreeRef}
};

pub struct DeferUnwrap<T>(T);
impl<T, R> FromResidual<R> for DeferUnwrap<T> {
    #[inline(always)]
    fn from_residual(_residual: R) -> Self {
        panic!()
    }
}
impl<T> Try for DeferUnwrap<T> {
    type Output = T;
    type Residual = Option<Infallible>;
    #[inline(always)]
    fn from_output(output: Self::Output) -> Self {
        Self(output)
    }
    #[inline]
    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        ControlFlow::Continue(self.0)
    }
}
macro_rules! unwrap {
    { $t:ty : $expr:expr } => { {
        let result: crate::DeferUnwrap< $t > = try { $expr };
        result.0
    } }
}
pub(crate) use unwrap;

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

trait Reader<T> {
    type Item;
    fn get(&self, index: T) -> Option<&Self::Item>;
    fn contains(&self, index: T) -> bool;
}

trait Writer<T, E>: Reader<T> {
    fn get_mut(&mut self, index: T) -> Option<&mut Self::Item>;
    fn get_many_mut<const N: usize>(&mut self, indices: [T; N]) -> Result<[&mut Self::Item; N], E>;
}

#[derive(Debug)]
pub struct Forest<K, V> {
    node_arena: Arena<Node<K, V>>,
    tree_arena: Arena<Tree<K, V>>,
    front: TreeRef,
    back: TreeRef
}