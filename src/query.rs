use crate::archetype::ArchetypeSignature;
use crate::component::ComponentId;
use crate::archetype::Archetype;
use crate::entity::Entity;

/// Describes which archetypes a query matches.
/// 
/// A filter holds two lists:
/// - Required IDs: every listed component must be present.
/// - Excluded IDs: every listed component must be absent.
/// 
/// An empty filter matches all archetypes, including the empty one.
pub struct QueryFilter {
    /// Component IDs that must all appear in the archetype's signature.
    required: Vec<ComponentId>,

    /// Component IDs that must all be absent from the archetype's signature.
    excluded: Vec<ComponentId>,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryFilter {
    /// Creates an empty filter that matches every archetype.
    pub fn new() -> Self {
        Self {
            required: Vec::new(),
            excluded: Vec::new(),
        }
    }

    /// Adds `id` to the required set.
    /// 
    /// Returns `self` so calls can be chained.
    pub fn requiring(mut self, id: ComponentId) -> Self {
        self.required.push(id);
        self
    }

    /// Add `id` to excluded set.
    /// 
    /// Returns `self` so calls can be chained.
    pub fn excluding(mut self, id: ComponentId) -> Self {
        self.excluded.push(id);
        self
    }

    /// Returns the required component IDs.
    pub fn required(&self) -> &[ComponentId] {
        &self.required
    }

    /// Returns the excluded component IDs.
    pub fn excluded(&self) -> &[ComponentId] {
        &self.excluded
    }

    /// Returns `true` if `signature` satisfies this filter.
    /// 
    /// A signature satisfies the filter when:
    /// - It contains every required component ID.
    /// - It contains no excluded component ID.
    pub(crate) fn matches(&self, signature: &ArchetypeSignature) -> bool {
        self.required.iter().all(|&id| signature.contains(id))
            && self.excluded.iter().all(|&id| !signature.contains(id))
    }
}

/// A compiled query over a world's archetypes.
/// 
/// `Query<'w>` holds a list of references to matching archetypes, all valid
/// for the lifetime `'w`. The world cannot be mutated while this value is
/// alive because it holds shared borrows into the world's archetype storage.
/// 
/// Created by `World::query_with_filter` or `World::query_builder`.
pub struct Query<'w> {
    /// References to every archetype that passed the filter at construction time.
    /// 
    /// Stored as direct references rather than IDs to avoid a world lookup on
    /// every iterator step.
    archetypes: Vec<&'w Archetype>,
}

impl<'w> Query<'w> {
    /// Creates a query from a list of matching archetype references.
    /// 
    /// Called internally by the world; users should use `World::query_with_filter`
    /// or `World::query_builder` instead.
    pub(crate) fn new(archetypes: Vec<&'w Archetype>) -> Self {
        Self { archetypes }
    }

    /// Returns an iterator over every entity row across all matching archetypes.
    /// 
    /// Yields a `RowRef<'w>` for each row, which provides access to
    /// the entity handle and typed component values.
    pub fn iter(&self) -> QueryIter<'_, 'w> {
        QueryIter {
            archetypes: &self.archetypes,
            archetype_index: 0,
            row: 0,
        }
    }

    /// Returns the number of matching archetypes.
    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }
}

/// Iterates over every entity row across all archetypes in a `Query`.
/// 
/// `'q` is the lifetime of the borrow of the `Query`.
/// `'w` is the lifetime of the world data the query references.
/// 
/// The bound `'w: 'q` means "world data must outlive the query borrow",
/// which is always true since the `Query` itself borrows from the world.
pub struct QueryIter<'q, 'w: 'q> {
    /// The slice of matching archetypes from the query.
    archetypes: &'q [&'w Archetype],

    /// Index into `archetypes` of the archetype currently being iterated.
    archetype_index: usize,

    /// Row within the current archetype.
    row: usize,
}

impl<'q, 'w: 'q> Iterator for QueryIter<'q, 'w> {
    type Item = RowRef<'w>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // If we've exhausted all archetypes, the iteration is complete.
            let archetype = self.archetypes.get(self.archetype_index)?;

            if self.row < archetype.len() {
                let row = self.row;
                self.row += 1;

                return Some(RowRef { archetype, row });
            }

            // This archetype is fully consumed; advance to the next one.
            self.archetype_index += 1;
            self.row = 0;
        }
    }
}

impl<'q, 'w: 'q> IntoIterator for &'q Query<'w> {
    type Item = RowRef<'w>;
    type IntoIter = QueryIter<'q, 'w>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A reference to one entity's data within a matching archetype row.
/// 
/// Yielded by `QueryIter`. The lifetime `'w` ties every reference obtained
/// through this type to the world's archetype storage, not to the `RowRef`
/// value itself. This means you can collect multiple component references
/// from the same row without conflicting borrows.
pub struct RowRef<'w> {
    /// The archetype containing this row.
    archetype: &'w Archetype,

    /// The row index within the archetype.
    row: usize,
}

impl<'w> RowRef<'w> {
    /// Returns the entity stored at this row.
    pub fn entity(&self) -> Entity {
        // entities() returns a slice valid for 'w, and indexing it gives
        // a Copy type (Entity), so no lifetime annotation is needed here.
        self.archetype.entities()[self.row]
    }

