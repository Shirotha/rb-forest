use std::ops::{Deref, DerefMut, Not};

use crate::{
    arena::Index,
    tree::Tree
};

pub(crate) type NodeIndex = Index;
pub(crate) type NodeRef = Option<NodeIndex>;

#[repr(u8)]
#[derive_const(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red = 0,
    Black = 1,
}
impl const Not for Color {
    type Output = Color;
    #[inline]
    fn not(self) -> Self::Output {
        match self {
            Color::Red => Color::Black,
            Color::Black => Color::Red
        }
    }
}
/// Smart pointer to a [Value].
/// This will ensure that any changes to the value will cause the updated cumulants to be propagated throughout the tree.
#[derive(Debug)]
pub struct ValueMut<'a, K: Ord, V: Value>(pub(crate) V::Mut<'a>, pub(crate) Index, pub(crate) &'a Tree<K, V>);
impl<'a, K: Ord, V: Value> const Deref for ValueMut<'a, K, V> {
    type Target = V::Mut<'a>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<'a, K: Ord, V: Value> const DerefMut for ValueMut<'a, K, V> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<'a, K: Ord, V: Value> const Drop for ValueMut<'a, K, V> {
    #[inline(always)]
    fn drop(&mut self) {
        let mut write = self.2.port.write();
        unsafe { Tree::propagate_cumulant(self.1, &mut write) }
    }
}
/// All values meant to be used with [Tree] need to implement this trait.
///
/// Instead of implementing this manually, consider using the [NoCumulant] type or
/// the [with_cumulant] macro.
#[const_trait]
pub trait Value {
    /// Data accossiated with the node itself.
    type Local;
    /// Data accossiated with the sub-tree rooted at the current node.
    type Cumulant;
    /// Read-only reference to the value.
    type Ref<'a> where Self: 'a;
    /// Mutable reference to the value
    type Mut<'a> where Self: 'a;
    /// Deconstructed value
    type Into;
    /// Constructs a new value.
    fn new(value: Self::Local) -> Self;
    fn into(self) -> Self::Into;
    fn get(&self) -> Self::Ref<'_>;
    /// # Safety
    /// Any changes to the value will invalidate the cumulants of this and all dependant values
    ///
    /// This is safe when `Value::has_cumulant` is `false`
    unsafe fn get_mut_unchecked(&mut self) -> Self::Mut<'_>;
    /// Cumulant of the current node.
    fn cumulant(&self) -> &Self::Cumulant;
    // Update the cumulant using the local value of this node and the cumulants of both children.
    fn update_cumulant(&mut self, children: [Option<&Self::Cumulant>; 2]);
    /// [Value::update_cumulant] will only be called when this returns `true`.
    fn has_cumulant() -> bool;
}
/// This type implements [Value] without cumulants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoCumulant<T>(T);
impl<T> const Value for NoCumulant<T> {
    type Local = T;
    type Cumulant = ();
    type Ref<'a> = &'a T where T: 'a;
    type Mut<'a> = &'a mut T where T: 'a;
    type Into = T;

    #[inline(always)]
    fn new(value: Self::Local) -> Self {
        Self(value)
    }
    #[inline(always)]
    fn into(self) -> Self::Into {
        self.0
    }
    #[inline(always)]
    fn get(&self) -> Self::Ref<'_> {
        &self.0
    }
    #[inline(always)]
    unsafe fn get_mut_unchecked(&mut self) -> Self::Mut<'_> {
        &mut self.0
    }
    #[inline(always)]
    fn cumulant(&self) -> &Self::Cumulant {
        &()
    }
    #[inline(always)]
    fn update_cumulant(&mut self, _children: [Option<&Self::Cumulant>; 2]) { }
    #[inline(always)]
    fn has_cumulant() -> bool { false }
}
/// Generates a new type and implements the [Value] trait.
///
/// # Examples
/// ```rust
/// with_cumulant!(
///     WithSum(value: &i32, children: [&i32] = 0) {
///         value + children[0] + children[1]
///     }
/// )
/// ```
// TODO: support passing by clone/copy
// TODO: support for mutating cumulant, instead of return
#[macro_export]
macro_rules! with_cumulant {
    {
        $visibility:vis $typename:ident $( <
            $( $param:tt $( : $( $constraint:path ),+ )? ),*
        > )? (
            $valuename:ident : & $valuetype:ty ,
            $childrenname:ident : [ & $cumulanttype:ty ] = $cumulantdefault:expr
        )
        $updatebody:block
    } => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $visibility struct $typename $( <
            $( $param $( : $( $constraint ),+ )? ),*
        > )? ($valuetype, $cumulanttype);
        impl $( <
            $( $param $( : $( $constraint ),+ )? ),*
        > )?
        const Value for $typename $( <
            $( $param ),*
        > )? {
            type Local = $valuetype ;
            type Cumulant = $cumulanttype ;
            type Ref<'a> = (&'a $valuetype , &'a $cumulanttype ) $( where $( $param : 'a ),+ )? ;
            type Mut<'a> = (&'a mut $valuetype , &'a $cumulanttype ) $( where $( $param : 'a ),+ )? ;
            type Into = $valuetype ;

            #[inline(always)]
            fn new(value: Self::Local) -> Self {
                Self(value, $cumulantdefault )
            }
            #[inline(always)]
            fn into(self) -> Self::Into {
                self.0
            }
            #[inline(always)]
            fn get(&self) -> Self::Ref<'_> {
                (&self.0, &self.1)
            }
            #[inline(always)]
            unsafe fn get_mut_unchecked(&mut self) -> Self::Mut<'_> {
                (&mut self.0, &self.1)
            }
            #[inline(always)]
            fn cumulant(&self) -> &Self::Cumulant {
                &self.1
            }
            #[inline(always)]
            fn update_cumulant(&mut self, children: [Option<&Self::Cumulant>; 2]) {
                let $valuename = &self.0;
                #[allow(non_snake_case)]
                let __default__ = $cumulantdefault;
                let $childrenname = [
                    children[0].unwrap_or(&__default__),
                    children[1].unwrap_or(&__default__),
                ];
                self.1 = $updatebody;
            }
            #[inline(always)]
            fn has_cumulant() -> bool { true }
        }
    };
}
pub use with_cumulant;

#[derive(Debug)]
pub(crate) struct Node<K: Ord, V: Value> {
    pub key: K,
    pub value: V,
    pub color: Color,
    pub parent: NodeRef,
    pub children: [NodeRef; 2],
    pub order: [NodeRef; 2]
}
impl<K: Ord, V: Value> Node<K, V> {
    #[inline]
    pub const fn new(key: K, value: V, color: Color) -> Self {
        Self {
            key, value, color,
            parent: None,
            children: [None, None],
            order: [None, None]
        }
    }
    #[inline(always)]
    pub const fn is_black(&self) -> bool {
        match self.color {
            Color::Black => true,
            Color::Red => false
        }
    }
    #[inline(always)]
    pub const fn is_red(&self) -> bool {
        match self.color {
            Color::Black => false,
            Color::Red => true
        }
    }
    #[inline]
    pub(crate) const fn clear(&mut self, color: Color) {
        self.parent = None;
        self.order = [None, None];
        self.children = [None, None];
        self.color = color;
    }
}