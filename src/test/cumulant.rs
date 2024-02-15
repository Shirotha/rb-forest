use super::*;

// FIXME: leaf node doesn't update its cumulant when removing the root node (after rotate)
#[test]
fn insert_remove() {
    with_cumulant!(WithSum(i32, i32 => 0) = |v: &i32, c: [Option<&i32>; 2]| *v + c[0].copied().unwrap_or(0) + c[1].copied().unwrap_or(0) );
    let values = vec![1, 7, 8, 9, 10, 6, 5, 2, 3, 4, 0, 11];
    let mut forest: WeakForest<_, WithSum> = WeakForest::new();
    let mut tree = forest.insert();
    let mut sum = 0;
    {
        let mut alloc = tree.alloc();
        for x in values.iter().copied() {
            println!("==================== +{} ====================", x);
            alloc.insert(x, x);
            print_tree(&alloc.0);
            validate_rb_tree(&alloc.0);
            sum += x;
            assert_eq!(alloc.cumulant().copied(), Some(sum));
        }
        for x in values.into_iter() {
            println!("==================== -{} ====================", x);
            let value = alloc.remove(x);
            print_tree(&alloc.0);
            validate_rb_tree(&alloc.0);
            assert_eq!(value, Some(x));
            sum -= x;
            if sum != 0 {
                assert_eq!(alloc.cumulant().copied(), Some(sum));
            } else {
                assert!(alloc.is_empty());
                assert_eq!(alloc.cumulant(), None);
            }
        }
    }
}