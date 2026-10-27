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
use crate::systems::perf::{Bucket, PerfStats};
use crate::voxel::format::{Cell, CHUNK_HEIGHT, CHUNK_SIDE, MeshWorldOrigin, WorldDimensions};
use crate::voxel::materials::{ids};//-, phases
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

    /// Абсолютные мировые координаты блока (для соседа-цели при установке).
    wx: i64,
    wy: i64,
    wz: i64,

    /// Ось, по которой луч вошёл в hit-воксель (0=X, 1=Y, 2=Z), и шаг DDA
    /// по этой оси (+1/-1). Сосед-цель для установки: hit - step * unit_axis.
    entry_axis: u8,
    entry_step: i64,
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
    origin: Res<MeshWorldOrigin>,
    stats: Res<PerfStats>,
    tick_timer: Res<Time>,
    mut action_cooldown: Local<f32>,
) {
    *action_cooldown -= tick_timer.delta_secs();
    let can_repeat = *action_cooldown <= 0.0;

    let key_place_block = KeyCode::KeyR;
    let key_remove_block = KeyCode::KeyT;

    let t = std::time::Instant::now();
    let toggle_pressed = keys.just_pressed(KeyCode::KeyI);
    let remove_pressed = keys.just_pressed(key_remove_block) || (keys.pressed(key_remove_block) && can_repeat);
    let place_pressed = keys.just_pressed(key_place_block) || (keys.pressed(key_place_block) && can_repeat);

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
        stats.record(Bucket::BlockInteraction, t.elapsed());
        return;
    }

    let Ok(window) = windows.single() else {
        stats.record(Bucket::BlockInteraction, t.elapsed());
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        stats.record(Bucket::BlockInteraction, t.elapsed());
        return;
    };

    let Ok((camera, camera_transform)) = cameras.single() else {
        stats.record(Bucket::BlockInteraction, t.elapsed());
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        stats.record(Bucket::BlockInteraction, t.elapsed());
        return;
    };

    // Схема «камера двигается, мир стоит»: Transform камеры (и, соответственно,
    // ray.origin из viewport_to_world) живёт в системе отсчёта мешей —
    // абсолютная позиция минус плавающее начало (`camera_transform_system`:
    // cam_mesh = cam_abs - origin). DDA ищет блоки по абсолютным мировым
    // координатам, поэтому переводим начало луча обратно:
    // world = mesh_local + origin. Якорь здесь НЕ прибавляем: он уже входит
    // в cam_abs, а cam_mesh = cam_abs - origin. Прежняя формула
    // world = anchor + ray.origin добавляла якорь второй раз — ошибка росла
    // линейно со смещением камеры (hit улетал всё дальше при движении).
    // При origin == 0 (старое поведение «мир сдвигается к якорю») эта же
    // формула сводится к world = cam_abs = anchor + local — обратная
    // совместимость сохранена.
    let origin_x = ray.origin.x as f64 + origin.x;
    let origin_y = ray.origin.y as f64;
    let origin_z = ray.origin.z as f64 + origin.z;

    let dir_x = ray.direction.x as f64;
    let dir_y = ray.direction.y as f64;
    let dir_z = ray.direction.z as f64;

    // DDA идёт по абсолютным мировым координатам блоков, а `normalize_block`
    // работает в системе чанковой сетки (rel = abs - min_block). Пока мир не
    // смещён (min_block_* == 0) это одно и то же; расхождение добавим здесь,
    // когда появится offset.
    // Нулевое направление (теоретически невозможно, viewport_to_world
    // нормализует) зациклило бы DDA на первом вокселе — выходим явно.
    if dir_x == 0.0 && dir_y == 0.0 && dir_z == 0.0 {
        stats.record(Bucket::BlockInteraction, t.elapsed());
        return;
    }

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
                &origin,
                hit,
            ),
            None => hide_highlight(&mut commands, highlight.entity),
        }
    }

    // Для R/T нужен успешный hit.
    let Some(hit) = hit else {
        stats.record(Bucket::BlockInteraction, t.elapsed());
        return;
    };

    let generation = read_window.generation(hit.slot);
    
    // Флаг: совершили ли мы реальное действие в этом кадре
    let mut action_performed = false;

    // T — удалить блок, в который смотрим.
    if remove_pressed {
        let mut cell = Cell::zeroed();
        cell.material_id = ids::AIR;
        //cell.phase_state = phases::SOLID;

        events.push(CellEvent::set_cell_expected(
            hit.slot,
            hit.cell_index,
            cell,
            generation,
        ));
        action_performed = true;
    }

    // R — поставить камень в соседнюю клетку по грани попадания. Цель:
    // hit - entry_step * unit_axis(entry_axis), т.е. клетка, из которой луч
    // вошёл в hit-воксель. Ось и шаг фиксируются внутри DDA (Amanatides-Woo)
    // и возвращаются в VoxelHit. Y вне [0..CHUNK_HEIGHT) отсекает
    // normalize_block ниже.
    if place_pressed {
        let (px, py, pz) = match hit.entry_axis {
            0 => (hit.wx - hit.entry_step, hit.wy, hit.wz),
            1 => (hit.wx, hit.wy - hit.entry_step, hit.wz),
            _ => (hit.wx, hit.wy, hit.wz - hit.entry_step),
        };

        // Сосед-цель нормализуем полностью (чанк + локальные координаты) и
        // берём слот по новому чанку: при переходе через край тора hit и цель
        // лежат в разных чанках, а на стыке окна slot из хита мог бы указать
        // на уже вытесненный/другой слот.
        let Some((cx, cz, lx, ly, lz)) = read_window.topology.normalize_block(px, py, pz) else {
            stats.record(Bucket::BlockInteraction, t.elapsed());
            return;
        };

        let Some(slot) = read_window.slot_for_chunk(cx, cz) else {
            stats.record(Bucket::BlockInteraction, t.elapsed());
            return;
        };

        let generation = read_window.generation(slot);
        // Layout чанка y-major: index = (z * CHUNK_SIDE + x) * CHUNK_HEIGHT + y.
        let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;

        // Ставим только в воздух.
        if read_world.0.get(slot, cell_index).material_id == ids::AIR {
            let mut cell = Cell::zeroed();
            cell.material_id = ids::STONE;
            //cell.phase_state = phases::SOLID;

            events.push(CellEvent::set_cell_expected(
                slot,
                cell_index,
                cell,
                generation,
            ));
        }
        action_performed = true;
    }

    if action_performed {
        *action_cooldown = 0.15;
    }

    stats.record(Bucket::BlockInteraction, t.elapsed());
}

