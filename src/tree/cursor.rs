use std::marker::PhantomData;

use crate::{
    arena::{Meta, PortAllocGuard},
    tree::{
        Tree, Bounds, Node, NodeRef, Value, SearchResult,
        Iter, IterMut,
        TreeReader, TreeWriter,
        TreeReadGuard, TreeWriteGuard, TreeAllocGuard
    }
};

#[derive(Debug, Clone)]
pub struct Cursor<'a, K: Ord, V, R: TreeReader<K, V>> {
    tree: &'a R,
    current: NodeRef,
    _phantom: PhantomData<(K, V)>
}

#[derive(Debug)]
pub struct CursorMut<'a, K: Ord, V, W: TreeWriter<K, V>> {
    tree: &'a mut W,
    current: NodeRef,
    _phantom: PhantomData<(K, V)>
}

#[derive(Debug)]
pub struct CursorAlloc<'a, 'b, K: Ord, V: Value> {
    tree: &'a mut PortAllocGuard<'b, Node<K, V>, Bounds>,
    current: NodeRef
}

macro_rules! impl_Cursor {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            #[inline]
            pub fn cursor(&self) -> Cursor<K, V, impl TreeReader<K, V> + 'a> {
                Cursor { tree: &self.0, current: self.0.meta().root, _phantom: PhantomData }
            }
            #[inline]
            pub fn cursor_at(&self, key: &K) -> Cursor<K, V, impl TreeReader<K, V> + 'a> {
                let current = match unsafe { Tree::search(self.0.meta().root, key, &self.0) } {
                    SearchResult::Here(ptr) => Some(ptr),
                    SearchResult::LeftOf(ptr) => self.0[ptr].order[0],
                    SearchResult::RightOf(ptr) => Some(ptr),
                    _ => None
                };
                Cursor { tree: &self.0, current, _phantom: PhantomData }
            }
        }
    };
}
impl_Cursor!(TreeReadGuard);
impl_Cursor!(TreeWriteGuard);
impl_Cursor!(TreeAllocGuard);

macro_rules! impl_CursorMut {
    ( $type:ident ) => {
        impl<'a, K: Ord, V: Value> $type <'a, K, V> {
            #[inline]
            pub fn cursor_mut(&mut self) -> CursorMut<K, V, impl TreeWriter<K, V> + 'a> {
                let current = self.0.meta().root;
                CursorMut { tree: &mut self.0, current, _phantom: PhantomData }
            }
            #[inline]
            pub fn cursor_mut_at(&mut self, key: &K) -> CursorMut<K, V, impl TreeWriter<K, V> + 'a> {
                let current = match unsafe { Tree::search(self.0.meta().root, key, &self.0) } {
                    SearchResult::Here(ptr) => Some(ptr),
                    SearchResult::LeftOf(ptr) => self.0[ptr].order[0],
                    SearchResult::RightOf(ptr) => Some(ptr),
                    _ => None
                };
                CursorMut { tree: &mut self.0, current, _phantom: PhantomData }
            }
        }
    };
}
impl_CursorMut!(TreeWriteGuard);
impl_CursorMut!(TreeAllocGuard);

impl<'a, K: Ord, V: Value> TreeAllocGuard<'a, K, V> {
    #[inline]
    pub fn cursor_alloc(&mut self) -> CursorAlloc<'_, 'a, K, V> {
        let current = self.0.meta().root;
        CursorAlloc { tree: &mut self.0, current }
    }
    #[inline]
    pub fn cursor_alloc_at(&mut self, key: &K) -> CursorAlloc<'_, 'a, K, V> {
        let current = match unsafe { Tree::search(self.0.meta().root, key, &self.0) } {
            SearchResult::Here(ptr) => Some(ptr),
            SearchResult::LeftOf(ptr) => self.0[ptr].order[0],
            SearchResult::RightOf(ptr) => Some(ptr),
            _ => None
        };
        CursorAlloc { tree: &mut self.0, current }
    }
}

pub trait CursorMove {
    fn move_order<const I: usize>(&mut self) where [(); 1 - I]:;
    fn move_parent(&mut self) -> Option<bool>;
    fn move_child<const I: usize>(&mut self) -> Option<bool> where [(); 1 - I]:;
    #[inline(always)]
    fn move_prev(&mut self) {
        self.move_order::<0>()
    }
    #[inline(always)]
    fn move_next(&mut self) {
        self.move_order::<1>()
    }
    #[inline(always)]
    fn move_left(&mut self) -> Option<bool> {
        self.move_child::<0>()
    }
    #[inline(always)]
    fn move_right(&mut self) -> Option<bool> {
        self.move_child::<1>()
    }
}

