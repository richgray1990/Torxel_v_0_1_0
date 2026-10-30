//! Построение мешей субчанков.

use bevy::prelude::*;

use crate::voxel::format::{
    CHUNK_HEIGHT, CHUNK_SIDE, MATERIAL_OUT_OF_WORLD, SUBCHUNK_HEIGHT, SUBCHUNK_SIDE, SUBCHUNKS_X,
    SUBCHUNKS_Y,
};
use crate::voxel::materials::{MatLut, ids, is_face_visible, mat_lut};
use crate::voxel::pool::ChunkPool;

use crate::manager::ReadWindowManager;
use crate::systems::perf::PERF_ENABLED;

/// Теоретическая верхняя граница вершин в субчанке (все ячейки — отдельные кубы).
/// Не используется для резервирования: реальный субчанк заполнен на доли процента,
/// и резерв по этому потолку стоит ~5 МБ на вызов.
pub const MAX_VERTICES_PER_SUBCHUNK: usize =
    SUBCHUNK_SIDE * SUBCHUNK_HEIGHT * SUBCHUNK_SIDE * 6 * 4;

/// Теоретическая верхняя граница индексов в субчанке. См. `MAX_VERTICES_PER_SUBCHUNK`.
pub const MAX_INDICES_PER_SUBCHUNK: usize = SUBCHUNK_SIDE * SUBCHUNK_HEIGHT * SUBCHUNK_SIDE * 6 * 6;

const FACES: [(i64, i64, i64); 6] = [
    (0, 1, 0),
    (0, -1, 0),
    (1, 0, 0),
    (-1, 0, 0),
    (0, 0, 1),
    (0, 0, -1),
];

const FACE_NORMALS: [[f32; 3]; 6] = [
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

const FACE_VERTICES: [[[f32; 3]; 4]; 6] = [
    [
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
    ], // Верхняя ← ИСПРАВЛЕНО
    [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
    ],
    [
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
    ],
    [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
    ],
];

const FACE_UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

const FACE_INDICES: [u32; 6] = [0, 1, 2, 2, 1, 3];

/// Данные одного слоя меша.
#[derive(Default)]
pub struct MeshLayerData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshLayerData {
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.uvs.clear();
        self.colors.clear();
        self.indices.clear();
    }
}

/// Итоговые данные субчанка, разделённые по типам прозрачности.
pub struct SubchunkMeshData {
    pub opaque: MeshLayerData,
    pub transparent: MeshLayerData,
    pub alpha_test: MeshLayerData,
}

impl SubchunkMeshData {
    pub fn new() -> Self {
        Self {
            opaque: MeshLayerData::default(),
            transparent: MeshLayerData::default(),
            alpha_test: MeshLayerData::default(),
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.opaque.clear();
        self.transparent.clear();
        self.alpha_test.clear();
    }

    #[inline]
    pub fn is_totally_empty(&self) -> bool {
        self.opaque.is_empty() && self.transparent.is_empty() && self.alpha_test.is_empty()
    }
}

impl Default for SubchunkMeshData {
    fn default() -> Self {
        Self::new()
    }
}

/// Сторона halo-буфера по горизонтали: субчанк + по одному блоку с каждой стороны.
pub const HALO_SIDE: usize = SUBCHUNK_SIDE + 2;
/// Высота halo-буфера: субчанк + по одному блоку снизу и сверху.
pub const HALO_HEIGHT: usize = SUBCHUNK_HEIGHT + 2;
/// Полное число ячеек halo (18³ = 5832).
pub const HALO_CELLS: usize = HALO_SIDE * HALO_SIDE * HALO_HEIGHT;

/// Сентинел «непроницаемо»: под `y = 0` грани не рисуются, поэтому сосед должен
/// считаться непрозрачным. В новой таблице `face_visible` незарегистрированный
/// id тоже даёт непрозрачного соседа (`transparent[nb] == false`), но BEDROCK
/// остаётся явным и не зависит от состояния LUT — безопаснее как сентинел.
const HALO_OPAQUE: u8 = ids::BEDROCK;

/// Материалы субчанка вместе с одноблочным кольцом соседей по всем шести граням.
///
/// Заполняется один раз за вызов мешера; горячий цикл читает соседа по индексу
/// без ветвлений, обращений к чужим слотам пула и тороидальных нормализаций.
/// Хранит только `material_id` — цвет/AO соседа мешеру не нужны, а 18³×Cell
/// стоили бы 93 КБ вместо 5.7 КБ.
///
/// Начальное содержимое — `MATERIAL_OUT_OF_WORLD`: незарегистрированный id,
/// который LUT считает непрозрачным (`known[id] == false`, `transparent[id]`
/// остаётся `false`). Это важно для столбцов за пределами активного окна:
/// `fill_halo` их не трогает, и они должны вести себя как «сплошной сосед»,
/// иначе у края окна мешер рисовал стены из воздуха («лоскуты»).
pub struct HaloBuf {
    cells: [u8; HALO_CELLS],
}

impl HaloBuf {
    pub const fn new() -> Self {
        Self {
            cells: [MATERIAL_OUT_OF_WORLD; HALO_CELLS],
        }
    }

