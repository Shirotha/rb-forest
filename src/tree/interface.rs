use std::{
    ops::RangeInclusive,
    mem::MaybeUninit
};

use crate::{
    Reader, Writer,
    arena::{
        Meta, MetaMut,
        PortReadGuard, PortWriteGuard, PortAllocGuard
    },
    tree::{Bounds, Color, Error, Node, NodeIndex, SearchResult, Tree}
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
    pub fn downgrade(self) -> TreeWriteGuard<'a, K, V> {
        TreeWriteGuard(self.0.downgrade())
    }
    #[inline]
    pub fn insert(&mut self, key: K, value: V) -> bool {
        match Tree::search(self.0.meta().root, &key, &self.0) {
            SearchResult::Here(ptr) => {
                self.0[ptr].value = value;
                return false;
            },
            SearchResult::Empty => {
                let ptr = Some(self.0.insert(Node::new(key, value, Color::Black)));
                let meta = self.0.meta_mut();
                meta.root = ptr;
                meta.range = [ptr, ptr]
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
    /// # Safety
    /// Calling this function implicitly moves the node pointer into this tree,
    /// using the same pointer in a different tree is undefined behaviour.
    #[inline]
    pub(crate) unsafe fn insert_node(&mut self, ptr: NodeIndex) -> Result<(), Error> {
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
                // SAFETY: parent is a leaf
                Tree::insert_at::<0>(ptr, parent, &mut self.0),
            SearchResult::RightOf(parent) =>
                // SAFETY: parent is a leaf
                Tree::insert_at::<1>(ptr, parent, &mut self.0)
        }
        Ok(())
    }
    #[inline]
    pub fn remove(&mut self, key: K) -> Option<V> {
        match Tree::search(self.0.meta().root, &key, &self.0) {
            SearchResult::Here(ptr) => {
                // SAFETY: node is the result of a search in tree
                unsafe { Tree::remove_at(ptr, &mut self.0); }
                // SAFETY: node was found, so it exists
                Some(self.0.remove(ptr).unwrap().value)
            }
            _ => None
        }
    }
    /// # Safety
    /// The node pointer has to point to a node in this tree.
    /// This will also implicitly move the node pointer out of this tree,
    /// using the pointer for anything else than inserting it into a different tree is undefined behaviour.
    #[inline(always)]
    pub(crate) unsafe fn remove_node(&mut self, ptr: NodeIndex) {
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

macro_rules! impl_Bounds {
    ( $type:ident ) => {
        impl<'a, K: Ord, V> $type <'a, K, V> {
            #[inline(always)]
            pub fn len(&self) -> usize {
                self.0.meta().len
            }
            #[inline(always)]
            pub fn is_empty(&self) -> bool {
                self.0.meta().len == 0
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
        }
    };
}
impl_Bounds!(TreeReadGuard);
impl_Bounds!(TreeWriteGuard);
impl_Bounds!(TreeAllocGuard);