macro_rules! impl_CursorMove {
    ( $type:ident ; $( $pre:lifetime ),* ; $( $post:ident : $postcond:path ),*) => {
        impl<'a, $( $pre , )* K: Ord, V: Value, $( $post : $postcond ),* > CursorMove for $type <'a, $( $pre , )* K, V, $( $post ),* > {
            #[inline]
            fn move_order<const I: usize>(&mut self)
                where [(); 1 - I]:
            {
                self.current = self.current.map_or_else(
                    || self.tree.meta().range[1 - I],
                    |index| self.tree[index].order[I]
                )
            }
            #[inline]
            fn move_parent(&mut self) -> Option<bool> {
                let Some(parent) = self.tree[self.current?].parent else { return Some(false) };
                self.current = Some(parent);
                Some(true)
            }
            #[inline]
            fn move_child<const I: usize>(&mut self) -> Option<bool>
                where [(); 1 - I]:
            {
                let Some(child) = self.tree[self.current?].children[I] else { return Some(false) };
                self.current = Some(child);
                Some(true)
            }
        }
    };
}
impl_CursorMove!(Cursor;; R: TreeReader<K, V>);
impl_CursorMove!(CursorMut;; W: TreeWriter<K, V>);
impl_CursorMove!(CursorAlloc; 'b;);

pub trait CursorRead<K, V> {
    fn key(&self) -> Option<&K>;
    fn value(&self) -> Option<&V>;
    fn key_value(&self) -> Option<(&K, &V)>;
}

macro_rules! impl_CursorRead {
    ( $type:ident ; $( $pre:lifetime ),* ; $( $post:ident : $postcond:path ),*) => {
        impl<'a, $( $pre , )* K: Ord, V: Value, $( $post : $postcond ),* > CursorRead<K, V> for $type <'a, $( $pre , )* K, V, $( $post ),* > {
            #[inline]
            fn key(&self) -> Option<&K> {
                Some(&self.tree[self.current?].key)
            }
            #[inline]
            fn value(&self) -> Option<&V> {
                Some(&self.tree[self.current?].value)
            }
            #[inline]
            fn key_value(&self) -> Option<(&K, &V)> {
                let node = &self.tree[self.current?];
                Some((&node.key, &node.value))
            }
        }
    };
}
impl_CursorRead!(Cursor;; R: TreeReader<K, V>);
impl_CursorRead!(CursorMut;; W: TreeWriter<K, V>);
impl_CursorRead!(CursorAlloc; 'b;);

pub trait CursorPeek<K, V>: CursorMove + CursorRead<K, V> {
    fn peek_order<const I: usize>(&self) -> Option<(&K, &V)> where [(); 1 - I]:;
    fn peek_parent(&self) -> Option<(&K, &V)>;
    fn peek_child<const I: usize>(&self) -> Option<(&K, &V)> where [(); 1 - I]:;
    #[inline(always)]
    fn peek_prev(&self) -> Option<(&K, &V)> {
        self.peek_order::<0>()
    }
    #[inline(always)]
    fn peek_next(&self) -> Option<(&K, &V)> {
        self.peek_order::<1>()
    }
    #[inline(always)]
    fn peek_left(&self) -> Option<(&K, &V)> {
        self.peek_child::<0>()
    }
    #[inline(always)]
    fn peek_right(&self) -> Option<(&K, &V)> {
        self.peek_child::<1>()
    }
}
macro_rules! impl_CursorPeek {
    ( $type:ident ; $( $pre:lifetime ),* ; $( $post:ident : $postcond:path ),*) => {
        impl<'a, $( $pre , )* K: Ord, V: Value, $( $post : $postcond ),* > CursorPeek<K, V> for $type <'a, $( $pre , )* K, V, $( $post ),* > {
            #[inline]
            fn peek_order<const I: usize>(&self) -> Option<(&K, &V)>
                where [(); 1 - I]:
            {
                let neighbour = if let Some(current) = self.current {
                    self.tree[current].order[I]?
                } else {
                    self.tree.meta().range[1 - I]?
                };
                let node = &self.tree[neighbour];
                Some((&node.key, &node.value))
            }
            #[inline]
            fn peek_parent(&self) -> Option<(&K, &V)> {
                let parent = self.tree[self.current?].parent?;
                let node = &self.tree[parent];
                Some((&node.key, &node.value))
            }
            #[inline]
            fn peek_child<const I: usize>(&self) -> Option<(&K, &V)>
                where [(); 1 - I]:
            {
                let child = self.tree[self.current?].children[I]?;
                let node = &self.tree[child];
                Some((&node.key, &node.value))
            }
        }
    };
}
impl_CursorPeek!(Cursor;; R: TreeReader<K, V>);
impl_CursorPeek!(CursorMut;; W: TreeWriter<K, V>);
impl_CursorPeek!(CursorAlloc; 'b;);

pub trait CursorWrite<K, V>: CursorRead<K, V> {
    fn value_mut(&mut self) -> Option<&mut V>;
}
macro_rules! impl_CursorWrite {
    ( $type:ident ; $( $pre:lifetime ),* ; $( $post:ident : $postcond:path ),*) => {
        impl<'a, $( $pre , )* K: Ord, V: Value, $( $post : $postcond ),* > CursorWrite<K, V> for $type <'a, $( $pre , )* K, V, $( $post ),* > {
            #[inline]
            fn value_mut(&mut self) -> Option<&mut V> {
                Some(&mut self.tree[self.current?].value)
            }
        }
    };
}
impl_CursorWrite!(CursorMut;; W: TreeWriter<K, V>);
impl_CursorWrite!(CursorAlloc; 'b;);

impl<'a, 'b, K: Ord, V: Value> CursorAlloc<'a, 'b, K, V> {
    #[inline]
    pub fn remove_order<const I: usize>(&mut self) -> Option<(K, V)>
        where [(); 1 - I]:
    {
        let neighbour = if let Some(current) = self.current {
            self.tree[current].order[I]?
        } else {
            self.tree.meta().range[1 - I]?
        };
        let node = self.tree.remove(neighbour)?;
        Some((node.key, node.value))
    }
    pub fn remove_parent(&mut self) -> Option<(K, V)> {
        let parent = self.tree[self.current?].parent?;
        let node = self.tree.remove(parent)?;
        Some((node.key, node.value))
    }
    pub fn remove_child<const I: usize>(&mut self) -> Option<(K, V)>
        where [(); 1 - I]:
    {
        let child = self.tree[self.current?].children[I]?;
        let node = self.tree.remove(child)?;
        Some((node.key, node.value))
    }
    #[inline(always)]
    pub fn remove_prev(&mut self) -> Option<(K, V)> {
        self.remove_order::<0>()
    }
    #[inline(always)]
    pub fn remove_next(&mut self) -> Option<(K, V)> {
        self.remove_order::<1>()
    }
    #[inline(always)]
    pub fn remove_left(&mut self) -> Option<(K, V)> {
        self.remove_child::<0>()
    }
    #[inline(always)]
    pub fn remove_right(&mut self) -> Option<(K, V)> {
        self.remove_child::<1>()
    }
}

macro_rules! impl_Iter {
    ( $type:ident ; $( $pre:lifetime ),* ; $( $post:ident : $postcond:path ),*) => {
        impl<'a, $( $pre , )* K: Ord, V: Value, $( $post : $postcond ),* > $type <'a, $( $pre , )* K, V, $( $post ),* > {
            #[inline]
            pub fn iter_below(&self) -> Iter<K, V, impl TreeReader<K, V> + 'a> {
                let [front, back] = if let Some(current) = self.current {
                    [
                        Some(Tree::limit::<0>(current, self.tree)),
                        Some(Tree::limit::<1>(current, self.tree))
                    ]
                } else { [None, None] };
                Iter { tree: self.tree, front, back, _phantom: PhantomData }
            }
        }
    };
}
impl_Iter!(Cursor;; R: TreeReader<K, V>);
impl_Iter!(CursorMut;; W: TreeWriter<K, V>);
impl_Iter!(CursorAlloc; 'b;);

macro_rules! impl_IterMut {
    ( $type:ident ; $( $pre:lifetime ),* ; $( $post:ident : $postcond:path ),*) => {
        impl<'a, $( $pre , )* K: Ord, V: Value, $( $post : $postcond ),* > $type <'a, $( $pre , )* K, V, $( $post ),* > {
            #[inline]
            pub fn iter_below_mut(&mut self) -> IterMut<K, V, impl TreeWriter<K, V> + 'a $( + $pre )*> {
                let [front, back] = if let Some(current) = self.current {
                    [
                        Some(Tree::limit::<0>(current, self.tree)),
                        Some(Tree::limit::<1>(current, self.tree))
                    ]
                } else { [None, None] };
                IterMut { tree: self.tree, front, back, _phantom: PhantomData }
            }
        }
    };
}
impl_IterMut!(CursorMut;; W: TreeWriter<K, V>);
impl_IterMut!(CursorAlloc; 'b;);