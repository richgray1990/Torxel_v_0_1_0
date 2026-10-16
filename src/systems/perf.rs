//! Инструментация времени систем. Полезный постоянный инструмент — НЕ удалять.
//!
//! Отключается одним флагом `PERF_ENABLED = false`: `record` тогда инлайнится
//! в ничто, системы не трогаются, а сигнатуры можно оставить как есть.
//!
//! `PerfStats` — `Res` с атомиками, а не `ResMut`: системы рендера и пост-свапа
//! идут параллельными ветками, и общий изменяемый ресурс их бы сериализовал,
//! изменив то самое поведение, которое мы измеряем.

use bevy::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Выключить, чтобы снять всю инструментацию без правки систем.
pub const PERF_ENABLED: bool = true;

/// Бакеты измерений. Порядок задаёт индекс в массиве — менять только вместе с `NAMES`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bucket {
    /// Полное время кадра (`Time::delta`). Опорная точка: если максимум кадра
    /// сильно больше суммы остальных бакетов — тормозит не наш код.
    Frame,
    PollTasks,
    InitialCopy,
    UpdateWindow,
    SwapPointers,
    PostSwapCopy,
    DispatchLoaded,
    DispatchDirty,
    ProcessMeshEvents,
    BuildMeshes,
    SwapMeshes,
    UpdateMeshEntities,
    ApplyCellEvents,
    Gc,
    /// Под-метки `BuildMeshes` — только для разложения пика, не самостоятельные системы.
    FillHalo,
    FaceLoop,
    MeshInsert,
    /// Wall-time всей цепочки `PollPhase → GCPhase` (не отдельная система).
    /// Диагностика: сравнить с `Frame`, чтобы понять, лежит ли пик кадра
    /// внутри наших систем или вне их (winit/vsync/render extract/present).
    GameFlowWall,
    /// Время самого `debug_report_system`, включая печать. Живёт **вне**
    /// `GameFlowWall` (та оборачивает только цепочку), поэтому попадает в `Frame`,
    /// но нигде больше не видно. Печатается со сдвигом на интервал: счётчик
    /// пишется после `report_and_reset`, то есть показывается в следующем отчёте.
    Report,
    /// Всё, что происходит МЕЖДУ концом нашей цепочки и её началом на следующем
    /// кадре: `PostUpdate` (в т.ч. propagate `Transform → GlobalTransform`),
    /// `Last`, extract в render-app, prepare/queue, present, winit.
    ///
    /// Ни один другой бакет туда не достаёт: наши системы мерят `Update`,
    /// `GameFlowWall` — цепочку, render-диагностики — проходы графа.
    /// `GameFlowWall + FrameTail` должны в сумме дать `Frame`.
    FrameTail,
    /// Системы, которые до 2026-10-15 не мерились вовсе.
    RequestSwap,
    CameraInput,
    CameraTransform,
    BlockInteraction,
}

pub const BUCKET_COUNT: usize = 24;

const NAMES: [&str; BUCKET_COUNT] = [
    "Frame",
    "PollTasks",
    "InitialCopy",
    "UpdateWindow",
    "SwapPointers",
    "PostSwapCopy",
    "DispatchLoaded",
    "DispatchDirty",
    "ProcessMeshEvents",
    "BuildMeshes",
    "SwapMeshes",
    "UpdateMeshEntities",
    "ApplyCellEvents",
    "Gc",
    "FillHalo",
    "FaceLoop",
    "MeshInsert",
    "GameFlowWall",
    "Report",
    "FrameTail",
    "RequestSwap",
    "CameraInput",
    "CameraTransform",
    "BlockInteraction",
];

impl Bucket {
    #[inline(always)]
    fn index(self) -> usize {
        self as usize
    }
}

/// Счётчики одного бакета. Выравнивание по кэш-линии — против false sharing
/// между бакетами, которые пишутся разными потоками.
#[repr(align(64))]
struct BucketStats {
    sum_ns: AtomicU64,
    max_ns: AtomicU64,
    count: AtomicU64,
}

