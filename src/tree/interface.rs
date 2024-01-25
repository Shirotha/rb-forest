use std::mem::MaybeUninit;

use crate::{
    arena::{Meta, PortReadGuard, PortWriteGuard, PortAllocGuard},
    Reader, Writer
};
use super::{
    Tree, Bounds, Node, NodeIndex,
    SearchResult, Error
};

impl<K: Ord, V> Tree<K, V> {
    #[inline]
    pub fn read(&self) -> TreeReadGuard<K, V> {
        TreeReadGuard(self.port.read())
    }
    #[inline]
    pub fn write(&mut self) -> TreeWriteGuard<K, V> {
        TreeWriteGuard(self.port.write())
    }
    #[inline]
    pub fn alloc(&mut self) -> TreeAllocGuard<K, V> {
        TreeAllocGuard(self.port.alloc())
    }
}

#[derive(Debug)]
pub struct TreeReadGuard<'a, K: Ord, V>(pub(crate) PortReadGuard<'a, Node<K, V>, Bounds>);

#[derive(Debug)]
pub struct TreeWriteGuard<'a, K: Ord, V>(pub(crate) PortWriteGuard<'a, Node<K, V>, Bounds>);

#[derive(Debug)]
pub struct TreeAllocGuard<'a, K: Ord, V>(pub(crate) PortAllocGuard<'a, Node<K, V>, Bounds>);
impl<'a, K: Ord, V> TreeAllocGuard<'a, K, V> {
#[inline]
    pub fn insert(&mut self, key: K, value: V) -> bool {
        match Tree::search(self.0.meta().root, &key, &self.0) {
            SearchResult::Here(ptr) => {
                self.0[ptr].value = value;
                return false;
            },
            SearchResult::Empty => {
                let ptr = Some(self.0.insert(Node::new(key, value)));
                let meta = self.0.meta_mut();
                meta.root = ptr;
                meta.range = [ptr, ptr]
            },
            SearchResult::LeftOf(parent) => {
                let ptr = self.0.insert(Node::new(key, value));
                Tree::insert_at::<0>(ptr, parent, &mut self.0);
            },
            SearchResult::RightOf(parent) => {
                let ptr = self.0.insert(Node::new(key, value));
                Tree::insert_at::<1>(ptr, parent, &mut self.0);
            }
        }
        true
    }
    #[inline]
    pub(crate) fn insert_node(&mut self, ptr: NodeIndex) -> Result<(), Error> {
        // ASSERT: node is not part of another tree
        let key = &self.0[ptr].key;
        match Tree::search(self.0.meta().root, key, &self.0) {
            SearchResult::Here(_) => return Err(Error::DuplicateKey),
            SearchResult::Empty => {
                // Case 1
                let meta = self.0.meta_mut();
                meta.root = Some(ptr);
                meta.range = [Some(ptr), Some(ptr)];
            },
            SearchResult::LeftOf(parent) =>
                // SAFETY: search was succesful, so tree cannot be empty
                Tree::insert_at::<0>(ptr, parent, &mut self.0),
            SearchResult::RightOf(parent) =>
                // SAFETY: search was succesful, so tree cannot be empty
                Tree::insert_at::<1>(ptr, parent, &mut self.0)
        }
        Ok(())
    }
    #[inline]
    pub fn remove(&mut self, key: K) -> Option<V> {
        match Tree::search(self.0.meta().root, &key, &self.0) {
            SearchResult::Here(ptr) => {
                Tree::remove_at(ptr, &mut self.0);
                // SAFETY: node was found, so it exists
                Some(self.0.remove(ptr).unwrap().value)
            }
            _ => None
        }
    }
    pub(crate) fn remove_node(&mut self, ptr: NodeIndex) {
        // ASSERT: node is part of this tree
        Tree::remove_at(ptr, &mut self.0);
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
        meta.len = 0;
    }
}

macro_rules! impl_Reader {
    ( $type:ident ) => {
        impl<'a, K: Ord, V> Reader<&K> for $type <'a, K, V> {
            type Item = V;
            #[inline]
            fn get(&self, key: &K) -> Option<&V> {
                match Tree::search(self.0.meta().root, key, &self.0) {
                    SearchResult::Here(ptr) => Some(&self.0[ptr].value),
                    _ => None
                }
            }
            #[inline]
            fn contains(&self, key: &K) -> bool {
                matches!(Tree::search(self.0.meta().root, key, &self.0), SearchResult::Here(_))
            }
        }
    };
}
impl_Reader!(TreeReadGuard);
impl_Reader!(TreeWriteGuard);
impl_Reader!(TreeAllocGuard);

macro_rules! impl_Writer {
    ( $type:ident ) => {
        impl<'a, K: Ord, V> Writer<&K, Error> for $type <'a, K, V> {
            #[inline]
            fn get_mut(&mut self, key: &K) -> Option<&mut V> {
                match Tree::search(self.0.meta().root, key, &self.0) {
                    SearchResult::Here(ptr) => Some(&mut self.0[ptr].value),
                    _ => None
                }
            }
            #[inline]
            fn get_many_mut<const N: usize>(&mut self, keys: [&K; N]) -> Result<[&mut V; N], Error> {
                let mut ptrs = MaybeUninit::uninit_array::<N>();
                for (ptr, key) in ptrs.iter_mut().zip(keys) {
                    match Tree::search(self.0.meta().root, key, &self.0) {
                        SearchResult::Here(found) => _ = ptr.write(found),
                        _ => Err(Error::GetManyMut)?
                    }
                }
                let ptrs = unsafe { MaybeUninit::array_assume_init(ptrs) };
                let nodes = self.0.get_many_mut(ptrs)?;
                let mut result = MaybeUninit::uninit_array::<N>();
                for (result, node) in result.iter_mut().zip(nodes) {
                    result.write(&mut node.value);
                }
                Ok(unsafe { MaybeUninit::array_assume_init(result) })
            }
        }
    };
}
impl_Writer!(TreeWriteGuard);
impl_Writer!(TreeAllocGuard);