//! Взаимодействие с блоками через Aggregate.
//!
//! R — поставить камень сверху на блок под курсором.
//! T — удалить блок под курсором.
//! I — включить/выключить подсветку блока под курсором.
//!
//! Без Arc, без Atomic, без unsafe.
//! Все изменения мира идут только через CellEventQueue.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::window::PrimaryWindow;
use bytemuck::Zeroable;

use crate::manager::ReadWindowManager;
use crate::queues::cell_events::{CellEvent, CellEventQueue};
use crate::render::camera::CameraController;
use crate::voxel::format::{Cell, CHUNK_HEIGHT, CHUNK_SIDE, WorldDimensions};
use crate::voxel::materials::{ids, phases};
use crate::voxel::pool::ReadWorld;

/// Максимальная дальность луча в блоках.
const MAX_RAY_DISTANCE: f64 = 4096.0;

/// Состояние подсветки целевого блока.
#[derive(Resource, Default, Debug)]
pub struct HighlightState {
    /// Включён ли режим подсветки клавишей I.
    pub enabled: bool,

    /// Сущность-маркер. Создаётся лениво при первом включении.
    pub entity: Option<Entity>,

    /// Меши маркера.
    pub mesh: Option<Handle<Mesh>>,

    /// Материал маркера.
    pub material: Option<Handle<StandardMaterial>>,
}

#[derive(Clone, Copy, Debug)]
struct VoxelHit {
    slot: usize,
    chunk_x: usize,
    chunk_z: usize,
    lx: usize,
    ly: usize,
    lz: usize,
    cell_index: usize,
}

/// Создаёт ресурсы подсветки один раз при старте.
pub fn init_highlight_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = create_highlight_cube_mesh(&mut meshes);

    let material = materials.add(StandardMaterial {
        // Ярко-кислотный зелёный.
        base_color: Color::srgb(0.0, 1.0, 0.0),

        // Без света, чтобы цвет был максимально заметным.
        unlit: true,

        ..default()
    });

    commands.insert_resource(HighlightState {
        enabled: false,
        entity: None,
        mesh: Some(mesh),
        material: Some(material),
    });
}

pub fn block_interaction_system(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    controller: Res<CameraController>,
    dimensions: Res<WorldDimensions>,
    read_world: Res<ReadWorld>,
    read_window: Res<ReadWindowManager>,
    mut events: ResMut<CellEventQueue>,
    mut commands: Commands,
    mut highlight: ResMut<HighlightState>,
) {
    let toggle_pressed = keys.just_pressed(KeyCode::KeyI);
    let remove_pressed = keys.just_pressed(KeyCode::KeyT);
    let place_pressed = keys.just_pressed(KeyCode::KeyR);

    // Переключение режима подсветки.
    if toggle_pressed {
        highlight.enabled = !highlight.enabled;

        // При выключении сразу прячем маркер.
        if !highlight.enabled {
            hide_highlight(&mut commands, highlight.entity);
        }
    }

    // Raycast нужен, если:
    // - включена подсветка;
    // - или нажаты R/T.
    let needs_ray = highlight.enabled || remove_pressed || place_pressed;

    if !needs_ray {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        return;
    };

    // Камера локальная: её X/Z живут вокруг нуля.
    // Возвращаем луч в мировые координаты.
    let origin_x = dimensions.wrap_x(controller.anchor_x + ray.origin.x as f64);
    let origin_y = ray.origin.y as f64;
    let origin_z = dimensions.wrap_z(controller.anchor_z + ray.origin.z as f64);

    let dir_x = ray.direction.x as f64;
    let dir_y = ray.direction.y as f64;
    let dir_z = ray.direction.z as f64;

    let hit = voxel_ray_cast(
        origin_x,
        origin_y,
        origin_z,
        dir_x,
        dir_y,
        dir_z,
        MAX_RAY_DISTANCE,
        &read_world,
        &read_window,
    );

    // Подсветка обновляется только если режим включён.
    if highlight.enabled {
        match hit {
            Some(hit) => show_highlight(
                &mut commands,
                &mut highlight,
                &controller,
                &dimensions,
                hit,
            ),
            None => hide_highlight(&mut commands, highlight.entity),
        }
    }

    // Для R/T нужен успешный hit.
    let Some(hit) = hit else {
        return;
    };

    let generation = read_window.generation(hit.slot);

    // T — удалить блок, в который смотрим.
    if remove_pressed {
        let mut cell = Cell::zeroed();
        cell.material_id = ids::AIR;
        cell.phase_state = phases::SOLID;

        events.push(CellEvent::set_cell_expected(
            hit.slot,
            hit.cell_index,
            cell,
            generation,
        ));
    }

    // R — поставить камень сверху над блоком, в который смотрим.
    if place_pressed {
        let place_ly = hit.ly + 1;

        // Вертикальных чанков пока нет, выше потолка ставить некуда.
        if place_ly >= CHUNK_HEIGHT {
            return;
        }

        let place_cell_index = (hit.lz * CHUNK_SIDE + hit.lx) * CHUNK_HEIGHT + place_ly;

        // Ставим только в воздух.
        if read_world.0.get(hit.slot, place_cell_index).material_id != ids::AIR {
            return;
        }

        let mut cell = Cell::zeroed();
        cell.material_id = ids::STONE;
        cell.phase_state = phases::SOLID;

        events.push(CellEvent::set_cell_expected(
            hit.slot,
            place_cell_index,
            cell,
            generation,
        ));
    }
}

