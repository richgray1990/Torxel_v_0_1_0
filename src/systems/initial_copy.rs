//! Начальное копирование данных из WriteWorld в ReadWorld.
//!
//! При старте игры чанки загружаются из файла в WriteWorld.
//! Рендер читает из ReadWorld. Нужно скопировать данные.
//! Указатели не меняем — только копирование.

use bevy::prelude::*;

use crate::voxel::format::POOL_CHUNK_COUNT;
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
    // Работаем только один раз
    if initial_copy.done {
        return;
    }

    // Работаем только если фаза ожидает пост-свап
    if pipeline.phase != ComputePhase::AwaitingPostSwap {
        return;
    }

    // Проверяем, что все чанки загружены
    let all_ready = (0..POOL_CHUNK_COUNT).all(|slot| {
        manager.get_slot_state(slot) == SlotState::Ready
    });

    if !all_ready {
        return;
    }

    // Копируем все чанки из WriteWorld в ReadWorld
    for slot in 0..POOL_CHUNK_COUNT {
        read_world.0.copy_chunk_from(&write_world.0, slot);
    }

    // Начальное копирование завершено
    initial_copy.done = true;
    pipeline.phase = ComputePhase::Computing;
}