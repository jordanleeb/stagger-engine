use std::any::TypeId;
use std::collections::HashMap;

/// Runtime identifier for a registered component type.
pub type ComponentId = u32;

/// Registers component types and assigns them unique IDs.
///
/// Maps Rust types (`T`) to a compact runtime `ComponentId` values.
///
/// This allows the ECS to:
/// - Refer to component types at runtime.
/// - Store heterogeneous component data in a uniform way.
///
/// Guarantees:
/// - Each type is assigned a unique ID.
/// - Registering the same type multiple times returns the same ID.
pub struct ComponentRegistry {
    /// Maps Rust `TypeId` values to internal component IDs.
    type_to_id: HashMap<TypeId, ComponentId>,

    /// The next unused component ID.
    next_id: ComponentId,
}

impl Default for ComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentRegistry {
    /// Creates a new, empty component registry.
    ///
    /// No component types are registered initially.
    pub fn new() -> Self {
        Self {
            type_to_id: HashMap::new(),
            next_id: 0,
        }
    }

    /// Registers a component type and returns its ID.
    ///
    /// If the type has already been registered, returns the existing ID.
    ///
    /// This ensures that:
    /// - Each type is assigned exactly one ID.
    /// - Repeated registration is safe and idempotent.
    pub fn register<T: 'static>(&mut self) -> ComponentId {
        let type_id = TypeId::of::<T>();

        // If already registered, return the existing ID.
        if let Some(&id) = self.type_to_id.get(&type_id) {
            return id;
        }

        // Assign a new unique ID.
        let id = self.next_id;
        self.next_id += 1;

        self.type_to_id.insert(type_id, id);

        id
    }

    /// Returns the ID of a registered component type.
    ///
    /// Returns `None` if the type has not been registered.
    pub fn get<T: 'static>(&self) -> Option<ComponentId> {
        let type_id = TypeId::of::<T>();
        self.type_to_id.get(&type_id).copied()
    }

    /// Returns `true` if the component type has been registered.
    ///
    /// This is a convenience method equivalent to `get::<T>().is_some()`.
    pub fn contains<T: 'static>(&self) -> bool {
        self.get::<T>().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct A;
    struct B;

    #[test]
    fn registering_same_type_returns_same_id() {
        let mut reg = ComponentRegistry::new();

        let a1 = reg.register::<A>();
        let a2 = reg.register::<A>();

        assert_eq!(a1, a2);
    }

    #[test]
    fn different_types_get_different_ids() {
        let mut reg = ComponentRegistry::new();

        let a = reg.register::<A>();
        let b = reg.register::<B>();

        assert_ne!(a, b);
    }

    #[test]
    fn get_returns_registered_id() {
        let mut reg = ComponentRegistry::new();

        let a = reg.register::<A>();
        let got = reg.get::<A>();

        assert_eq!(Some(a), got);
    }

    #[test]
    fn get_returns_none_for_unregistered() {
        let reg = ComponentRegistry::new();

        assert_eq!(None, reg.get::<A>());
    }

    #[test]
    fn first_registered_type_gets_zero() {
        let mut reg = ComponentRegistry::new();
        let id = reg.register::<A>();
        assert_eq!(id, 0);
    }

    #[test]
    fn ids_are_assigned_sequentially() {
        let mut reg = ComponentRegistry::new();

        let a = reg.register::<A>();
        let b = reg.register::<B>();

        assert_eq!(a, 0);
        assert_eq!(b, 1);
    }

    #[test]
    fn contains_returns_false_for_unregistered_type() {
        let reg = ComponentRegistry::new();
        assert!(!reg.contains::<A>());
    }

    #[test]
    fn contains_returns_true_for_registered_type() {
        let mut reg = ComponentRegistry::new();
        reg.register::<A>();
        assert!(reg.contains::<A>());
    }

    #[test]
    fn contains_matches_get() {
        let mut reg = ComponentRegistry::new();

        assert_eq!(reg.contains::<A>(), reg.get::<A>().is_some());

        reg.register::<A>();

        assert_eq!(reg.contains::<A>(), reg.get::<A>().is_some());
    }
}
