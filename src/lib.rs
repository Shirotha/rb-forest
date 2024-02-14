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
            SearchResult
        }
    };
}

#[cfg(test)]
mod test {
    use sorted_iter::assume::AssumeSortedByKeyExt;

    use crate::{
        prelude::*,
        tree::{NodeIndex, NodeRef, Value, TreeReader}
    };

    fn validate_rb_node<'a, K, V>(index: NodeIndex,
        tree: &'a impl TreeReader<K, V>
    ) -> ([&'a K; 2], u8)
        where K: Ord + std::fmt::Debug, V: Value + 'a
    {
        let node = &tree[index];
        assert!(node.parent.is_some() || node.is_black(), "root has too be black");
        match node.order {
            [None, None] => {
                assert_eq!(node.parent, None, "order implies root");
            },
            [Some(prev), None] => {
                let prev_node = &tree[prev];
                assert_eq!(node.children[1], None, "order implies max node");
                assert!(prev_node.key < node.key, "out of bounds");
            },
            [None, Some(next)] => {
                let next_node = &tree[next];
                assert_eq!(node.children[0], None, "order implies min node");
                assert!(node.key < next_node.key, "out of bounds");
            },
            [Some(prev), Some(next)] => {
                let prev_node = &tree[prev];
                let next_node = &tree[next];
                assert!(prev_node.key < node.key, "out of bounds");
                assert!(node.key < next_node.key, "out of bounds");
            }
        }
        match node.children {
            [None, None] => ([&node.key, &node.key], node.color as u8),
            [Some(left), None] => {
                let left_node = &tree[left];
                assert!(node.is_black() || left_node.is_black(), "cannot have two red nodes in a row");
                assert!(left_node.is_red(), "single child has to be red");
                let ([min, prev], left_height) = validate_rb_node(left, tree);
                assert!(min <= prev, "bad order");
                assert!(*prev < node.key, "left tree overlap");
                assert_eq!(*prev, tree[node.order[0].expect("not null")].key, "biggest node of left sub-tree has to be prev");
                ([min, &node.key], left_height + (node.color as u8))
            },
            [None, Some(right)] => {
                let right_node = &tree[right];
                assert!(node.is_black() || right_node.is_black(), "cannot have two red nodes in a row");
                assert!(right_node.is_red(), "single child has to be red");
                let ([next, max], right_height) = validate_rb_node(right, tree);
                assert!(next <= max, "bad order");
                assert!(node.key < *next, "right tree overlap");
                assert_eq!(*next, tree[node.order[1].expect("not null")].key, "smallest node of right sub-tree has to be next");
                ([&node.key, max], right_height + (node.color as u8))
            }
            [Some(left), Some(right)] => {
                let left_node = &tree[left];
                assert!(node.is_black() || left_node.is_black(), "cannot have two red nodes in a row");
                let ([min, prev], left_height) = validate_rb_node(left, tree);
                assert!(min <= prev, "bad order");
                assert!(*prev < node.key, "left tree overlap");
                assert_eq!(*prev, tree[node.order[0].expect("not null")].key, "biggest node of left sub-tree has to be prev");

                let right_node = &tree[right];
                assert!(node.is_black() || right_node.is_black(), "cannot have two red nodes in a row");
                let ([next, max], right_height) = validate_rb_node(right, tree);
                assert!(next <= max, "bad order");
                assert!(node.key < *next, "right tree overlap");
                assert_eq!(*next, tree[node.order[1].expect("not null")].key, "smallest node of right sub-tree has to be next");

                assert_eq!(left_height, right_height, "black height of all paths has to be equal");
                ([min, max], left_height + (node.color as u8))
            }
        }
    }
    fn validate_rb_tree<K, V>(tree: &impl TreeReader<K, V>)
        where K: Ord + std::fmt::Debug, V: Value
    {
        let meta = tree.meta();
        if let Some(root) = meta.root {
            let ([min, max], black_height) = validate_rb_node(root, tree);
            let min_node = &tree[meta.range[0].expect("not null")];
            let max_node = &tree[meta.range[1].expect("not null")];
            assert_eq!(*min, min_node.key, "bad min range");
            assert_eq!(*max, max_node.key, "bad max range");
            assert_eq!(meta.black_height, black_height, "tracked black-height and true black-height mismatch");
        } else {
            assert_eq!(meta.range, [None, None], "empty tree implies empty range");
            assert_eq!(meta.black_height, 0, "empty tree has no black nodes");
        }
    }
    fn print_subtree<'a, K, V>(root: NodeRef, depth: u8, markers: u32,
        tree: &'a impl TreeReader<K, V>
    )
        where K: Ord + std::fmt::Debug + 'a, V: Value + 'a, V::Ref<'a>: std::fmt::Debug
    {
        for i in 0..depth {
            if markers & (1 << i) == 0 {
                print!("| ");
            } else {
                print!("  ");
            }
        }
        let Some(root) = root
            else {
                println!("[B] NIL");
                return;
            };
        let node = &tree[root];
        println!("[{}] {:?} => {:?}", if node.is_red() { "R" } else { "B" }, &node.key, node.value.get());
        print_subtree(node.children[0], depth + 1, markers, tree);
        print_subtree(node.children[1], depth + 1, markers | (1 << (depth + 1)), tree);
    }
    #[allow(unused)]
    fn print_tree<'a, K, V>(tree: &'a impl TreeReader<K, V>)
        where K: Ord + std::fmt::Debug + 'a, V: Value + 'a, V::Ref<'a>: std::fmt::Debug
    {
        print_subtree(tree.meta().root, 0, 1, tree);
    }

    // FIXME: remove will not balance correctly
    #[test]
    fn insert_remove() {
        let values = vec![1, 7, 8, 9, 10, 6, 5, 2, 3, 4, 0, 11];
        let mut forest = SimpleWeakForest::new();
        let mut tree = forest.insert();
        {
            let mut alloc = tree.alloc();
            for x in values.iter().copied() {
                println!("==================== +{} ====================", x);
                alloc.insert(x, x);
                print_tree(&alloc.0);
                validate_rb_tree(&alloc.0);
            }
            for x in values.into_iter() {
                println!("==================== -{} ====================", x);
                let value = alloc.remove(x);
                print_tree(&alloc.0);
                validate_rb_tree(&alloc.0);
                assert_eq!(value, Some(x));
            }
        }
    }
    #[test]
    fn iter() {
        let mut values = vec![1, 7, 8, 9, 10, 6, 5, 2, 3, 4, 0, 11];
        let mut forest = SimpleWeakForest::with_capacity(values.len());
        let mut tree = forest.insert();
        {
            let mut alloc = tree.alloc();
            for x in values.iter().copied() {
                alloc.insert(x, x);
            }
            print_tree(&alloc.0);
        }
        values.sort_unstable();
        {
            let read = tree.read();
            let result = read.iter().map( |(_, v)| *v ).collect::<Vec<_>>();
            assert_eq!(&values, &result);
        }
    }
    #[test]
    fn union_disjoint() {
        const N: usize = 5;
        for i in 0..=N {
            println!("==================== {} ====================", i);
            let mut forest = SimpleWeakForest::with_capacity(N);
            let lower = unsafe { forest.insert_sorted_iter_unchecked(
                (0..i).map( |n| (n, n) )
            ) };
            {
                let read = lower.read();
                print_tree(&read.0);
                validate_rb_tree(&read.0);
            }
            let higher = unsafe { forest.insert_sorted_iter_unchecked(
                (i..N).map( |n| (n, n) )
            ) };
            {
                let read = higher.read();
                print_tree(&read.0);
                validate_rb_tree(&read.0);
            }
            let all = higher.union_disjoint(lower).expect("disjoint trees");
            {
                let read = all.read();
                print_tree(&read.0);
                validate_rb_tree(&read.0);
                assert_eq!(read.min(), Some(&0));
                assert_eq!(read.max(), Some(&(N - 1)));
                assert_eq!(read.iter().count(), N);
            }
        }
    }
    #[test]
    fn split() {
        let items = [1,3,5,7,9];
        let n = items.len();
        for i in 0..11 {
            println!("==================== {} ====================", i);
            let mut forest = SimpleWeakForest::with_capacity(n);
            let tree = forest.insert_sorted_iter(
                items.iter().copied()
                    .map( |i| (i, i) )
                    .assume_sorted_by_key()
            );
            {
                let read = tree.read();
                print_tree(&read.0);
                validate_rb_tree(&read.0);
            }
            let j = items.binary_search(&i);
            let (lower, pivot, upper) = tree.split(&i);
            if j.is_ok() {
                assert_eq!(pivot, Some(i));
            } else {
                assert_eq!(pivot, None);
            }
            {
                let read = lower.read();
                print_tree(&read.0);
                validate_rb_tree(&read.0);
                assert_eq!(read.max(),
                    j.map_or_else( |j| items.get(j.wrapping_sub(1)) , |j| items.get(j.wrapping_sub(1)) )
                );
                assert_eq!(read.iter().count(),
                    j.unwrap_or_else( |j| j )
                );
            }
            {
                let read = upper.read();
                print_tree(&read.0);
                validate_rb_tree(&read.0);
                assert_eq!(read.min(),
                    j.map_or_else( |j| items.get(j) , |j| items.get(j + 1) )
                );
                assert_eq!(read.iter().count(),
                    j.map_or_else( |j| n - j , |j| n - j - 1 )
                );
            }
        }
    }
    // FIXME: wrong coloring (produces [B: [R: NIL NIL] [R: NIL NIL]] sub-tree (should all be black?))
    #[test]
    fn union() {
        const N: usize = 10;
        let mut forest = SimpleWeakForest::with_capacity(N << 1);
        let even = unsafe { forest.insert_sorted_iter_unchecked(
            (0..N).map( |n| (2*n, n) )
        ) };
        {
            let read = even.read();
            print_tree(&read.0);
            validate_rb_tree(&read.0);
        }
        let odd = unsafe { forest.insert_sorted_iter_unchecked(
            (0..N).map( |n| (2*n+1, n) )
        ) };
        {
            let read = odd.read();
            print_tree(&read.0);
            validate_rb_tree(&read.0);
        }
        let all = odd.union_merge(even, |_, _| panic!("duplicate key") );
        {
            let read = all.read();
            print_tree(&read.0);
            validate_rb_tree(&read.0);
            assert_eq!(read.min(), Some(&0));
            assert_eq!(read.max(), Some(&((N << 1) - 1)));
            assert_eq!(read.iter().count(), N << 1);
        }
    }

    #[cfg(feature = "sorted-iter")]
    #[test]
    fn search() {
        const N: usize = 5;
        let mut forest = SimpleWeakForest::with_capacity(N);
        let tree = forest.insert_sorted_iter(
            (0..N)
                .map( |i| (2*i+1, 2*i+1) )
                .assume_sorted_by_key()
        );
        {
            let read = tree.read();
            print_tree(&read.0);
            validate_rb_tree(&read.0);
            for i in 0..=(N<<1) {
                let result = read.search_by( |_, v| v.cmp(&i) );
                if i & 1 == 1 {
                    assert_eq!(result, SearchResult::Here(&i));
                } else {
                    match result {
                        SearchResult::LeftOf(next) => assert_eq!(next, &(i + 1)),
                        SearchResult::RightOf(prev) => assert_eq!(prev, &(i - 1)),
                        _ => panic!("unexpected search result")
                    }
                }
            }
        }
    }
}