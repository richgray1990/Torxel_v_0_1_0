//! Системы рендера воксельного мира.

use bevy::prelude::*;

use crate::manager::ReadWindowManager;
use crate::queues::mesh_queue::{MeshUpdateKind, MeshUpdateQueue};   //-MeshUpdateEvent
use crate::render::camera::CameraController;
use crate::render::mesh_builder::{build_subchunk_mesh, HaloBuf, SubchunkMeshData};
use crate::render::mesh_storage::{MeshStorage, SubchunkKey, SubchunkRenderData, SubchunkRenderState};
use crate::render::entity_pool::ChunkEntityPool;
use crate::voxel::format::{
    //CHUNK_HEIGHT,
    CHUNK_SIDE, SUBCHUNKS_PER_CHUNK, SUBCHUNKS_X, SUBCHUNKS_Y,
    SUBCHUNK_HEIGHT, MeshWorldOrigin, ORIGIN_RESET_THRESHOLD, WorldDimensions,
};
use crate::voxel::pool::ReadWorld;
use crate::systems::perf::{Bucket, PerfStats, PERF_ENABLED};

// Точные трейты вместо `rayon::prelude::*`: в файле уже есть `bevy::prelude::*`,
// и второй глобальный прелюд рискует конфликтом имён.
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator};  //-IntoParallelIterator

/// Общий материал для всех субчанков.
///
/// Цвет приходит через вершинный атрибут `ATTRIBUTE_COLOR`,
/// поэтому нужен один белый материал на весь рендер.
#[derive(Resource)]
pub struct VoxelMaterialHandle(pub Handle<StandardMaterial>);

/// Создаёт общий материал вокселя один раз при старте.
pub fn init_voxel_material_system(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let handle = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        ..Default::default()
    });

    commands.insert_resource(VoxelMaterialHandle(handle));
}

pub fn process_mesh_events_system(
    mut mesh_queue: ResMut<MeshUpdateQueue>,
    mut mesh_storage: ResMut<MeshStorage>,
    mut commands: Commands,
    mut pool: ResMut<ChunkEntityPool>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();

    for event in mesh_queue.events.iter() {
        match event.kind {
            MeshUpdateKind::ChunkLoaded => {
                for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
                    let key = SubchunkKey {
                        chunk_x: event.grid_x,
                        chunk_z: event.grid_z,
                        subchunk_index: sub_idx,
                    };
                    if !mesh_storage.contains(&key) {
                        let data = SubchunkRenderData::new(event.grid_x, event.grid_z, sub_idx);
                        mesh_storage.insert(key, data);
                        // Ключа не было — он в `Empty`, значит ждёт построения.
                        mesh_storage.push_build(key);
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
                            // Переход в `Dirty` только из `Ready`: `Dirty` уже стоит
                            // в очереди, а `Empty`/`Rebuilding` в неё попадут своим
                            // путём. Так дедуп не требует отдельной проверки.
                            if data.state == SubchunkRenderState::Ready {
                                data.state = SubchunkRenderState::Dirty;
                                mesh_storage.push_build(key);
                            }
                        }
                    }
                }
            }

            MeshUpdateKind::ChunkUnloaded => {
                for sub_idx in 0..SUBCHUNKS_PER_CHUNK {
                    let key = SubchunkKey {
                        chunk_x: event.grid_x,
                        chunk_z: event.grid_z,
                        subchunk_index: sub_idx,
                    };
                    
                    if let Some(mut data) = mesh_storage.remove(&key) {
                        if let Some(entity) = data.entity.take() {
                            if let Some(slot) = data.pool_slot {
                                pool.release(slot);
                                // ТОЛЬКО скрываем
                                commands.entity(entity)
                                    .insert(Visibility::Hidden)
                                    .remove::<PooledChunkEntity>();
                                continue;
                            }
                            commands.entity(entity).despawn();
                        }
                    }
                }
            }
        }
    }

    mesh_queue.clear();

    stats.record(Bucket::ProcessMeshEvents, t.elapsed());
}

/// Один субчанк, разрешённый к сборке: слот уже найден и готов.
struct MeshJob {
    key: SubchunkKey,
    slot: usize,
}

