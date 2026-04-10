//! Core **Arrow** scaffolding on [`Scan`]: identity, copy, conditional emit, tuple zip, and
//! “apply with environment” decomposition.
//!
//! - **Vec outputs:** every scan already supports 0..N emits per step; use [`Scan::step_collect`] when
//!   you want a `Vec` without hand-rolling [`VecEmitter`](crate::VecEmitter).
//! - **`app`:** [`ArrowApply`] is `(C, S::In) → …`: **environment** `C` plus **operand** (decompose input,
//!   then run `inner` only on the operand).
//! - **No change to `Scan::step` signature:** the `Emit` sink stays zero-cost for 1:1 hot paths.

use std::marker::PhantomData;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::control::FlushReason;
use crate::emit::{Emit, VecEmitter};
use crate::scan::{FlushableScan, Scan, SnapshottingScan};

// --- Id ---

/// Arrow `id`: pass input through unchanged (one emit per step).
#[derive(Debug, Clone, Copy)]
pub struct Id<T>(PhantomData<T>);

impl<T> Id<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for Id<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: Clone> Scan for Id<T> {
    type In = T;
    type Out = T;
    type State = ();

    fn init(&self) -> Self::State {}

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        emit.emit(input);
    }
}

impl<T: Clone> FlushableScan for Id<T> {
    type Offset = ();

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

// --- Dup ---

/// Emit the input twice (arrow `dup` / copy for streams).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dup<T>(PhantomData<T>);

impl<T> Dup<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T: Clone> Scan for Dup<T> {
    type In = T;
    type Out = T;
    type State = ();

    fn init(&self) -> Self::State {}

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        emit.emit(input.clone());
        emit.emit(input);
    }
}

impl<T: Clone> FlushableScan for Dup<T> {
    type Offset = ();

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

// --- EmitWhen: conditional emit after inner step (saturated window, etc.) ---

/// Always runs `inner.step` (state advances), but **forwards emissions** only when `allow_emit`
/// holds on the **post-step** state (e.g. rolling buffer full).
#[derive(Clone)]
pub struct EmitWhen<S, G> {
    pub inner: S,
    pub allow_emit: G,
}

impl<S, G> Scan for EmitWhen<S, G>
where
    S: Scan,
    G: Fn(&S::State) -> bool,
{
    type In = S::In;
    type Out = S::Out;
    type State = S::State;

    fn init(&self) -> Self::State {
        self.inner.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut buf = VecEmitter::new();
        self.inner.step(state, input, &mut buf);
        if (self.allow_emit)(state) {
            for x in buf.into_inner() {
                emit.emit(x);
            }
        }
    }
}

impl<S, G, O> FlushableScan for EmitWhen<S, G>
where
    S: FlushableScan<Offset = O>,
    G: Fn(&S::State) -> bool,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.flush(state, signal, emit);
    }
}

impl<S, G> SnapshottingScan for EmitWhen<S, G>
where
    S: SnapshottingScan,
    G: Fn(&S::State) -> bool,
    S::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.inner.restore(snapshot)
    }
}

// --- ZipTuple: product input decomposition ---

/// Input is a pair `(a, b)`; feed `a` to `left`, `b` to `right`; output is `(oa, ob)` per
/// **pairwise combination** of emissions (left outer × right inner for each step).
///
/// Type alias: [`Both`].
#[derive(Clone)]
pub struct ZipTuple<A, B> {
    pub left: A,
    pub right: B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipTupleState<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZipTupleSnapshot<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

impl<A, B> Scan for ZipTuple<A, B>
where
    A: Scan,
    B: Scan,
    A::Out: Clone,
    B::Out: Clone,
{
    type In = (A::In, B::In);
    type Out = (A::Out, B::Out);
    type State = ZipTupleState<A::State, B::State>;

    fn init(&self) -> Self::State {
        ZipTupleState {
            left: self.left.init(),
            right: self.right.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let (a_in, b_in) = input;
        let mut la = VecEmitter::new();
        self.left.step(&mut state.left, a_in, &mut la);
        let mut lb = VecEmitter::new();
        self.right.step(&mut state.right, b_in, &mut lb);
        let outs_b = lb.into_inner();
        for oa in la.into_inner() {
            for ob in outs_b.iter().cloned() {
                emit.emit((oa.clone(), ob));
            }
        }
    }
}

impl<A, B, O> FlushableScan for ZipTuple<A, B>
where
    A: FlushableScan<Offset = O>,
    B: FlushableScan<Offset = O>,
    A::Out: Clone,
    B::Out: Clone,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut la = VecEmitter::new();
        self.left
            .flush(&mut state.left, signal.clone(), &mut la);
        let mut lb = VecEmitter::new();
        self.right.flush(&mut state.right, signal, &mut lb);
        let outs_b = lb.into_inner();
        for oa in la.into_inner() {
            for ob in outs_b.iter().cloned() {
                emit.emit((oa.clone(), ob));
            }
        }
    }
}

