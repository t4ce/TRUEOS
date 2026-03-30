extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

const PARTICLE_WORKER_TASK_POOL: usize = 2;
const DEFAULT_CHUNK_LEN: usize = 256;

static PARTICLE_UPDATE_BUSY: AtomicBool = AtomicBool::new(false);
static PARTICLE_JOB_SYSTEM: AtomicPtr<ParticleSystem> = AtomicPtr::new(core::ptr::null_mut());
static PARTICLE_JOB_DT_BITS: AtomicU32 = AtomicU32::new(0);
static PARTICLE_JOB_ALIVE: AtomicUsize = AtomicUsize::new(0);
static PARTICLE_JOB_CHUNK_LEN: AtomicUsize = AtomicUsize::new(DEFAULT_CHUNK_LEN);
static PARTICLE_JOB_NEXT: AtomicUsize = AtomicUsize::new(0);
static PARTICLE_JOB_REMOTE_REMAINING: AtomicUsize = AtomicUsize::new(0);
static PARTICLE_JOB_SEQ: AtomicU64 = AtomicU64::new(1);
static PARTICLE_JOB_ACTIVE_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct ParticleSystem {
    pos_x: UnsafeCell<Vec<f32>>,
    pos_y: UnsafeCell<Vec<f32>>,
    vel_x: UnsafeCell<Vec<f32>>,
    vel_y: UnsafeCell<Vec<f32>>,
    life: UnsafeCell<Vec<f32>>,
    size_px: UnsafeCell<Vec<f32>>,
    color_rgba: UnsafeCell<Vec<u32>>,
    dead: UnsafeCell<Vec<u8>>,
    alive_count: usize,
    max_count: usize,
}

unsafe impl Sync for ParticleSystem {}

#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateReport {
    pub alive_before: usize,
    pub alive_after: usize,
    pub chunk_len: usize,
    pub remote_workers_spawned: usize,
    pub local_fallback: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ParticleSnapshot {
    pub x: f32,
    pub y: f32,
    pub size_px: f32,
    pub color_rgba: u32,
}

impl ParticleSystem {
    pub fn new(max_count: usize) -> Self {
        Self {
            pos_x: UnsafeCell::new(vec![0.0; max_count]),
            pos_y: UnsafeCell::new(vec![0.0; max_count]),
            vel_x: UnsafeCell::new(vec![0.0; max_count]),
            vel_y: UnsafeCell::new(vec![0.0; max_count]),
            life: UnsafeCell::new(vec![0.0; max_count]),
            size_px: UnsafeCell::new(vec![1.0; max_count]),
            color_rgba: UnsafeCell::new(vec![0xFFFFFFFF; max_count]),
            dead: UnsafeCell::new(vec![0; max_count]),
            alive_count: 0,
            max_count,
        }
    }

    #[inline]
    pub fn alive_count(&self) -> usize {
        self.alive_count
    }

    #[inline]
    pub fn max_count(&self) -> usize {
        self.max_count
    }

    pub fn spawn(&mut self, x: f32, y: f32, vx: f32, vy: f32, life: f32) {
        self.spawn_styled(x, y, vx, vy, life, 2.0, 0xFFFFFFFF);
    }

    pub fn spawn_styled(
        &mut self,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        life: f32,
        size_px: f32,
        color_rgba: u32,
    ) {
        if self.alive_count >= self.max_count {
            return;
        }

        let i = self.alive_count;
        unsafe {
            let pos_x = &mut *self.pos_x.get();
            let pos_y = &mut *self.pos_y.get();
            let vel_x = &mut *self.vel_x.get();
            let vel_y = &mut *self.vel_y.get();
            let life_buf = &mut *self.life.get();
            let size_buf = &mut *self.size_px.get();
            let color_buf = &mut *self.color_rgba.get();
            let dead_buf = &mut *self.dead.get();

            pos_x[i] = x;
            pos_y[i] = y;
            vel_x[i] = vx;
            vel_y[i] = vy;
            life_buf[i] = life;
            size_buf[i] = size_px;
            color_buf[i] = color_rgba;
            dead_buf[i] = 0;
        }
        self.alive_count += 1;
    }

