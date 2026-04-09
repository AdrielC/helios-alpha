use helio_time::WindowSpec;

use crate::agg::EvictingWindowAggregator;
use crate::buffer::WindowBuffer;

/// Combined **spec + ring buffer + eviction-aware aggregator** (sample-count trailing windows).
#[derive(Debug, Clone)]
pub struct WindowState<T, A> {
    spec: WindowSpec,
    buffer: WindowBuffer<T>,
    agg: A,
}

impl<T: Clone, A: EvictingWindowAggregator<T>> WindowState<T, A> {
    pub fn new(spec: WindowSpec, agg: A) -> Option<Self> {
        let cap = spec.sample_capacity()?;
        if cap == 0 {
            return None;
        }
        Some(Self {
            spec,
            buffer: WindowBuffer::new(cap),
            agg,
        })
    }

    pub fn spec(&self) -> WindowSpec {
        self.spec
    }

    pub fn buffer(&self) -> &WindowBuffer<T> {
        &self.buffer
    }

    /// Push `value`; evicted front (if any) is passed to `evict` before `insert` of the new tail.
    pub fn push(&mut self, value: T) {
        if let Some(old) = self.buffer.push(value) {
            self.agg.evict(&old);
        }
        if let Some(v) = self.buffer.back() {
            self.agg.insert(v);
        }
    }

    pub fn summary(&self) -> A::Summary {
        self.agg.snapshot()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.agg.clear();
    }
}

/// **O(n) per snapshot**: keeps values in a ring buffer and applies `fold` to the full slice.
/// Use when the summary is not incrementally evictable.
#[derive(Debug, Clone)]
pub struct FoldWindowState<T, S, F> {
    spec: WindowSpec,
    buf: std::collections::VecDeque<T>,
    fold: F,
    empty_summary: S,
}

impl<T: Clone, S: Clone, F: Fn(&[T]) -> S> FoldWindowState<T, S, F> {
    pub fn new(spec: WindowSpec, empty_summary: S, fold: F) -> Option<Self> {
        let cap = spec.sample_capacity()?;
        if cap == 0 {
            return None;
        }
        Some(Self {
            spec,
            buf: std::collections::VecDeque::with_capacity(cap),
            fold,
            empty_summary,
        })
    }

    pub fn spec(&self) -> WindowSpec {
        self.spec
    }

    pub fn push(&mut self, value: T) {
        let cap = self.spec.sample_capacity().unwrap_or(0);
        if cap == 0 {
            return;
        }
        if self.buf.len() == cap {
            self.buf.pop_front();
        }
        self.buf.push_back(value);
    }

    pub fn summary(&self) -> S {
        if self.buf.is_empty() {
            return self.empty_summary.clone();
        }
        let (a, b) = self.buf.as_slices();
        if b.is_empty() {
            return (self.fold)(a);
        }
        let tmp: Vec<T> = a.iter().chain(b.iter()).cloned().collect();
        (self.fold)(&tmp)
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
}