    /// Returns a reference to the component value of type `T` at this row.
    /// 
    /// Returns `None` if the archetype does not contain `component_id`,
    /// or if `T` does not match the stored type.
    /// 
    /// The returned reference lives for `'w`, the world's lifetime, not
    /// just for the lifetime of `self`. This means you can hold references
    /// to multiple components from the same row simultaneously.
    pub fn get<T: 'static>(&self, component_id: ComponentId) -> Option<&'w T> {
        // self.archetype is &'w Archetype, so column() return Option<&'w Column>,
        // and get() on that returns Option<&'w T>.
        // The explicit 'w on the return type tells Rust to use that chain
        // rather than tying the lifetime to &self.
        self.archetype.column(component_id)?.get::<T>(self.row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    struct Position {
        x: f32,
        y: f32,
    }
    struct Velocity {
        x: f32,
        y: f32,
    }
    struct Mass(f32);

    #[test]
    fn empty_filter_matches_all_archetypes() {
        let mut world = World::new();
        world.register_component::<Position>();

        let e = world.spawn();
        world.add_component(e, Position { x: 0.0, y: 0.0 });

        let filter = QueryFilter::new();
        // Matches the empty archetype and the Position archetype.
        assert_eq!(world.matching_archetypes(&filter).len(), 2);
    }

    #[test]
    fn filter_with_required_excludes_archetypes_missing_that_component() {
        let mut world = World::new();
        let pos_id = world.register_component::<Position>();
        let vel_id = world.register_component::<Velocity>();

        let e1 = world.spawn();
        world.add_component(e1, Position { x: 0.0, y: 0.0 });
        world.add_component(e1, Velocity { x: 1.0, y: 0.0 });

        let e2 = world.spawn();
        world.add_component(e2, Position { x: 0.0, y: 0.0 });

        let filter = QueryFilter::new().requiring(pos_id).requiring(vel_id);
        let matching = world.matching_archetypes(&filter);

        // Only the archetype with both components matches.
        assert_eq!(matching.len(), 1);
        let arch = world.archetype(matching[0]).unwrap();
        assert!(arch.signature().contains(vel_id));
    }

    #[test]
    fn filter_with_excluded_omits_archtypes_containing_that_component() {
        let mut world = World::new();
        let pos_id = world.register_component::<Position>();
        let vel_id = world.register_component::<Velocity>();

        let e1 = world.spawn();
        world.add_component(e1, Position { x: 0.0, y: 0.0 });
        world.add_component(e1, Velocity { x: 1.0, y: 0.0 });

        let e2 = world.spawn();
        world.add_component(e2, Position { x: 0.0, y: 0.0 });

        // Require Position, exclude Velocity.
        let filter = QueryFilter::new().requiring(pos_id).excluding(vel_id);
        let matching = world.matching_archetypes(&filter);

        // Only the Position-only archetype passes.
        assert_eq!(matching.len(), 1);
        let arch = world.archetype(matching[0]).unwrap();
        assert!(!arch.signature().contains(vel_id));
    }

    #[test]
    fn filter_returns_no_archetypes_when_none_match() {
        let mut world = World::new();
        let mass_id = world.register_component::<Mass>();

        // No entity has Mass, so no archetype contains it.
        let filter = QueryFilter::new().requiring(mass_id);
        assert!(world.matching_archetypes(&filter).is_empty());
    }

    #[test]
    fn query_iterates_all_matching_entities() {
        let mut world = World::new();
        let pos_id = world.register_component::<Position>();
        let vel_id = world.register_component::<Velocity>();

        let e1 = world.spawn();
        world.add_component(e1, Position { x: 1.0, y: 0.0 });
        world.add_component(e1, Velocity { x: 1.0, y: 0.0 });

        let e2 = world.spawn();
        world.add_component(e2, Position { x: 2.0, y: 0.0 });
        world.add_component(e2, Velocity { x: 0.0, y: 1.0 });

        // e3 has Position but no Velocity; should not appear.
        let e3 = world.spawn();
        world.add_component(e3, Position { x: 3.0, y: 0.0 });

        let filter = QueryFilter::new().requiring(pos_id).requiring(vel_id);
        let query = world.query_with_filter(filter);

        let mut visited: Vec<Entity> = query.iter().map(|r| r.entity()).collect();
        visited.sort_by_key(|e| e.index);

        assert_eq!(visited, vec![e1, e2]);
    }

    #[test]
    fn row_ref_get_returns_correct_component_value() {
        let mut world = World::new();
        let pos_id = world.register_component::<Position>();

        let e = world.spawn();
        world.add_component(e, Position { x: 7.0, y: 3.0 });

        let filter = QueryFilter::new().requiring(pos_id);
        let query = world.query_with_filter(filter);

        let row = query.iter().next().unwrap();
        let pos = row.get::<Position>(pos_id).unwrap();

        assert_eq!(pos.x, 7.0);
        assert_eq!(pos.y, 3.0);
    }

    #[test]
    fn query_over_empty_world_yields_nothing() {
        let mut world = World::new();
        let pos_id = world.register_component::<Position>();

        let filter = QueryFilter::new().requiring(pos_id);
        let query = world.query_with_filter(filter);

        assert_eq!(query.iter().count(), 0);
    }

    #[test]
    fn into_iterator_works_in_for_loop() {
        let mut world = World::new();
        let pos_id = world.register_component::<Position>();

        let e = world.spawn();
        world.add_component(e, Position { x: 1.0, y: 2.0 });

        let filter = QueryFilter::new().requiring(pos_id);
        let query = world.query_with_filter(filter);

        let mut count = 0;
        for _row in &query {
            count += 1;
        }

        assert_eq!(count, 1);
    }
}