    pub fn update_single_threaded(&mut self, dt: f32) {
        let alive = self.alive_count;
        if alive == 0 {
            return;
        }

        unsafe {
            process_range(self as *mut Self, 0, alive, dt);
        }
        self.compact_dead();
    }

    pub fn update_dual_driven(&mut self, dt: f32) -> UpdateReport {
        let alive_before = self.alive_count;
        if alive_before == 0 {
            return UpdateReport::default();
        }

        let chunk_len = choose_chunk_len(alive_before);
        let mut report = UpdateReport {
            alive_before,
            alive_after: alive_before,
            chunk_len,
            remote_workers_spawned: 0,
            local_fallback: false,
        };

        if PARTICLE_UPDATE_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            self.update_single_threaded(dt);
            report.alive_after = self.alive_count;
            report.local_fallback = true;
            return report;
        }

        struct UpdateGuard;
        impl Drop for UpdateGuard {
            fn drop(&mut self) {
                PARTICLE_JOB_SYSTEM.store(core::ptr::null_mut(), Ordering::Release);
                PARTICLE_JOB_ALIVE.store(0, Ordering::Release);
                PARTICLE_JOB_CHUNK_LEN.store(DEFAULT_CHUNK_LEN, Ordering::Release);
                PARTICLE_JOB_NEXT.store(0, Ordering::Release);
                PARTICLE_JOB_REMOTE_REMAINING.store(0, Ordering::Release);
                PARTICLE_JOB_ACTIVE_SEQ.store(0, Ordering::Release);
                PARTICLE_UPDATE_BUSY.store(false, Ordering::Release);
            }
        }
        let _guard = UpdateGuard;

        let seq = PARTICLE_JOB_SEQ
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        PARTICLE_JOB_SYSTEM.store(self as *mut Self, Ordering::Release);
        PARTICLE_JOB_DT_BITS.store(dt.to_bits(), Ordering::Release);
        PARTICLE_JOB_ALIVE.store(alive_before, Ordering::Release);
        PARTICLE_JOB_CHUNK_LEN.store(chunk_len, Ordering::Release);
        PARTICLE_JOB_NEXT.store(0, Ordering::Release);
        PARTICLE_JOB_REMOTE_REMAINING.store(0, Ordering::Release);
        PARTICLE_JOB_ACTIVE_SEQ.store(seq, Ordering::Release);

        let remote_workers = spawn_remote_workers(seq);
        report.remote_workers_spawned = remote_workers;

        run_claim_loop(seq);
        while PARTICLE_JOB_REMOTE_REMAINING.load(Ordering::Acquire) != 0 {
            crate::wait::spin_step();
        }

        self.compact_dead();
        report.alive_after = self.alive_count;
        report
    }

    pub fn pos_x(&self) -> &[f32] {
        unsafe { &(&*self.pos_x.get())[..self.alive_count] }
    }

    pub fn pos_y(&self) -> &[f32] {
        unsafe { &(&*self.pos_y.get())[..self.alive_count] }
    }

    pub fn life(&self) -> &[f32] {
        unsafe { &(&*self.life.get())[..self.alive_count] }
    }

    pub fn snapshot_into(&self, out: &mut Vec<ParticleSnapshot>) {
        out.clear();
        out.reserve(self.alive_count);

        let pos_x = unsafe { &*self.pos_x.get() };
        let pos_y = unsafe { &*self.pos_y.get() };
        let size_px = unsafe { &*self.size_px.get() };
        let color_rgba = unsafe { &*self.color_rgba.get() };

        for i in 0..self.alive_count {
            out.push(ParticleSnapshot {
                x: pos_x[i],
                y: pos_y[i],
                size_px: size_px[i],
                color_rgba: color_rgba[i],
            });
        }
    }

    fn compact_dead(&mut self) {
        let alive = self.alive_count;
        let pos_x = unsafe { &mut *self.pos_x.get() };
        let pos_y = unsafe { &mut *self.pos_y.get() };
        let vel_x = unsafe { &mut *self.vel_x.get() };
        let vel_y = unsafe { &mut *self.vel_y.get() };
        let life = unsafe { &mut *self.life.get() };
        let size_px = unsafe { &mut *self.size_px.get() };
        let color_rgba = unsafe { &mut *self.color_rgba.get() };
        let dead = unsafe { &mut *self.dead.get() };

        let mut write = 0usize;
        for read in 0..alive {
            if dead[read] != 0 {
                continue;
            }

            if write != read {
                pos_x[write] = pos_x[read];
                pos_y[write] = pos_y[read];
                vel_x[write] = vel_x[read];
                vel_y[write] = vel_y[read];
                life[write] = life[read];
                size_px[write] = size_px[read];
                color_rgba[write] = color_rgba[read];
                dead[write] = 0;
            }
            write += 1;
        }

        self.alive_count = write;
    }
}

