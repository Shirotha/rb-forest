mod port;
#[allow(unused_imports)]
pub use port::*;

use std::mem::replace;

use thiserror::Error;

#[derive_const(Debug, Error)]
pub enum Error {
    #[error("indices have to be pairwise different")]
    IndexAlias,
    #[error("one of the indices is invalid")]
    NotOccupied,
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
    #[inline(always)]
    fn is_occupied(&self) -> bool {
        matches!(self, Self::Occupied(_))
    }
    #[inline]
    fn value(&self) -> Option<&T> {
        let Self::Occupied(value) = self else { return None };
        Some(value)
    }
    #[inline]
    fn value_mut(&mut self) -> Option<&mut T> {
        let Self::Occupied(value) = self else { return None };
        Some(value)
    }
    #[inline]
    fn into_value(self) -> Option<T> {
        let Self::Occupied(value) = self else { return None };
        Some(value)
    }
    #[inline]
    fn into_head(self) -> Option<Ref> {
        let Self::Free(head) = self else { return None };
        Some(head)
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
                // SAFETY: the free list can only hold free nodes
                self.free = next.into_head().unwrap();
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
        if entry.is_occupied() {
                let old = replace(entry, Entry::Free(self.free));
                self.free = Some(index);
                old.into_value()
        } else { None }
    }
    #[inline]
    fn get(&self, index: Index) -> Option<&T> {
        self.items.get(index.0).and_then(Entry::value)
    }
    #[inline]
    fn contains(&self, index: Index) -> bool {
        self.items.get(index.0).is_some_and(Entry::is_occupied)
    }
    #[inline]
    fn get_mut(&mut self, index: Index) -> Option<&mut T> {
        self.items.get_mut(index.0).and_then(Entry::value_mut)
    }
    #[inline]
    fn get_pair_mut(&mut self, a: Index, b: Index) -> Result<[Option<&mut T>; 2], Error> {
        if a == b {
            return Err(Error::IndexAlias);
        }
        let len = self.items.len();
        if a.0 >= len {
            return Ok([None, self.get_mut(b)]);
        }
        if b.0 >= len {
            return Ok([self.get_mut(a), None]);
        }
        // SAFETY: indices are checked explicitly before
        Ok(unsafe { self.get_pair_mut_unchecked(a, b) })
    }
    /// # Safety
    /// No bounds- or alias checking is done.
    #[inline]
    unsafe fn get_pair_mut_unchecked(&mut self, a: Index, b: Index) -> [Option<&mut T>; 2] {
        let ptr = self.items.as_mut_ptr();
        let a = ptr.add(a.0).as_mut().unwrap();
        let b = ptr.add(b.0).as_mut().unwrap();
        [a.value_mut(), b.value_mut()]
    }
    #[inline]
    fn get_mut_with<const N: usize>(&mut self, index: Index, others: [Option<Index>; N]) -> Result<(Option<&mut T>, [Option<&T>; N]), Error> {
        if others.iter().any( |i| i.is_some_and( |i| i == index ) ) {
            return Err(Error::IndexAlias)
        }
        // SAFETY: indices are checked explicitly before
        Ok(unsafe { self.get_mut_with_unchecked(index, others) })
    }
    /// # Safety
    /// No bounds- or alias checking is done.
    #[inline]
    unsafe fn get_mut_with_unchecked<const N: usize>(&mut self, index: Index, others: [Option<Index>; N]) -> (Option<&mut T>, [Option<&T>; N]) {
        let x = self.items.as_mut_ptr().add(index.0).as_mut().unwrap();
        let others = others.map( |i| i.and_then( |i| self.get(i) ) );
        (x.value_mut(), others)
    }
}