impl BucketStats {
    const fn new() -> Self {
        Self {
            sum_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    fn record(&self, ns: u64) {
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Снимает накопленное и сбрасывает счётчики.
    #[inline]
    fn take(&self) -> (u64, u64, u64) {
        (
            self.sum_ns.swap(0, Ordering::Relaxed),
            self.max_ns.swap(0, Ordering::Relaxed),
            self.count.swap(0, Ordering::Relaxed),
        )
    }
}

#[derive(Resource)]
pub struct PerfStats {
    buckets: [BucketStats; BUCKET_COUNT],
}

impl Default for PerfStats {
    fn default() -> Self {
        const ZERO: BucketStats = BucketStats::new();
        Self {
            buckets: [ZERO; BUCKET_COUNT],
        }
    }
}

impl PerfStats {
    /// Записывает длительность в бакет. При `PERF_ENABLED = false` — ничто.
    #[inline(always)]
    pub fn record(&self, bucket: Bucket, elapsed: Duration) {
        if !PERF_ENABLED {
            return;
        }

        self.buckets[bucket.index()].record(elapsed.as_nanos() as u64);
    }

    /// Пишет накопленное за интервал в `out` и обнуляет счётчики.
    ///
    /// Ничего не печатает сам: 19 отдельных `print!` — это 19 заходов в stdout,
    /// а stdout в Rust буферизован построчно и сбрасывается на каждом переводе
    /// строки. Вызывающий собирает всё в один буфер и печатает одним вызовом.
    ///
    /// Пишет только бакеты с ненулевым счётчиком — в покое строка короткая.
    pub fn report_and_reset(&self, out: &mut String) {
        if !PERF_ENABLED {
            return;
        }

        let (_, frame_max, frame_count) = self.buckets[Bucket::Frame.index()].take();
        let frame_max_ms = frame_max as f64 / 1e6;

        use std::fmt::Write as _;
        let _ = write!(out, "[PERF] frame n={} max={:.2}ms", frame_count, frame_max_ms);

        for i in 0..BUCKET_COUNT {
            if i == Bucket::Frame.index() {
                continue;
            }

            let (sum, max, count) = self.buckets[i].take();

            if count == 0 {
                continue;
            }

            let _ = write!(
                out,
                " | {} n={} sum={:.2}ms max={:.2}ms",
                NAMES[i],
                count,
                sum as f64 / 1e6,
                max as f64 / 1e6
            );
        }

        out.push('\n');
    }
}

/// Метки времени для `GameFlowWall` и `FrameTail`.
///
/// `start` пишет первая система цепочки (`poll_background_tasks_system`), читает
/// последняя (`chunk_garbage_collector_system`). `end` — момент конца цепочки
/// **прошлого** кадра: его читает первая система следующего кадра и получает
/// `FrameTail` = всё, что легло между концом цепочки и её следующим началом.
///
/// `Option` вместо дефолтного `Instant` — на первом кадре читать нечего.
#[derive(Resource, Default)]
pub struct FrameFlowStart {
    pub start: Option<std::time::Instant>,
    pub end: Option<std::time::Instant>,
}

/// Байтовый объём копии `post_swap_copy_system` за один вызов. Отдельный ресурс,
/// а не `Bucket`: `PerfStats` хранит наносекунды, байты — другая единица, подсовывать
/// их в тот же `AtomicU64` значило бы врать в выводе.
#[derive(Resource, Default)]
pub struct SwapCopyStats {
    sum_bytes: AtomicU64,
    max_bytes: AtomicU64,
    count: AtomicU64,
}

impl SwapCopyStats {
    #[inline(always)]
    pub fn record_bytes(&self, bytes: u64) {
        if !PERF_ENABLED {
            return;
        }

        self.sum_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.max_bytes.fetch_max(bytes, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Пишет накопленное за интервал в `out` и обнуляет счётчики. Молчит, если за
    /// интервал не было ни одного свапа с грязными чанками.
    pub fn report_and_reset(&self, out: &mut String) {
        if !PERF_ENABLED {
            return;
        }

        let sum = self.sum_bytes.swap(0, Ordering::Relaxed);
        let max = self.max_bytes.swap(0, Ordering::Relaxed);
        let count = self.count.swap(0, Ordering::Relaxed);

        if count == 0 {
            return;
        }

        use std::fmt::Write as _;
        let _ = writeln!(
            out,
            "[PERF-COPY] n={} sum={:.2}MB max={:.2}MB",
            count,
            sum as f64 / (1024.0 * 1024.0),
            max as f64 / (1024.0 * 1024.0)
        );
    }
}
