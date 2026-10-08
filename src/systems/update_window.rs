//! Обновление активного окна при движении игрока.
use bevy::prelude::*;

use crate::render::camera::CameraController;
use crate::voxel::format::{CHUNK_SIDE, POOL_CHUNK_COUNT, WINDOW_SIDE, WorldDimensions};
use crate::io::channels::{IoManager, LoadRequest, SaveRequest};
use crate::io::file_format::WorldHeader;
use crate::manager::{ChunkManager, SlotState, UNLOAD_DELAY_FRAMES};
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue, RenderSource};
use crate::signals::{ComputePhase, ComputePipeline, SwapSignal};

/// Флаг инициализации окна.
#[derive(Resource, Default)]
pub struct WindowInitialized {
    pub done: bool,
}

/// Система инициализации окна (один раз при старте).
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

    // Позиция спавна из заголовка файла.
    let spawn_chunk_x = header.spawn_x / CHUNK_SIDE as i32;
    let spawn_chunk_z = header.spawn_z / CHUNK_SIDE as i32;

    // Нормализуем центр спавна через тор.
    let (center_x, center_z) =
        manager
            .topology
            .normalize_chunk(spawn_chunk_x as i64, spawn_chunk_z as i64);

    manager.set_window_center(center_x as i64, center_z as i64);

    // Ставим начальные чанки окна в очередь загрузки.
    for dz in 0..WINDOW_SIDE {
        for dx in 0..WINDOW_SIDE {
            let chunk_x_raw = manager.window_min_x + dx as i64;
            let chunk_z_raw = manager.window_min_z + dz as i64;

            let (nx, nz) = manager.topology.normalize_chunk(chunk_x_raw, chunk_z_raw);

            if let Some(slot) = manager.assign_chunk(nx, nz) {
                manager.load_queue.push(LoadRequest {
                    slot,
                    chunk_x: nx,
                    chunk_z: nz,
                    priority: 0,
                });
            } else {
                println!(
                    "[INIT] WARNING: no free slot for spawn chunk ({}, {})",
                    nx, nz
                );
            }
        }
    }

    // Инициализируем размеры мира для тороидальной топологии в рендере.
    commands.insert_resource(WorldDimensions::new(header.chunks_x, header.chunks_z));

    // Устанавливаем якорь камеры на позицию спавна из файла.
    controller.anchor_x = header.spawn_x as f64;
    controller.anchor_y = 32.0;
    controller.anchor_z = header.spawn_z as f64;

    // Ждём загрузку и начальное копирование.
    pipeline.phase = ComputePhase::AwaitingPostSwap;

    initialized.done = true;

    println!(
        "[INIT] Window centered on spawn chunk ({}, {})",
        center_x, center_z
    );
    println!(
        "[INIT] Camera anchor set to ({}, {}, {})",
        controller.anchor_x, controller.anchor_y, controller.anchor_z
    );
}

/// Система обновления окна при движении игрока.
pub fn update_window_system(
    mut manager: ResMut<ChunkManager>,
    io_manager: Res<IoManager>,
    initialized: Res<WindowInitialized>,
    controller: Res<CameraController>,
    mut mesh_queue: ResMut<MeshUpdateQueue>,
    mut swap_signal: ResMut<SwapSignal>,
) {
    if !initialized.done {
        return;
    }

    // Желаемый центр окна по якорю камеры.
    let desired_x = (controller.anchor_x / CHUNK_SIDE as f64).floor() as i64;
    let desired_z = (controller.anchor_z / CHUNK_SIDE as f64).floor() as i64;

    let (desired_nx, desired_nz) = manager.topology.normalize_chunk(desired_x, desired_z);
    let desired_cx = desired_nx as i64;
    let desired_cz = desired_nz as i64;

    let window_changed =
        desired_cx != manager.window_center_x || desired_cz != manager.window_center_z;

    let mut swap_needed = false;

    if window_changed {
        manager.set_window_center(desired_cx, desired_cz);

        // 1. Деактивируем чанки, которые вышли из нового окна.
        for slot in 0..POOL_CHUNK_COUNT {
            let (is_active, grid_x, grid_z) = {
                let meta = &manager.metadata[slot];
                (meta.is_active, meta.grid_x, meta.grid_z)
            };

            if !is_active || grid_x < 0 || grid_z < 0 {
                continue;
            }

            let chunk_x = grid_x as usize;
            let chunk_z = grid_z as usize;

            if !manager.is_in_window(chunk_x, chunk_z) {
                manager.metadata[slot].is_active = false;
                manager.metadata[slot].unload_timer = UNLOAD_DELAY_FRAMES;
                manager.metadata[slot].render_notified = false;

                mesh_queue.push(MeshUpdateEvent {
                    kind: MeshUpdateKind::ChunkUnloaded,
                    source: RenderSource::Active,
                    slot_index: slot,
                    grid_x,
                    grid_z,
                    dirty_subchunks: 0,
                });

                swap_needed = true;
            }
        }

        // 2. Активируем/запрашиваем чанки нового окна.
        for dz in 0..WINDOW_SIDE {
            for dx in 0..WINDOW_SIDE {
                let chunk_x_raw = manager.window_min_x + dx as i64;
                let chunk_z_raw = manager.window_min_z + dz as i64;

                let (nx, nz) = manager.topology.normalize_chunk(chunk_x_raw, chunk_z_raw);

                if let Some(slot) = manager.slot_for_chunk(nx, nz) {
                    let state = manager.metadata[slot].state;

                    if !manager.metadata[slot].is_active {
                        manager.metadata[slot].is_active = true;
                        manager.metadata[slot].unload_timer = 0;
                        manager.metadata[slot].render_notified = false;
                        swap_needed = true;
                    }

                    if state == SlotState::Failed {
                        manager.metadata[slot].state = SlotState::Queued;
                        manager.metadata[slot].render_notified = false;

                        manager.load_queue.push(LoadRequest {
                            slot,
                            chunk_x: nx,
                            chunk_z: nz,
                            priority: 0,
                        });

                        swap_needed = true;
                    }
                } else if let Some(slot) = manager.assign_chunk(nx, nz) {
                    manager.load_queue.push(LoadRequest {
                        slot,
                        chunk_x: nx,
                        chunk_z: nz,
                        priority: 0,
                    });

                    swap_needed = true;
                } else {
                    println!(
                        "[WINDOW] WARNING: no free slot for chunk ({}, {})",
                        nx, nz
                    );
                }
            }
        }
    }

    if swap_needed {
        swap_signal.request();
    }

    // 3. Отправляем запросы загрузки из очереди в воркер.
    let load_requests: Vec<LoadRequest> = manager.load_queue.drain(..).collect();

    for request in load_requests {
        let (is_active, state) = {
            let meta = &manager.metadata[request.slot];
            (meta.is_active, meta.state)
        };

        // Если чанк успел выйти из окна до отправки запроса — освобождаем слот.
        if !is_active {
            manager.release_slot(request.slot);
            continue;
        }

        if state == SlotState::Queued {
            manager.metadata[request.slot].is_active = true;
            manager.set_slot_state(request.slot, SlotState::Loading);
            io_manager.queue_load(request);
        }
    }

    // 4. Отправляем запросы сохранения из очереди в воркер.
    let save_requests: Vec<SaveRequest> = manager.save_queue.drain(..).collect();

    for request in save_requests {
        let state = manager.metadata[request.slot].state;

        if state == SlotState::AwaitingSave {
            manager.set_slot_state(request.slot, SlotState::Saving);
            io_manager.queue_save(request);
        }
    }
}