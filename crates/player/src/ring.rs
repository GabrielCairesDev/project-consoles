//! Fila de amostras de um produtor (o jogo) para um consumidor (o callback
//! de áudio), sem lock: o callback roda numa thread de tempo real e não
//! pode esperar.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct SampleRing {
    slots: Box<[AtomicU32]>,
    /// Índices crescentes (módulo `usize`); a posição é `índice % capacidade`.
    read: AtomicUsize,
    write: AtomicUsize,
}

impl SampleRing {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            slots: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        let write = self.write.load(Ordering::Acquire);
        let read = self.read.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }

    /// Enfileira o que couber; devolve quantas amostras entraram.
    /// Só pode ser chamado por um único produtor.
    pub fn push(&self, samples: &[f32]) -> usize {
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let free = self.slots.len() - write.wrapping_sub(read);
        let n = samples.len().min(free);
        for (i, &s) in samples[..n].iter().enumerate() {
            let slot = write.wrapping_add(i) % self.slots.len();
            self.slots[slot].store(s.to_bits(), Ordering::Relaxed);
        }
        self.write.store(write.wrapping_add(n), Ordering::Release);
        n
    }

    /// Retira uma amostra. Só pode ser chamado por um único consumidor.
    pub fn pop(&self) -> Option<f32> {
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        let bits = self.slots[read % self.slots.len()].load(Ordering::Relaxed);
        self.read.store(read.wrapping_add(1), Ordering::Release);
        Some(f32::from_bits(bits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_order_and_wraps_around() {
        let ring = SampleRing::new(4);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(ring.pop(), Some(1.0));
        assert_eq!(ring.pop(), Some(2.0));
        assert_eq!(ring.push(&[4.0, 5.0, 6.0]), 3);
        let drained: Vec<f32> = std::iter::from_fn(|| ring.pop()).collect();
        assert_eq!(drained, vec![3.0, 4.0, 5.0, 6.0]);
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn drops_samples_when_full() {
        let ring = SampleRing::new(2);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0]), 2);
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.pop(), Some(1.0));
    }

    #[test]
    fn concurrent_producer_and_consumer_see_every_sample_once() {
        let ring = std::sync::Arc::new(SampleRing::new(64));
        let consumer = {
            let ring = ring.clone();
            std::thread::spawn(move || {
                let mut got = Vec::with_capacity(10_000);
                while got.len() < 10_000 {
                    if let Some(s) = ring.pop() {
                        got.push(s);
                    } else {
                        std::thread::yield_now();
                    }
                }
                got
            })
        };
        let mut next = 0;
        while next < 10_000 {
            let sample = [next as f32];
            if ring.push(&sample) == 1 {
                next += 1;
            } else {
                std::thread::yield_now();
            }
        }
        let got = consumer.join().expect("thread consumidora falhou");
        assert!(got.iter().enumerate().all(|(i, &s)| s == i as f32));
    }
}
