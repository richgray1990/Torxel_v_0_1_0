//! Метаданные слотов.
//!
//! Без Atomic, без скрытой мутабельности.
//! Мутация только через &mut self.

//use crate::voxel::format::CHUNK_HEIGHT;

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

/// Сколько кадров слоту разрешено висеть в `Loading` до принудительного `Failed`.
pub const LOAD_TIMEOUT_FRAMES: u32 = 300;

/// Метаданные одного слота
pub struct SlotMetadata {
    pub state: SlotState,
    pub grid_x: i64,
    pub grid_z: i64,
    pub file_dirty: bool,
    pub pending_b_dirty: bool,
    pub is_active: bool,
    pub unload_timer: u32,

    /// Кадров подряд в состоянии `Loading`. Сбрасывается при входе в `Loading`.
    /// Превышение `LOAD_TIMEOUT_FRAMES` переводит слот в `Failed`.
    pub load_frames: u32,

    /// Битовая маска грязных субчанков
    pub dirty_subchunks: u64,

    /// Флаг: уведомлён ли рендер о загрузке чанка
    pub render_notified: bool,

    pub generation: u64,
}

impl SlotMetadata {
    pub fn new() -> Self {
        Self {
            state: SlotState::Empty,
            grid_x: -1,
            grid_z: -1,
            file_dirty: false,
            pending_b_dirty: false,
            is_active: false,
            unload_timer: 0,
            load_frames: 0,
            dirty_subchunks: 0,
            render_notified: false,
            generation: 0,
        }
    }

    #[inline(always)]
    pub fn get_state(&self) -> SlotState {
        self.state
    }

    #[inline(always)]
    pub fn set_state(&mut self, state: SlotState) {
        self.state = state;
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    #[inline(always)]
    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    #[inline(always)]
    pub fn get_unload_timer(&self) -> u32 {
        self.unload_timer
    }

    #[inline(always)]
    pub fn set_unload_timer(&mut self, value: u32) {
        self.unload_timer = value;
    }

    #[inline(always)]
    pub fn decrement_unload_timer(&mut self) {
        if self.unload_timer > 0 {
            self.unload_timer -= 1;
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Субчанки
    // ═══════════════════════════════════════════════════════════

    /// Помечает субчанк как грязный
    #[inline(always)]
    pub fn mark_subchunk_dirty(&mut self, subchunk_idx: usize) {
        self.dirty_subchunks |= 1u64 << subchunk_idx;
    }

    /// Возвращает маску грязных субчанков
    #[inline(always)]
    pub fn get_dirty_subchunks(&self) -> u64 {
        self.dirty_subchunks
    }

    /// Возвращает маску грязных субчанков и сбрасывает её
    #[inline(always)]
    pub fn take_dirty_subchunks(&mut self) -> u64 {
        let mask = self.dirty_subchunks;
        self.dirty_subchunks = 0;
        mask
    }

    /// Сбрасывает маску грязных субчанков
    #[inline(always)]
    pub fn clear_dirty_subchunks(&mut self) {
        self.dirty_subchunks = 0;
    }

    // ═══════════════════════════════════════════════════════════
    // Уведомление рендера
    // ═══════════════════════════════════════════════════════════

    /// Уведомлён ли рендер о загрузке чанка
    #[inline(always)]
    pub fn is_render_notified(&self) -> bool {
        self.render_notified
    }

    /// Установить флаг уведомления рендера
    #[inline(always)]
    pub fn set_render_notified(&mut self, notified: bool) {
        self.render_notified = notified;
    }
}

impl Default for SlotMetadata {
    fn default() -> Self {
        Self::new()
    }
}