    /// Локальные координаты halo: 0..18. Центр субчанка — 1..17.
    #[inline(always)]
    pub fn get(&self, hx: usize, hy: usize, hz: usize) -> u8 {
        self.cells[(hz * HALO_HEIGHT + hy) * HALO_SIDE + hx]
    }

    #[inline(always)]
    pub fn set(&mut self, hx: usize, hy: usize, hz: usize, material_id: u8) {
        self.cells[(hz * HALO_HEIGHT + hy) * HALO_SIDE + hx] = material_id;
    }
}

impl Default for HaloBuf {
    fn default() -> Self {
        Self::new()
    }
}

/// Строит меш субчанка в переданный буфер.
///
/// `mesh_data` переиспользуется между вызовами (`clear()` вместо `new()`): при
/// параллельном вызове каждый воркер держит свой буфер, поэтому аллокаций в
/// установившемся режиме нет.
pub fn build_subchunk_mesh(
    mesh_data: &mut SubchunkMeshData,
    halo: &mut HaloBuf,
    subchunk_index: usize,
    pool: &ChunkPool,
    chunk_x: i64,
    chunk_z: i64,
    _slot: usize,
    window: &ReadWindowManager,
    // Под-метки для `PerfStats`: чтобы разложить пик, замер не должен требовать
    // второго прохода по граням. `None` — замер выключен (см. `PERF_ENABLED`).
    fill_ns: Option<&mut u64>,
    face_ns: Option<&mut u64>,
) {
    mesh_data.clear();

    let sub_x = (subchunk_index % SUBCHUNKS_X) * SUBCHUNK_SIDE;
    let sub_y = ((subchunk_index / SUBCHUNKS_X) % SUBCHUNKS_Y) * SUBCHUNK_HEIGHT;
    let sub_z = (subchunk_index / (SUBCHUNKS_X * SUBCHUNKS_Y)) * SUBCHUNK_SIDE;

    let fill_elapsed = if PERF_ENABLED {
        let t = std::time::Instant::now();
        fill_halo(
            halo, pool, _slot, chunk_x, chunk_z, sub_x, sub_y, sub_z, window,
        );
        t.elapsed()
    } else {
        fill_halo(
            halo, pool, _slot, chunk_x, chunk_z, sub_x, sub_y, sub_z, window,
        );
        std::time::Duration::ZERO
    };

    let t_face = if PERF_ENABLED {
        Some(std::time::Instant::now())
    } else {
        None
    };

    let sub_y_end = sub_y + SUBCHUNK_HEIGHT;

    // LUT материалов: цвет/opacity/transparent — прямой доступ по id вместо
    // линейного поиска по MATERIALS на каждый блок и каждую грань.
    let lut: &MatLut = mat_lut();

    for lz in sub_z..sub_z + SUBCHUNK_SIDE {
        for lx in sub_x..sub_x + SUBCHUNK_SIDE {
            // Локальные координаты внутри субчанка и соответствующие им +1 в halo.
            let hx = lx - sub_x + 1;
            let hz = lz - sub_z + 1;

            for ly in sub_y..sub_y_end {
                let hy = ly - sub_y + 1;

                let material_id = halo.get(hx, hy, hz);

                // Двойной guard вместо прежних `== AIR` + `get_material().None`:
                // воздух (самый частый случай) отсекается первой проверкой без
                // обращения к LUT; второй проверяет незарегистрированные id —
                // в реальных мирах он всегда true и предсказуем для CPU.
                if material_id == ids::AIR || !lut.known[material_id as usize] {
                    continue;
                }

                let material_id = material_id as usize;
                let color = lut.color[material_id];
                let a = lut.opacity[material_id];

                // id текущего блока — левый индекс таблицы видимости граней.
                let cur_id = material_id as u8;

                for (face_idx, (dx, dy, dz)) in FACES.iter().enumerate() {
                    let nbx = (hx as i64 + dx) as usize;
                    let nby = (hy as i64 + dy) as usize;
                    let nbz = (hz as i64 + dz) as usize;

                    let nb_id = halo.get(nbx, nby, nbz);
                    let visible = is_face_visible(lut, cur_id, nb_id);

                    if visible {
                        let brightness = 1.0 - (ly as f32 / CHUNK_HEIGHT as f32) * 0.15;
                        let r = color[0] as f32 / 255.0 * brightness;
                        let g = color[1] as f32 / 255.0 * brightness;
                        let b = color[2] as f32 / 255.0 * brightness;

                        let normal = FACE_NORMALS[face_idx];
                        let face_verts = FACE_VERTICES[face_idx];

                        // Явное разделение по трём слоям.
                        let layer = if lut.cutoff[material_id] {
                            &mut mesh_data.alpha_test
                        } else if a < 1.0 {
                            &mut mesh_data.transparent
                        } else {
                            &mut mesh_data.opaque
                        };

                        let base_index = layer.positions.len() as u32;

                        for (v_idx, vertex) in face_verts.iter().enumerate() {
                            layer.positions.push([
                                lx as f32 + vertex[0],
                                ly as f32 + vertex[1],
                                lz as f32 + vertex[2],
                            ]);
                            layer.normals.push(normal);
                            layer.uvs.push(FACE_UVS[v_idx]);
                            layer.colors.push([r, g, b, a]);
                        }

                        for idx in FACE_INDICES.iter() {
                            layer.indices.push(base_index + *idx);
                        }
                    }
                }
            }
        }
    }

    if let Some(slot) = fill_ns {
        *slot += fill_elapsed.as_nanos() as u64;
    }

    if let (Some(slot), Some(t)) = (face_ns, t_face) {
        *slot += t.elapsed().as_nanos() as u64;
    }
    
}

/// Заполняет `halo` блоками субчанка и одноблочным кольцом соседей.
///
/// Центр берётся из своего слота, кольцо по X/Z — из соседних чанков через тор
/// (слота нет → воздух), кольцо по Y — из соседних субчанков того же слота
/// (`SUBCHUNKS_X = SUBCHUNKS_Z = 1`, поэтому горизонтальные координаты субчанка
/// совпадают с чанковыми). Ниже `y = 0` соседа нет — ставим непрозрачный сентинел,
/// выше `CHUNK_HEIGHT` — воздух. Видимость граней решается таблицей
/// Видимость граней решается `is_face_visible` в горячем цикле мешера (см. `materials.rs`).
#[allow(clippy::too_many_arguments)]
fn fill_halo(
    halo: &mut HaloBuf,
    pool: &ChunkPool,
    _slot: usize,
    chunk_x: i64,
    chunk_z: i64,
    sub_x: usize,
    sub_y: usize,
    sub_z: usize,
    window: &ReadWindowManager,
) {
    // Слоты девяти соседних чанков; индекс `dx + dz * 3`, где dx/dz ∈ {-1, 0, 1}.
    let mut neighbor_slots: [Option<usize>; 9] = [None; 9];
    for dz in 0..3i64 {
        for dx in 0..3i64 {
            let (ncx, ncz) = window.normalize_chunk(chunk_x + dx - 1, chunk_z + dz - 1);
            neighbor_slots[(dx + dz * 3) as usize] = window.slot_for_chunk(ncx, ncz);
        }
    }

    // Карта высот здесь не используется намеренно: `column_tops` учитывает только
    // непустые ячейки, но пропуска чтения пула по этой карте ломали меш при
    // жидкостях (вода выше «вершины» колонки становилась воздухом → стены под
    // поверхностью). Обход row-major ниже сохраняет те же адреса источника.

    #[inline(always)]
    const fn row_start(hy: usize, hz: usize) -> usize {
        (hz * HALO_HEIGHT + hy) * HALO_SIDE
    }

    for hz in 0..HALO_SIDE {
        let world_z = (sub_z + hz) as isize - 1;
        let dz = world_z.div_euclid(CHUNK_SIDE as isize);
        let local_z = world_z.rem_euclid(CHUNK_SIDE as isize) as usize;

        // Для каждого hx ∈ 0..18: слот чанка (dx,dz) и индекс колонки внутри
        // него. Совпадают с прежними выражениями для источника, просто вынесены
        // из hy-цикла.
        let mut col_of: [usize; HALO_SIDE] = [0; HALO_SIDE];
        let mut slot_of: [Option<usize>; HALO_SIDE] = [None; HALO_SIDE];
        for hx in 0..HALO_SIDE {
            let world_x = (sub_x + hx) as isize - 1;
            let dx = world_x.div_euclid(CHUNK_SIDE as isize);
            let local_x = world_x.rem_euclid(CHUNK_SIDE as isize) as usize;
            // `dx`/`dz` ∈ {-1, 0, 1}; +1 приводит их к тем же 0..3, что и в таблице выше.
            let ns = neighbor_slots[((dx + 1) + (dz + 1) * 3) as usize];
            slot_of[hx] = ns;
            col_of[hx] = local_z * CHUNK_SIDE + local_x;
        }

        for hy in 0..HALO_HEIGHT {
            // `hy = 0` → `y = sub_y - 1`; `hy = 17` → `y = sub_y + SUBCHUNK_HEIGHT`.
            let y_signed = sub_y as isize + hy as isize - 1;
            let row = &mut halo.cells[row_start(hy, hz)..][..HALO_SIDE];

            if y_signed < 0 {
                // Под `y = 0` соседа нет — грани не рисуются, нужен
                // существующий непрозрачный материал.
                row.fill(HALO_OPAQUE);
                continue;
            }
            if y_signed >= CHUNK_HEIGHT as isize {
                row.fill(ids::AIR);
                continue;
            }
            let y = y_signed as usize;

            for hx in 0..HALO_SIDE {
                row[hx] = match slot_of[hx] {
                    Some(s) => pool.get(s, col_of[hx] * CHUNK_HEIGHT + y).material_id,
                    // Слота соседа нет — это край активного окна. Считаем его
                    // сплошным (`HALO_OPAQUE`), чтобы на границе окна не строились
                    // стены: за краем всё равно ничего не видно, а пересборка при
                    // изменении состава соседей (инвалидация) больше не нужна.
                    //
                    // Раньше здесь был `ids::AIR` (семантика прежней
                    // `is_face_visible`: «соседа нет → грань видна»), и граница
                    // окна рисовалась стеной. Возврат к AIR вернёт стены и
                    // потребует снова включить `invalidate_neighbors`.
                    None => HALO_OPAQUE,
                };
            }
        }
    }
}
