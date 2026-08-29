/// Tagged union of outputs from [`crate::ZipInput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipInputOut<Ao, Bo> {
    A(Ao),
    B(Bo),
}

/// Sink for outputs produced by a [`crate::Scan`] step or flush.
pub trait Emit<T> {
    fn emit(&mut self, item: T);
}

/// Explicitly discard emissions without allocating a temporary collection.
#[derive(Debug, Default, Clone, Copy)]
pub struct DiscardEmitter;

impl<T> Emit<T> for DiscardEmitter {
    #[inline]
    fn emit(&mut self, _item: T) {}
}

/// Feed emissions directly into another [`crate::Scan`] without an intermediate collection.
pub struct ScanEmit<'a, S, E>
where
    S: crate::Scan,
{
    scan: &'a S,
    state: &'a mut S::State,
    sink: &'a mut E,
}

impl<'a, S, E> ScanEmit<'a, S, E>
where
    S: crate::Scan,
{
    pub fn new(scan: &'a S, state: &'a mut S::State, sink: &'a mut E) -> Self {
        Self { scan, state, sink }
    }
}

impl<S, E> Emit<S::In> for ScanEmit<'_, S, E>
where
    S: crate::Scan,
    E: Emit<S::Out>,
{
    #[inline]
    fn emit(&mut self, item: S::In) {
        self.scan.step(self.state, item, self.sink);
    }
}

/// Collect outputs into a `Vec` (handy in tests).
#[derive(Debug, Default)]
pub struct VecEmitter<T>(pub Vec<T>);

impl<T> VecEmitter<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
}

impl<T> Emit<T> for VecEmitter<T> {
    fn emit(&mut self, item: T) {
        self.0.push(item);
    }
}

/// Adapt an inner sink so the outer scan sees a different output type (used by combinators).
pub struct MapEmit<'a, SOut, U, E, M> {
    pub sink: &'a mut E,
    pub map: &'a M,
    _p: std::marker::PhantomData<(SOut, U)>,
}

impl<'a, SOut, U, E, M> MapEmit<'a, SOut, U, E, M> {
    pub fn new(sink: &'a mut E, map: &'a M) -> Self {
        Self {
            sink,
            map,
            _p: std::marker::PhantomData,
        }
    }
}

impl<'a, SOut, U, E, M> Emit<SOut> for MapEmit<'a, SOut, U, E, M>
where
    E: Emit<U>,
    M: Fn(SOut) -> U,
{
    fn emit(&mut self, item: SOut) {
        self.sink.emit((self.map)(item));
    }
}

pub struct FilterMapEmit<'a, SOut, U, E, F> {
    pub sink: &'a mut E,
    pub f: &'a F,
    _p: std::marker::PhantomData<(SOut, U)>,
}

impl<'a, SOut, U, E, F> FilterMapEmit<'a, SOut, U, E, F> {
    pub fn new(sink: &'a mut E, f: &'a F) -> Self {
        Self {
            sink,
            f,
            _p: std::marker::PhantomData,
        }
    }
}

impl<'a, SOut, U, E, F> Emit<SOut> for FilterMapEmit<'a, SOut, U, E, F>
where
    E: Emit<U>,
    F: Fn(SOut) -> Option<U>,
{
    fn emit(&mut self, item: SOut) {
        if let Some(u) = (self.f)(item) {
            self.sink.emit(u);
        }
    }
}