impl<A, B> SnapshottingScan for ZipTuple<A, B>
where
    A: SnapshottingScan,
    B: SnapshottingScan,
    A::Out: Clone,
    B::Out: Clone,
    A::Snapshot: Serialize + DeserializeOwned,
    B::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = ZipTupleSnapshot<A::Snapshot, B::Snapshot>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        ZipTupleSnapshot {
            left: self.left.snapshot(&state.left),
            right: self.right.snapshot(&state.right),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ZipTupleState {
            left: self.left.restore(snapshot.left),
            right: self.right.restore(snapshot.right),
        }
    }
}

/// Alias for [`ZipTuple`] (arrow `***` / parallel product on two streams).
pub type Both<A, B> = ZipTuple<A, B>;

#[inline]
pub fn both<A, B>(left: A, right: B) -> Both<A, B> {
    ZipTuple { left, right }
}

// --- ArrowApply: environment + operand ---

/// Decompose input as **`(environment, operand)`** and run `inner` only on `operand`.
///
/// This is the scan analogue of holding a function-valued arrow and an argument: the `inner`
/// scan is the program; `operand` is `S::In`; `environment` is carried for product/arrow wiring
/// (you can merge paths upstream with [`ZipTuple`](ZipTuple) or [`super::Split`](crate::Split)).
#[derive(Clone)]
pub struct ArrowApply<C, S> {
    pub inner: S,
    _c: PhantomData<C>,
}

impl<C, S> ArrowApply<C, S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _c: PhantomData,
        }
    }
}

impl<C, S> Scan for ArrowApply<C, S>
where
    S: Scan,
{
    type In = (C, S::In);
    type Out = S::Out;
    type State = S::State;

    fn init(&self) -> Self::State {
        self.inner.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.step(state, input.1, emit);
    }
}

impl<C, S, O> FlushableScan for ArrowApply<C, S>
where
    S: FlushableScan<Offset = O>,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.flush(state, signal, emit);
    }
}

impl<C, S> SnapshottingScan for ArrowApply<C, S>
where
    S: SnapshottingScan,
    S::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.inner.restore(snapshot)
    }
}

// --- OnLeft / OnRight: inject constant into a sum ---

/// Feed `inner` only [`super::Either::Left`](crate::Either) values; **drop** `Right` inputs.
#[derive(Clone)]
pub struct OnLeft<S, R> {
    pub inner: S,
    _r: PhantomData<R>,
}

impl<S, R> OnLeft<S, R> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _r: PhantomData,
        }
    }
}

impl<S, L, R> Scan for OnLeft<S, R>
where
    S: Scan<In = L>,
{
    type In = crate::Either<L, R>;
    type Out = S::Out;
    type State = S::State;

    fn init(&self) -> Self::State {
        self.inner.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let crate::Either::Left(x) = input {
            self.inner.step(state, x, emit);
        }
    }
}

impl<S, L, R, O> FlushableScan for OnLeft<S, R>
where
    S: FlushableScan<Offset = O>,
    S: Scan<In = L>,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.flush(state, signal, emit);
    }
}

impl<S, L, R> SnapshottingScan for OnLeft<S, R>
where
    S: SnapshottingScan<In = L>,
    S: Scan<In = L>,
    S::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.inner.restore(snapshot)
    }
}

/// Feed `inner` only [`super::Either::Right`](crate::Either) values; **drop** `Left` inputs.
#[derive(Clone)]
pub struct OnRight<L, S> {
    pub inner: S,
    _l: PhantomData<L>,
}

impl<L, S> OnRight<L, S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _l: PhantomData,
        }
    }
}

impl<S, L, R> Scan for OnRight<L, S>
where
    S: Scan<In = R>,
{
    type In = crate::Either<L, R>;
    type Out = S::Out;
    type State = S::State;

    fn init(&self) -> Self::State {
        self.inner.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let crate::Either::Right(y) = input {
            self.inner.step(state, y, emit);
        }
    }
}

impl<S, L, R, O> FlushableScan for OnRight<L, S>
where
    S: FlushableScan<Offset = O>,
    S: Scan<In = R>,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.flush(state, signal, emit);
    }
}

impl<S, L, R> SnapshottingScan for OnRight<L, S>
where
    S: SnapshottingScan<In = R>,
    S: Scan<In = R>,
    S::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.inner.restore(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow::Arr;

    struct Window3;

    impl Scan for Window3 {
        type In = i32;
        type Out = i32;
        type State = [i32; 3];
        fn init(&self) -> Self::State {
            [0; 3]
        }
        fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: Emit<Self::Out>,
        {
            state[0] = state[1];
            state[1] = state[2];
            state[2] = input;
            emit.emit(state[0] + state[1] + state[2]);
        }
    }

    #[test]
    fn emit_when_drops_until_saturated() {
        let s = EmitWhen {
            inner: Window3,
            allow_emit: |st: &[i32; 3]| st[0] != 0,
        };
        let mut st = s.init();
        assert!(s.step_collect(&mut st, 1).is_empty());
        assert!(s.step_collect(&mut st, 2).is_empty());
        let v = s.step_collect(&mut st, 3);
        assert_eq!(v, vec![6]);
    }

    #[test]
    fn arrow_apply_ignores_env() {
        let a = ArrowApply::<String, _>::new(Arr::new(|x: i32| x + 1));
        let mut st = a.init();
        assert_eq!(a.step_collect(&mut st, ("e".into(), 41)), vec![42]);
    }
}
