use std::ops::{Deref, DerefMut, Not};

use crate::arena::Index;

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

#[const_trait]
pub trait Value {
    type Local;
    type Cumulant;
    type Ref<'a> where Self: 'a;
    type Mut<'a> where Self: 'a;
    type Into;
    fn new(value: Self::Local) -> Self;
    fn into(self) -> Self::Into;
    fn get(&self) -> Self::Ref<'_>;
    /// # Safety
    /// Any changes to the value will invalidate the cumulants of this and all dependant values
    ///
    /// This is safe when `Value::has_cumulant` is `false`
    unsafe fn get_mut_unchecked(&mut self) -> Self::Mut<'_>;
    fn cumulant(&self) -> &Self::Cumulant;
    fn update_cumulant(&mut self, children: [Option<&Self::Cumulant>; 2]);
    fn has_cumulant() -> bool;
}

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

#[macro_export]
macro_rules! with_cumulant {
    { $typename:ident ( $default:expr ) = $update:expr } => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $typename <T, C>(T, C);
        impl<T, C, F: Fn(&mut C, &T, [Option<&C>; 2])> const Value for $typename <T, C> {
            type Local = T;
            type Cumulant = C;
            type Ref<'a> = (&'a T, &'a C) where T: 'a, C: 'a, F: 'a;
            type Mut<'a> = (&'a mut T, &'a C) where T: 'a, C: 'a, F: 'a;

            #[inline(always)]
            fn new(value: Self::Local) -> Self {
                Self(value, $default )
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
                $update (&mut self.1, &self.0, children)
            }
            #[inline(always)]
            fn has_cumulant() -> bool { true }
        }
    };
}
pub use with_cumulant;

use super::Tree;

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