/// Буферы, живущие между кадрами (по образцу прежнего `Local<HaloBuf>`).
///
/// `pub` требуется только потому, что тип стоит в сигнатуре `build_meshes_system`
/// (публичная система) — поля и `MeshJob` наружу не выходят.
///
/// Оба `Vec` держат ёмкость: внешний буфер `results` (~1 МБ на 8192 субчанка)
/// между кадрами не переаллоцируется. Вершинные `Vec` внутри `SubchunkMeshData`
/// переиспользовать нельзя — они уходят в `Mesh` через `mem::take`; это не
/// потеря, а перенос единственной аллокации туда, где она всё равно нужна.
///
/// `HaloBuf` здесь не хранится: его держит `map_init` — по одному на воркер.
#[derive(Default)]
pub struct MeshWorkBuffers {
    jobs: Vec<MeshJob>,
    results: Vec<SubchunkMeshData>,
}

pub fn build_meshes_system(
    mut mesh_storage: ResMut<MeshStorage>,
    mut meshes: ResMut<Assets<Mesh>>,
    read_world: Res<ReadWorld>,
    read_window: Res<ReadWindowManager>,
    //dimensions: Res<WorldDimensions>,
    stats: Res<PerfStats>,
    mut work: Local<MeshWorkBuffers>,
) {
    let t = std::time::Instant::now();

    // `jobs` набирается заново каждый кадр, ёмкость сохраняется. `results` НЕ
    // очищаем: очистка дропнула бы элементы, а переиспользовать их нечего —
    // вершины уходят в `Mesh` через `mem::take`. Достаточно дорастить до нужной
    // длины и взять срез `[..n]`.
    work.jobs.clear();

    // ── Фаза 1: разрешение работ (последовательная) ──────────────────────────
    //
    // Слив `pending_build` с компакцией на месте. Отложенные ключи возвращаются
    // в список на позицию `w` и будут взяты на следующем кадре. `mem::take` тут
    // не годится: он подставляет пустой `Vec` и теряет зарезервированную ёмкость.
    let mut r = 0usize;
    let mut w = 0usize;
    let count = mesh_storage.pending_build.len();

    while r < count {
        let key = mesh_storage.pending_build[r];
        r += 1;

        // Единая точка выхода на итерацию: любой `continue` превращается в
        // `break 'item`, чтобы работа с ключом шла ровно один раз.
        'item: {
            // Проверки без удержания `get_mut`: в ветках откладывания ключ пишется
            // обратно в `pending_build`, а это второе заимствование `mesh_storage`.
            let present = match mesh_storage.get(&key) {
                Some(d) => {
                    // Состояние могли сменить извне (выгрузили и загрузили те же
                    // координаты) — тогда работа уже сделана другим путём.
                    d.state == SubchunkRenderState::Empty || d.state == SubchunkRenderState::Dirty
                }
                // Чанк выгружен, пока ключ ждал своей очереди — выбрасываем.
                None => false,
            };

            if !present {
                break 'item;
            }

            let slot = match find_chunk_slot(&read_window, key.chunk_x, key.chunk_z) {
                Some(s) => s,
                // Чанк ещё не опубликован в снапшоте — ключ обязан вернуться в очередь,
                // иначе субчанк не отрисуется никогда.
                None => {
                    mesh_storage.pending_build[w] = key;
                    w += 1;
                    break 'item;
                }
            };

            if !read_window.is_ready(slot) {
                mesh_storage.pending_build[w] = key;
                w += 1;
                break 'item;
            }

            // Пропускаем субчанки вне диапазона содержимого чанка: геометрии там нет,
            // а скан всего чанка стоит ~9 мс на сдвиг окна. Диапазон `None` означает
            // «границы недостоверны» (чанк правили) — тогда сканируем как раньше.
            //
            // Пустой субчанк разбирается здесь же: параллельный участок ему не нужен,
            // а состояние обязано перейти в `Ready`, иначе его будут собирать каждый кадр.
            if let Some((min_y, max_y)) = read_window.content_y_range(slot) {
                let sub_y = (key.subchunk_index / SUBCHUNKS_X % SUBCHUNKS_Y) * SUBCHUNK_HEIGHT;
                let sub_y_end = sub_y + SUBCHUNK_HEIGHT;

                if sub_y > max_y as usize || sub_y_end <= min_y as usize {
                    if let Some(data) = mesh_storage.get_mut(&key) {
                        data.current_mesh = None;
                        data.pending_mesh = None;
                        data.state = SubchunkRenderState::Ready;
                    }
                    break 'item;
                }
            }

            work.jobs.push(MeshJob { key, slot });
        }
    }

    // Отложенные ключи уже сдвинуты к началу списка; хвост отбрасываем без
    // освобождения ёмкости.
    mesh_storage.pending_build.truncate(w);

    // ── Фаза 2: генерация (параллельная) ─────────────────────────────────────
    //
    // `read_world`/`read_window` читаются по `&`, `mesh_storage` здесь недоступен
    // вовсе — гонок нет. `map_init` даёт каждому воркеру свой `HaloBuf`: это и
    // есть пул, которого требовал инвариант 19 при переходе на parallel iter.
    //
    // Расщепляем `work` на два поля один раз: иначе `jobs` (immutable) и
    // `results` (mutable) не разошлись бы — оба идут через `Deref` на `Local`.
    let MeshWorkBuffers { jobs, results } = &mut *work;

    let n = jobs.len();
    if results.len() < n {
        results.resize_with(n, SubchunkMeshData::new);
    }
    let out = &mut results[..n];

    let pool = &read_world.0;
    let window = &read_window;

    let (fill_ns, face_ns) = jobs
        .par_iter()
        .zip(out.par_iter_mut())
        .map_init(HaloBuf::new, |halo, (job, buf)| {
            // Под-метки копятся в локальных `u64` каждого воркера и суммируются
            // `reduce` — никаких атомиков и сырых указателей.
            let mut fill = 0u64;
            let mut face = 0u64;

            build_subchunk_mesh(
                buf,
                halo,
                job.key.subchunk_index,
                pool,
                job.key.chunk_x,
                job.key.chunk_z,
                job.slot,
                window,
                if PERF_ENABLED { Some(&mut fill) } else { None },
                if PERF_ENABLED { Some(&mut face) } else { None },
            );

            (fill, face)
        })
        .reduce(|| (0u64, 0u64), |a, b| (a.0 + b.0, a.1 + b.1));

    // ── Фаза 3: применение (последовательная, детерминированная) ─────────────
    //
    // Порядок = порядок `jobs` = порядок `pending_build`, то есть ровно тот, что
    // был в последовательной версии. `push_swap`/`mark_live` не зависят от
    // планировщика.
    for (job, mesh_data) in jobs.iter().zip(out.iter_mut()) {
        let key = job.key;

        // Единая точка выхода: `MeshInsert` обязан замериться ровно один раз.
        'apply: {
            let t_ins = std::time::Instant::now();

            let Some(data) = mesh_storage.get_mut(&key) else {
                break 'apply;
            };

            // ИСПРАВЛЕНИЕ: Не создаём меш если нет вершин
            if mesh_data.positions.is_empty() {
                // Субчанк пустой — помечаем как Ready без меша. В `live` он не нужен:
                // сущности у него нет и позиционировать нечего.
                data.current_mesh = None;
                data.pending_mesh = None;
                data.state = SubchunkRenderState::Ready;
                break 'apply;
            }

            let mut mesh = Mesh::new(
                bevy::render::mesh::PrimitiveTopology::TriangleList,
                Default::default(),
            );

            // `mem::take` переносит аллокацию в `Mesh` без копирования вершин;
            // буфер остаётся пустым и переиспользуется на следующем кадре.
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, std::mem::take(&mut mesh_data.positions));
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, std::mem::take(&mut mesh_data.normals));
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, std::mem::take(&mut mesh_data.uvs));
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, std::mem::take(&mut mesh_data.colors));
            mesh.insert_indices(bevy::render::mesh::Indices::U32(std::mem::take(
                &mut mesh_data.indices,
            )));

            let mesh_handle = meshes.add(mesh);

            let was_dirty = match data.state {
                SubchunkRenderState::Empty => {
                    data.current_mesh = Some(mesh_handle);
                    data.state = SubchunkRenderState::Ready;
                    false
                }
                SubchunkRenderState::Dirty => {
                    data.pending_mesh = Some(mesh_handle);
                    data.state = SubchunkRenderState::Rebuilding;
                    true
                }
                _ => break 'apply,
            };

            if was_dirty {
                // Перестройка завершится в `swap_meshes_system` — там меш попадёт в живой.
                mesh_storage.push_swap(key);
            } else {
                // Первый меш готов, сущности пока нет — её создаст следующий этап.
                mesh_storage.mark_live(key);
            }

            stats.record(Bucket::MeshInsert, t_ins.elapsed());
        }
    }

    stats.record(Bucket::FillHalo, std::time::Duration::from_nanos(fill_ns));
    stats.record(Bucket::FaceLoop, std::time::Duration::from_nanos(face_ns));
    stats.record(Bucket::BuildMeshes, t.elapsed());
}


