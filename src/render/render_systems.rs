//! Системы рендера воксельного мира.

use bevy::prelude::*;

use crate::manager::ChunkManager;
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue};
use crate::render::camera::CameraController;
use crate::render::mesh_builder::build_subchunk_mesh;
use crate::render::mesh_storage::{MeshStorage, SubchunkKey, SubchunkRenderData, SubchunkRenderState};
use crate::voxel::format::{
    CHUNK_HEIGHT, CHUNK_SIDE, SUBCHUNKS_PER_CHUNK, SUBCHUNKS_X, SUBCHUNKS_Y,
    SUBCHUNK_SIDE, SUBCHUNK_HEIGHT, WorldDimensions,
};
use crate::voxel::pool::ReadWorld;

pub fn process_mesh_events_system(
    mut mesh_queue: ResMut<MeshUpdateQueue>,
    mut mesh_storage: ResMut<MeshStorage>,
    mut commands: Commands,
) {
    for event in mesh_queue.events.iter() {
        match event.kind {
            MeshUpdateKind::ChunkLoaded | MeshUpdateKind::ShadowChunkLoaded => {
                for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
                    let key = SubchunkKey {
                        chunk_x: event.grid_x,
                        chunk_z: event.grid_z,
                        subchunk_index: sub_idx,
                    };
                    if !mesh_storage.contains(&key) {
                        let data = SubchunkRenderData::new(event.grid_x, event.grid_z, sub_idx);
                        mesh_storage.insert(key, data);
                    }
                }
            }

            MeshUpdateKind::ChunkDirty => {
                let dirty_mask = event.dirty_subchunks;
                for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
                    if dirty_mask & (1u64 << sub_idx) != 0 {
                        let key = SubchunkKey {
                            chunk_x: event.grid_x,
                            chunk_z: event.grid_z,
                            subchunk_index: sub_idx,
                        };
                        if let Some(data) = mesh_storage.get_mut(&key) {
                            if data.state == SubchunkRenderState::Ready {
                                data.state = SubchunkRenderState::Dirty;
                            }
                        }
                    }
                }
            }

            MeshUpdateKind::ChunkUnloaded | MeshUpdateKind::ShadowChunkUnloaded => {
                for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
                    let key = SubchunkKey {
                        chunk_x: event.grid_x,
                        chunk_z: event.grid_z,
                        subchunk_index: sub_idx,
                    };
                    if let Some(data) = mesh_storage.remove(&key) {
                        if let Some(entity) = data.entity {
                            commands.entity(entity).despawn();
                        }
                    }
                }
            }
        }
    }

    mesh_queue.clear();
}

pub fn build_meshes_system(
    mut mesh_storage: ResMut<MeshStorage>,
    mut meshes: ResMut<Assets<Mesh>>,
    read_world: Res<ReadWorld>,
    manager: Res<ChunkManager>,
    dimensions: Res<WorldDimensions>,
) {
    let mut to_build: Vec<SubchunkKey> = Vec::new();

    for (key, data) in mesh_storage.subchunks.iter() {
        if data.state == SubchunkRenderState::Empty || data.state == SubchunkRenderState::Dirty {
            to_build.push(*key);
        }
    }

    for key in to_build {
        let data = match mesh_storage.get_mut(&key) {
            Some(d) => d,
            None => continue,
        };

        let slot = match find_chunk_slot(&manager, key.chunk_x, key.chunk_z) {
            Some(s) => s,
            None => continue,
        };

        let mesh_data = build_subchunk_mesh(
            key.subchunk_index,
            &read_world.0,
            slot,
            dimensions.width as i64,
            dimensions.depth as i64,
            &manager,
        );

        // ИСПРАВЛЕНИЕ: Не создаём меш если нет вершин
        if mesh_data.positions.is_empty() {
            // Субчанк пустой — помечаем как Ready без меша
            data.current_mesh = None;
            data.pending_mesh = None;
            data.state = SubchunkRenderState::Ready;
            continue;
        }

        let mut mesh = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            Default::default(),
        );

        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, mesh_data.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, mesh_data.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, mesh_data.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, mesh_data.colors);
        mesh.insert_indices(bevy::render::mesh::Indices::U32(mesh_data.indices));

        let mesh_handle = meshes.add(mesh);

        match data.state {
            SubchunkRenderState::Empty => {
                data.current_mesh = Some(mesh_handle);
                data.state = SubchunkRenderState::Ready;
            }
            SubchunkRenderState::Dirty => {
                data.pending_mesh = Some(mesh_handle);
                data.state = SubchunkRenderState::Rebuilding;
            }
            _ => {}
        }
    }
}

