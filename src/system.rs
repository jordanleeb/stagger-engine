use crate::world::World;

/// A named, callable unit of game logic.
/// 
/// A `System` wraps an arbitrary `FnMut(&mut` closure and gives it a
/// human-readable name for use in diagnostics and debugging.
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

/// An ordered list of ['System']s that run sequentially each tick.
/// 
/// Systems are executed in the order they were added via [`Schedule::add_system`].
/// There is no parallelism; each system runs to completion before the next one begins.
/// 
/// State written by one system is immediately visible to all systems that
/// follow it in the same call to [`Schedule::run`].
/// 
/// # Example
/// 
/// ```rust
/// let mut schedule = Schedule::new();
/// 
/// schedule
///     .add_system(System::new("physics", physics_fn))
///     .add_system(System::new("animation", animation_fn))
///     .add_system(System::new("render", render_fn));
/// 
/// // Each call runs all systems in insertion order.
/// loop {
///     schedule.run(&mut world);
/// }
/// ```
pub struct Schedule {
    /// The ordered list of systems to run.
    /// 
    /// Systems are appended in `add_system` and iterated in insertion order
    /// during `run`. Mutable access to each element is required because systems
    /// hold `FnMut` closures that may update their own captured state.
    systems: Vec<System>,
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

impl Schedule {
    /// Creates a new, empty schedule with no systems registered.
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    /// Appends a system to the end of the schedule.
    /// 
    /// Systems run in insertion order. Returns `&mut Self` so calls can be
    /// chained fluently:
    /// 
    /// ```rust
    /// schedule
    ///     .add_system(physics)
    ///     .add_system(animation)
    ///     .add_system(render);
    /// ```
    pub fn add_system(&mut self, system: System) -> &mut Self {
        self.systems.push(system);
        self
    }

    /// Run every system in order, each with exclusive access to the world.
    /// 
    /// Systems are not isolated from each other's effects: mutations written
    /// by system N are immediately visible to system N+1 within the same call
    /// to `run`.
    pub fn run(&mut self, world: &mut World) {
        for system in &mut self.systems {
            system.run(world);
        }
    }

    /// Returns the number of systems currently in the schedule.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
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

    #[test]
    fn empty_schedule_runs_without_panic() {
        let mut world = World::new();
        let mut schedule = Schedule::new();

        // Should complete without error even with nothing registered.
        schedule.run(&mut world);
    }

    #[test]
    fn add_system_increases_system_count() {
        let mut schedule = Schedule::new();

        assert_eq!(schedule.system_count(), 0);

        schedule.add_system(System::new("a", |_w| {}));
        assert_eq!(schedule.system_count(), 1);

        schedule.add_system(System::new("b", |_w| {}));
        assert_eq!(schedule.system_count(), 2);
    }

    #[test]
    fn systems_run_in_insertion_order() {
        let mut world = World::new();

        // Rc<RefCell<_>> provides shared interior mutability without requiring
        // the closures to be Sync, which they don't need to be here.
        let log: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));

        let log_a = log.clone();
        let log_b = log.clone();
        let log_c = log.clone();

        let mut schedule = Schedule::new();
        schedule
            .add_system(System::new("a", move |_w| log_a.borrow_mut().push(1)))
            .add_system(System::new("b", move |_w| log_b.borrow_mut().push(2)))
            .add_system(System::new("c", move |_w| log_c.borrow_mut().push(3)));
        
        schedule.run(&mut world);

        assert_eq!(*log.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn later_system_sees_earlier_systems_mutations() {
        let mut world = World::new();
        let id = world.register_component::<u32>();
        let e = world.spawn();

        // System A writes a value; system B reads it and writes a derived value.
        // If execution order is correct, B will see A's write in the same tick.
        let mut schedule = Schedule::new();
        schedule
            .add_system(System::new("writer", move |w| {
                w.add_component(e, 10_u32);
            }))
            .add_system(System::new("doubler", move |w| {
                // Read what A write, double it, and write it back.
                let current = {
                    let query = w.query_builder().build();
                    let loc = w.location(e).unwrap();
                    let arch = w.archetype(loc.archetype()).unwrap();
                    arch.column(id).and_then(|col| col.get::<u32>(loc.row())).copied()
                };

                if let Some(v) = current {
                    w.add_component(e, v * 2);
                }
            }));
        
        schedule.run(&mut world);

        let loc = world.location(e).unwrap();
        let arch = world.archetype(loc.archetype()).unwrap();
        assert_eq!(arch.column(id).unwrap().get::<u32>(loc.row()), Some(&20));
    }

    #[test]
    fn schedule_runs_physics_loop_correctly() {
        struct Position { x: f32, y: f32 }
        struct Velocity { x: f32, y: f32 }

        let mut world = World::new();
        let pos_id = world.register_component::<Position>();
        let vel_id = world.register_component::<Velocity>();

        let e = world.spawn();
        world.add_component(e, Position { x: 0.0, y: 0.0 });
        world.add_component(e, Velocity { x: 3.0, y: -1.0 });

        // The movement system follow the collect-then-apply pattern:
        // it reads position and velocity into a Vec, drops the query,
        // then writes the updated positions back.
        let movement = System::new("movement", move |w| {
            let updates: Vec<(crate::entity::Entity, f32, f32)> = {
                let query = w
                    .query_builder()
                    .require::<Position>()
                    .require::<Velocity>()
                    .build();

                query
                    .iter()
                    .map(|row| {
                        let pos = row.get::<Position>(pos_id).unwrap();
                        let vel = row.get::<Velocity>(vel_id).unwrap();
                        (row.entity(), pos.x + vel.x, pos.y + vel.y)
                    })
                    .collect()
            };
            // Query is here; `w` is now exclusively owned again.

            for (entity, new_x, new_y) in updates {
                w.add_component(entity, Position { x: new_x, y: new_y });
            }
        });

        let mut schedule = Schedule::new();
        schedule.add_system(movement);
        schedule.run(&mut world);

        let loc = world.location(e).unwrap();
        let arch = world.archetype(loc.archetype()).unwrap();
        let pos = arch.column(pos_id).unwrap().get::<Position>(loc.row()).unwrap();

        assert_eq!(pos.x, 3.0);
        assert_eq!(pos.y, -1.0);
    }
}