pub fn swap_meshes_system(
    mut mesh_storage: ResMut<MeshStorage>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();

    // Слив `pending_swap`. Откладывать нечего: либо меш подменяется, либо ключ
    // мёртв (чанк выгружен) — поэтому список просто очищается целиком.
    let count = mesh_storage.pending_swap.len();

    for i in 0..count {
        let key = mesh_storage.pending_swap[i];

        let data = match mesh_storage.get_mut(&key) {
            Some(d) => d,
            None => continue,
        };

        if data.state != SubchunkRenderState::Rebuilding {
            continue;
        }

        // Удаляем старый меш из Assets
        if let Some(old_mesh) = data.current_mesh.take() {
            meshes.remove(&old_mesh);
        }

        // Заменяем на новый
        let Some(new_mesh) = data.pending_mesh.take() else {
            // Меша нет — субчанк стал пустым. Живым ему быть незачем.
            data.state = SubchunkRenderState::Ready;
            continue;
        };

        // Обновляем компонент Mesh3d на сущности
        if let Some(entity) = data.entity {
            commands.entity(entity).insert(Mesh3d(new_mesh.clone()));
        }
        data.current_mesh = Some(new_mesh);
        data.state = SubchunkRenderState::Ready;
        let _ = data;

        mesh_storage.mark_live(key);
    }

    mesh_storage.pending_swap.clear();

    stats.record(Bucket::SwapMeshes, t.elapsed());
}

