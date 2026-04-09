use crate::combinator::{ThenState, ZipInputState};

/// Typed path into composed state (minimal optic).
///
/// Deliberately **not** a full optic/proc-macro layer. If composed state becomes painful to navigate,
/// prefer **named state structs at pipeline boundaries** (or a focused macro) over ad hoc
/// reach-into-private-fields hacks.
pub trait Focus<T> {
    type Target;

    fn get<'a>(&self, root: &'a T) -> &'a Self::Target
    where
        Self::Target: 'a;

    fn get_mut<'a>(&self, root: &'a mut T) -> &'a mut Self::Target
    where
        Self::Target: 'a;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ThenLeft;

#[derive(Debug, Clone, Copy, Default)]
pub struct ThenRight;

#[derive(Debug, Clone, Copy, Default)]
pub struct ZipInputA;

#[derive(Debug, Clone, Copy, Default)]
pub struct ZipInputB;

impl<A, B> Focus<ThenState<A, B>> for ThenLeft {
    type Target = A;

    fn get<'a>(&self, root: &'a ThenState<A, B>) -> &'a A
    where
        Self::Target: 'a,
    {
        &root.left
    }

    fn get_mut<'a>(&self, root: &'a mut ThenState<A, B>) -> &'a mut A
    where
        Self::Target: 'a,
    {
        &mut root.left
    }
}

impl<A, B> Focus<ThenState<A, B>> for ThenRight {
    type Target = B;

    fn get<'a>(&self, root: &'a ThenState<A, B>) -> &'a B
    where
        Self::Target: 'a,
    {
        &root.right
    }

    fn get_mut<'a>(&self, root: &'a mut ThenState<A, B>) -> &'a mut B
    where
        Self::Target: 'a,
    {
        &mut root.right
    }
}

impl<A, B> Focus<ZipInputState<A, B>> for ZipInputA {
    type Target = A;

    fn get<'a>(&self, root: &'a ZipInputState<A, B>) -> &'a A
    where
        Self::Target: 'a,
    {
        &root.a
    }

    fn get_mut<'a>(&self, root: &'a mut ZipInputState<A, B>) -> &'a mut A
    where
        Self::Target: 'a,
    {
        &mut root.a
    }
}

impl<A, B> Focus<ZipInputState<A, B>> for ZipInputB {
    type Target = B;

    fn get<'a>(&self, root: &'a ZipInputState<A, B>) -> &'a B
    where
        Self::Target: 'a,
    {
        &root.b
    }

    fn get_mut<'a>(&self, root: &'a mut ZipInputState<A, B>) -> &'a mut B
    where
        Self::Target: 'a,
    {
        &mut root.b
    }
}