pub fn swap_meshes_system(
    mut mesh_storage: ResMut<MeshStorage>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mut to_swap: Vec<SubchunkKey> = Vec::new();

    for (key, data) in mesh_storage.subchunks.iter() {
        if data.state == SubchunkRenderState::Rebuilding && data.pending_mesh.is_some() {
            to_swap.push(*key);
        }
    }

    for key in to_swap {
        let data = match mesh_storage.get_mut(&key) {
            Some(d) => d,
            None => continue,
        };

        // Удаляем старый меш из Assets
        if let Some(old_mesh) = data.current_mesh.take() {
            meshes.remove(&old_mesh);
        }

        // Заменяем на новый
        if let Some(new_mesh) = data.pending_mesh.take() {
            // Обновляем компонент Mesh3d на сущности
            if let Some(entity) = data.entity {
                commands.entity(entity).insert(Mesh3d(new_mesh.clone()));
            }
            data.current_mesh = Some(new_mesh);
        }
        data.state = SubchunkRenderState::Ready;
    }
}

pub fn update_mesh_entities_system(
    mut mesh_storage: ResMut<MeshStorage>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    controller: Res<CameraController>,
    dimensions: Res<WorldDimensions>,
    mut transforms: Query<&mut Transform>,
) {
    for (key, data) in mesh_storage.subchunks.iter_mut() {
        if data.state != SubchunkRenderState::Ready {
            continue;
        }

        // ИСПРАВЛЕНИЕ: Пропускаем субчанки без меша (пустые)
        let mesh_handle = match &data.current_mesh {
            Some(h) => h.clone(),
            None => {
                // Если была сущность — удаляем
                if let Some(entity) = data.entity.take() {
                    commands.entity(entity).despawn();
                }
                continue;
            }
        };

        let sub_x_offset = (key.subchunk_index % SUBCHUNKS_X) as f32 * SUBCHUNK_SIDE as f32;
        let sub_y_offset = ((key.subchunk_index / SUBCHUNKS_X) % SUBCHUNKS_Y) as f32 * SUBCHUNK_HEIGHT as f32;
        let sub_z_offset = (key.subchunk_index / (SUBCHUNKS_X * SUBCHUNKS_Y)) as f32 * SUBCHUNK_SIDE as f32;

        let world_x = key.chunk_x as f64 * CHUNK_SIDE as f64 + sub_x_offset as f64;
        let world_y = sub_y_offset;
        let world_z = key.chunk_z as f64 * CHUNK_SIDE as f64 + sub_z_offset as f64;

        let offset_x = dimensions.normalize_dx(controller.anchor_x, world_x);
        let offset_z = dimensions.normalize_dz(controller.anchor_z, world_z);

        let position = Vec3::new(offset_x as f32, world_y as f32, offset_z as f32);

        match data.entity {
            Some(entity) => {
                if let Ok(mut transform) = transforms.get_mut(entity) {
                    transform.translation = position;
                }
            }
            None => {
                let material_handle = materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    ..Default::default()
                });

                let entity = commands
                    .spawn((
                        Mesh3d(mesh_handle),
                        MeshMaterial3d(material_handle),
                        Transform::from_translation(position),
                        Visibility::default(),
                    ))
                    .id();
                data.entity = Some(entity);
            }
        }
    }
}

pub fn cleanup_mesh_entities_system(
    mut mesh_storage: ResMut<MeshStorage>,
    mut commands: Commands,
) {
    let mut to_remove: Vec<SubchunkKey> = Vec::new();

    for (key, data) in mesh_storage.subchunks.iter() {
        if data.state == SubchunkRenderState::Unloading {
            to_remove.push(*key);
        }
    }

    for key in to_remove {
        if let Some(data) = mesh_storage.remove(&key) {
            if let Some(entity) = data.entity {
                commands.entity(entity).despawn();
            }
        }
    }
}

fn find_chunk_slot(manager: &ChunkManager, chunk_x: i64, chunk_z: i64) -> Option<usize> {
    for slot in manager.active_window_slots() {
        if let Some((cx, cz)) = manager.slot_to_chunk_coords(slot) {
            if cx == chunk_x && cz == chunk_z {
                return Some(slot);
            }
        }
    }
    None
}