fn choose_chunk_len(alive_count: usize) -> usize {
    if alive_count >= 4096 {
        1024
    } else if alive_count >= 1024 {
        512
    } else if alive_count >= 256 {
        256
    } else {
        alive_count.max(1)
    }
}

fn spawn_remote_workers(seq: u64) -> usize {
    let mut spawned = 0usize;
    let total_slots = crate::percpu::total_slots();

    if total_slots > 1 {
        for slot in 1..total_slots {
            if spawned >= PARTICLE_WORKER_TASK_POOL {
                break;
            }

            let Some(spawner) = trueos_qjs::workers::spawner_for_slot(slot as u32) else {
                continue;
            };

            PARTICLE_JOB_REMOTE_REMAINING.fetch_add(1, Ordering::AcqRel);
            if spawner.spawn(particle_worker_task(seq)).is_ok() {
                spawned += 1;
            } else {
                PARTICLE_JOB_REMOTE_REMAINING.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    if spawned == 0 {
        if let Some(spawner) = trueos_qjs::workers::pick_background_spawner() {
            PARTICLE_JOB_REMOTE_REMAINING.fetch_add(1, Ordering::AcqRel);
            if spawner.spawn(particle_worker_task(seq)).is_ok() {
                spawned = 1;
            } else {
                PARTICLE_JOB_REMOTE_REMAINING.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    spawned
}

fn run_claim_loop(seq: u64) {
    if PARTICLE_JOB_ACTIVE_SEQ.load(Ordering::Acquire) != seq {
        return;
    }

    loop {
        let start = PARTICLE_JOB_NEXT
            .fetch_add(PARTICLE_JOB_CHUNK_LEN.load(Ordering::Acquire), Ordering::AcqRel);
        let alive = PARTICLE_JOB_ALIVE.load(Ordering::Acquire);
        if start >= alive {
            break;
        }
        let end = core::cmp::min(start + PARTICLE_JOB_CHUNK_LEN.load(Ordering::Acquire), alive);
        let system = PARTICLE_JOB_SYSTEM.load(Ordering::Acquire);
        if system.is_null() {
            break;
        }
        let dt = f32::from_bits(PARTICLE_JOB_DT_BITS.load(Ordering::Acquire));
        unsafe {
            process_range(system, start, end, dt);
        }
    }
}

unsafe fn process_range(system: *mut ParticleSystem, start: usize, end: usize, dt: f32) {
    let pos_x = (&mut *(*system).pos_x.get()).as_mut_ptr();
    let pos_y = (&mut *(*system).pos_y.get()).as_mut_ptr();
    let vel_x = (&mut *(*system).vel_x.get()).as_mut_ptr();
    let vel_y = (&mut *(*system).vel_y.get()).as_mut_ptr();
    let life = (&mut *(*system).life.get()).as_mut_ptr();
    let dead = (&mut *(*system).dead.get()).as_mut_ptr();

    for i in start..end {
        let vx = *vel_x.add(i);
        let vy = *vel_y.add(i);
        *pos_x.add(i) += vx * dt;
        *pos_y.add(i) += vy * dt;

        let new_life = (*life.add(i)) - dt;
        *life.add(i) = new_life;
        *dead.add(i) = if new_life <= 0.0 { 1 } else { 0 };
    }
}

#[embassy_executor::task(pool_size = PARTICLE_WORKER_TASK_POOL)]
async fn particle_worker_task(seq: u64) {
    if PARTICLE_JOB_ACTIVE_SEQ.load(Ordering::Acquire) != seq {
        return;
    }

    run_claim_loop(seq);
    PARTICLE_JOB_REMOTE_REMAINING.fetch_sub(1, Ordering::AcqRel);
}
