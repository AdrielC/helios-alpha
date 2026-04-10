//! **Arrow-style** combinators for [`Scan`]: categorical composition beyond plain [`Then`](crate::Then).
//!
//! Rough map to Haskell **Arrows**:
//! - [`Arr`] — `arr` (pure, stateless).
//! - [`Then`] / [`scan_then!`](crate::scan_then) — `>>>` (sequential composition).
//! - [`First`] / [`Second`] — `first` / `second` on product inputs `(T, C)` or `(C, T)`.
//! - [`Split`] — fan-out on the same input (`&&&`-like; outputs tagged [`SplitOut`]).
//! - [`Merge`] — fan-in on [`MergeIn`] (tagged sum stream).
//! - [`Choose`] — route [`Either`] to left or right scan (`+++` on the input side).
//! - [`Fanin`] — run both scans on the same input; wrap outputs in [`Either`] (`|||` on the output side).
//!
//! **Flush / snapshot:** Only [`Arr`], [`Split`], [`Merge`], and [`Choose`] implement [`FlushableScan`] /
//! [`SnapshottingScan`] when their inner scans do (with aligned `Offset` types). [`First`] / [`Second`] /
//! [`Fanin`] are **step-only** today because pairing with flush has no natural `C` without extra state.

use std::marker::PhantomData;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::combinator::Then;
use crate::control::FlushReason;
use crate::emit::{Emit, VecEmitter, ZipInputOut};
use crate::scan::{FlushableScan, Scan, SnapshottingScan};

// --- Either (sum type for choice / fan-in) ---

/// Tagged sum for [`Choose`] / [`Fanin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

/// Tag for [`Merge`] inputs (fan-in of two logical streams).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeIn<L, R> {
    L(L),
    R(R),
}

/// Output of [`Split`] — same shape as [`ZipInputOut`](crate::emit::ZipInputOut).
pub type SplitOut<A, B> = ZipInputOut<A, B>;

// --- Arr ---

/// `arr f`: stateless map (one emit per step).
///
/// `I` and `O` are phantom — use `Arr::new` and type inference from context, or annotate
/// `Arr::<_, i32, i32>::new(|x| x + 1)`.
#[derive(Clone)]
pub struct Arr<F, I, O> {
    pub f: F,
    _io: PhantomData<fn(I) -> O>,
}

impl<F, I, O> Arr<F, I, O> {
    pub fn new(f: F) -> Self {
        Self {
            f,
            _io: PhantomData,
        }
    }
}

impl<F, I, O> Scan for Arr<F, I, O>
where
    F: Fn(I) -> O,
{
    type In = I;
    type Out = O;
    type State = ();

    fn init(&self) -> Self::State {}

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        emit.emit((self.f)(input));
    }
}

impl<F, I, O> FlushableScan for Arr<F, I, O>
where
    F: Fn(I) -> O,
{
    type Offset = ();

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

// --- First / Second ---

/// `first s`: `(t, c)` → run `inner` on `t`, pair each output with `c`.
#[derive(Clone)]
pub struct First<S, C> {
    pub inner: S,
    _c: PhantomData<C>,
}

impl<S, C> First<S, C> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _c: PhantomData,
        }
    }
}

/// `second s`: `(c, t)` → run `inner` on `t`, pair each output with `c`.
#[derive(Clone)]
pub struct Second<S, C> {
    pub inner: S,
    _c: PhantomData<C>,
}

impl<S, C> Second<S, C> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _c: PhantomData,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductPassState<Ss> {
    pub inner: Ss,
}

impl<S, T, C, O> Scan for First<S, C>
where
    S: Scan<In = T, Out = O>,
    C: Clone,
{
    type In = (T, C);
    type Out = (O, C);
    type State = ProductPassState<S::State>;

    fn init(&self) -> Self::State {
        ProductPassState {
            inner: self.inner.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let (t, c) = input;
        let mut buf = VecEmitter::new();
        self.inner.step(&mut state.inner, t, &mut buf);
        for o in buf.into_inner() {
            emit.emit((o, c.clone()));
        }
    }
}

impl<S, T, C, O> Scan for Second<S, C>
where
    S: Scan<In = T, Out = O>,
    C: Clone,
{
    type In = (C, T);
    type Out = (C, O);
    type State = ProductPassState<S::State>;

    fn init(&self) -> Self::State {
        ProductPassState {
            inner: self.inner.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let (c, t) = input;
        let mut buf = VecEmitter::new();
        self.inner.step(&mut state.inner, t, &mut buf);
        for o in buf.into_inner() {
            emit.emit((c.clone(), o));
        }
    }
}

impl<S, T, C, O> SnapshottingScan for First<S, C>
where
    S: SnapshottingScan<In = T, Out = O>,
    S: Scan<In = T, Out = O>,
    C: Clone,
    S::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(&state.inner)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ProductPassState {
            inner: self.inner.restore(snapshot),
        }
    }
}

