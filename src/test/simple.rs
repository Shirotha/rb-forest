use super::*;

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
            let result = read.search( |_, v| v.cmp(&i) );
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
#[cfg(feature = "sorted-iter")]
#[test]
fn filter() {
    const N: usize = 5;
    let mut forest = SimpleWeakForest::with_capacity(N);
    let tree = forest.insert_sorted_iter(
        (0..N)
            .map( |i| (i, i) )
            .assume_sorted_by_key()
    );
    {
        let read = tree.read();
        print_tree(&read.0);
        validate_rb_tree(&read.0);
        for i in 0..=N {
            let mut result = read.filter( |_, v| SearchAction::new(true, *v < i, *v < i) ).map( |(_, v)| *v ).collect::<Vec<_>>();
            println!("v < {}: {:?}", i, result);
            assert_eq!(result.len(), i);
            result.sort_unstable();
            for (i, v) in result.into_iter().enumerate() {
                assert_eq!(i, v);
            }
        }
    }
}