fn show_highlight(
    commands: &mut Commands,
    highlight: &mut HighlightState,
    controller: &CameraController,
    dimensions: &WorldDimensions,
    hit: VoxelHit,
) {
    let (Some(mesh), Some(material)) = (highlight.mesh.clone(), highlight.material.clone()) else {
        return;
    };

    let entity = match highlight.entity {
        Some(entity) => entity,
        None => {
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Visibility::Hidden,
                ))
                .id();

            highlight.entity = Some(entity);
            entity
        }
    };

    // Центр блока в мировых координатах.
    let world_x = hit.chunk_x as f64 * CHUNK_SIDE as f64 + hit.lx as f64 + 0.5;
    let world_y = hit.ly as f64 + 0.5;
    let world_z = hit.chunk_z as f64 * CHUNK_SIDE as f64 + hit.lz as f64 + 0.5;

    // Переводим в локальную систему рендера относительно якоря камеры.
    let offset_x = dimensions.normalize_dx(controller.anchor_x, world_x);
    let offset_z = dimensions.normalize_dz(controller.anchor_z, world_z);

    let position = Vec3::new(offset_x as f32, world_y as f32, offset_z as f32);

    // Немного увеличиваем, чтобы маркер не сливался с гранями блока.
    let transform = Transform {
        translation: position,
        scale: Vec3::splat(1.02),
        ..default()
    };

    commands.entity(entity).insert((transform, Visibility::Visible));
}

fn hide_highlight(commands: &mut Commands, entity: Option<Entity>) {
    if let Some(entity) = entity {
        commands.entity(entity).insert(Visibility::Hidden);
    }
}

