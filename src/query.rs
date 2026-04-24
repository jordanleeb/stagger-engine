use crate::archetype::ArchetypeSignature;
use crate::component::ComponentId;

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
}