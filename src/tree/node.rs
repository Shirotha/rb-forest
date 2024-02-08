use std::ops::Not;

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

#[const_trait]
pub trait Value {
    type Local;
    type Cumulant;
    fn get(&self) -> &Self::Local;
    fn get_mut(&mut self) -> &mut Self::Local;
    fn cumulant(&self) -> &Self::Cumulant;
    fn update_cumulant(&mut self, children: [Option<&Self::Cumulant>; 2]);
    fn need_update() -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoCumulant<T>(pub T);
impl<T> const Value for NoCumulant<T> {
    type Local = T;
    type Cumulant = ();
    #[inline(always)]
    fn get(&self) -> &Self::Local {
        &self.0
    }
    #[inline(always)]
    fn get_mut(&mut self) -> &mut Self::Local {
        &mut self.0
    }
    #[inline(always)]
    fn cumulant(&self) -> &Self::Cumulant {
        &()
    }
    #[inline(always)]
    fn update_cumulant(&mut self, _children: [Option<&Self::Cumulant>; 2]) { }
    #[inline(always)]
    fn need_update() -> bool { false }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WithCumulant<T, C, F: Fn(&mut C, &T, [Option<&C>; 2])>(T, C, F);
impl<T, C, F: Fn(&mut C, &T, [Option<&C>; 2])> const Value for WithCumulant<T, C, F> {
    type Local = T;
    type Cumulant = C;
    #[inline(always)]
    fn get(&self) -> &Self::Local {
        &self.0
    }
    #[inline(always)]
    fn get_mut(&mut self) -> &mut Self::Local {
        &mut self.0
    }
    #[inline(always)]
    fn cumulant(&self) -> &Self::Cumulant {
        &self.1
    }
    #[inline(always)]
    fn update_cumulant(&mut self, children: [Option<&Self::Cumulant>; 2]) {
        self.2(&mut self.1, &self.0, children)
    }
    #[inline(always)]
    fn need_update() -> bool { true }
}

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