fn voxel_ray_cast(
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    max_dist: f64,
    read_world: &ReadWorld,
    read_window: &ReadWindowManager,
) -> Option<VoxelHit> {
    let height = CHUNK_HEIGHT as i64;

    let mut vx = ox.floor() as i64;
    let mut vy = oy.floor() as i64;
    let mut vz = oz.floor() as i64;

    // Если луч горизонтальный и уже вне мира по Y — попасть внутрь он не может.
    if dy == 0.0 && (vy < 0 || vy >= height) {
        return None;
    }

    let step_x = signum_i64(dx);
    let step_y = signum_i64(dy);
    let step_z = signum_i64(dz);

    let t_delta_x = if dx != 0.0 { (1.0 / dx).abs() } else { f64::INFINITY };
    let t_delta_y = if dy != 0.0 { (1.0 / dy).abs() } else { f64::INFINITY };
    let t_delta_z = if dz != 0.0 { (1.0 / dz).abs() } else { f64::INFINITY };

    let mut t_max_x = if step_x > 0 {
        ((vx + 1) as f64 - ox) / dx
    } else if step_x < 0 {
        (vx as f64 - ox) / dx
    } else {
        f64::INFINITY
    };

    let mut t_max_y = if step_y > 0 {
        ((vy + 1) as f64 - oy) / dy
    } else if step_y < 0 {
        (vy as f64 - oy) / dy
    } else {
        f64::INFINITY
    };

    let mut t_max_z = if step_z > 0 {
        ((vz + 1) as f64 - oz) / dz
    } else if step_z < 0 {
        (vz as f64 - oz) / dz
    } else {
        f64::INFINITY
    };

    let mut t = 0.0_f64;

    loop {
        if t > max_dist {
            return None;
        }

        // Ниже мира.
        // Если летим вниз или горизонтально — дальше смысла нет.
        // Если летим вверх — ещё можем войти в мир.
        if vy < 0 {
            if step_y <= 0 {
                return None;
            }
        }
        // Выше мира.
        // Если летим вверх или горизонтально — дальше смысла нет.
        // Если летим вниз — ещё можем войти в мир.
        else if vy >= height {
            if step_y >= 0 {
                return None;
            }
        }
        // Внутри мира — проверяем воксель.
        else {
            if let Some(hit) = check_voxel(vx, vy, vz, read_world, read_window) {
                return Some(hit);
            }
        }

        // Идём к следующему вокселю.
        if t_max_x <= t_max_y && t_max_x <= t_max_z {
            t = t_max_x;
            vx += step_x;
            t_max_x += t_delta_x;
        } else if t_max_y <= t_max_x && t_max_y <= t_max_z {
            t = t_max_y;
            vy += step_y;
            t_max_y += t_delta_y;
        } else {
            t = t_max_z;
            vz += step_z;
            t_max_z += t_delta_z;
        }
    }
}

fn check_voxel(
    wx: i64,
    wy: i64,
    wz: i64,
    read_world: &ReadWorld,
    read_window: &ReadWindowManager,
) -> Option<VoxelHit> {
    if wy < 0 || wy >= CHUNK_HEIGHT as i64 {
        return None;
    }

    let chunk_x_raw = wx.div_euclid(CHUNK_SIDE as i64);
    let chunk_z_raw = wz.div_euclid(CHUNK_SIDE as i64);

    let lx = wx.rem_euclid(CHUNK_SIDE as i64) as usize;
    let lz = wz.rem_euclid(CHUNK_SIDE as i64) as usize;
    let ly = wy as usize;

    let (cx, cz) = read_window.normalize_chunk(chunk_x_raw, chunk_z_raw);
    let slot = read_window.slot_for_chunk(cx, cz)?;

    if !read_window.is_ready(slot) {
        return None;
    }

    let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;

    if read_world.0.get(slot, cell_index).material_id == ids::AIR {
        return None;
    }

    Some(VoxelHit {
        slot,
        chunk_x: cx,
        chunk_z: cz,
        lx,
        ly,
        lz,
        cell_index,
    })
}

#[inline(always)]
fn signum_i64(v: f64) -> i64 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

fn create_highlight_cube_mesh(meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(24);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);

    let mut push_face = |verts: [[f32; 3]; 4], normal: [f32; 3]| {
        let base = positions.len() as u32;

        for vertex in verts {
            positions.push(vertex);
            normals.push(normal);
        }

        indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base,
            base + 2,
            base + 3,
        ]);
    };

    // +X
    push_face(
        [
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, 0.5],
        ],
        [1.0, 0.0, 0.0],
    );

    // -X
    push_face(
        [
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, -0.5],
        ],
        [-1.0, 0.0, 0.0],
    );

    // +Y
    push_face(
        [
            [-0.5, 0.5, -0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
        ],
        [0.0, 1.0, 0.0],
    );

    // -Y
    push_face(
        [
            [-0.5, -0.5, 0.5],
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
        ],
        [0.0, -1.0, 0.0],
    );

    // +Z
    push_face(
        [
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, -0.5, 0.5],
        ],
        [0.0, 0.0, 1.0],
    );

    // -Z
    push_face(
        [
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, -0.5, -0.5],
        ],
        [0.0, 0.0, -1.0],
    );

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));

    meshes.add(mesh)
}