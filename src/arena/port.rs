use std::{
    ops::{Index as IndexRO, IndexMut},
    cell::SyncUnsafeCell,
    sync::Arc
};

use parking_lot::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

use crate::{Index, Reader, Writer};
use super::{Arena, Error};

pub trait Meta {
    type Type;
    fn meta(&self) -> &Self::Type;
}
pub trait MetaMut: Meta {
    fn meta_mut(&mut self) -> &mut Self::Type;
}

#[derive(Debug)]
pub struct Port<T, M = ()>(Arc<RwLock<SyncUnsafeCell<Arena<T>>>>, RwLock<M>);
impl<T, M> Port<T, M> {
    #[inline]
    pub fn read(&self) -> PortReadGuard<T, M> {
        let arena = self.0.read();
        let port = self.1.read();
        PortReadGuard { arena, port }
    }
    #[inline]
    pub fn write(&self) -> PortWriteGuard<T, M> {
        // SAFETY: only access to mutable reference is to port-owned items while owning write lock to port
        let arena = self.0.read();
        let port = self.1.write();
        PortWriteGuard { arena, port }
    }
    #[inline]
    pub fn alloc(&self) -> PortAllocGuard<T, M> {
        // SAFETY: only access to mutable reference is to port-owned items while owning write lock to port
        let arena = self.0.upgradable_read();
        let port = self.1.write();
        PortAllocGuard { arena, port }
    }
}

#[derive(Debug)]
pub struct PortReadGuard<'a, T, M> {
    arena: RwLockReadGuard<'a, SyncUnsafeCell<Arena<T>>>,
    port: RwLockReadGuard<'a, M>
}
impl<'a, T, M> PortReadGuard<'a, T, M> {
    #[inline]
    fn arena(&self) -> &Arena<T> {
        // SAFETY: arena is not null
        unsafe { self.arena.get().as_ref().unwrap() }
    }
}

#[derive(Debug)]
pub struct PortWriteGuard<'a, T, M> {
    arena: RwLockReadGuard<'a, SyncUnsafeCell<Arena<T>>>,
    port: RwLockWriteGuard<'a, M>
}
impl<'a, T, M> PortWriteGuard<'a, T, M> {
    #[inline]
    fn arena(&self) -> &Arena<T> {
        // SAFETY: arena is not null
        unsafe { self.arena.get().as_ref().unwrap() }
    }
    #[inline]
    fn arena_mut(&mut self) -> &mut Arena<T> {
        // SAFETY: arena is not null
        unsafe { self.arena.get().as_mut().unwrap() }
    }
    #[inline(always)]
    pub fn meta_mut(&mut self) -> &mut M {
        &mut self.port
    }
}
impl<'a, T, M> Writer<Index, Error> for PortWriteGuard<'a, T, M> {
    #[inline]
    fn get_mut(&mut self, index: Index) -> Option<&mut T> {
        self.arena_mut().get_mut(index)
    }
    #[inline]
    fn get_many_mut<const N: usize>(&mut self, indices: [Index; N]) -> Result<[&mut T; N], Error> {
        self.arena_mut().get_many_mut(indices)
    }
}

#[derive(Debug)]
pub struct PortAllocGuard<'a, T, M> {
    arena: RwLockUpgradableReadGuard<'a, SyncUnsafeCell<Arena<T>>>,
    port: RwLockWriteGuard<'a, M>
}
impl<'a, T, M> PortAllocGuard<'a, T, M> {
    #[inline]
    pub fn downgrade(self) -> PortWriteGuard<'a, T, M> {
        let arena = RwLockUpgradableReadGuard::downgrade(self.arena);
        PortWriteGuard { arena, port: self.port }
    }
    #[inline]
    fn arena(&self) -> &Arena<T> {
        // SAFETY: arena is not null
        unsafe { self.arena.get().as_ref().unwrap() }
    }
    #[inline]
    fn arena_mut(&mut self) -> &mut Arena<T> {
        // SAFETY: arena is not null
        unsafe { self.arena.get().as_mut().unwrap() }
    }
    #[inline(always)]
    pub fn meta_mut(&mut self) -> &mut M {
        &mut self.port
    }
    #[inline]
    pub fn insert(&mut self, value: T) -> Index {
        if self.arena().is_full() {
            self.arena.with_upgraded( |arena|
                arena.get_mut().reserve()
            );
        }
        // SAFETY: space was reserved in advance
        unsafe { self.arena_mut().insert_within_capacity(value).unwrap_unchecked() }
    }
    #[inline]
    pub fn remove(&mut self, index: Index) -> Option<T> {
        // SAFETY: there can only be one upgradable lock, so this has exclusive access to the free list
        self.arena_mut().remove(index)
    }
}
impl<'a, T, M> Writer<Index, Error> for PortAllocGuard<'a, T, M> {
    #[inline]
    fn get_mut(&mut self, index: Index) -> Option<&mut T> {
        self.arena_mut().get_mut(index)
    }
    #[inline]
    fn get_many_mut<const N: usize>(&mut self, indices: [Index; N]) -> Result<[&mut T; N], Error> {
        self.arena_mut().get_many_mut(indices)
    }
}

macro_rules! impl_Meta {
    ( $type:ident ) => {
        impl<'a, T, M> Meta for $type <'a, T, M> {
            type Type = M;
            #[inline(always)]
            fn meta(&self) -> &M {
                &self.port
            }
        }
    };
}
impl_Meta!(PortReadGuard);
impl_Meta!(PortWriteGuard);
impl_Meta!(PortAllocGuard);

macro_rules! impl_Meta {
    ( $type:ident ) => {
        impl<'a, T, M> MetaMut for $type <'a, T, M> {
            #[inline(always)]
            fn meta_mut(&mut self) -> &mut M {
                &mut self.port
            }
        }
    };
}
impl_Meta!(PortWriteGuard);
impl_Meta!(PortAllocGuard);

macro_rules! impl_Reader {
    ( $type:ident ) => {
        impl<'a, T, M> Reader<Index> for $type <'a, T, M> {
            type Item = T;
            #[inline]
            fn get(&self, index: Index) -> Option<&T> {
                self.arena().get(index)
            }
            #[inline]
            fn contains(&self, index: Index) -> bool {
                self.arena().contains(index)
            }
        }
    };
}
impl_Reader!(PortReadGuard);
impl_Reader!(PortWriteGuard);
impl_Reader!(PortAllocGuard);

macro_rules! impl_Index {
    ( $type:ident ) => {
        impl<'a, T, M> IndexRO<Index> for $type <'a, T, M> {
            type Output = T;
            #[inline]
            fn index(&self, index: Index) -> &Self::Output {
                self.get(index).unwrap()
            }
        }
    };
}
impl_Index!(PortReadGuard);
impl_Index!(PortWriteGuard);
impl_Index!(PortAllocGuard);

macro_rules! impl_IndexMut {
    ( $type:ident ) => {
        impl<'a, T, M> IndexMut<Index> for $type <'a, T, M> {
            #[inline]
            fn index_mut(&mut self, index: Index) -> &mut Self::Output {
                self.get_mut(index).unwrap()
            }
        }
    };
}
impl_IndexMut!(PortWriteGuard);
impl_IndexMut!(PortAllocGuard);