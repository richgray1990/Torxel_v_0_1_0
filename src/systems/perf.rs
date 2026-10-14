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
}

pub const BUCKET_COUNT: usize = 17;

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

    /// Печатает накопленное за интервал и обнуляет счётчики.
    ///
    /// Печатает только бакеты с ненулевым счётчиком — в покое строка короткая.
    pub fn report_and_reset(&self) {
        if !PERF_ENABLED {
            return;
        }

        let (_, frame_max, frame_count) = self.buckets[Bucket::Frame.index()].take();
        let frame_max_ms = frame_max as f64 / 1e6;

        print!("[PERF] frame n={} max={:.2}ms", frame_count, frame_max_ms);

        for i in 0..BUCKET_COUNT {
            if i == Bucket::Frame.index() {
                continue;
            }

            let (sum, max, count) = self.buckets[i].take();

            if count == 0 {
                continue;
            }

            print!(
                " | {} n={} sum={:.2}ms max={:.2}ms",
                NAMES[i],
                count,
                sum as f64 / 1e6,
                max as f64 / 1e6
            );
        }

        println!();
    }
}