impl<S, T, C, O> SnapshottingScan for Second<S, C>
where
    S: SnapshottingScan<In = T, Out = O>,
    S: Scan<In = T, Out = O>,
    C: Clone,
    S::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(&state.inner)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ProductPassState {
            inner: self.inner.restore(snapshot),
        }
    }
}

// --- Split (fan-out) ---

/// Same input to two scans; outputs tagged [`SplitOut`].
#[derive(Clone)]
pub struct Split<A, B> {
    pub left: A,
    pub right: B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitState<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitSnapshot<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

impl<A, B> Scan for Split<A, B>
where
    A: Scan,
    B: Scan<In = A::In>,
    A::In: Clone,
{
    type In = A::In;
    type Out = SplitOut<A::Out, B::Out>;
    type State = SplitState<A::State, B::State>;

    fn init(&self) -> Self::State {
        SplitState {
            left: self.left.init(),
            right: self.right.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut la = VecEmitter::new();
        self.left.step(&mut state.left, input.clone(), &mut la);
        for o in la.into_inner() {
            emit.emit(SplitOut::A(o));
        }
        let mut lb = VecEmitter::new();
        self.right.step(&mut state.right, input, &mut lb);
        for o in lb.into_inner() {
            emit.emit(SplitOut::B(o));
        }
    }
}

impl<A, B, O> FlushableScan for Split<A, B>
where
    A: FlushableScan<Offset = O>,
    B: FlushableScan<Offset = O>,
    B: Scan<In = A::In>,
    A::In: Clone,
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
        for o in la.into_inner() {
            emit.emit(SplitOut::A(o));
        }
        let mut lb = VecEmitter::new();
        self.right.flush(&mut state.right, signal, &mut lb);
        for o in lb.into_inner() {
            emit.emit(SplitOut::B(o));
        }
    }
}

impl<A, B> SnapshottingScan for Split<A, B>
where
    A: SnapshottingScan,
    B: SnapshottingScan,
    B: Scan<In = A::In>,
    A::In: Clone,
    A::Snapshot: Serialize + DeserializeOwned,
    B::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = SplitSnapshot<A::Snapshot, B::Snapshot>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        SplitSnapshot {
            left: self.left.snapshot(&state.left),
            right: self.right.snapshot(&state.right),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        SplitState {
            left: self.left.restore(snapshot.left),
            right: self.right.restore(snapshot.right),
        }
    }
}

// --- Merge (fan-in on sum) ---

/// Route tagged [`MergeIn`] to `left` or `right` scan.
#[derive(Clone)]
pub struct Merge<A, B> {
    pub left: A,
    pub right: B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeState<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeSnapshot<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

impl<A, B> Scan for Merge<A, B>
where
    A: Scan,
    B: Scan,
    A::In: Clone,
    B::In: Clone,
{
    type In = MergeIn<A::In, B::In>;
    type Out = Either<A::Out, B::Out>;
    type State = MergeState<A::State, B::State>;

    fn init(&self) -> Self::State {
        MergeState {
            left: self.left.init(),
            right: self.right.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            MergeIn::L(x) => {
                let mut buf = VecEmitter::new();
                self.left.step(&mut state.left, x, &mut buf);
                for o in buf.into_inner() {
                    emit.emit(Either::Left(o));
                }
            }
            MergeIn::R(y) => {
                let mut buf = VecEmitter::new();
                self.right.step(&mut state.right, y, &mut buf);
                for o in buf.into_inner() {
                    emit.emit(Either::Right(o));
                }
            }
        }
    }
}

impl<A, B, O> FlushableScan for Merge<A, B>
where
    A: FlushableScan<Offset = O>,
    B: FlushableScan<Offset = O>,
    A::In: Clone,
    B::In: Clone,
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
        for o in la.into_inner() {
            emit.emit(Either::Left(o));
        }
        let mut lb = VecEmitter::new();
        self.right.flush(&mut state.right, signal, &mut lb);
        for o in lb.into_inner() {
            emit.emit(Either::Right(o));
        }
    }
}

