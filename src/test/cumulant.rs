use super::*;

trait Sum = std::ops::Add<Self, Output = Self> + Default + Copy;
with_cumulant!(
    WithSum<T: Sum>(v: &T, c: [&T] = T::default()) {
        *v + *c[0] + *c[1]
    }
);

#[test]
fn insert_remove() {
    let values = vec![1, 7, 8, 9, 10, 6, 5, 2, 3, 4, 0, 11];
    let mut forest: WeakForest<_, WithSum<_>> = WeakForest::new();
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
#[test]
fn union() {
    const N: usize = 10;
    let mut forest: WeakForest<_, WithSum<_>> = WeakForest::with_capacity(N << 1);
    let even = unsafe { forest.insert_sorted_iter_unchecked(
        (0..N).map( |n| (2*n, 2*n) )
    ) };
    {
        let read = even.read();
        print_tree(&read.0);
        validate_rb_tree(&read.0);
        assert_eq!(read.cumulant().copied(), Some(N * (N - 1)));
    }
    let odd = unsafe { forest.insert_sorted_iter_unchecked(
        (0..N).map( |n| (2*n+1, 2*n+1) )
    ) };
    {
        let read = odd.read();
        print_tree(&read.0);
        validate_rb_tree(&read.0);
        assert_eq!(read.cumulant().copied(), Some(N * N));
    }
    let all = odd.union_merge(even, |_, _| panic!("duplicate key") );
    {
        let read = all.read();
        print_tree(&read.0);
        validate_rb_tree(&read.0);
        assert_eq!(read.cumulant().copied(), Some(N * ((N<<1) - 1)));
    }
}
#[test]
fn iter_mut() {
    const N: usize = 10;
    let mut forest: WeakForest<_, WithSum<_>> = WeakForest::with_capacity(N << 1);
    let mut tree = unsafe { forest.insert_sorted_iter_unchecked(
        (0..N).map( |n| (n, n) )
    ) };
    {
        let mut write = tree.write();
        print_tree(&write.0);
        validate_rb_tree(&write.0);
        assert_eq!(write.cumulant().copied(), Some((N * (N - 1)) >> 1));
        for (_k, (v, _c)) in write.iter_mut() {
            *v *= 2;
        }
        print_tree(&write.0);
        validate_rb_tree(&write.0);
        assert_eq!(write.cumulant().copied(), Some(N * (N - 1)));
    }
}