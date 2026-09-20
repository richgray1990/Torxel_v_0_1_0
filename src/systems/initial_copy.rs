//! Начальное копирование данных из WriteWorld в ReadWorld.

use bevy::prelude::*;

use crate::voxel::format::WINDOW_CHUNK_COUNT;
use crate::voxel::pool::{ReadWorld, WriteWorld};
use crate::manager::{ChunkManager, SlotState};
use crate::signals::{ComputePhase, ComputePipeline};

/// Флаг завершения начального копирования
#[derive(Resource, Default)]
pub struct InitialCopyDone {
    pub done: bool,
}

/// Система начального копирования
pub fn initial_copy_system(
    mut read_world: ResMut<ReadWorld>,
    write_world: Res<WriteWorld>,
    manager: Res<ChunkManager>,
    mut pipeline: ResMut<ComputePipeline>,
    mut initial_copy: ResMut<InitialCopyDone>,
) {
    
    if initial_copy.done {
        return;
    }

    // ИСПРАВЛЕНИЕ: пропускаем Failed слоты
    let all_ready = (0..WINDOW_CHUNK_COUNT).all(|slot| {
        let state = manager.get_slot_state(slot);
        state == SlotState::Ready || state == SlotState::Failed
    });

    if !all_ready {
        return;
    }

    // ИСПРАВЛЕНИЕ: копируем только Ready слоты
    let mut copied = 0;
    for slot in 0..WINDOW_CHUNK_COUNT {
        if manager.get_slot_state(slot) == SlotState::Ready {
            read_world.0.copy_chunk_from(&write_world.0, slot);
            copied += 1;
        }
    }

    println!("[INITIAL_COPY] DONE! Copied {} chunks from WriteWorld to ReadWorld", copied);

    // Сбрасываем render_notified для всех Ready чанков
    // Чтобы dispatch_loaded_events_system отправил события на следующем кадре
    for slot in 0..WINDOW_CHUNK_COUNT {
        if manager.get_slot_state(slot) == SlotState::Ready {
            manager.metadata[slot].set_render_notified(false);
        }
    }
    
    initial_copy.done = true;
    pipeline.phase = ComputePhase::Computing;
}