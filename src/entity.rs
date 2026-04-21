/// A generational entity handle.
///
/// `index` refers to a slot in the allocator.
/// `generation` ensures that stale handles become invalid when slots are reused.
///
/// If an entity is destroyed and its slot is reused, the generation is incremented.
/// This prevents old handles from accidentally referring to a new entity.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Entity {
    pub index: u32,
    pub generation: u32,
}

/// Allocates and manages entity lifetimes.
///
/// Uses a generational indexing scheme:
/// - Each entity is identified by an index + generation pair.
/// - Destroying an entity increments the generation of its slot.
/// - Reusing a slot gives a new entity with updated generation.
///
/// This prevents stale handles from becoming valid again.
pub struct EntityAllocator {
    /// Current generation for each slot.
    generations: Vec<u32>,

    /// Whether each slot is currently alive.
    alive: Vec<bool>,

    /// Indices of free (destroyed) slots available for reuse.
    free_list: Vec<u32>,
}

impl Default for EntityAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityAllocator {
    /// Creates a new, empty allocator.
    ///
    /// No entities exist initially.
    pub fn new() -> Self {
        Self {
            generations: Vec::new(),
            alive: Vec::new(),
            free_list: Vec::new(),
        }
    }

    /// Creates a new entity.
    ///
    /// Reuses a previously freed slot if available. Otherwise allocates a new slot.
    ///
    /// When reusing a slot, the existing generation is preserved, ensuring
    /// that old handles to that slot remain invalid.
    pub fn create(&mut self) -> Entity {
        match self.free_list.pop() {
            Some(index) => {
                let slot = index as usize;

                // Reactivate a previously freed slot.
                self.alive[slot] = true;

                Entity {
                    index,
                    generation: self.generations[slot],
                }
            }
            None => {
                let index = self.generations.len() as u32;

                // Allocate a brand new slot with generation 0.
                self.generations.push(0);
                self.alive.push(true);

                Entity {
                    index,
                    generation: 0,
                }
            }
        }
    }

    /// Destroys an entity.
    ///
    /// Returns `true` if the entity was alive and successfully destroyed.
    /// Returns `false` if the entity was already dead or invalid.
    ///
    /// Destroying an entity:
    /// - Marks the slot as free.
    /// - Increments its generation.
    /// - Ensures all previous handles become invalid.
    pub fn destroy(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }

        let index = entity.index as usize;

        // Mark slot as no longer alive.
        self.alive[index] = false;

        // Increment generation so old handles become invalid.
        self.generations[index] += 1;

        // Push slot into free list so it can be reused.
        self.free_list.push(entity.index);

        true
    }

    /// Checks whether an entity handle is currently valid and alive.
    ///
    /// Returns `true` only if:
    /// - The index is in bounds.
    /// - The slot is marked alive.
    /// - The generation matches the current slot generation.
    ///
    /// This prevents stale handles from being considered valid.
    pub fn is_alive(&self, entity: Entity) -> bool {
        let index = entity.index as usize;

        match (self.alive.get(index), self.generations.get(index)) {
            (Some(is_alive), Some(generation)) => *is_alive && *generation == entity.generation,
            _ => false,
        }
    }

    /// Removes all entities and resets the allocator.
    ///
    /// After calling this:
    /// - All previous entity handles become invalid.
    /// - All internal storage is cleared.
    ///
    /// This is a full reset. Newly created entities may later reuse the same
    /// index and generation values as entities that existed before the clear.
    pub fn clear(&mut self) {
        self.generations.clear();
        self.alive.clear();
        self.free_list.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_entity_is_alive() {
        let mut alloc = EntityAllocator::new();
        let e = alloc.create();
        assert!(alloc.is_alive(e));
    }

    #[test]
    fn stale_handle_does_not_become_valid_again() {
        let mut alloc = EntityAllocator::new();

        let old = alloc.create();
        alloc.destroy(old);

        let new = alloc.create();

        assert_eq!(old.index, new.index);
        assert_ne!(old.generation, new.generation);

        assert!(!alloc.is_alive(old));
        assert!(alloc.is_alive(new));
    }

    #[test]
    fn destroying_twice_fails() {
        let mut alloc = EntityAllocator::new();

        let e = alloc.create();

        assert!(alloc.destroy(e));
        assert!(!alloc.destroy(e));
    }

    #[test]
    fn clear_invalidates_all_entities() {
        let mut alloc = EntityAllocator::new();

        let e1 = alloc.create();
        let e2 = alloc.create();

        alloc.clear();

        assert!(!alloc.is_alive(e1));
        assert!(!alloc.is_alive(e2));
    }

    #[test]
    fn reused_index_gets_new_generation() {
        let mut alloc = EntityAllocator::new();

        let e1 = alloc.create();
        let old_index = e1.index;
        let old_generation = e1.generation;

        alloc.destroy(e1);
        let e2 = alloc.create();

        assert_eq!(e2.index, old_index);
        assert_eq!(e2.generation, old_generation + 1);
    }

    #[test]
    fn invalid_entity_is_not_alive() {
        let alloc = EntityAllocator::new();

        let fake = Entity {
            index: 999,
            generation: 0,
        };

        assert!(!alloc.is_alive(fake));
    }

    #[test]
    fn create_after_clear_starts_fresh() {
        let mut alloc = EntityAllocator::new();

        let e1 = alloc.create();
        alloc.clear();
        let e2 = alloc.create();

        assert_eq!(e2.index, 0);
        assert_eq!(e2.generation, 0);
        assert!(!alloc.is_alive(e1));
        assert!(alloc.is_alive(e2));
    }
}
