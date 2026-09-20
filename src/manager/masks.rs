//! Битовые маски для быстрого управления слотами.

/// Количество бит в одном машинном слове u64
const BITS_PER_WORD: usize = u64::BITS as usize;

/// Максимальное количество слотов в пуле
const MAX_SLOTS: usize = 4096;

/// Количество слов для хранения битов всех слотов
const WORD_COUNT: usize = MAX_SLOTS / BITS_PER_WORD;

/// Битовая маска грязных слотов
pub struct DirtyMask {
    bits: [u64; WORD_COUNT],
}

impl DirtyMask {
    pub fn new() -> Self {
        Self { bits: [0; WORD_COUNT] }
    }

    #[inline(always)]
    pub fn set(&mut self, index: usize) {
        debug_assert!(index < MAX_SLOTS, "index out of range");
        self.bits[index / BITS_PER_WORD] |= 1u64 << (index % BITS_PER_WORD);
    }

    #[inline(always)]
    pub fn is_set(&self, index: usize) -> bool {
        debug_assert!(index < MAX_SLOTS, "index out of range");
        (self.bits[index / BITS_PER_WORD] & (1u64 << (index % BITS_PER_WORD))) != 0
    }

    #[inline(always)]
    pub fn clear(&mut self, index: usize) {
        debug_assert!(index < MAX_SLOTS, "index out of range");
        self.bits[index / BITS_PER_WORD] &= !(1u64 << (index % BITS_PER_WORD));
    }

    #[inline(always)]
    pub fn clear_all(&mut self) {
        self.bits.fill(0);
    }

    pub fn iter_set(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits
            .iter()
            .enumerate()
            .flat_map(|(word_idx, &word)| {
                (0..BITS_PER_WORD).filter_map(move |bit_idx| {
                    if word & (1u64 << bit_idx) != 0 {
                        Some(word_idx * BITS_PER_WORD + bit_idx)
                    } else {
                        None
                    }
                })
            })
    }
}

impl Default for DirtyMask {
    fn default() -> Self {
        Self::new()
    }
}

/// Битовая маска теневых слотов
pub struct ShadowMask {
    bits: [u64; WORD_COUNT],
}

impl ShadowMask {
    pub fn new() -> Self {
        Self { bits: [0; WORD_COUNT] }
    }

    #[inline(always)]
    pub fn set(&mut self, index: usize) {
        debug_assert!(index < MAX_SLOTS, "index out of range");
        self.bits[index / BITS_PER_WORD] |= 1u64 << (index % BITS_PER_WORD);
    }

    #[inline(always)]
    pub fn is_set(&self, index: usize) -> bool {
        debug_assert!(index < MAX_SLOTS, "index out of range");
        (self.bits[index / BITS_PER_WORD] & (1u64 << (index % BITS_PER_WORD))) != 0
    }

    #[inline(always)]
    pub fn clear(&mut self, index: usize) {
        debug_assert!(index < MAX_SLOTS, "index out of range");
        self.bits[index / BITS_PER_WORD] &= !(1u64 << (index % BITS_PER_WORD));
    }

    #[inline(always)]
    pub fn clear_all(&mut self) {
        self.bits.fill(0);
    }

    pub fn iter_set(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits
            .iter()
            .enumerate()
            .flat_map(|(word_idx, &word)| {
                (0..BITS_PER_WORD).filter_map(move |bit_idx| {
                    if word & (1u64 << bit_idx) != 0 {
                        Some(word_idx * BITS_PER_WORD + bit_idx)
                    } else {
                        None
                    }
                })
            })
    }
}

impl Default for ShadowMask {
    fn default() -> Self {
        Self::new()
    }
}