impl<A, B> SnapshottingScan for Merge<A, B>
where
    A: SnapshottingScan,
    B: SnapshottingScan,
    A::In: Clone,
    B::In: Clone,
    A::Snapshot: Serialize + DeserializeOwned,
    B::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = MergeSnapshot<A::Snapshot, B::Snapshot>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        MergeSnapshot {
            left: self.left.snapshot(&state.left),
            right: self.right.snapshot(&state.right),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        MergeState {
            left: self.left.restore(snapshot.left),
            right: self.right.restore(snapshot.right),
        }
    }
}

// --- Choose (sum input → one branch) ---

/// Arrow `+++`: [`Either::Left`] → `left`, [`Either::Right`] → `right`.
#[derive(Clone)]
pub struct Choose<A, B> {
    pub left: A,
    pub right: B,
}

pub type ChooseState<A, B> = MergeState<A, B>;
pub type ChooseSnapshot<A, B> = MergeSnapshot<A, B>;

impl<A, B> Scan for Choose<A, B>
where
    A: Scan,
    B: Scan,
    A::In: Clone,
    B::In: Clone,
{
    type In = Either<A::In, B::In>;
    type Out = Either<A::Out, B::Out>;
    type State = ChooseState<A::State, B::State>;

    fn init(&self) -> Self::State {
        ChooseState {
            left: self.left.init(),
            right: self.right.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            Either::Left(x) => {
                let mut buf = VecEmitter::new();
                self.left.step(&mut state.left, x, &mut buf);
                for o in buf.into_inner() {
                    emit.emit(Either::Left(o));
                }
            }
            Either::Right(y) => {
                let mut buf = VecEmitter::new();
                self.right.step(&mut state.right, y, &mut buf);
                for o in buf.into_inner() {
                    emit.emit(Either::Right(o));
                }
            }
        }
    }
}

impl<A, B, O> FlushableScan for Choose<A, B>
where
    A: FlushableScan<Offset = O>,
    B: FlushableScan<Offset = O>,
    A::In: Clone,
    B::In: Clone,
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
        for o in la.into_inner() {
            emit.emit(Either::Left(o));
        }
        let mut lb = VecEmitter::new();
        self.right.flush(&mut state.right, signal, &mut lb);
        for o in lb.into_inner() {
            emit.emit(Either::Right(o));
        }
    }
}

impl<A, B> SnapshottingScan for Choose<A, B>
where
    A: SnapshottingScan,
    B: SnapshottingScan,
    A::In: Clone,
    B::In: Clone,
    A::Snapshot: Serialize + DeserializeOwned,
    B::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = ChooseSnapshot<A::Snapshot, B::Snapshot>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        ChooseSnapshot {
            left: self.left.snapshot(&state.left),
            right: self.right.snapshot(&state.right),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ChooseState {
            left: self.left.restore(snapshot.left),
            right: self.right.restore(snapshot.right),
        }
    }
}

// --- Fanin (same input → both → sum out) ---

/// Arrow `|||` on streams: one input fed to both scans; outputs tagged [`Either`] (A emits first, then B).
#[derive(Clone)]
pub struct Fanin<A, B> {
    pub left: A,
    pub right: B,
}

pub type FaninState<A, B> = SplitState<A, B>;
pub type FaninSnapshot<A, B> = SplitSnapshot<A, B>;

impl<A, B> Scan for Fanin<A, B>
where
    A: Scan,
    B: Scan<In = A::In>,
    A::In: Clone,
{
    type In = A::In;
    type Out = Either<A::Out, B::Out>;
    type State = FaninState<A::State, B::State>;

    fn init(&self) -> Self::State {
        FaninState {
            left: self.left.init(),
            right: self.right.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut la = VecEmitter::new();
        self.left.step(&mut state.left, input.clone(), &mut la);
        for o in la.into_inner() {
            emit.emit(Either::Left(o));
        }
        let mut lb = VecEmitter::new();
        self.right.step(&mut state.right, input, &mut lb);
        for o in lb.into_inner() {
            emit.emit(Either::Right(o));
        }
    }
}

impl<A, B, O> FlushableScan for Fanin<A, B>
where
    A: FlushableScan<Offset = O>,
    B: FlushableScan<Offset = O>,
    B: Scan<In = A::In>,
    A::In: Clone,
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
        for o in la.into_inner() {
            emit.emit(Either::Left(o));
        }
        let mut lb = VecEmitter::new();
        self.right.flush(&mut state.right, signal, &mut lb);
        for o in lb.into_inner() {
            emit.emit(Either::Right(o));
        }
    }
}

