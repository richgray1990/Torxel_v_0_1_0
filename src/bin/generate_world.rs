//! Генератор мира для Torxel с использованием 4D FBM шума для бесшовной тороидальной топологии.
//!
//! Использование:
//! bash //! cargo run --release --bin generate_world -- 50 60 world.bin //!
use bytemuck::Zeroable;
use noise::{Fbm, NoiseFn, OpenSimplex};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;
use torxel::io::file_format::{ChunkHeader, WorldHeader};
use torxel::voxel::format::{CELLS_PER_CHUNK, CHUNK_HEIGHT, CHUNK_SIDE, Cell};
use torxel::voxel::height_map::column_tops;
use torxel::voxel::materials::ids::{AIR, BEDROCK, CLAY, GRASS, STONE, WATER};
// ═══════════════════════════════════════════════════════════
// Константы генерации (каскад карт высот, всё на шумах)
// ═══════════════════════════════════════════════════════════
/// Уровень моря для воды (заполняет AIR ниже этой отметки)
const SEA_LEVEL: usize = 30;
/// Верхняя граница слоя каньонов (низменья с distance-map вырезанием)
const CANYON_TOP: usize = 64;
/// Максимальная высота гор (отдельная карта, множитель до диапазона)
const MOUNTAIN_MAX: usize = 384;
/// Нижняя точка профиля горы (старт «ядра» после нормализации)
const MOUNTAIN_MIN: usize = 64;
/// Верхняя граница пиков перед обрезкой (см. lerp-полигон ниже)
const SPIKE_LERP_MAX: usize = 800;
/// Верхняя граница пика в блоках (после lerp до 800 и обрезки).
const SPIKE_MAX: usize = 512;
/// Масштаб шума для 4D-отображения (общий базовый радиус в 4D-торе)
const NOISE_SCALE: f64 = 1.5;
/// Количество октав для FBM (детализация: 3-6 оптимально)
const FBM_OCTAVES: usize = 4;
/// Стойкость октав (амплитуда каждой следующей)
const FBM_PERSISTENCE: f64 = 0.5;
/// Частота октав (масштаб каждой следующей)
const FBM_LACUNARITY: f64 = 2.0;
/// Множители масштаба шума для отдельных карт (умножают NOISE_SCALE):
/// большие мягкие формы — мелкие масштабы, резкие детали — крупные.
const HILLS_FREQ: f64 = 0.7; // слабохолмистый фундамент (1..10)
const BASE_FREQ: f64 = 1.0; // низменья под каньонами
const CANYON_FREQ: f64 = 2.2; // ridged-шум для каньонов (тонные линии разлома)
const MOUNT_FREQ: f64 = 1.6; // горы (до 384)
const SPIKE_FREQ: f64 = 6.0; // пики: очень редкие, высокие, тонкие
const SPRAY_FREQ: f64 = 9.0; // ВЧ-карта прореживания травы (spray)
const BIOME_FREQ: f64 = 1.1; // НЧ-карта «плодородия» для пятен травы
/// Амплитуды слоёв каскада (в блоках по высоте)
const HILLS_AMP: usize = 9; // холмы: y ∈ [1, 10]
const BASE_AMP: usize = 46; // низменья: y ∈ [CANYON_TOP - BASE_AMP, CANYON_TOP]
/// Порог ridged-маски каньонов: чем ближе шум к 1, тем глубже вырез
const CANYON_THRESHOLD: f64 = 0.62;
/// Глубина выреза каньона (максимум, в блоках)
const CANYON_DEPTH: usize = 40;
/// Доля от фактического максимума карты, после которой начинается «ядро» горы.
/// Выше MOUNT_GATE * max нормализованный профиль растёт к вершине горы.
const MOUNT_GATE: f64 = 0.80;
/// То же для пиков: ядро должно быть крошечным (1-2 башни на мир).
const SPIKE_GATE: f64 = 0.93;
/// Трава: диапазон высот + параметры прореживания
const GRASS_MIN_Y: usize = 12; // ниже — берег/вода, травы нет
const GRASS_MAX_Y: usize = 200; // выше — голый камень
const GRASS_BIOME_GATE: f64 = 0.55; // порог плодородия (НЧ-карта)
const GRASS_SPRAY_GATE: f64 = 0.60; // порог точечного распыления (ВЧ-карта)
/// Береговой пояс: clay вокруг мелководья (в пределах этого дельты уровня воды)
const SHORE_BAND: usize = 3;
fn main() {
    let start_total = Instant::now();
    let args: Vec<String> = std::env::args().collect();

    let chunks_x: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50);
    let chunks_z: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let output_path = args
        .get(3)
        .map(PathBuf::from)
        .unwrap_or(PathBuf::from("world.bin"));

    println!("Generating world: {}x{} chunks", chunks_x, chunks_z);
    println!("Output: {}", output_path.display());

    generate_world(chunks_x, chunks_z, &output_path, &start_total);

    println!("World generation complete! {:?}", start_total.elapsed());
}

