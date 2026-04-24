use crate::world::World;

/// A named, callable unit of game logic.
/// 
/// A `System` wraps an arbitrary `FnMut(&mut` closure and gives it a
/// human-readable name for usse in diagnostics and debugging.
/// 
/// Each call to [`System::run`] passes exclusive access to the world to the
/// stored closure. Because the closure is `FnMut`, a system may accumulate
/// state between runs. For example, to track elapsed time or frame counts.
/// 
/// # Read-then-write pattern
/// 
/// Queries borrow the world immutably (`&World`), while structural mutations
/// such as adding and removing components require `&mut World`. A system that
/// both reads and writes must therefore follow the collect-then-apply pattern:
/// 
/// 1. Build a query and collect the data you need into a temporary `Vec`.
/// 2. Drop the query to release the immutable borrow.
/// 3. Apply mutations using the now-available `&mut World`.
/// 
/// This is the same pattern used in the world's integration tests and is the
/// expected idiom for all read-modify-write systems.
pub struct System {
    /// A human-readable label used in diagnostics and debug output.
    /// 
    /// `&'static str` avoids allocation for the common case of a string literal.
    /// The name is not required to be unique.
    name: &'static str,

    /// The closure implementing this system's logic.
    /// 
    /// Stored as a boxed trait object so that any `FnMut(&mut World` closure,
    /// regardless of its captured state, can be held uniformly in a collection.
    /// 
    /// `FnMut` (rather than `Fn`) is required because closures that mutate
    /// captured state between calls, such as timers or frame counters, must be
    /// supported.
    /// 
    /// The `'static` bound ensures the closure does not capture short-lived
    /// references, which would make it unsafe to store inside a long-lived
    /// schedule.
    func: Box<dyn FnMut(&mut World)>,
}

impl System {
    /// Creates a new system with the given name and closure.
    /// 
    /// `name` is used only for diagnosis and does not need to be unique.
    /// 
    /// `func` may be any closure that accepts `&mut World` and returns `()`.
    /// The `'static` bound prevents capturing references with shorter lifetimes
    /// than the system itself.
    pub fn new(name: &'static str, func: impl FnMut(&mut World) + 'static) -> Self {
        Self {
            name,
            func: Box::new(func),
        }
    }

    /// Runs the system against the given world.
    /// 
    /// Calls the stored closure with exclusive access to the world.
    /// `&mut self` is required because the closure is `FnMut` and may modify
    /// its own captured state on each invocation.
    pub fn run(&mut self, world: &mut World) {
        (self.func)(world);
    }

    /// Returns the system's name.
    pub fn name(&self) -> &str {
        self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn system_name_is_accessible() {
        let system = System::new("my_system", |_world| {});

        assert_eq!(system.name(), "my_system");
    }

    #[test]
    fn system_run_executes_the_closure() {
        let mut world = World::new();
        let id = world.register_component::<u32>();
        let e = world.spawn();

        // The system writes a known value to the entity's component slot.
        let mut system = System::new("writer", move |w| {
            w.add_component(e, 42_u32);
        });

        system.run(&mut world);

        // Verify the closure actually ran by reading back the written value.
        let loc = world.location(e).unwrap();
        let arch = world.archetype(loc.archetype()).unwrap();
        assert_eq!(arch.column(id).unwrap().get::<u32>(loc.row()), Some(&42));
    }

    #[test]
    fn system_preserves_captured_state_across_runs() {
        let mut world = World::new();

        // Rc<Cell<_>> is 'static (no borrowed references) and provides
        // interior mutability without requiring the closure to by Sync.
        let counter = Rc::new(Cell::new(0_u32));
        let counter_clone = counter.clone();

        let mut system = System::new("counter", move |_w| {
            counter_clone.set(counter_clone.get() + 1);
        });

        system.run(&mut world);
        system.run(&mut world);
        system.run(&mut world);

        // The closure captured and mutated `counter_clone` across three calls.
        assert_eq!(counter.get(), 3);
    }
}