pub fn update_mesh_entities_system(
    mut mesh_storage: ResMut<MeshStorage>,
    mut commands: Commands,
    voxel_material: Res<VoxelMaterialHandle>,
    controller: Res<CameraController>,
    dimensions: Res<WorldDimensions>,
    origin: Res<MeshWorldOrigin>,
    mut pool: ResMut<ChunkEntityPool>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();

    if !mesh_storage.live_dirty {
        stats.record(Bucket::UpdateMeshEntities, t.elapsed());
        return;
    }

    mesh_storage.live_dirty = false;

    let mut r = 0usize;
    let mut w = 0usize;
    let count = mesh_storage.live.len();

    let mut cached_chunk_x = i64::MIN;
    let mut cached_chunk_z = i64::MIN;
    let mut cached_position = Vec3::ZERO;

    while r < count {
        let key = mesh_storage.live[r];
        r += 1;

        let (mesh_handle, entity) = {
            let data = match mesh_storage.get_mut(&key) {
                Some(d) => d,
                None => continue,
            };

            let mesh_handle = match &data.current_mesh {
                Some(h) => h.clone(),
                None => {
                    if let Some(entity) = data.entity.take() {
                        if let Some(slot) = data.pool_slot {
                            pool.release(slot);
                            // ТОЛЬКО скрываем
                            commands.entity(entity)
                                .insert(Visibility::Hidden)
                                .remove::<PooledChunkEntity>();
                            data.pool_slot = None;
                        } else {
                            commands.entity(entity).despawn();
                        }
                    }
                    data.in_live = false;
                    continue;
                }
            };

            (mesh_handle, data.entity)
        };

        let position = if key.chunk_x == cached_chunk_x && key.chunk_z == cached_chunk_z {
            cached_position
        } else {
            let base_x = key.chunk_x as f64 * CHUNK_SIDE as f64;
            let base_z = key.chunk_z as f64 * CHUNK_SIDE as f64;

            let copy_x = ((controller.anchor_x - base_x) / dimensions.width).round();
            let copy_z = ((controller.anchor_z - base_z) / dimensions.depth).round();

            let local_x = base_x + dimensions.width * copy_x - origin.x;
            let local_z = base_z + dimensions.depth * copy_z - origin.z;

            cached_chunk_x = key.chunk_x;
            cached_chunk_z = key.chunk_z;
            cached_position = Vec3::new(local_x as f32, 0.0, local_z as f32);
            cached_position
        };

        match entity {
            Some(_) => {}
            None => {
                if let Some(slot) = pool.acquire() {
                    let entity = pool.entity(slot).unwrap();
                    pool.activate(slot);
                    
                    // Перезаписываем компоненты (MeshMaterial3d уже есть)
                    commands.entity(entity).insert((
                        Mesh3d(mesh_handle),
                        Transform::from_translation(position),
                        Visibility::Visible,
                        PooledChunkEntity { slot },
                    ));

                    if let Some(d) = mesh_storage.get_mut(&key) {
                        d.entity = Some(entity);
                        d.pool_slot = Some(slot);
                    }
                } else {
                    error!("Chunk entity pool exhausted!");
                    
                    let entity = commands
                        .spawn((
                            Mesh3d(mesh_handle),
                            MeshMaterial3d(voxel_material.0.clone()),
                            Transform::from_translation(position),
                            Visibility::Visible,
                        ))
                        .id();

                    if let Some(d) = mesh_storage.get_mut(&key) {
                        d.entity = Some(entity);
                        d.pool_slot = None;
                    }
                }
            }
        }

        mesh_storage.live[w] = key;
        w += 1;
    }

    mesh_storage.live.truncate(w);

    stats.record(Bucket::UpdateMeshEntities, t.elapsed());
}