fn generate_world(chunks_x: u32, chunks_z: u32, path: &PathBuf, starter: &Instant) {
    let file = File::create(path).expect("Failed to create world file");
    let mut writer = BufWriter::with_capacity(1024 * 1024, file);

    let world_seed: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u32;

    let mut header = WorldHeader::new(chunks_x, chunks_z, CHUNK_HEIGHT as u32).with_spawn(5, 3);

    header.with_world_seed(world_seed);

    writer
        .write_all(bytemuck::bytes_of(&header))
        .expect("Failed to write header");

    // Инициализация генератора шума: отдельный FBM на каждую карту,
    // со своим seed (смещаем базовый seed) — карты статистически независимы.
    let mk_fbm = |seed_off: u32, freq_scale: f64| -> (Fbm<OpenSimplex>, f64) {
        let mut fbm = Fbm::<OpenSimplex>::new(world_seed.wrapping_add(seed_off));
        fbm.octaves = FBM_OCTAVES;
        fbm.persistence = FBM_PERSISTENCE;
        fbm.lacunarity = FBM_LACUNARITY;
        (fbm, NOISE_SCALE * freq_scale)
    };
    let (hills_fbm, hills_s) = mk_fbm(1, HILLS_FREQ);
    let (base_fbm, base_s) = mk_fbm(2, BASE_FREQ);
    let (canyon_fbm, canyon_s) = mk_fbm(3, CANYON_FREQ);
    let (spray_fbm, spray_s) = mk_fbm(6, SPRAY_FREQ);
    let (biome_fbm, biome_s) = mk_fbm(7, BIOME_FREQ);

    let world_width = (chunks_x as usize) * CHUNK_SIDE;
    let world_depth = (chunks_z as usize) * CHUNK_SIDE;
    let two_pi = std::f64::consts::PI * 2.0;

    // ── Подбор карт гор и пиков (схема: новая карта → проверка → ещё) ──
    // Профиль нормализуем по фактическому максимуму шума, поэтому «дотянет ли»
    // гора до потолка зависит только от редкости максимума: считаем глобальный
    // профиль высот один раз, находим экстремумы и выбираем первую карту,
    // у которой пик достаточно выражен, чтобы после нормализации достать
    // до `CHUNK_HEIGHT` (пики) / `MOUNTAIN_MAX` (горы). Время генерации не
    // критично — мир пишется в файл однократно.
    let profile = |fbm: &Fbm<OpenSimplex>, scale: f64, gate: f64| -> (f64, usize) {
        // Возвращает (max_noise, сколько колонок выше gate·max).
        let mut max_n = 0.0f64;
        for cz in 0..chunks_z {
            for cx in 0..chunks_x {
                for lz in 0..CHUNK_SIDE {
                    for lx in 0..CHUNK_SIDE {
                        let gx = (cx as i64 * CHUNK_SIDE as i64 + lx as i64) as f64;
                        let gz = (cz as i64 * CHUNK_SIDE as i64 + lz as i64) as f64;
                        let n = noise01(
                            fbm,
                            torus_point(gx, gz, world_width, world_depth, two_pi, scale),
                        );
                        if n > max_n {
                            max_n = n;
                        }
                    }
                }
            }
        }
        let thr = max_n * gate;
        let mut above = 0usize;
        for cz in 0..chunks_z {
            for cx in 0..chunks_x {
                for lz in 0..CHUNK_SIDE {
                    for lx in 0..CHUNK_SIDE {
                        let gx = (cx as i64 * CHUNK_SIDE as i64 + lx as i64) as f64;
                        let gz = (cz as i64 * CHUNK_SIDE as i64 + lz as i64) as f64;
                        let n = noise01(
                            fbm,
                            torus_point(gx, gz, world_width, world_depth, two_pi, scale),
                        );
                        if n >= thr {
                            above += 1;
                        }
                    }
                }
            }
        }
        (max_n, above)
    };

    // Пики: 1–2 башни на весь мир → доля колонок над порогом ~2/мировых колонок.
    let spike_area_target = (2 * CHUNK_SIDE * CHUNK_SIDE) as usize;
    let mut spike_choice: Option<(Fbm<OpenSimplex>, f64, f64)> = None;
    for attempt in 0..8u32 {
        let (fbm, s) = mk_fbm(5 + attempt, SPIKE_FREQ);
        let (max_n, above) = profile(&fbm, s, SPIKE_GATE);
        println!(
            "spike map #{}: max={:.4}, cols>=gate={} (target~{})",
            attempt, max_n, above, spike_area_target
        );
        if above <= spike_area_target * 4 {
            spike_choice = Some((fbm, s, max_n));
            break;
        }
    }
    let (spike_fbm, spike_s, spike_max) =
        spike_choice.expect("не удалось подобрать карту пиков за 8 попыток");

    // Горы: должны гарантированно доставать до MOUNTAIN_MAX после нормализации;
    // площадь гор грубо 1-3% мира.
    let mount_area_target = (world_width * world_depth) / 50;
    let mut mount_choice: Option<(Fbm<OpenSimplex>, f64, f64)> = None;
    for attempt in 0..8u32 {
        let (fbm, s) = mk_fbm(20 + attempt, MOUNT_FREQ);
        let (max_n, above) = profile(&fbm, s, MOUNT_GATE);
        println!(
            "mountain map #{}: max={:.4}, cols>=gate={} (target~{})",
            attempt, max_n, above, mount_area_target
        );
        if above <= mount_area_target * 4 {
            mount_choice = Some((fbm, s, max_n));
            break;
        }
    }
    let (mount_fbm, mount_s, mount_max) =
        mount_choice.expect("не удалось подобрать карту гор за 8 попыток");

    for cz in 0..chunks_z {
        for cx in 0..chunks_x {
            let (chunk_data, chunk_header) = generate_chunk(
                cx as i64,
                cz as i64,
                world_width,
                world_depth,
                two_pi,
                (&hills_fbm, hills_s),
                (&base_fbm, base_s),
                (&canyon_fbm, canyon_s),
                (&mount_fbm, mount_s, mount_max),
                (&spike_fbm, spike_s, spike_max),
                (&spray_fbm, spray_s),
                (&biome_fbm, biome_s),
            );

            writer
                .write_all(bytemuck::bytes_of(&chunk_header))
                .expect("Failed to write chunk header");
            writer
                .write_all(bytemuck::cast_slice(&chunk_data))
                .expect("Failed to write chunk data");
        }
    }

    writer.flush().expect("Failed to flush file");
    println!("Chunks written! {:?}", starter.elapsed());

    let sync_start = Instant::now();
    let inner_file = writer.into_inner().expect("Failed to get file from writer");
    inner_file.sync_all().expect("Failed to sync to disk");
    println!("Synced to disk! {:?}", sync_start.elapsed());
}

