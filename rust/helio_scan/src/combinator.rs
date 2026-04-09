use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::control::FlushReason;
use crate::emit::{Emit, FilterMapEmit, MapEmit, VecEmitter, ZipInputOut};
use crate::scan::{FlushableScan, Scan, SnapshottingScan};

// --- Map ---

pub struct Map<S, M> {
    pub inner: S,
    pub map: M,
}

impl<S, M, U> Scan for Map<S, M>
where
    S: Scan,
    M: Fn(S::Out) -> U,
{
    type In = S::In;
    type Out = U;
    type State = S::State;

    fn init(&self) -> Self::State {
        self.inner.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let map = &self.map;
        let mut bridge = MapEmit::new(emit, map);
        self.inner.step(state, input, &mut bridge);
    }
}

impl<S, M, U, O> FlushableScan for Map<S, M>
where
    S: FlushableScan<Offset = O>,
    M: Fn(S::Out) -> U,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<O>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let map = &self.map;
        let mut bridge = MapEmit::new(emit, map);
        self.inner.flush(state, signal, &mut bridge);
    }
}

impl<S, M, U> SnapshottingScan for Map<S, M>
where
    S: SnapshottingScan,
    M: Fn(S::Out) -> U,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.inner.restore(snapshot)
    }
}

// --- FilterMap ---

pub struct FilterMap<S, F> {
    pub inner: S,
    pub f: F,
}

impl<S, F, U> Scan for FilterMap<S, F>
where
    S: Scan,
    F: Fn(S::Out) -> Option<U>,
{
    type In = S::In;
    type Out = U;
    type State = S::State;

    fn init(&self) -> Self::State {
        self.inner.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let f = &self.f;
        let mut bridge = FilterMapEmit::new(emit, f);
        self.inner.step(state, input, &mut bridge);
    }
}

impl<S, F, U, O> FlushableScan for FilterMap<S, F>
where
    S: FlushableScan<Offset = O>,
    F: Fn(S::Out) -> Option<U>,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<O>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let f = &self.f;
        let mut bridge = FilterMapEmit::new(emit, f);
        self.inner.flush(state, signal, &mut bridge);
    }
}

impl<S, F, U> SnapshottingScan for FilterMap<S, F>
where
    S: SnapshottingScan,
    F: Fn(S::Out) -> Option<U>,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.inner.restore(snapshot)
    }
}

// --- Then ---

pub struct Then<A, B> {
    pub left: A,
    pub right: B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThenState<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThenSnapshot<Sa, Sb> {
    pub left: Sa,
    pub right: Sb,
}

impl<A, B> Scan for Then<A, B>
where
    A: Scan,
    B: Scan<In = A::Out>,
{
    type In = A::In;
    type Out = B::Out;
    type State = ThenState<A::State, B::State>;

    fn init(&self) -> Self::State {
        ThenState {
            left: self.left.init(),
            right: self.right.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut bridge = VecEmitter::new();
        self.left.step(&mut state.left, input, &mut bridge);
        for mid in bridge.into_inner() {
            self.right.step(&mut state.right, mid, emit);
        }
    }
}

impl<A, B, O> FlushableScan for Then<A, B>
where
    A: FlushableScan<Offset = O>,
    B: FlushableScan<Offset = O>,
    B: Scan<In = A::Out>,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<O>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut bridge = VecEmitter::new();
        self.left
            .flush(&mut state.left, signal.clone(), &mut bridge);
        for mid in bridge.into_inner() {
            self.right.step(&mut state.right, mid, emit);
        }
        self.right.flush(&mut state.right, signal, emit);
    }
}

impl<A, B> SnapshottingScan for Then<A, B>
where
    A: SnapshottingScan,
    B: SnapshottingScan,
    B: Scan<In = A::Out>,
    A::Snapshot: Serialize + DeserializeOwned,
    B::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = ThenSnapshot<A::Snapshot, B::Snapshot>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        ThenSnapshot {
            left: self.left.snapshot(&state.left),
            right: self.right.snapshot(&state.right),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ThenState {
            left: self.left.restore(snapshot.left),
            right: self.right.restore(snapshot.right),
        }
    }
}

// --- ZipInput ---

pub struct ZipInput<A, B> {
    pub a: A,
    pub b: B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipInputState<Sa, Sb> {
    pub a: Sa,
    pub b: Sb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZipInputSnapshot<Sa, Sb> {
    pub a: Sa,
    pub b: Sb,
}

impl<A, B> Scan for ZipInput<A, B>
where
    A: Scan,
    B: Scan<In = A::In>,
    A::In: Clone,
{
    type In = A::In;
    type Out = ZipInputOut<A::Out, B::Out>;
    type State = ZipInputState<A::State, B::State>;

    fn init(&self) -> Self::State {
        ZipInputState {
            a: self.a.init(),
            b: self.b.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut buf_a = VecEmitter::new();
        self.a.step(&mut state.a, input.clone(), &mut buf_a);
        let mut buf_b = VecEmitter::new();
        self.b.step(&mut state.b, input, &mut buf_b);
        for o in buf_a.into_inner() {
            emit.emit(ZipInputOut::A(o));
        }
        for o in buf_b.into_inner() {
            emit.emit(ZipInputOut::B(o));
        }
    }
}

impl<A, B, O> FlushableScan for ZipInput<A, B>
where
    A: FlushableScan<Offset = O>,
    B: FlushableScan<Offset = O>,
    B: Scan<In = A::In>,
    A::In: Clone,
    O: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<O>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut buf_a = VecEmitter::new();
        self.a.flush(&mut state.a, signal.clone(), &mut buf_a);
        let mut buf_b = VecEmitter::new();
        self.b.flush(&mut state.b, signal, &mut buf_b);
        for o in buf_a.into_inner() {
            emit.emit(ZipInputOut::A(o));
        }
        for o in buf_b.into_inner() {
            emit.emit(ZipInputOut::B(o));
        }
    }
}

impl<A, B> SnapshottingScan for ZipInput<A, B>
where
    A: SnapshottingScan,
    B: SnapshottingScan,
    B: Scan<In = A::In>,
    A::In: Clone,
    A::Snapshot: Serialize + DeserializeOwned,
    B::Snapshot: Serialize + DeserializeOwned,
{
    type Snapshot = ZipInputSnapshot<A::Snapshot, B::Snapshot>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        ZipInputSnapshot {
            a: self.a.snapshot(&state.a),
            b: self.b.snapshot(&state.b),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ZipInputState {
            a: self.a.restore(snapshot.a),
            b: self.b.restore(snapshot.b),
        }
    }
}

// --- Extension helpers ---

pub trait ScanExt: Scan + Sized {
    fn map<M, U>(self, map: M) -> Map<Self, M>
    where
        M: Fn(Self::Out) -> U,
    {
        Map { inner: self, map }
    }

    fn filter_map<F, U>(self, f: F) -> FilterMap<Self, F>
    where
        F: Fn(Self::Out) -> Option<U>,
    {
        FilterMap { inner: self, f }
    }

    fn then<B>(self, right: B) -> Then<Self, B>
    where
        B: Scan<In = Self::Out>,
    {
        Then { left: self, right }
    }
}

impl<S: Scan + Sized> ScanExt for S {}
