//! Реестр материалов.

pub mod ids {
    pub const AIR: u8 = 0;
    pub const BEDROCK: u8 = 1;
    pub const STONE: u8 = 2;
    pub const DIRT: u8 = 3;
    pub const GRASS: u8 = 4;
    pub const WATER: u8 = 5;
    pub const CLAY: u8 = 6;
    pub const GLASS: u8 = 7;
    pub const GLOWSTONE: u8 = 8;
}

pub mod phases {
    pub const SOLID: u8 = 0;
    pub const LIQUID: u8 = 1;
    pub const GAS: u8 = 2;
    pub const PLASMA: u8 = 3;
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub id: u8,
    pub name: &'static str,
    pub color: [u8; 3],
    pub opacity: f32,
    pub phase: u8,
    pub bloom: u8,  //bloom multiplier, 1 - regular (no bloom), 255 - max bloom
    pub cutoff: bool,   //cutoff material
}

pub const MATERIALS: &[Material] = &[
    Material { id: ids::AIR, name: "Air", color: [0, 0, 0], opacity: 0.0, phase: phases::GAS, bloom: 1, cutoff: false },
    Material { id: ids::BEDROCK, name: "Bedrock", color: [30, 130, 130], opacity: 1.0, phase: phases::SOLID, bloom: 1, cutoff: false },
    Material { id: ids::STONE, name: "Stone", color: [168, 168, 168], opacity: 1.0, phase: phases::SOLID, bloom: 1, cutoff: false },
    Material { id: ids::DIRT, name: "Dirt", color: [139, 90, 43], opacity: 1.0, phase: phases::SOLID, bloom: 1, cutoff: false },
    Material { id: ids::GRASS, name: "Grass", color: [34, 139, 34], opacity: 1.0, phase: phases::SOLID, bloom: 1, cutoff: true },
    Material { id: ids::WATER, name: "Water", color: [30, 100, 200], opacity: 0.7, phase: phases::LIQUID, bloom: 1, cutoff: false },
    Material { id: ids::CLAY, name: "Clay", color: [180, 130, 100], opacity: 1.0, phase: phases::SOLID, bloom: 1, cutoff: false },
    Material { id: ids::GLASS, name: "Glass", color: [220, 220, 220], opacity: 0.5, phase: phases::SOLID, bloom: 1, cutoff: false },
    Material { id: ids::GLOWSTONE, name: "Glowstone", color: [255, 255, 255], opacity: 0.5, phase: phases::SOLID, bloom: 50, cutoff: false },
];

#[inline]
pub fn get_material(id: u8) -> Option<&'static Material> {
    MATERIALS.iter().find(|m| m.id == id)
}

/// LUT свойств материала по `id` (256 записей, строится в рантайме один раз).
///
/// Горячий цикл мешера проверяет видимость грани (`is_face_visible`) на каждую грань
/// и цвет/opacity на каждый непустой блок; прежний линейный поиск по `MATERIALS`
/// здесь — чистый waste. Семантика совпадает с функциями выше:
/// - неизвестный id → прозрачный, цвет `[0,0,0]`, не Air;
/// - запись заполняется из `MATERIALS` по индексу `m.id`.
pub struct MatLut {
    pub transparent: [bool; 256],
    pub air: [bool; 256],
    /// id зарегистрирован в реестре (есть запись в MATERIALS). Незарегистрированные
    /// id мешер пропускает — прежняя семантика `get_material(..) => None => continue`.
    pub known: [bool; 256],
    pub color: [[u8; 3]; 256],
    pub opacity: [f32; 256],
    pub bloom: [u8; 256],
    pub cutoff: [bool; 256],
}

static MAT_LUT: std::sync::LazyLock<MatLut> = std::sync::LazyLock::new(|| {
    let mut lut = MatLut {
        transparent: [true; 256],
        air: [false; 256],
        known: [false; 256],
        color: [[0, 0, 0]; 256],
        opacity: [0.0; 256],
        bloom: [0; 256],
        cutoff: [false; 256],
    };
    for m in MATERIALS {
        let i = m.id as usize;
        lut.known[i] = true;
        lut.transparent[i] = m.opacity < 1.0;
        lut.air[i] = m.id == ids::AIR;
        lut.color[i] = m.color;
        lut.opacity[i] = m.opacity;
        lut.bloom[i] = m.bloom;
        lut.cutoff[i] = m.cutoff;
    }
    lut
});

/// Проверка видимости грани блока `current` при соседе `neighbor`.
///
/// Правило (согласованное): грань видна, если сосед прозрачен И не того же
/// материала, что текущий блок (вода-вода / стекло-стекло — без внутренних
/// швов и z-fighting). Реализовано двумя предсказуемыми ветвлениями над
/// 256-байтовыми таблицами LUT вместо 64-КБ двумерной таблицы `face_visible`:
/// та не помещалась в L1 и ловила cache miss на каждую грань.
#[inline(always)]
pub fn is_face_visible(lut: &MatLut, current: u8, neighbor: u8) -> bool {
    if !lut.transparent[neighbor as usize] || lut.cutoff[neighbor as usize] {
        return false;
    }
    current != neighbor || current == ids::AIR
}

#[inline]
pub fn is_air(id: u8) -> bool {
    id == ids::AIR
}

/// Прямой доступ к LUT для горячего цикла мешера (без повторного индексирования
/// статического массива на каждое обращение).
#[inline]
pub fn mat_lut() -> &'static MatLut {
    &MAT_LUT
}