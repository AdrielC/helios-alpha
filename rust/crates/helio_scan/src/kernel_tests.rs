#[cfg(test)]
mod tests {
    use crate::combinator::{Then, ZipInput};
    use crate::control::FlushReason;
    use crate::emit::{Emit, VecEmitter};
    use crate::focus::{Focus, ThenLeft, ThenRight, ZipInputA, ZipInputB};
    use crate::persist::{CheckpointKeyFn, HashMapStore, Persisted, SnapshotStore};
    use crate::runner::Runner;
    use crate::scan::{FlushableScan, Scan, SnapshottingScan};

    #[derive(Debug, Clone, Copy)]
    struct IncU64;

    impl Scan for IncU64 {
        type In = u64;
        type Out = u64;
        type State = u64;

        fn init(&self) -> Self::State {
            0
        }

        fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: Emit<Self::Out>,
        {
            *state += input;
            emit.emit(*state);
        }
    }

    impl FlushableScan for IncU64 {
        type Offset = u64;

        fn flush<E>(
            &self,
            _state: &mut Self::State,
            _signal: FlushReason<Self::Offset>,
            _emit: &mut E,
        ) where
            E: Emit<Self::Out>,
        {
        }
    }

    impl SnapshottingScan for IncU64 {
        type Snapshot = u64;

        fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
            *state
        }

        fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
            snapshot
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct DoubleI32;

    impl Scan for DoubleI32 {
        type In = i32;
        type Out = i32;
        type State = ();

        fn init(&self) -> Self::State {}

        fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: Emit<Self::Out>,
        {
            emit.emit(input * 2);
        }
    }

    #[test]
    fn then_chains() {
        let pipe = Then {
            left: DoubleI32,
            right: DoubleI32,
        };
        let mut st = pipe.init();
        let mut e = VecEmitter::new();
        pipe.step(&mut st, 3, &mut e);
        assert_eq!(e.0, vec![12]);
        let tr = ThenRight;
        let _: &() = tr.get(&st);
        let tl = ThenLeft;
        let _: &() = tl.get(&st);
    }

    #[test]
    fn zip_input_runs_both() {
        let z = ZipInput {
            a: IncU64,
            b: IncU64,
        };
        let mut st = z.init();
        let mut e = VecEmitter::new();
        z.step(&mut st, 2u64, &mut e);
        assert_eq!(e.0.len(), 2);
        let a = ZipInputA;
        let _: &u64 = a.get(&st);
        let b = ZipInputB;
        let _: &u64 = b.get(&st);
    }

    #[test]
    fn persisted_checkpoint_roundtrip() {
        #[derive(Clone)]
        struct KeyU64;
        impl CheckpointKeyFn<u64> for KeyU64 {
            type Key = &'static str;
            fn key_for_offset(&self, _offset: &u64) -> Self::Key {
                "main"
            }
        }

        let inner = IncU64;
        let persisted = Persisted::new(inner, HashMapStore::default(), KeyU64);
        let mut r = Runner::new(persisted);
        let mut e = VecEmitter::new();
        r.step(5u64, &mut e);
        r.flush(FlushReason::Checkpoint(9u64), &mut e);
        let cp = r.machine.store.borrow_mut().get(&"main").unwrap().unwrap();
        assert_eq!(cp.offset, 9);
        r.state = r.machine.restore(cp.snapshot.clone());
        r.step(1u64, &mut e);
        assert_eq!(e.0.last().copied(), Some(6));
    }
}
