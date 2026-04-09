use std::collections::VecDeque;

/// FIFO ring-style buffer: **push** at back, **evict** from front when over `capacity`.
#[derive(Debug, Clone)]
pub struct WindowBuffer<T> {
    capacity: usize,
    inner: VecDeque<T>,
}

impl<T> WindowBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: VecDeque::with_capacity(capacity),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Push `item`. If at capacity, pops front and returns it (for eviction callbacks).
    pub fn push(&mut self, item: T) -> Option<T> {
        let mut evicted = None;
        if self.capacity > 0 && self.inner.len() == self.capacity {
            evicted = self.inner.pop_front();
        }
        if self.capacity > 0 {
            self.inner.push_back(item);
        }
        evicted
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.inner.iter()
    }

    pub fn front(&self) -> Option<&T> {
        self.inner.front()
    }

    pub fn back(&self) -> Option<&T> {
        self.inner.back()
    }
}

impl<T: Clone> WindowBuffer<T> {
    pub fn to_vec(&self) -> Vec<T> {
        self.inner.iter().cloned().collect()
    }
}
