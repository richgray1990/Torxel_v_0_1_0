//! Временная система отладки. Удалить после стабилизации рендера.

use bevy::prelude::*;

use crate::manager::{ChunkManager, SlotState};
use crate::queues::mesh_queue::MeshUpdateQueue;
use crate::render::camera::CameraController;
use crate::render::mesh_storage::{MeshStorage, SubchunkRenderState, SubchunkKey, };
use crate::signals::{ComputePhase, ComputePipeline, SwapSignal};
use crate::systems::initial_copy::InitialCopyDone;
use crate::systems::update_window::WindowInitialized;
use crate::voxel::format::{CELLS_PER_CHUNK, CHUNK_HEIGHT, CHUNK_SIDE, WINDOW_CHUNK_COUNT, WINDOW_SIDE, SUBCHUNKS_PER_CHUNK, WorldDimensions};
use crate::voxel::materials::ids;
use crate::voxel::pool::{ReadWorld, WriteWorld};
use crate::voxel::materials::is_transparent;

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
    dimensions: Option<Res<WorldDimensions>>,
    transforms: Query<&Transform>,
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

    // ─── 2. Размеры мира ───────────────────────────────────
    println!("[WORLD_DIMENSIONS]");
    if let Some(dims) = &dimensions {
        println!("  width: {:.1} depth: {:.1} height: {:.1}", dims.width, dims.depth, dims.height);
    } else {
        println!("  NOT INITIALIZED");
    }

    // ─── 3. Состояние конвейера ────────────────────────────
    println!("[PIPELINE]");
    println!("  initialized: {}", initialized.done);
    println!("  initial_copy.done: {}", initial_copy.done);
    println!("  pipeline.phase: {:?}", pipeline.phase);
    println!("  swap_signal.requested: {}", swap_signal.requested);

    // ─── 4. Камера ─────────────────────────────────────────
    println!("[CAMERA]");
    println!("  anchor: ({:.1}, {:.1}, {:.1})", controller.anchor_x, controller.anchor_y, controller.anchor_z);
    println!("  pitch: {:.1}° yaw_index: {} zoom_index: {}",
        controller.pitch.to_degrees(), controller.yaw_index, controller.zoom_index);
    let cam_pos = controller.camera_position();
    println!("  camera_pos: ({:.1}, {:.1}, {:.1})", cam_pos.x, cam_pos.y, cam_pos.z);

    // ─── 5. Очередь событий рендера ────────────────────────
    println!("[MESH_QUEUE] events pending: {}", mesh_queue.events.len());

    // ─── 6. Состояние MeshStorage ──────────────────────────
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

    // ─── 7. Выборочные точки из чанков ─────────────────────
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

    // ─── 8. Подсчёт воздуха и блоков в ReadWorld ───────────
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

    // ─── 9. Детальная отладка чанка (1,1) и соседей ────────
    println!("[DETAILED_CHUNK_DEBUG]");
    
    // Найдём слот для чанка (1,1)
    let target_chunk_x = 1i64;
    let target_chunk_z = 1i64;
    
    // Проверяем чанк (1,1) и всех 8 соседей
    for dz in -1i64..=1 {
        for dx in -1i64..=1 {
            let cx = target_chunk_x + dx;
            let cz = target_chunk_z + dz;
            let (norm_cx, norm_cz) = manager.topology.normalize_chunk(cx, cz);
            
            if let Some(slot) = manager.slot_for_chunk(norm_cx, norm_cz) {
                let meta = &manager.metadata[slot];
                let state = manager.get_slot_state(slot);
                
                println!("  Chunk ({},{}) → norm ({},{}) → slot {} state={:?}",
                    cx, cz, norm_cx, norm_cz, slot, state);
                
                // Послойный срез по Y
                print!("    Y-slices: ");
                for y in [0, 8, 16, 24, 32, 40, 48, 56] {
                    let idx = (CHUNK_SIDE / 2 * CHUNK_SIDE + CHUNK_SIDE / 2) * CHUNK_HEIGHT + y;
                    let mat = read_world.0.get(slot, idx).material_id;
                    print!("y{}={} ", y, mat);
                }
                println!();
                
                // Подсчёт материалов в чанке
                let mut air_count = 0u32;
                let mut solid_count = 0u32;
                for i in 0..CELLS_PER_CHUNK {
                    let mat = read_world.0.get(slot, i).material_id;
                    if mat == ids::AIR {
                        air_count += 1;
                    } else {
                        solid_count += 1;
                    }
                }
                println!("    Materials: air={} solid={}", air_count, solid_count);
                
                // Информация о мешах для этого чанка
                let mut subchunk_mesh_info = Vec::new();
                for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
                    let key = SubchunkKey {
                        chunk_x: norm_cx as i64,
                        chunk_z: norm_cz as i64,
                        subchunk_index: sub_idx,
                    };
                    if let Some(data) = mesh_storage.get(&key) {
                        let has_mesh = data.current_mesh.is_some();
                        let has_entity = data.entity.is_some();
                        let state_str = match data.state {
                            SubchunkRenderState::Empty => "Empty",
                            SubchunkRenderState::Building => "Building",
                            SubchunkRenderState::Ready => "Ready",
                            SubchunkRenderState::Dirty => "Dirty",
                            SubchunkRenderState::Rebuilding => "Rebuilding",
                            SubchunkRenderState::Unloading => "Unloading",
                        };
                        subchunk_mesh_info.push(format!("sub{}:{}(m={},e={})", 
                            sub_idx, state_str, has_mesh, has_entity));
                    }
                }
                if !subchunk_mesh_info.is_empty() {
                    println!("    Meshes: {}", subchunk_mesh_info.join(" "));
                } else {
                    println!("    Meshes: NONE");
                }
            } else {
                println!("  Chunk ({},{}) → norm ({},{}) → NOT IN WINDOW",
                    cx, cz, norm_cx, norm_cz);
            }
        }
    }
    
    // ─── 10. Детальный срез чанка (1,1) по вертикали ───────
    if let Some(slot) = manager.slot_for_chunk(1, 1) {
        println!("[CHUNK_1_1_VERTICAL_SLICE]");
        println!("  Slot: {}", slot);
        
        // Срез по центру чанка (x=8, z=8) для всех Y
        print!("  Center column (x=8,z=8): ");
        for y in 0..CHUNK_HEIGHT {
            let idx = (8 * CHUNK_SIDE + 8) * CHUNK_HEIGHT + y;
            let mat = read_world.0.get(slot, idx).material_id;
            if mat != ids::AIR {
                print!("y{}={} ", y, mat);
            }
        }
        println!();
        
        // Срез по поверхности (y=32) для всех X,Z
        println!("  Surface slice (y=32):");
        for z in 0..CHUNK_SIDE {
            print!("    z={:2}: ", z);
            for x in 0..CHUNK_SIDE {
                let idx = (z * CHUNK_SIDE + x) * CHUNK_HEIGHT + 32;
                let mat = read_world.0.get(slot, idx).material_id;
                print!("{:2} ", mat);
            }
            println!();
        }
        
        // Проверка видимости граней для нескольких блоков
        println!("  Face visibility check (center blocks):");
        for y in [30, 31, 32, 33, 34] {
            let idx = (8 * CHUNK_SIDE + 8) * CHUNK_HEIGHT + y;
            let mat = read_world.0.get(slot, idx).material_id;
            if mat == ids::AIR {
                continue;
            }
            
            // Проверяем все 6 соседей
            let mut faces = String::new();
            for (face_name, dx, dy, dz) in [
                ("+Y", 0, 1, 0), ("-Y", 0, -1, 0),
                ("+X", 1, 0, 0), ("-X", -1, 0, 0),
                ("+Z", 0, 0, 1), ("-Z", 0, 0, -1),
            ] {
                let nx = 8 + dx;
                let ny = y as i64 + dy;
                let nz = 8 + dz;
                
                let visible = if ny < 0 || ny >= CHUNK_HEIGHT as i64 {
                    ny >= CHUNK_HEIGHT as i64
                } else if nx >= 0 && nx < CHUNK_SIDE as i64 && nz >= 0 && nz < CHUNK_SIDE as i64 {
                    let n_idx = (nz as usize * CHUNK_SIDE + nx as usize) * CHUNK_HEIGHT + ny as usize;
                    let n_mat = read_world.0.get(slot, n_idx).material_id;
                    is_transparent(n_mat)
                } else {
                    false // За пределами чанка — не проверяем здесь
                };
                
                faces.push_str(&format!("{}={} ", face_name, if visible { "V" } else { "." }));
            }
            println!("    y={}: mat={} {}", y, mat, faces);
        }
    }

    // ─── 11. Позиции мешей для чанка (1,1) ─────────────────
    if let Some(slot) = manager.slot_for_chunk(1, 1) {
        println!("[MESH_POSITIONS_CHUNK_1_1]");
        for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
            let key = SubchunkKey {
                chunk_x: 1,
                chunk_z: 1,
                subchunk_index: sub_idx,
            };
            if let Some(data) = mesh_storage.get(&key) {
                if let Some(entity) = data.entity {
                    if let Ok(transform) = transforms.get(entity) {
                        println!("  sub{}: pos=({:.1}, {:.1}, {:.1})",
                            sub_idx,
                            transform.translation.x,
                            transform.translation.y,
                            transform.translation.z);
                    }
                }
            }
        }
    }

    // ─── 12. Проверка краёв чанка (1,1) ────────────────────
    if let Some(slot) = manager.slot_for_chunk(1, 1) {
        println!("[CHUNK_1_1_EDGES]");
        
        // Проверяем столбец на краю чанка (x=0, z=8) — граница с чанком (0,1)
        println!("  Edge x=0, z=8 (border with chunk 0,1):");
        for y in [40, 42, 44, 46, 48, 50] {
            let idx = (8 * CHUNK_SIDE + 0) * CHUNK_HEIGHT + y;
            let mat = read_world.0.get(slot, idx).material_id;
            
            // Сосед слева: чанк (0,1), x=15, z=8
            let neighbor_slot = manager.slot_for_chunk(0, 1);
            let n_mat = if let Some(n_slot) = neighbor_slot {
                let n_idx = (8 * CHUNK_SIDE + 15) * CHUNK_HEIGHT + y;
                read_world.0.get(n_slot, n_idx).material_id
            } else {
                999 // нет слота
            };
            
            let visible = n_mat == ids::AIR || is_transparent(n_mat);
            println!("    y={}: self={} neighbor(x=15)={} visible={}",
                y, mat, n_mat, visible);
        }
        
        // Проверяем столбец на краю чанка (x=15, z=8) — граница с чанком (2,1)
        println!("  Edge x=15, z=8 (border with chunk 2,1):");
        for y in [40, 42, 44, 46, 48, 50] {
            let idx = (8 * CHUNK_SIDE + 15) * CHUNK_HEIGHT + y;
            let mat = read_world.0.get(slot, idx).material_id;
            
            // Сосед справа: чанк (2,1), x=0, z=8
            let neighbor_slot = manager.slot_for_chunk(2, 1);
            let n_mat = if let Some(n_slot) = neighbor_slot {
                let n_idx = (8 * CHUNK_SIDE + 0) * CHUNK_HEIGHT + y;
                read_world.0.get(n_slot, n_idx).material_id
            } else {
                999
            };
            
            let visible = n_mat == ids::AIR || is_transparent(n_mat);
            println!("    y={}: self={} neighbor(x=0)={} visible={}",
                y, mat, n_mat, visible);
        }
    }
    
    // ─── 13. Позиции ВСЕХ мешей в окне ─────────────────────
    println!("[ALL_MESH_POSITIONS]");
    for slot in 0..WINDOW_CHUNK_COUNT {
        if manager.get_slot_state(slot) != SlotState::Ready {
            continue;
        }
        let (norm_x, norm_z) = match manager.slot_to_chunk_coords(slot) {
            Some(coords) => coords,
            None => continue,
        };
        
        for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
            let key = SubchunkKey {
                chunk_x: norm_x,
                chunk_z: norm_z,
                subchunk_index: sub_idx,
            };
            if let Some(data) = mesh_storage.get(&key) {
                if let Some(entity) = data.entity {
                    if let Ok(transform) = transforms.get(entity) {
                        println!("  slot={} chunk=({},{}) sub{}: pos=({:.1}, {:.1}, {:.1})",
                            slot, norm_x, norm_z, sub_idx,
                            transform.translation.x,
                            transform.translation.y,
                            transform.translation.z);
                    }
                }
            }
        }
    }

        // ─── 14. Подсчёт sub0 мешей ────────────────────────────
    let mut sub0_with_mesh = 0;
    let mut sub0_without_mesh = 0;
    for (_, data) in mesh_storage.subchunks.iter() {
        if data.subchunk_index == 0 {
            if data.current_mesh.is_some() {
                sub0_with_mesh += 1;
            } else {
                sub0_without_mesh += 1;
            }
        }
    }
    println!("[SUB0_COUNT] with_mesh={} without_mesh={}", sub0_with_mesh, sub0_without_mesh);

    println!("════════════════════════════════════════════\n");
}