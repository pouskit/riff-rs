//! Sans plugin actif, un événement ne coûte ni allocation ni fil (allocateur compteur de
//! `crates/riff-audio/tests/no_alloc.rs`).

use riff_plugins::{PluginEvent, PluginHost, threads_spawned};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

// Drapeau et compteur locaux au fil : les tests tournent en parallèle dans le même processus,
// un compteur global mélangerait les allocations (et la remise à zéro) de l'un avec celles de l'autre.
thread_local! {
  static COUNTING: Cell<bool> = const { Cell::new(false) };
  static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

impl CountingAllocator {
  fn record() {
    // `try_with` : le stockage local du fil peut être détruit pendant la fin du fil.
    let _ = COUNTING.try_with(|flag| {
      if flag.get() {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
      }
    });
  }
}

// SAFETY: délègue à `System` sans changer les paramètres ; le compteur ne touche pas à la mémoire.
unsafe impl GlobalAlloc for CountingAllocator {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.alloc(layout) }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.dealloc(ptr, layout) }
  }

  unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.alloc_zeroed(layout) }
  }

  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.realloc(ptr, layout, new_size) }
  }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn count_allocations(work: impl FnOnce()) -> usize {
  ALLOCATIONS.with(|count| count.set(0));
  COUNTING.with(|flag| flag.set(true));
  work();
  COUNTING.with(|flag| flag.set(false));
  ALLOCATIONS.with(Cell::get)
}

#[test]
fn counter_detects_an_allocation() {
  let count = count_allocations(|| drop(std::hint::black_box(vec![0u8; 64])));
  assert!(count > 0, "le compteur doit voir une allocation");
}

#[test]
fn events_without_an_active_plugin_allocate_nothing_and_spawn_no_thread() {
  let threads_before = threads_spawned();
  let (host, _incoming) = PluginHost::new(std::env::temp_dir());
  let play = PluginEvent::Play;
  let pause = PluginEvent::Pause;
  let ready = PluginEvent::Ready;
  let count = count_allocations(|| {
    for _ in 0..10_000 {
      if host.is_active() {
        unreachable!("aucun plugin");
      }
      host.dispatch(&play);
      host.dispatch(&pause);
      host.dispatch(&ready);
      assert!(!host.picks());
    }
  });
  assert_eq!(count, 0, "allocations sans plugin actif");
  assert_eq!(threads_spawned(), threads_before, "fil créé sans plugin");
}
