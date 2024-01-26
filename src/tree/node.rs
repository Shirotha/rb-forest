use std::ops::Not;

use crate::arena::Index;

pub type NodeIndex = Index;
pub type NodeRef = Option<NodeIndex>;

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

// TODO: implement Cumulants as CumulantType trait + NoCumulant/WithCumulant(data, update_callback) structs

#[derive(Debug)]
pub struct Node<K, V> {
    pub(crate) key: K,
    pub(crate) value: V,
    pub(crate) color: Color,
    pub(crate) parent: NodeRef,
    pub(crate) children: [NodeRef; 2],
    pub(crate) order: [NodeRef; 2]
}

impl<K, V> Node<K, V> {
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
    pub const fn is_root(&self) -> bool {
        self.parent.is_none()
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
}