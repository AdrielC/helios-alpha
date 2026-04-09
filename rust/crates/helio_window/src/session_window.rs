use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan, VersionedSnapshot,
};
use serde::{Deserialize, Serialize};

/// One observation tagged with its session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSample<T> {
    pub session: SessionDate,
    pub value: T,
}

/// Buffers values per session; emits the **previous** session’s buffer when the session key changes
/// or on terminal flush.
#[derive(Debug, Clone)]
pub struct SessionWindowScan<T> {
    _p: std::marker::PhantomData<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionWindowState<T> {
    pub current: Option<SessionDate>,
    pub buffer: Vec<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionWindowSnapshot<T> {
    pub current: Option<SessionDate>,
    pub buffer: Vec<T>,
}

impl<T: Clone> Default for SessionWindowScan<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> SessionWindowScan<T> {
    pub fn new() -> Self {
        Self {
            _p: std::marker::PhantomData,
        }
    }

    fn emit_session<E: Emit<Vec<T>>>(buffer: &mut Vec<T>, emit: &mut E) {
        if !buffer.is_empty() {
            emit.emit(std::mem::take(buffer));
        }
    }
}

impl<T: Clone> Scan for SessionWindowScan<T> {
    type In = SessionSample<T>;
    type Out = Vec<T>;
    type State = SessionWindowState<T>;

    fn init(&self) -> Self::State {
        SessionWindowState {
            current: None,
            buffer: Vec::new(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match state.current {
            None => {
                state.current = Some(input.session);
                state.buffer.push(input.value);
            }
            Some(cur) if cur == input.session => {
                state.buffer.push(input.value);
            }
            Some(_) => {
                Self::emit_session(&mut state.buffer, emit);
                state.current = Some(input.session);
                state.buffer.push(input.value);
            }
        }
    }
}

impl<T: Clone> FlushableScan for SessionWindowScan<T> {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let end = matches!(
            signal,
            FlushReason::EndOfInput
                | FlushReason::Shutdown
                | FlushReason::Manual
                | FlushReason::SessionClose(_)
        );
        if end {
            Self::emit_session(&mut state.buffer, emit);
            state.current = None;
        }
    }
}

impl<T: Clone + Serialize + for<'de> Deserialize<'de>> SnapshottingScan for SessionWindowScan<T> {
    type Snapshot = SessionWindowSnapshot<T>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        SessionWindowSnapshot {
            current: state.current,
            buffer: state.buffer.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        SessionWindowState {
            current: snapshot.current,
            buffer: snapshot.buffer,
        }
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>> VersionedSnapshot for SessionWindowSnapshot<T> {
    const VERSION: u32 = 1;
}