impl<A, B> SnapshottingScan for Fanin<A, B>
where
    A: SnapshottingScan,
    B: SnapshottingScan,
    B: Scan<In = A::In>,
    A::In: Clone,
    A::Snapshot: Serialize + DeserializeOwned,
    B::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = FaninSnapshot<A::Snapshot, B::Snapshot>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        FaninSnapshot {
            left: self.left.snapshot(&state.left),
            right: self.right.snapshot(&state.right),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        FaninState {
            left: self.left.restore(snapshot.left),
            right: self.right.restore(snapshot.right),
        }
    }
}

// --- AndThen: ergonomic alias ---

/// Alias for [`Then`] with a constructor-style name (`and_then(left, right)`).
#[inline]
pub fn and_then<A, B>(left: A, right: B) -> Then<A, B>
where
    B: Scan<In = A::Out>,
    A: Scan,
{
    Then { left, right }
}

// --- ArrowExt ---

/// Extension methods for arrow vocabulary on any [`Scan`].
pub trait ArrowScanExt: Scan + Sized {
    fn arr_then<B>(self, right: B) -> Then<Self, B>
    where
        B: Scan<In = Self::Out>,
    {
        Then {
            left: self,
            right,
        }
    }

    fn split<B>(self, right: B) -> Split<Self, B>
    where
        B: Scan<In = Self::In>,
        Self::In: Clone,
    {
        Split {
            left: self,
            right,
        }
    }

    /// Wrap as [`First`]; specify carry type `C` (e.g. `scan.first_carry::<String>()`).
    fn first_carry<C>(self) -> First<Self, C>
    where
        C: Clone,
    {
        First::new(self)
    }

    /// Wrap as [`Second`]; specify carry type `C`.
    fn second_carry<C>(self) -> Second<Self, C>
    where
        C: Clone,
    {
        Second::new(self)
    }
}

impl<S: Scan + Sized> ArrowScanExt for S {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::VecEmitter;

    #[test]
    fn choose_routes_left_right() {
        let c = Choose {
            left: Arr::<_, i32, i32>::new(|x: i32| x + 1),
            right: Arr::<_, i32, i32>::new(|x: i32| x * 2),
        };
        let mut st = c.init();
        let mut e = VecEmitter::new();
        c.step(&mut st, Either::Left(10), &mut e);
        c.step(&mut st, Either::Right(10), &mut e);
        assert_eq!(e.into_inner(), vec![Either::Left(11), Either::Right(20)]);
    }

    #[test]
    fn fanin_emits_both() {
        let f = Fanin {
            left: Arr::<_, i32, i32>::new(|x: i32| x + 1),
            right: Arr::<_, i32, i32>::new(|x: i32| -x),
        };
        let mut st = f.init();
        let mut e = VecEmitter::new();
        f.step(&mut st, 5, &mut e);
        assert_eq!(e.into_inner(), vec![Either::Left(6), Either::Right(-5)]);
    }

    #[test]
    fn first_pairs_carry() {
        let p = First::<_, String>::new(Arr::new(|x: i32| x * 2));
        let mut st = p.init();
        let mut e = VecEmitter::new();
        p.step(&mut st, (3, "hi".to_string()), &mut e);
        assert_eq!(e.into_inner(), vec![(6, "hi".to_string())]);
    }

    #[test]
    fn complex_arrow_pipeline() {
        use crate::combinator::Map;
        let pipe = Map {
            inner: Split {
                left: Arr::<_, i32, i32>::new(|x: i32| x + 1),
                right: Arr::<_, i32, i32>::new(|x: i32| x * 2),
            },
            map: |o| match o {
                SplitOut::A(a) => format!("A{a}"),
                SplitOut::B(b) => format!("B{b}"),
            },
        };
        let mut st = pipe.init();
        let mut e = VecEmitter::new();
        pipe.step(&mut st, 10, &mut e);
        let out = e.into_inner();
        assert!(out.contains(&"A11".to_string()));
        assert!(out.contains(&"B20".to_string()));
    }
}