/// Однократная перекладка мира при уходе камеры слишком далеко от начала координат.
///
/// Работает только тогда, когда `|якорь − начало|` превысил
/// [`ORIGIN_RESET_THRESHOLD`]: начало сдвигается на накопленный якорь, а все
/// живые меши и якорь камеры переезжают на ту же величину. Тем самым мир
/// остаётся на месте, а координаты не растут до значений, где `f32` начинает
/// дрожать. Событие редкое (раз в 100 000 блоков пролёта), поэтому простительно
/// потратить на него один долгий кадр.
pub fn reset_world_origin_system(
    mut origin: ResMut<MeshWorldOrigin>,
    mut controller: ResMut<CameraController>,
    mesh_storage: Res<MeshStorage>,
    mut transforms: Query<&mut Transform>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();

    if controller.anchor_x.abs() <= ORIGIN_RESET_THRESHOLD
        && controller.anchor_z.abs() <= ORIGIN_RESET_THRESHOLD
    {
        stats.record(Bucket::ResetOrigin, t.elapsed());
        return;
    }

    let shift_x = controller.anchor_x;
    let shift_z = controller.anchor_z;

    origin.x += shift_x;
    origin.z += shift_z;
    controller.anchor_x = 0.0;
    controller.anchor_z = 0.0;

    // Перекладываем живые меши. Ключ без сущности здесь безвреден: его позиция
    // будет посчитана заново при следующем разборе `live`, где `origin` уже новый.
    for key in mesh_storage.live.iter() {
        let Some(data) = mesh_storage.get(key) else {
            continue;
        };
        let Some(entity) = data.entity else {
            continue;
        };
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation.x -= shift_x as f32;
            transform.translation.z -= shift_z as f32;
        }
    }

    stats.record(Bucket::ResetOrigin, t.elapsed());
}

fn find_chunk_slot(window: &ReadWindowManager, chunk_x: i64, chunk_z: i64) -> Option<usize> {
    let (cx, cz) = window.normalize_chunk(chunk_x, chunk_z);
    window.slot_for_chunk(cx, cz)
}

/// Маркер для pooled сущностей, хранит индекс слота в пуле
#[derive(Component)]
pub struct PooledChunkEntity {
    pub slot: usize,
}