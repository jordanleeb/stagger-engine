# Stagger Engine Plan

## Phase 1
- [x] Entity allocator
- [x] Component registry
- [x] Basic world structure

## Phase 2
- [x] Archetype signatures
- [x] Archetype entity storage
- [x] Component storage
- [x] Entity location tracking
- [x] World archetype management
- [x] Entity movement between archetypes
- [x] Component column integration
- [x] Add/remove component API
- [x] O(1) HashMap archetype index
- [x] Archetype edge graph for O(1) structural transitions
- [x] Zero-sized marker component support
- [x] QueryFilter and archetype matching
- [x] Query, QueryIter, and RowRef with lifetime-safe world borrows
- [x] QueryBuilder fluent API
- [x] Systems

## Phase 2 QoL
- [x] `World::get_component::<T>(entity)` convenience accessor
- [x] `World::get_component_mut::<T>(entity)` mutable convenience accessor
- [x] `World::has_component::<T>(entity)` presence check
- [x] `remove_component` returns the removed value instead of dropping it
- [x] Resources (insert_resource, get_resources, get_resource_mut)

## Phase 3
- [x] Window and event loop
- [x] Renderer backend
- [x] Transform-driven rendering
- [ ] Camera component and render system
- [ ] Mesh and material components
- [ ] Debug rendering

## Phase 4
- [ ] Physics engine
- [ ] Collision detection
- [ ] Ragdoll prototype
- [ ] Balance controller