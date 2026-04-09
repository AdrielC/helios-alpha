#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::combinator::Then;
    use crate::control::{Checkpoint, FlushReason};
    use crate::emit::VecEmitter;
    use crate::persist::{CheckpointKeyFn, HashMapStore, Persisted, SnapshotStore};
    use crate::runner::Runner;
    use crate::runners::{run_batch, run_iter, run_receiver, run_slice};
    use crate::scan::{FlushableScan, Scan, SnapshottingScan};
    use crate::{ScanBatchExt, ScanExt};

    #[derive(Debug, Clone, Copy)]
    struct Mul2;

    impl Scan for Mul2 {
        type In = i32;
        type Out = i32;
        type State = ();

        fn init(&self) -> Self::State {}

        fn step<E>(&self, _st: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: crate::emit::Emit<Self::Out>,
        {
            emit.emit(input * 2);
        }
    }

    #[test]
    fn step_batch_matches_sequential_step() {
        let s = Mul2;
        let mut st1 = s.init();
        let mut st2 = s.init();
        let mut e1 = VecEmitter::new();
        let mut e2 = VecEmitter::new();
        for x in [1, 2, 3] {
            s.step(&mut st1, x, &mut e1);
        }
        s.step_batch(&mut st2, [1, 2, 3], &mut e2);
        assert_eq!(e1.0, e2.0);
    }

    #[test]
    fn run_iter_matches_run_slice() {
        let s = Mul2.map(|x| x + 1);
        let mut st_a = s.init();
        let mut st_b = s.init();
        let mut ea = VecEmitter::new();
        let mut eb = VecEmitter::new();
        run_iter(&s, &mut st_a, [1, 2, 3], &mut ea);
        run_slice(&s, &mut st_b, &[1, 2, 3], &mut eb);
        assert_eq!(ea.0, eb.0);
    }

    #[test]
    fn run_iter_matches_run_batch() {
        let s = Mul2.map(|x| x + 1);
        let mut st_a = s.init();
        let mut st_b = s.init();
        let mut ea = VecEmitter::new();
        let mut eb = VecEmitter::new();
        run_iter(&s, &mut st_a, [1, 2, 3], &mut ea);
        run_batch(&s, &mut st_b, [1, 2, 3], &mut eb);
        assert_eq!(ea.0, eb.0);
    }

    #[test]
    fn run_receiver_matches_iterator_order() {
        let pipe = Then {
            left: Mul2,
            right: Mul2,
        };
        let (tx, rx) = mpsc::channel();
        for x in [1_i32, 2, 3] {
            tx.send(x).unwrap();
        }
        drop(tx);

        let mut st_r = pipe.init();
        let mut er = VecEmitter::new();
        run_receiver(&pipe, &mut st_r, &rx, &mut er);

        let mut st_i = pipe.init();
        let mut ei = VecEmitter::new();
        run_iter(&pipe, &mut st_i, [1, 2, 3], &mut ei);

        assert_eq!(er.0, ei.0);
    }

    #[test]
    fn runner_step_batch_matches_step_loop() {
        let mut r1 = Runner::new(Mul2);
        let mut r2 = Runner::new(Mul2);
        let mut e1 = VecEmitter::new();
        let mut e2 = VecEmitter::new();
        for x in [1, 2, 3] {
            r1.step(x, &mut e1);
        }
        r2.step_batch([1, 2, 3], &mut e2);
        assert_eq!(e1.0, e2.0);
    }

    #[derive(Clone)]
    struct Key;
    impl CheckpointKeyFn<u64> for Key {
        type Key = &'static str;
        fn key_for_offset(&self, _o: &u64) -> Self::Key {
            "k"
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Inc;

    impl Scan for Inc {
        type In = u64;
        type Out = u64;
        type State = u64;

        fn init(&self) -> Self::State {
            0
        }

        fn step<E>(&self, st: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: crate::emit::Emit<Self::Out>,
        {
            *st += input;
            emit.emit(*st);
        }
    }

    impl FlushableScan for Inc {
        type Offset = u64;

        fn flush<E>(&self, _st: &mut Self::State, _sig: FlushReason<Self::Offset>, _emit: &mut E)
        where
            E: crate::emit::Emit<Self::Out>,
        {
        }
    }

    impl SnapshottingScan for Inc {
        type Snapshot = u64;

        fn snapshot(&self, st: &Self::State) -> Self::Snapshot {
            *st
        }

        fn restore(&self, snap: Self::Snapshot) -> Self::State {
            snap
        }
    }

    #[test]
    fn checkpoint_replay_matches_continuous() {
        type Store = HashMapStore<&'static str, Checkpoint<u64, u64>>;
        let persisted = Persisted::<Inc, Store, Key, u64>::new(Inc, Store::default(), Key);
        let inputs: Vec<u64> = (1..=20).collect();

        let mut r_full = Runner::new(Persisted::<Inc, Store, Key, u64>::new(
            Inc,
            Store::default(),
            Key,
        ));
        let mut e_full = VecEmitter::new();
        for &x in &inputs {
            r_full.step(x, &mut e_full);
        }

        let mut r = Runner::new(persisted);
        let mut e = VecEmitter::new();
        for &x in inputs.iter().take(10) {
            r.step(x, &mut e);
        }
        r.flush(FlushReason::Checkpoint(99), &mut e);
        let cp = r.machine.store.borrow_mut().get(&"k").unwrap().unwrap();
        let mut r2 = Runner::new(Persisted::<Inc, Store, Key, u64>::new(
            Inc,
            Store::default(),
            Key,
        ));
        r2.state = r2.machine.restore(cp.snapshot.clone());
        for &x in inputs.iter().skip(10) {
            r2.step(x, &mut e);
        }

        assert_eq!(e.0, e_full.0);
    }
}
