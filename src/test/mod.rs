
mod simple;
mod cumulant;

#[cfg(feature = "sorted-iter")]
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