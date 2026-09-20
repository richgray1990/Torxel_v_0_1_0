//! Временная система отладки. Удалить после стабилизации рендера.

use bevy::prelude::*;

use crate::manager::{ChunkManager, SlotState};
use crate::queues::mesh_queue::MeshUpdateQueue;
use crate::render::camera::CameraController;
use crate::render::mesh_storage::{MeshStorage, SubchunkRenderState};
use crate::signals::{ComputePhase, ComputePipeline, SwapSignal};
use crate::systems::initial_copy::InitialCopyDone;
use crate::systems::update_window::WindowInitialized;
use crate::voxel::format::{CELLS_PER_CHUNK, CHUNK_HEIGHT, CHUNK_SIDE, WINDOW_CHUNK_COUNT, WINDOW_SIDE};
use crate::voxel::materials::ids;
use crate::voxel::pool::{ReadWorld, WriteWorld};

/// Таймер печати отладки (раз в секунду)
#[derive(Resource)]
pub struct DebugTimer(pub Timer);

impl Default for DebugTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(1.0, TimerMode::Repeating))
    }
}

pub fn debug_report_system(
    time: Res<Time>,
    mut timer: ResMut<DebugTimer>,
    manager: Res<ChunkManager>,
    read_world: Res<ReadWorld>,
    write_world: Res<WriteWorld>,
    mesh_storage: Res<MeshStorage>,
    mesh_queue: Res<MeshUpdateQueue>,
    pipeline: Res<ComputePipeline>,
    swap_signal: Res<SwapSignal>,
    initialized: Res<WindowInitialized>,
    initial_copy: Res<InitialCopyDone>,
    controller: Res<CameraController>,
) {
    timer.0.tick(time.delta());
    if !timer.0.just_finished() {
        return;
    }

    println!("\n═══════════════ DEBUG REPORT ═══════════════");

    // ─── 1. Состояние слотов ───────────────────────────────
    let mut count_empty = 0;
    let mut count_queued = 0;
    let mut count_loading = 0;
    let mut count_ready = 0;
    let mut count_awaiting_save = 0;
    let mut count_saving = 0;
    let mut count_failed = 0;
    let mut count_active = 0;

    for slot in 0..WINDOW_CHUNK_COUNT {
        match manager.get_slot_state(slot) {
            SlotState::Empty => count_empty += 1,
            SlotState::Queued => count_queued += 1,
            SlotState::Loading => count_loading += 1,
            SlotState::Ready => count_ready += 1,
            SlotState::AwaitingSave => count_awaiting_save += 1,
            SlotState::Saving => count_saving += 1,
            SlotState::Failed => count_failed += 1,
        }
        if manager.metadata[slot].is_active() {
            count_active += 1;
        }
    }

    println!("[SLOTS] Window {}x{} = {} slots", WINDOW_SIDE, WINDOW_SIDE, WINDOW_CHUNK_COUNT);
    println!("  Ready: {} | Loading: {} | Queued: {} | Empty: {} | Failed: {}",
        count_ready, count_loading, count_queued, count_empty, count_failed);
    println!("  AwaitingSave: {} | Saving: {} | Active: {}",
        count_awaiting_save, count_saving, count_active);

    // ─── 2. Состояние конвейера ────────────────────────────
    println!("[PIPELINE]");
    println!("  initialized: {}", initialized.done);
    println!("  initial_copy.done: {}", initial_copy.done);
    println!("  pipeline.phase: {:?}", pipeline.phase);
    println!("  swap_signal.requested: {}", swap_signal.requested);

    // ─── 3. Камера ─────────────────────────────────────────
    println!("[CAMERA]");
    println!("  anchor: ({:.1}, {:.1}, {:.1})", controller.anchor_x, controller.anchor_y, controller.anchor_z);
    println!("  pitch: {:.1}° yaw_index: {} zoom_index: {}",
        controller.pitch.to_degrees(), controller.yaw_index, controller.zoom_index);
    let cam_pos = controller.camera_position();
    println!("  camera_pos: ({:.1}, {:.1}, {:.1})", cam_pos.x, cam_pos.y, cam_pos.z);

    // ─── 4. Очередь событий рендера ────────────────────────
    println!("[MESH_QUEUE] events pending: {}", mesh_queue.events.len());

    // ─── 5. Состояние MeshStorage ──────────────────────────
    let mut count_s_empty = 0;
    let mut count_s_building = 0;
    let mut count_s_ready = 0;
    let mut count_s_dirty = 0;
    let mut count_s_rebuilding = 0;
    let mut count_s_unloading = 0;
    let mut count_with_mesh = 0;
    let mut count_with_entity = 0;

    for (_, data) in mesh_storage.subchunks.iter() {
        match data.state {
            SubchunkRenderState::Empty => count_s_empty += 1,
            SubchunkRenderState::Building => count_s_building += 1,
            SubchunkRenderState::Ready => count_s_ready += 1,
            SubchunkRenderState::Dirty => count_s_dirty += 1,
            SubchunkRenderState::Rebuilding => count_s_rebuilding += 1,
            SubchunkRenderState::Unloading => count_s_unloading += 1,
        }
        if data.current_mesh.is_some() {
            count_with_mesh += 1;
        }
        if data.entity.is_some() {
            count_with_entity += 1;
        }
    }

    println!("[MESH_STORAGE] total subchunks: {}", mesh_storage.subchunks.len());
    println!("  Empty: {} | Building: {} | Ready: {} | Dirty: {} | Rebuilding: {} | Unloading: {}",
        count_s_empty, count_s_building, count_s_ready,
        count_s_dirty, count_s_rebuilding, count_s_unloading);
    println!("  with_mesh: {} | with_entity: {}", count_with_mesh, count_with_entity);

    // ─── 6. Выборочные точки из чанков ─────────────────────
    println!("[SAMPLE_CELLS]");
    for slot in [0, 127, 128, 255] {
        if slot >= WINDOW_CHUNK_COUNT {
            continue;
        }
        let meta = &manager.metadata[slot];
        let state = manager.get_slot_state(slot);

        // Читаем из ReadWorld и WriteWorld
        let center_idx = (CHUNK_SIDE / 2 * CHUNK_SIDE + CHUNK_SIDE / 2) * CHUNK_HEIGHT + CHUNK_HEIGHT / 2;
        let surface_idx = (CHUNK_SIDE / 2 * CHUNK_SIDE + CHUNK_SIDE / 2) * CHUNK_HEIGHT + 32;
        let bottom_idx = (CHUNK_SIDE / 2 * CHUNK_SIDE + CHUNK_SIDE / 2) * CHUNK_HEIGHT + 0;

        let read_center = read_world.0.get(slot, center_idx).material_id;
        let read_surface = read_world.0.get(slot, surface_idx).material_id;
        let read_bottom = read_world.0.get(slot, bottom_idx).material_id;

        let write_center = write_world.0.get(slot, center_idx).material_id;
        let write_surface = write_world.0.get(slot, surface_idx).material_id;
        let write_bottom = write_world.0.get(slot, bottom_idx).material_id;

        println!("  slot {} ({},{}) state={:?} active={}",
            slot, meta.grid_x, meta.grid_z, state, meta.is_active());
        println!("    ReadWorld:  center={} surface={} bottom={}",
            read_center, read_surface, read_bottom);
        println!("    WriteWorld: center={} surface={} bottom={}",
            write_center, write_surface, write_bottom);
    }

    // ─── 7. Подсчёт воздуха и блоков в ReadWorld ───────────
    let mut total_air = 0u64;
    let mut total_solid = 0u64;
    for slot in 0..WINDOW_CHUNK_COUNT {
        if manager.get_slot_state(slot) != SlotState::Ready {
            continue;
        }
        for i in 0..CELLS_PER_CHUNK {
            let id = read_world.0.get(slot, i).material_id;
            if id == ids::AIR {
                total_air += 1;
            } else {
                total_solid += 1;
            }
        }
    }
    println!("[READ_WORLD_CONTENT] air={} solid={}", total_air, total_solid);

    println!("════════════════════════════════════════════\n");
}