/// 4D-координаты точки (gx,gz) на торе с заданным масштабом — бесшовность по XZ.
#[inline]
fn torus_point(gx: f64, gz: f64, w: usize, d: usize, two_pi: f64, scale: f64) -> [f64; 4] {
    let u = (gx / w as f64) * two_pi;
    let v = (gz / d as f64) * two_pi;
    [
        u.cos() * scale,
        u.sin() * scale,
        v.cos() * scale,
        v.sin() * scale,
    ]
}
/// FBM → нормализация в [0,1].
#[inline]
fn noise01(fbm: &Fbm<OpenSimplex>, p: [f64; 4]) -> f64 {
    (fbm.get(p) + 1.0) / 2.0
}
/// Ridged-шум: 1 - |n| → гребни (тонкие линии разлома для каньонов).
#[inline]
fn ridged(fbm: &Fbm<OpenSimplex>, p: [f64; 4]) -> f64 {
    1.0 - fbm.get(p).abs()
}
fn generate_chunk(
    chunk_x: i64,
    chunk_z: i64,
    world_width: usize,
    world_depth: usize,
    two_pi: f64,
    hills: (&Fbm<OpenSimplex>, f64),
    base: (&Fbm<OpenSimplex>, f64),
    canyon: (&Fbm<OpenSimplex>, f64),
    mount: (&Fbm<OpenSimplex>, f64, f64),
    spike: (&Fbm<OpenSimplex>, f64, f64),
    spray: (&Fbm<OpenSimplex>, f64),
    biome: (&Fbm<OpenSimplex>, f64),
) -> (Vec<Cell>, ChunkHeader) {
    let mut cells = vec![Cell::zeroed(); CELLS_PER_CHUNK];
    // Формально обновляем для валидности заголовка, хотя основная логика опирается на column_tops
    
    for lz in 0..CHUNK_SIDE {
        for lx in 0..CHUNK_SIDE {
            // Глобальные координаты блока
            let gx = (chunk_x * CHUNK_SIDE as i64 + lx as i64) as f64;
            let gz = (chunk_z * CHUNK_SIDE as i64 + lz as i64) as f64;

            // ── Каскад карт высот (всё на шумах, никаких кругов) ──────────

            // 1. Слабохолмистый фундамент: y ∈ [1, 10], stone.
            let hills_n = noise01(
                hills.0,
                torus_point(gx, gz, world_width, world_depth, two_pi, hills.1),
            );
            let foundation = 1 + (hills_n * HILLS_AMP as f64) as usize; // 1..=10

            // 2. Низменья до CANYON_TOP=64 + вырез каньонами (ridged-маска).
            let base_n = noise01(
                base.0,
                torus_point(gx, gz, world_width, world_depth, two_pi, base.1),
            );
            let lowlands = (CANYON_TOP - BASE_AMP) + (base_n * BASE_AMP as f64) as usize; // 18..=64
            let ridge = ridged(
                canyon.0,
                torus_point(gx, gz, world_width, world_depth, two_pi, canyon.1),
            );
            let canyon_cut = if ridge > CANYON_THRESHOLD {
                // квадратичный спад глубины от краёв к оси разлома
                let t = (ridge - CANYON_THRESHOLD) / (1.0 - CANYON_THRESHOLD);
                (t * t * CANYON_DEPTH as f64) as usize
            } else {
                0
            };
            let mut surface = lowlands.saturating_sub(canyon_cut).max(foundation + 1);

            // 3. Горы: отдельная карта, нормализованная по фактическому максимуму.
            //    Порог MOUNT_GATE·max задаёт «ядро» горы; внутри ядра кубический
            //    профиль даёт пологие склоны и острую вершину. Нормализация по
            //    глобальному максимуму гарантирует: самая высокая гора мира после
            //    масштабирования достанет ровно до `MOUNTAIN_MAX`.
            let (mount_fbm, mount_s, mount_max) = mount;
            let m_n = noise01(
                mount_fbm,
                torus_point(gx, gz, world_width, world_depth, two_pi, mount_s),
            );
            let m_thr = mount_max * MOUNT_GATE;
            if m_n > m_thr {
                let t = ((m_n - m_thr) / (mount_max - m_thr)).clamp(0.0, 1.0);
                let mountain_h =
                    MOUNTAIN_MIN + (t.powi(3) * (MOUNTAIN_MAX - MOUNTAIN_MIN) as f64) as usize;
                if mountain_h > surface {
                    surface = mountain_h.max(foundation + 1);
                }
            }

            // 4. Пики: редкая высокочастотная карта, тонкие шпильки. Профиль —
            //    lerp от 1 до SPIKE_LERP_MAX (800) с нормализацией по глобальному
            //    максимуму шума: самый высокий пик мира гарантированно уходит за
            //    CHUNK_HEIGHT и обрезается потолком → плоская вершина на y=511,
            //    все субчанки чанка задействованы (проверка швов). Остальные
            //    пики ниже — естественный разброс от формы шума.
            let (spike_fbm, spike_s, spike_max) = spike;
            let s_n = noise01(
                spike_fbm,
                torus_point(gx, gz, world_width, world_depth, two_pi, spike_s),
            );
            let s_thr = spike_max * SPIKE_GATE;
            if s_n > s_thr {
                let t = ((s_n - s_thr) / (spike_max - s_thr)).clamp(0.0, 1.0);
                let raw = 1.0 + t.powi(3) * (SPIKE_LERP_MAX - 1) as f64;
                let spike_h = (raw as usize).min(SPIKE_MAX); // > CHUNK_HEIGHT отсекут ниже
                if spike_h > surface {
                    surface = spike_h;
                }
            }

            let surface = surface.min(CHUNK_HEIGHT - 1);

            // Трава: НЧ-пятно плодородия × ВЧ-spray прореживание + диапазон высот.
            let fert = noise01(
                biome.0,
                torus_point(gx, gz, world_width, world_depth, two_pi, biome.1),
            );
            let sp = noise01(
                spray.0,
                torus_point(gx, gz, world_width, world_depth, two_pi, spray.1),
            );
            let grass_ok = fert > GRASS_BIOME_GATE && sp > GRASS_SPRAY_GATE;

            // Заполняем колонку: полный диапазон индексов `0..CHUNK_HEIGHT`.
            for ly in 0..CHUNK_HEIGHT {
                let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;

                let mut material = if ly == 0 || ly <= foundation {
                    if ly == 0 { BEDROCK } else { STONE }
                } else if ly <= surface {
                    STONE
                } else if ly <= SEA_LEVEL {
                    // Вода только ниже уровня моря и только там, где воздух.
                    WATER
                } else {
                    AIR
                };

                if material == STONE && ly == surface {
                    // Береговой пояс: у кромки воды clay вместо камня.
                    if surface >= SEA_LEVEL.saturating_sub(SHORE_BAND)
                        && surface <= SEA_LEVEL + SHORE_BAND
                    {
                        material = CLAY;
                    } else if grass_ok && surface >= GRASS_MIN_Y && surface <= GRASS_MAX_Y {
                        // Прореженная трава: пятно плодородия × spray-шум.
                        material = GRASS;
                    }
                }

                cells[cell_index].material_id = material;
            }
        }
    }

    let chunk_data_size = (CELLS_PER_CHUNK * std::mem::size_of::<Cell>()) as u32;
    let mut header = ChunkHeader::new(chunk_data_size, CELLS_PER_CHUNK as u32);

    // Карта высот вычисляется один раз на основе сгенерированных ячеек
    header.column_tops = column_tops(&cells);

    (cells, header)
}