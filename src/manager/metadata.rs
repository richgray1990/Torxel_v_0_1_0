//! Метаданные слотов.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use crate::voxel::format::CHUNK_HEIGHT;

/// Состояния слотов
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum SlotState {
    Empty = 0,
    Queued = 1,
    Loading = 2,
    Ready = 3,
    AwaitingSave = 4,
    Saving = 5,
    Failed = 6,
}

/// Метаданные одного слота
pub struct SlotMetadata {
    pub state: AtomicU8,
    pub grid_x: i64,
    pub grid_z: i64,
    pub file_dirty: AtomicBool,
    pub pending_b_dirty: AtomicBool,
    pub is_active: AtomicBool,
    pub is_shadow: AtomicBool,
    pub unload_timer: AtomicU32,

    /// Битовая маска грязных субчанков
    pub dirty_subchunks: AtomicU64,

    // Высотные границы (записываются один раз при загрузке)
    pub min_solid_y: u16,
    pub max_solid_y: u16,
    pub min_liquid_y: u16,
    pub max_liquid_y: u16,
}

impl SlotMetadata {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(SlotState::Empty as u8),
            grid_x: 0,
            grid_z: 0,
            file_dirty: AtomicBool::new(false),
            pending_b_dirty: AtomicBool::new(false),
            is_active: AtomicBool::new(false),
            is_shadow: AtomicBool::new(false),
            unload_timer: AtomicU32::new(0),
            dirty_subchunks: AtomicU64::new(0),
            min_solid_y: CHUNK_HEIGHT as u16,
            max_solid_y: 0,
            min_liquid_y: CHUNK_HEIGHT as u16,
            max_liquid_y: 0,
        }
    }

    #[inline(always)]
    pub fn get_state(&self) -> SlotState {
        match self.state.load(Ordering::Acquire) {
            0 => SlotState::Empty,
            1 => SlotState::Queued,
            2 => SlotState::Loading,
            3 => SlotState::Ready,
            4 => SlotState::AwaitingSave,
            5 => SlotState::Saving,
            _ => SlotState::Failed,
        }
    }

    #[inline(always)]
    pub fn set_state(&self, state: SlotState) {
        self.state.store(state as u8, Ordering::Release);
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn set_active(&self, active: bool) {
        self.is_active.store(active, Ordering::Release);
    }

    #[inline(always)]
    pub fn is_shadow(&self) -> bool {
        self.is_shadow.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn set_shadow(&self, shadow: bool) {
        self.is_shadow.store(shadow, Ordering::Release);
    }

    #[inline(always)]
    pub fn get_unload_timer(&self) -> u32 {
        self.unload_timer.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn set_unload_timer(&self, value: u32) {
        self.unload_timer.store(value, Ordering::Release);
    }

    #[inline(always)]
    pub fn decrement_unload_timer(&self) {
        let current = self.unload_timer.load(Ordering::Acquire);
        if current > 0 {
            self.unload_timer.store(current - 1, Ordering::Release);
        }
    }
    
    // ═══════════════════════════════════════════════════════════
    // Субчанки
    // ═══════════════════════════════════════════════════════════

    /// Помечает субчанк как грязный
    #[inline(always)]
    pub fn mark_subchunk_dirty(&self, subchunk_idx: usize) {
        self.dirty_subchunks.fetch_or(1u64 << subchunk_idx, Ordering::Release);
    }

    /// Возвращает маску грязных субчанков
    #[inline(always)]
    pub fn get_dirty_subchunks(&self) -> u64 {
        self.dirty_subchunks.load(Ordering::Acquire)
    }

    /// Возвращает маску грязных субчанков и сбрасывает её
    #[inline(always)]
    pub fn take_dirty_subchunks(&self) -> u64 {
        self.dirty_subchunks.swap(0, Ordering::AcqRel)
    }

    /// Сбрасывает маску грязных субчанков
    #[inline(always)]
    pub fn clear_dirty_subchunks(&self) {
        self.dirty_subchunks.store(0, Ordering::Release);
    }
}

impl Default for SlotMetadata {
    fn default() -> Self {
        Self::new()
    }
}