fn show_highlight(
    commands: &mut Commands,
    highlight: &mut HighlightState,
    controller: &CameraController,
    dimensions: &WorldDimensions,
    origin: &MeshWorldOrigin,
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

    // Переводим в систему отсчёта мешей: абсолютная координата минус плавающее
    // начало. Ту же формулу использует спавн меша, поэтому маркер не разъедется
    // с блоком. Копию вдоль тора берём ближайшую к якорю — как и у мешей.
    let copy_x = ((controller.anchor_x - world_x) / dimensions.width).round();
    let copy_z = ((controller.anchor_z - world_z) / dimensions.depth).round();

    let local_x = world_x + dimensions.width * copy_x - origin.x;
    let local_z = world_z + dimensions.depth * copy_z - origin.z;

    let position = Vec3::new(local_x as f32, world_y as f32, local_z as f32);

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

    // ── Amanatides–Woo с корректным лицом входа ──────────────────────────
    // Стартовый воксель может лежать ВНУТРИ блока (камера в геометрии, либо
    // начало луча `ray.origin + origin` дало floor() внутрь блока). Тогда
    // лицо входа — не «предыдущая пройденная ось», а граница ячейки, которую
    // луч пересечёт БЛИЖАЙШЕЙ; сосед-цель для установки = воксель за этой
    // границей. Прежняя заглушка (вход сверху по Y) ставила блок на 1..N
    // ячеек мимо цели — тем сильнее, чем глубже камера в толще.
    //
    // `t_enter` — расстояние до первой границы из текущей ячейки по каждой оси
    // (f64::INFINITY, если движения по оси нет), `axis_at_*` — ось шага.
    let frac_x = ox - vx as f64; // ∈ [0,1)
    let frac_y = oy - vy as f64;
    let frac_z = oz - vz as f64;

    let t_enter_x = if step_x > 0 { (1.0 - frac_x) * t_delta_x }
                   else if step_x < 0 { frac_x * t_delta_x }
                   else { f64::INFINITY };
    let t_enter_y = if step_y > 0 { (1.0 - frac_y) * t_delta_y }
                   else if step_y < 0 { frac_y * t_delta_y }
                   else { f64::INFINITY };
    let t_enter_z = if step_z > 0 { (1.0 - frac_z) * t_delta_z }
                   else if step_z < 0 { frac_z * t_delta_z }
                   else { f64::INFINITY };

    // Лицо входа стартового вокселя (используется только если он сразу hit).
    let (mut entry_axis, mut entry_step) = if t_enter_x <= t_enter_y && t_enter_x <= t_enter_z {
        (0u8, step_x)
    } else if t_enter_y <= t_enter_z {
        (1u8, step_y)
    } else {
        (2u8, step_z)
    };
    // По бездвижной оси step = 0 — фолбэк «вход сверху», как раньше.
    if entry_step == 0 {
        entry_axis = 1;
        entry_step = -1;
    }

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
                let mut hit = hit;
                hit.entry_axis = entry_axis;
                hit.entry_step = entry_step;
                return Some(hit);
            }
        }

        // Идём к следующему вокселю; войдём в него через ту же грань.
        if t_max_x <= t_max_y && t_max_x <= t_max_z {
            t = t_max_x;
            vx += step_x;
            t_max_x += t_delta_x;
            entry_axis = 0;
            entry_step = step_x;
        } else if t_max_y <= t_max_x && t_max_y <= t_max_z {
            t = t_max_y;
            vy += step_y;
            t_max_y += t_delta_y;
            entry_axis = 1;
            entry_step = step_y;
        } else {
            t = t_max_z;
            vz += step_z;
            t_max_z += t_delta_z;
            entry_axis = 2;
            entry_step = step_z;
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
        wx,
        wy,
        wz,
        // Заполняется вызывающим DDA (voxel_ray_cast) по последней пройденной оси.
        entry_axis: 1,
        entry_step: -1,
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