mod port;
#[allow(unused_imports)]
pub use port::*;

use core::slice::GetManyMutError;
use std::{
    mem::{replace, MaybeUninit},
    intrinsics::transmute_unchecked,
    fmt::Debug,
};

use thiserror::Error;

#[derive_const(Debug, Error)]
pub enum Error {
    #[error("invalid index combination")]
    GetManyMut,
    #[error("one of the indices is invalid")]
    NotOccupied,
}
impl<const N: usize> const From<GetManyMutError<N>> for Error {
    #[inline]
    fn from(_value: GetManyMutError<N>) -> Self {
        Self::GetManyMut
    }
}

#[derive_const(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
#[cfg_attr(target_pointer_width = "64", rustc_layout_scalar_valid_range_end(0xffffffff_fffffffe))]
#[cfg_attr(target_pointer_width = "32", rustc_layout_scalar_valid_range_end(0xfffffffe))]
#[rustc_nonnull_optimization_guaranteed]
pub(crate) struct Index(usize);
impl Index {
    /// # Safety
    /// The given value cannot be equal to `usize::MAX`.
    #[inline(always)]
    const unsafe fn new_unchecked(value: usize) -> Self {
        Self(value)
    }
}

type Ref = Option<Index>;

#[derive(Debug)]
enum Entry<T> {
    Occupied(T),
    Free(Ref)
}
impl<T> Entry<T> {
    fn into_value(self) -> Option<T> {
        let Self::Occupied(value) = self else { return None };
        Some(value)
    }
}

// ASSERT: user is responsible for dangling references
#[derive(Debug)]
pub(crate) struct Arena<T> {
    items: Vec<Entry<T>>,
    free: Ref,
    len: usize
}
impl<T> Arena<T> {
    #[inline]
    pub const fn new() -> Self {
        Self { items: Vec::new(), free: None, len: 0 }
    }
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { items: Vec::with_capacity(capacity), free: None, len: 0 }
    }
    #[inline]
    pub fn into_port<M>(self, meta: M) -> Port<T, M> {
        Port::new(self, meta)
    }
    #[inline]
    fn insert_within_capacity(&mut self, value: T) -> Result<Index, T> {
        self.len += 1;
        match self.free {
            Some(head) => {
                let next = replace(&mut self.items[head.0], Entry::Occupied(value));
                match next {
                    Entry::Free(next) => self.free = next,
                    _ => panic!("this should never happen!")
                }
                Ok(head)
            },
            None => {
                // SAFETY: even for sizeof::<T>() == 1 memory will run out before reaching usize::MAX
                let index = unsafe { Index::new_unchecked(self.items.len()) };
                self.items.push_within_capacity(Entry::Occupied(value))
                    .map( |_| index)
                    .map_err( |e| e.into_value().unwrap() )
            }
        }
    }
    #[inline]
    fn reserve(&mut self) {
        if self.free.is_none() {
            self.items.reserve(1);
        }
    }
    #[inline]
    fn is_full(&self) -> bool {
        self.free.is_none() && self.items.len() == self.items.capacity()
    }
    #[inline]
    fn remove(&mut self, index: Index) -> Option<T> {
        if index.0 >= self.items.len() {
            return None;
        }
        let entry = &mut self.items[index.0];
        match entry {
            Entry::Occupied(_) => {
                let old = replace(entry, Entry::Free(self.free));
                self.free = Some(index);
                match old {
                    Entry::Occupied(value) => Some(value),
                    _ => panic!("this should never happen!")
                }
            },
            _ => None
        }
    }
    #[inline]
    fn get(&self, index: Index) -> Option<&T> {
        match self.items.get(index.0) {
            Some(Entry::Occupied(value)) => Some(value),
            _ => None
        }
    }
    #[inline]
    fn contains(&self, index: Index) -> bool {
        matches!(self.items.get(index.0), Some(Entry::Occupied(_)))
    }
    #[inline]
    fn get_mut(&mut self, index: Index) -> Option<&mut T> {
        match self.items.get_mut(index.0) {
            Some(Entry::Occupied(value)) => Some(value),
            _ => None
        }
    }
    #[inline]
    fn get_many_mut<const N: usize>(&mut self, indices: [Index; N]) -> Result<[&mut T; N], Error> {
        // SATEFY: Index is guarantied to have the same memory layout as usize
        let indices: [usize; N] = unsafe { transmute_unchecked(indices) };
        let entries = self.items.get_many_mut(indices)?;
        let mut result = MaybeUninit::uninit_array();
        for (result, entry) in result.iter_mut().zip(entries) {
            match entry {
                Entry::Occupied(value) => _ = result.write(value),
                _ => Err(Error::NotOccupied)?
            }
        }
        // SAFETY: initialized in previous loop
        Ok(unsafe { MaybeUninit::array_assume_init(result) })
    }
    #[inline]
    fn get_many_mut_option<const N: usize>(&mut self, indices: [Option<Index>; N]) -> Result<[Option<&mut T>; N], Error> {
        // TODO: implement this
        todo!()
    }
}
