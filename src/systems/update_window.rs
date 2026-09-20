//! Обновление активного окна при движении игрока.

use bevy::prelude::*;

use crate::render::camera::CameraController;
use crate::voxel::format::WINDOW_SIDE;
use crate::voxel::format::WorldDimensions;
use crate::io::channels::LoadRequest;
use crate::io::file_format::WorldHeader;
use crate::manager::{ChunkManager, SlotState};
use crate::signals::{ComputePhase, ComputePipeline};

/// Флаг инициализации окна
#[derive(Resource, Default)]
pub struct WindowInitialized {
    pub done: bool,
}

/// Система инициализации окна (один раз при старте).
///
/// Читает позицию спавна из заголовка файла,
/// ставит начальные чанки в очередь загрузки.
pub fn initialize_window_system(
    mut commands: Commands,
    header: Res<WorldHeader>,
    mut controller: ResMut<CameraController>,
    mut manager: ResMut<ChunkManager>,
    mut initialized: ResMut<WindowInitialized>,
    mut pipeline: ResMut<ComputePipeline>,
) {
    if initialized.done {
        return;
    }

    // Позиция спавна из заголовка файла
    let spawn_chunk_x = header.spawn_x / crate::voxel::format::CHUNK_SIDE as i32;
    let spawn_chunk_z = header.spawn_z / crate::voxel::format::CHUNK_SIDE as i32;

    // Устанавливаем центр окна на спавн
    manager.set_window_center(spawn_chunk_x as i64, spawn_chunk_z as i64);

    // Ставим начальные чанки в очередь загрузки
    for dz in 0..WINDOW_SIDE {
        for dx in 0..WINDOW_SIDE {
            let chunk_x = (manager.window_min_x + dx as i64) as usize;
            let chunk_z = (manager.window_min_z + dz as i64) as usize;

            // Нормализуем координаты через тор
            let (norm_x, norm_z) = manager.topology.normalize_chunk(
                chunk_x as i64,
                chunk_z as i64,
            );

            let slot = dz * WINDOW_SIDE + dx;

            manager.load_queue.push(LoadRequest {
                slot,
                chunk_x: norm_x,
                chunk_z: norm_z,
                priority: 0,
            });

            manager.set_slot_state(slot, SlotState::Queued);
            manager.metadata[slot].grid_x = chunk_x as i64;
            manager.metadata[slot].grid_z = chunk_z as i64;
        }
    }

    // Инициализируем размеры мира для тороидальной топологии в рендере
    commands.insert_resource(WorldDimensions::new(header.chunks_x, header.chunks_z));

    // Устанавливаем якорь камеры на позицию спавна из файла
    controller.anchor_x = header.spawn_x as f64;
    controller.anchor_y = 32.0;
    controller.anchor_z = header.spawn_z as f64;

    // Ставим фазу: ждём загрузку и начальное копирование
    pipeline.phase = ComputePhase::AwaitingPostSwap;

    initialized.done = true;
}

/// Система обновления окна при движении игрока.
///
/// Отправляет запросы из очереди загрузки в фоновый воркер.
pub fn update_window_system(
    mut manager: ResMut<ChunkManager>,
    io_manager: Res<crate::io::channels::IoManager>,
    initialized: Res<WindowInitialized>,
) {
    if !initialized.done {
        return;
    }

    // Отправляем запросы загрузки из очереди в воркер
    let requests: Vec<LoadRequest> = manager.load_queue.drain(..).collect();
    for request in requests {
        manager.set_slot_state(request.slot, SlotState::Loading);
        io_manager.queue_load(request);
    }

    // Отправляем запросы сохранения из очереди в воркер
    let save_requests: Vec<crate::io::channels::SaveRequest> = manager.save_queue.drain(..).collect();
    for request in save_requests {
        manager.set_slot_state(request.slot, SlotState::Saving);
        io_manager.queue_save(request);
    }

    // TODO: Отслеживание позиции игрока
    // Если игрок пересёк границу чанка +/- 3:
    // 1. Сдвигаем окно
    // 2. Ставим новые чанки в очередь загрузки
    // 3. Помечаем старые чанки на выгрузку
}