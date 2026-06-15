use std::num::NonZeroU64;

#[derive(Debug, Clone)]
pub struct Entry<T> {
    pub timestamp: u64,
    pub value: T,
}

#[derive(Debug, Clone)]
pub struct ExponentialDecayBuffer<T> {
    entries: Vec<Entry<T>>,
    capacity: usize,
    half_life: f32,
    rng: u64,
}

impl<T> ExponentialDecayBuffer<T> {
    pub fn new(capacity: usize, half_life: f32, seed: NonZeroU64) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
            half_life,
            rng: seed.into(),
        }
    }

    pub fn push<E>(&mut self, entry: E)
    where
        E: Into<Entry<T>>,
    {
        if self.entries.len() < self.capacity {
            self.entries.push(entry.into());
            return;
        }

        let i = self.sample_geometric_index();
        let i = i.clamp(0, self.entries.len() - 2);
        self.entries.remove(i + 1);
        self.entries.push(entry.into());
    }

    pub fn find_nearest_before(&self, timestamp: u64) -> Option<&Entry<T>> {
        match self
            .entries
            .binary_search_by_key(&timestamp, |&Entry { timestamp, .. }| timestamp)
        {
            Ok(i) => Some(&self.entries[i]),
            Err(0) => None,
            Err(i) => Some(&self.entries[i - 1]),
        }
    }

    fn sample_geometric_index(&mut self) -> usize {
        let u = self.generate_random_f32();
        let i = u.log(1.0 - 2_f32.ln() / self.half_life);
        i.floor() as usize
    }

    fn generate_random_f32(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;

        (self.rng >> 40) as f32 / (0xFF_FFFF as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl From<u64> for Entry<u64> {
        fn from(value: u64) -> Self {
            Self {
                timestamp: value,
                value,
            }
        }
    }

    const SEED: NonZeroU64 = NonZeroU64::new(0xBABA_ABAB).expect("fits");

    #[test]
    fn test_never_exceeds_capacity() {
        let cap = 20;
        let mut buf = ExponentialDecayBuffer::new(cap, 50.0, SEED);
        for i in 0..1000 {
            buf.push(i);
            assert!(buf.entries.len() <= cap);
        }
    }

    #[test]
    fn test_entries_stay_sorted_by_timestamp() {
        let mut buf = ExponentialDecayBuffer::new(20, 50.0, SEED);
        for i in 0..500 {
            buf.push(i);
            for w in buf.entries.windows(2) {
                assert!(w[0].timestamp < w[1].timestamp);
            }
        }
    }

    #[test]
    fn test_recent_entries_survive_more_than_old() {
        let cap = 50;
        let mut buf = ExponentialDecayBuffer::new(cap, 50.0, SEED);
        for i in 0..1000 {
            buf.push(i);
        }

        assert_eq!(buf.entries.last().unwrap().timestamp, 999);

        let recent_500 = buf.entries.iter().filter(|e| e.timestamp >= 500).count();
        let old_500 = buf.entries.iter().filter(|e| e.timestamp < 500).count();

        // the # of recent should outnumber the past
        assert!(recent_500 > old_500);
    }

    #[test]
    fn test_no_collapse_when_under_capacity() {
        let mut buf = ExponentialDecayBuffer::new(10, 50.0, SEED);
        for i in 0..10 {
            buf.push(i);
        }
        assert_eq!(buf.entries.len(), 10);

        for (i, entry) in buf.entries.iter().enumerate() {
            assert_eq!(entry.timestamp, i as u64);
        }
    }

    #[test]
    fn test_collapse_happens_at_capacity() {
        let mut buf = ExponentialDecayBuffer::new(10, 50.0, SEED);
        for i in 0..10 {
            buf.push(i);
        }
        assert_eq!(buf.entries.len(), 10);

        buf.push(10);
        assert_eq!(buf.entries.len(), 10);

        assert_eq!(buf.entries.last().unwrap().timestamp, 10);
    }

    #[test]
    fn test_deterministic_with_same_seed() {
        let mut buf1 = ExponentialDecayBuffer::new(20, 50.0, SEED);
        let mut buf2 = ExponentialDecayBuffer::new(20, 50.0, SEED);

        for i in 0..500 {
            buf1.push(i);
            buf2.push(i);
        }

        let ts1 = buf1.entries.iter().map(|e| e.timestamp).collect::<Vec<_>>();
        let ts2 = buf2.entries.iter().map(|e| e.timestamp).collect::<Vec<_>>();

        assert_eq!(ts1, ts2);
    }

    #[test]
    fn test_different_seeds_produce_different_results() {
        let mut buf1 = ExponentialDecayBuffer::new(20, 50.0, NonZeroU64::new(1).unwrap());
        let mut buf2 = ExponentialDecayBuffer::new(20, 50.0, NonZeroU64::new(2).unwrap());

        for i in 0..500 {
            buf1.push(i);
            buf2.push(i);
        }

        let ts1 = buf1.entries.iter().map(|e| e.timestamp).collect::<Vec<_>>();
        let ts2 = buf2.entries.iter().map(|e| e.timestamp).collect::<Vec<_>>();

        assert_ne!(ts1, ts2);
    }
}
